//! NU7 capacity benchmark harness
//!
//! Replays immutable blocks from RocksDB into an isolated PostgreSQL database,
//! recording per-stage timings: source read, decode/parse, prevout resolution,
//! SQL write, and end-to-end latency.
//!
//! The harness measures the full CipherScan explorer workload: blocks,
//! transactions, inputs, outputs, address activity, spent marking, public-key
//! exposures, and shielded-flow classification — not a compact-only subset.

use crate::config::Config;
use crate::db::{PostgresWriter, ZebraState};
use crate::indexer::TransactionParser;
use crate::models::ShieldedFlow;
use sqlx::Row;
use std::time::Instant;

/// Accumulated per-stage timing breakdown.
#[derive(Debug, Default, Clone)]
struct StageTimings {
    source_read_us: u64,
    parse_us: u64,
    prevout_us: u64,
    flow_us: u64,
    sql_write_us: u64,
    sql_metadata_us: u64,
    sql_transaction_data_us: u64,
    sql_pubkey_exposures_us: u64,
    sql_spent_marking_us: u64,
    sql_flows_us: u64,
    sql_address_summaries_us: u64,
    sql_commit_us: u64,
}

/// Per-block metrics recorded during the run.
#[allow(dead_code)]
struct BlockMetrics {
    height: u32,
    tx_count: u32,
    flow_count: u32,
    input_count: u32,
    output_count: u32,
    block_bytes: u64,
    end_to_end_us: u64,
    stages: StageTimings,
}

/// Summary statistics for a benchmark run.
struct RunSummary {
    blocks: u32,
    transactions: u64,
    flows: u64,
    inputs: u64,
    outputs: u64,
    source_bytes: u64,
    normalized_rows: u64,
    wal_bytes: u64,
    database_bytes_growth: i64,
    postgres_version: Option<String>,
    postgres_wal_level: Option<String>,
    postgres_synchronous_commit: Option<String>,
    database_bytes_start: i64,
    elapsed_secs: f64,
    stages: StageTimings,
    per_block: Vec<BlockMetrics>,
}

impl RunSummary {
    fn blocks_per_sec(&self) -> f64 {
        self.blocks as f64 / self.elapsed_secs.max(f64::EPSILON)
    }

    fn txs_per_sec(&self) -> f64 {
        self.transactions as f64 / self.elapsed_secs.max(f64::EPSILON)
    }

    fn source_mib_per_sec(&self) -> f64 {
        (self.source_bytes as f64 / (1024.0 * 1024.0)) / self.elapsed_secs.max(f64::EPSILON)
    }

    fn p50_block_us(&self) -> u64 {
        percentile_us(&self.per_block, 50)
    }

    fn p95_block_us(&self) -> u64 {
        percentile_us(&self.per_block, 95)
    }

    fn p99_block_us(&self) -> u64 {
        percentile_us(&self.per_block, 99)
    }
}

fn percentile_us(metrics: &[BlockMetrics], pct: u32) -> u64 {
    if metrics.is_empty() {
        return 0;
    }
    let mut latencies: Vec<u64> = metrics.iter().map(|m| m.end_to_end_us).collect();
    latencies.sort_unstable();
    let idx = ((pct as f64 / 100.0) * (latencies.len() as f64 - 1.0)).round() as usize;
    latencies[idx.min(latencies.len() - 1)]
}

#[derive(Default)]
struct WriteSnapshot {
    wal_lsn: String,
    database_bytes: i64,
    normalized_rows: i64,
    postgres_version: String,
    wal_level: String,
    synchronous_commit: String,
}

async fn write_snapshot(writer: &PostgresWriter) -> Result<WriteSnapshot, String> {
    let row = sqlx::query(
        r#"
        SELECT pg_current_wal_lsn()::text AS wal_lsn,
               pg_database_size(current_database())::bigint AS database_bytes,
               current_setting('server_version') AS postgres_version,
               current_setting('wal_level') AS wal_level,
               current_setting('synchronous_commit') AS synchronous_commit,
               (
                   (SELECT COUNT(*) FROM blocks)
                 + (SELECT COUNT(*) FROM transactions)
                 + (SELECT COUNT(*) FROM transaction_inputs)
                 + (SELECT COUNT(*) FROM transaction_outputs)
                 + (SELECT COUNT(*) FROM address_transactions)
                 + (SELECT COUNT(*) FROM addresses)
                 + (SELECT COUNT(*) FROM shielded_flows)
                 + (SELECT COUNT(*) FROM transparent_key_exposures)
               )::bigint AS normalized_rows
        "#,
    )
    .fetch_one(writer.pool())
    .await
    .map_err(|e| format!("Failed to collect benchmark database statistics: {e}"))?;

    Ok(WriteSnapshot {
        wal_lsn: row.get("wal_lsn"),
        database_bytes: row.get("database_bytes"),
        normalized_rows: row.get("normalized_rows"),
        postgres_version: row.get("postgres_version"),
        wal_level: row.get("wal_level"),
        synchronous_commit: row.get("synchronous_commit"),
    })
}

async fn wal_bytes_since(writer: &PostgresWriter, start_lsn: &str) -> Result<u64, String> {
    let bytes: i64 =
        sqlx::query_scalar("SELECT pg_wal_lsn_diff(pg_current_wal_lsn(), $1::pg_lsn)::bigint")
            .bind(start_lsn)
            .fetch_one(writer.pool())
            .await
            .map_err(|e| format!("Failed to measure benchmark WAL: {e}"))?;

    u64::try_from(bytes).map_err(|_| format!("PostgreSQL returned negative WAL growth: {bytes}"))
}

/// Index a range of blocks from RocksDB into PostgreSQL and collect timings.
async fn run_benchmark_pass(
    config: &Config,
    db_url: &str,
    from: u32,
    to: u32,
    parse_only: bool,
    quiet: bool,
) -> Result<RunSummary, String> {
    let zebra = ZebraState::open(config)?;

    let writer = if !parse_only {
        Some(
            PostgresWriter::connect(db_url)
                .await
                .map_err(|e| format!("Failed to connect to benchmark DB: {}", e))?,
        )
    } else {
        None
    };

    let block_count = to
        .checked_sub(from)
        .and_then(|count| count.checked_add(1))
        .ok_or_else(|| format!("Invalid benchmark range: {from}..={to}"))?;
    let write_start = match writer.as_ref() {
        Some(writer) => Some(write_snapshot(writer).await?),
        None => None,
    };
    let overall_start = Instant::now();
    let mut total_txs = 0u64;
    let mut total_flows = 0u64;
    let mut total_inputs = 0u64;
    let mut total_outputs = 0u64;
    let mut total_bytes = 0u64;
    let mut cumulative = StageTimings::default();
    let mut per_block: Vec<BlockMetrics> = Vec::with_capacity(block_count as usize);

    for height in from..=to {
        let block_start = Instant::now();
        let mut stages = StageTimings::default();

        // Stage 1: Source read (RocksDB)
        let read_start = Instant::now();
        let hash_bytes = zebra.get_block_hash(height)?;
        let block_hash = crate::util::display_hash(&hash_bytes);
        let raw_txs = zebra.iter_block_transactions(height)?;
        let header = zebra
            .get_block_header(height)
            .map_err(|e| format!("Header error at {}: {}", height, e))?;
        let block_bytes: u64 = raw_txs.iter().map(|(_, raw)| raw.len() as u64).sum();
        stages.source_read_us = read_start.elapsed().as_micros() as u64;

        // Stage 2: Parse
        let parse_start = Instant::now();
        let mut transactions = Vec::with_capacity(raw_txs.len());
        for (tx_index, raw) in &raw_txs {
            let tx = TransactionParser::parse(raw, height, &block_hash, config.network)
                .map_err(|e| format!("Parse error at {height}:{tx_index}: {e}"))?;
            transactions.push(tx);
        }
        stages.parse_us = parse_start.elapsed().as_micros() as u64;

        // Stage 3: Prevout resolution
        let prevout_start = Instant::now();
        TransactionParser::resolve_block_inputs(&mut transactions, &zebra)
            .map_err(|e| format!("Input resolution failed at {}: {}", height, e))?;
        stages.prevout_us = prevout_start.elapsed().as_micros() as u64;

        // Stage 4: Flow classification
        let flow_start = Instant::now();
        let mut all_flows = Vec::new();
        for tx in &transactions {
            let flows = ShieldedFlow::from_transaction(tx);
            all_flows.extend(flows);
        }
        stages.flow_us = flow_start.elapsed().as_micros() as u64;

        let tx_count = transactions.len() as u32;
        let flow_count = all_flows.len() as u32;
        let input_count: u32 = transactions
            .iter()
            .map(|tx| tx.vin.iter().filter(|v| !v.is_coinbase).count() as u32)
            .sum();
        let output_count: u32 = transactions.iter().map(|tx| tx.vout.len() as u32).sum();

        // Stage 5: SQL write
        if let Some(ref w) = writer {
            let sql_start = Instant::now();
            let write = w
                .batch_insert_with_header_and_flows_measured(
                    height,
                    &block_hash,
                    header.time,
                    &transactions,
                    &all_flows,
                    &header,
                )
                .await
                .map_err(|e| format!("DB write error at {}: {}", height, e))?;
            stages.sql_write_us = sql_start.elapsed().as_micros() as u64;
            stages.sql_metadata_us = write.metrics.metadata_us;
            stages.sql_transaction_data_us = write.metrics.transaction_data_us;
            stages.sql_pubkey_exposures_us = write.metrics.pubkey_exposures_us;
            stages.sql_spent_marking_us = write.metrics.spent_marking_us;
            stages.sql_flows_us = write.metrics.flows_us;
            stages.sql_address_summaries_us = write.metrics.address_summaries_us;
            stages.sql_commit_us = write.metrics.commit_us;
        }

        let end_to_end_us = block_start.elapsed().as_micros() as u64;

        total_txs += tx_count as u64;
        total_flows += flow_count as u64;
        total_inputs += input_count as u64;
        total_outputs += output_count as u64;
        total_bytes += block_bytes;

        cumulative.source_read_us += stages.source_read_us;
        cumulative.parse_us += stages.parse_us;
        cumulative.prevout_us += stages.prevout_us;
        cumulative.flow_us += stages.flow_us;
        cumulative.sql_write_us += stages.sql_write_us;
        cumulative.sql_metadata_us += stages.sql_metadata_us;
        cumulative.sql_transaction_data_us += stages.sql_transaction_data_us;
        cumulative.sql_pubkey_exposures_us += stages.sql_pubkey_exposures_us;
        cumulative.sql_spent_marking_us += stages.sql_spent_marking_us;
        cumulative.sql_flows_us += stages.sql_flows_us;
        cumulative.sql_address_summaries_us += stages.sql_address_summaries_us;
        cumulative.sql_commit_us += stages.sql_commit_us;

        per_block.push(BlockMetrics {
            height,
            tx_count,
            flow_count,
            input_count,
            output_count,
            block_bytes,
            end_to_end_us,
            stages,
        });

        // Progress every 100 blocks or at the end
        let done = height - from + 1;
        if !quiet && (done.is_multiple_of(100) || height == to) {
            let elapsed = overall_start.elapsed().as_secs_f64();
            let rate = done as f64 / elapsed.max(f64::EPSILON);
            println!(
                "   {} / {} ({:.1}%) | {:.1} blk/s | {} txs | {:.1} MiB source",
                done,
                block_count,
                done as f64 / block_count as f64 * 100.0,
                rate,
                total_txs,
                total_bytes as f64 / (1024.0 * 1024.0),
            );
        }
    }

    let elapsed = overall_start.elapsed().as_secs_f64();
    let (
        normalized_rows,
        wal_bytes,
        database_bytes_growth,
        postgres_version,
        postgres_wal_level,
        postgres_synchronous_commit,
        database_bytes_start,
    ) = if let (Some(writer), Some(start)) = (writer.as_ref(), write_start) {
        let end = write_snapshot(writer).await?;
        let wal_bytes = wal_bytes_since(writer, &start.wal_lsn).await?;
        (
            end.normalized_rows.saturating_sub(start.normalized_rows) as u64,
            wal_bytes,
            end.database_bytes.saturating_sub(start.database_bytes),
            Some(start.postgres_version),
            Some(start.wal_level),
            Some(start.synchronous_commit),
            start.database_bytes,
        )
    } else {
        (0, 0, 0, None, None, None, 0)
    };

    Ok(RunSummary {
        blocks: block_count,
        transactions: total_txs,
        flows: total_flows,
        inputs: total_inputs,
        outputs: total_outputs,
        source_bytes: total_bytes,
        normalized_rows,
        wal_bytes,
        database_bytes_growth,
        postgres_version,
        postgres_wal_level,
        postgres_synchronous_commit,
        database_bytes_start,
        elapsed_secs: elapsed,
        stages: cumulative,
        per_block,
    })
}

/// Top-level benchmark entry point.
pub(crate) async fn run_benchmark(
    config: &Config,
    db_url: &str,
    from_height: u32,
    to_height: u32,
    warmup_blocks: u32,
    parse_only: bool,
    json_output: bool,
) -> Result<(), String> {
    let block_count = to_height
        .checked_sub(from_height)
        .and_then(|count| count.checked_add(1))
        .ok_or_else(|| format!("Invalid benchmark range: {from_height}..={to_height}"))?;

    if !json_output {
        println!("================================================================");
        println!("  CipherScan NU7 Capacity Benchmark");
        println!("================================================================");
        println!(
            "  Range:       {} -> {} ({} blocks)",
            from_height, to_height, block_count
        );
        println!("  Parse-only:  {}", parse_only);
        println!("  Warmup:      {} blocks", warmup_blocks);
        println!(
            "  Database:    {}",
            if parse_only {
                "(skipped)"
            } else {
                "(configured isolated database)"
            }
        );
        if parse_only {
            println!("  Workload:    source + parse + transparent prevout resolution");
        } else {
            println!("  Workload:    full explorer (blocks, txs, inputs, outputs,");
            println!("               addresses, spent marking, pubkey exposures,");
            println!("               shielded flows)");
        }
        println!("  Source:      RocksDB direct read (Zebra/Zakura state)");
        println!("  Git commit:  {}", env!("CIPHERSCAN_GIT_SHA"));
        println!(
            "  Platform:    {}-{}",
            std::env::consts::OS,
            std::env::consts::ARCH
        );
        println!();
    }

    // Warmup pass: exercise RocksDB page cache and PostgreSQL connection pool
    if warmup_blocks > 0 && from_height >= warmup_blocks {
        let warmup_from = from_height.saturating_sub(warmup_blocks);
        let warmup_to = from_height - 1;
        if !json_output {
            println!(
                "-- Warmup: {} blocks ({} -> {}) --",
                warmup_blocks, warmup_from, warmup_to
            );
        }
        run_benchmark_pass(
            config,
            db_url,
            warmup_from,
            warmup_to,
            parse_only,
            json_output,
        )
        .await?;
        if !json_output {
            println!();
        }
    }

    // Main benchmark pass
    if !json_output {
        println!(
            "-- Benchmark: {} blocks ({} -> {}) --",
            block_count, from_height, to_height
        );
    }
    let result = run_benchmark_pass(
        config,
        db_url,
        from_height,
        to_height,
        parse_only,
        json_output,
    )
    .await?;

    if json_output {
        print_json_summary(&result, from_height, to_height, parse_only);
    } else {
        print_human_summary(&result, from_height, to_height, parse_only);
    }

    Ok(())
}

fn print_human_summary(r: &RunSummary, from: u32, to: u32, parse_only: bool) {
    println!();
    println!("================================================================");
    println!("  BENCHMARK RESULTS");
    println!("================================================================");
    println!();
    println!("  Range:            {} -> {}", from, to);
    println!("  Blocks:           {}", r.blocks);
    println!("  Transactions:     {}", r.transactions);
    println!("  Inputs:           {}", r.inputs);
    println!("  Outputs:          {}", r.outputs);
    println!("  Flows:            {}", r.flows);
    if !parse_only {
        println!("  Normalized rows:  {}", r.normalized_rows);
        println!(
            "  WAL generated:    {:.2} MiB",
            r.wal_bytes as f64 / (1024.0 * 1024.0)
        );
        println!(
            "  Database growth:  {:.2} MiB",
            r.database_bytes_growth as f64 / (1024.0 * 1024.0)
        );
        println!(
            "  PostgreSQL:       {} (wal_level={}, synchronous_commit={})",
            r.postgres_version.as_deref().unwrap_or("unknown"),
            r.postgres_wal_level.as_deref().unwrap_or("unknown"),
            r.postgres_synchronous_commit
                .as_deref()
                .unwrap_or("unknown")
        );
    }
    println!(
        "  Source bytes:     {:.2} MiB",
        r.source_bytes as f64 / (1024.0 * 1024.0)
    );
    println!();
    println!("  Throughput:");
    println!("    Blocks/s:       {:.1}", r.blocks_per_sec());
    println!("    Transactions/s: {:.1}", r.txs_per_sec());
    println!("    Source MiB/s:   {:.2}", r.source_mib_per_sec());
    println!();
    println!("  Per-block latency:");
    println!(
        "    p50:            {:.2} ms",
        r.p50_block_us() as f64 / 1000.0
    );
    println!(
        "    p95:            {:.2} ms",
        r.p95_block_us() as f64 / 1000.0
    );
    println!(
        "    p99:            {:.2} ms",
        r.p99_block_us() as f64 / 1000.0
    );
    println!();
    println!("  Stage breakdown (cumulative):");
    let total_us = r.stages.source_read_us
        + r.stages.parse_us
        + r.stages.prevout_us
        + r.stages.flow_us
        + r.stages.sql_write_us;
    let total_us = total_us.max(1);
    println!(
        "    Source read:     {:>8.1} ms  ({:.1}%)",
        r.stages.source_read_us as f64 / 1000.0,
        r.stages.source_read_us as f64 / total_us as f64 * 100.0,
    );
    println!(
        "    Parse/decode:   {:>8.1} ms  ({:.1}%)",
        r.stages.parse_us as f64 / 1000.0,
        r.stages.parse_us as f64 / total_us as f64 * 100.0,
    );
    println!(
        "    Prevout resolve:{:>8.1} ms  ({:.1}%)",
        r.stages.prevout_us as f64 / 1000.0,
        r.stages.prevout_us as f64 / total_us as f64 * 100.0,
    );
    println!(
        "    Flow classify:  {:>8.1} ms  ({:.1}%)",
        r.stages.flow_us as f64 / 1000.0,
        r.stages.flow_us as f64 / total_us as f64 * 100.0,
    );
    if !parse_only {
        println!(
            "    SQL write:      {:>8.1} ms  ({:.1}%)",
            r.stages.sql_write_us as f64 / 1000.0,
            r.stages.sql_write_us as f64 / total_us as f64 * 100.0,
        );
        println!(
            "      metadata/locks:      {:>8.1} ms",
            r.stages.sql_metadata_us as f64 / 1000.0
        );
        println!(
            "      tx/input/output rows:{:>8.1} ms",
            r.stages.sql_transaction_data_us as f64 / 1000.0
        );
        println!(
            "      pubkey exposures:    {:>8.1} ms",
            r.stages.sql_pubkey_exposures_us as f64 / 1000.0
        );
        println!(
            "      spent marking:       {:>8.1} ms",
            r.stages.sql_spent_marking_us as f64 / 1000.0
        );
        println!(
            "      shielded flows:      {:>8.1} ms",
            r.stages.sql_flows_us as f64 / 1000.0
        );
        println!(
            "      address summaries:   {:>8.1} ms",
            r.stages.sql_address_summaries_us as f64 / 1000.0
        );
        println!(
            "      commit:              {:>8.1} ms",
            r.stages.sql_commit_us as f64 / 1000.0
        );
    }
    println!();
    println!("  Elapsed:          {:.2} s", r.elapsed_secs);

    // NU7 readiness gate
    let required_rate = 0.04; // 1 block per 25 seconds
    let headroom = r.blocks_per_sec() / required_rate;
    println!();
    if parse_only {
        println!("  NU7 gate:         (parse-only — write not measured)");
    } else if headroom >= 10.0 {
        println!(
            "  NU7 gate:         PASS  ({:.0}x headroom over {:.2} blk/s required rate)",
            headroom, required_rate,
        );
    } else {
        println!(
            "  NU7 gate:         WARN  ({:.1}x headroom — target is 10x minimum)",
            headroom,
        );
    }
    println!("================================================================");
}

fn print_json_summary(r: &RunSummary, from: u32, to: u32, parse_only: bool) {
    let total_us = r.stages.source_read_us
        + r.stages.parse_us
        + r.stages.prevout_us
        + r.stages.flow_us
        + r.stages.sql_write_us;

    println!(
        "{}",
        serde_json::json!({
            "benchmark": "cipherscan-nu7-capacity",
            "version": env!("CARGO_PKG_VERSION"),
            "git_sha": env!("CIPHERSCAN_GIT_SHA"),
            "platform": {
                "os": std::env::consts::OS,
                "arch": std::env::consts::ARCH,
            },
            "range": { "from": from, "to": to },
            "parse_only": parse_only,
            "workload": if parse_only { "parse_and_prevout" } else { "full_explorer" },
            "counts": {
                "blocks": r.blocks,
                "transactions": r.transactions,
                "inputs": r.inputs,
                "outputs": r.outputs,
                "flows": r.flows,
                "source_bytes": r.source_bytes,
                "normalized_rows": r.normalized_rows,
            },
            "throughput": {
                "blocks_per_sec": r.blocks_per_sec(),
                "txs_per_sec": r.txs_per_sec(),
                "source_mib_per_sec": r.source_mib_per_sec(),
                "normalized_rows_per_sec": r.normalized_rows as f64 / r.elapsed_secs.max(f64::EPSILON),
            },
            "latency_ms": {
                "p50": r.p50_block_us() as f64 / 1000.0,
                "p95": r.p95_block_us() as f64 / 1000.0,
                "p99": r.p99_block_us() as f64 / 1000.0,
            },
            "stage_breakdown_ms": {
                "source_read": r.stages.source_read_us as f64 / 1000.0,
                "parse_decode": r.stages.parse_us as f64 / 1000.0,
                "prevout_resolve": r.stages.prevout_us as f64 / 1000.0,
                "flow_classify": r.stages.flow_us as f64 / 1000.0,
                "sql_write": r.stages.sql_write_us as f64 / 1000.0,
                "sql_metadata": r.stages.sql_metadata_us as f64 / 1000.0,
                "sql_transaction_data": r.stages.sql_transaction_data_us as f64 / 1000.0,
                "sql_pubkey_exposures": r.stages.sql_pubkey_exposures_us as f64 / 1000.0,
                "sql_spent_marking": r.stages.sql_spent_marking_us as f64 / 1000.0,
                "sql_flows": r.stages.sql_flows_us as f64 / 1000.0,
                "sql_address_summaries": r.stages.sql_address_summaries_us as f64 / 1000.0,
                "sql_commit": r.stages.sql_commit_us as f64 / 1000.0,
                "total": total_us as f64 / 1000.0,
            },
            "postgres": {
                "version": r.postgres_version.as_deref(),
                "wal_level": r.postgres_wal_level.as_deref(),
                "synchronous_commit": r.postgres_synchronous_commit.as_deref(),
                "database_bytes_start": r.database_bytes_start,
                "wal_bytes": r.wal_bytes,
                "database_bytes_growth": r.database_bytes_growth,
            },
            "elapsed_secs": r.elapsed_secs,
            "nu7_headroom_multiple": r.blocks_per_sec() / 0.04,
        })
    );
}
