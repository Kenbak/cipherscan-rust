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
use std::time::Instant;

/// Accumulated per-stage timing breakdown.
#[derive(Debug, Default, Clone)]
struct StageTimings {
    source_read_us: u64,
    parse_us: u64,
    prevout_us: u64,
    flow_us: u64,
    sql_write_us: u64,
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

/// Index a range of blocks from RocksDB into PostgreSQL and collect timings.
async fn run_benchmark_pass(
    config: &Config,
    db_url: &str,
    from: u32,
    to: u32,
    parse_only: bool,
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

    let block_count = to - from + 1;
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
            match TransactionParser::parse(raw, height, &block_hash, config.network) {
                Ok(tx) => transactions.push(tx),
                Err(e) => {
                    tracing::warn!("Parse error at {}:{}: {}", height, tx_index, e);
                }
            }
        }
        stages.parse_us = parse_start.elapsed().as_micros() as u64;

        // Stage 3: Prevout resolution
        let prevout_start = Instant::now();
        for tx in &mut transactions {
            TransactionParser::resolve_inputs(tx, &zebra).map_err(|e| {
                format!("Input resolution failed at {}: {}", height, e)
            })?;
        }
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
            w.batch_insert_with_header_and_flows(
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
        if done.is_multiple_of(100) || height == to {
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

    Ok(RunSummary {
        blocks: block_count,
        transactions: total_txs,
        flows: total_flows,
        inputs: total_inputs,
        outputs: total_outputs,
        source_bytes: total_bytes,
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
    let block_count = to_height - from_height + 1;

    if !json_output {
        println!("================================================================");
        println!("  CipherScan NU7 Capacity Benchmark");
        println!("================================================================");
        println!("  Range:       {} -> {} ({} blocks)", from_height, to_height, block_count);
        println!("  Parse-only:  {}", parse_only);
        println!("  Warmup:      {} blocks", warmup_blocks);
        println!("  Database:    {}", if parse_only { "(skipped)" } else { &db_url[..40.min(db_url.len())] });
        println!("  Workload:    full explorer (blocks, txs, inputs, outputs,");
        println!("               addresses, spent marking, pubkey exposures,");
        println!("               shielded flows)");
        println!("  Source:      RocksDB direct read (Zebra/Zakura state)");
        println!();
    }

    // Warmup pass: exercise RocksDB page cache and PostgreSQL connection pool
    if warmup_blocks > 0 && from_height >= warmup_blocks {
        let warmup_from = from_height.saturating_sub(warmup_blocks);
        let warmup_to = from_height - 1;
        if !json_output {
            println!("-- Warmup: {} blocks ({} -> {}) --", warmup_blocks, warmup_from, warmup_to);
        }
        run_benchmark_pass(config, db_url, warmup_from, warmup_to, true).await?;
        if !json_output {
            println!();
        }
    }

    // Main benchmark pass
    if !json_output {
        println!("-- Benchmark: {} blocks ({} -> {}) --", block_count, from_height, to_height);
    }
    let result = run_benchmark_pass(config, db_url, from_height, to_height, parse_only).await?;

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
    println!("  Source bytes:     {:.2} MiB", r.source_bytes as f64 / (1024.0 * 1024.0));
    println!();
    println!("  Throughput:");
    println!("    Blocks/s:       {:.1}", r.blocks_per_sec());
    println!("    Transactions/s: {:.1}", r.txs_per_sec());
    println!("    Source MiB/s:   {:.2}", r.source_mib_per_sec());
    println!();
    println!("  Per-block latency:");
    println!("    p50:            {:.2} ms", r.p50_block_us() as f64 / 1000.0);
    println!("    p95:            {:.2} ms", r.p95_block_us() as f64 / 1000.0);
    println!("    p99:            {:.2} ms", r.p99_block_us() as f64 / 1000.0);
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
            "range": { "from": from, "to": to },
            "parse_only": parse_only,
            "workload": "full_explorer",
            "counts": {
                "blocks": r.blocks,
                "transactions": r.transactions,
                "inputs": r.inputs,
                "outputs": r.outputs,
                "flows": r.flows,
                "source_bytes": r.source_bytes,
            },
            "throughput": {
                "blocks_per_sec": format!("{:.1}", r.blocks_per_sec()),
                "txs_per_sec": format!("{:.1}", r.txs_per_sec()),
                "source_mib_per_sec": format!("{:.2}", r.source_mib_per_sec()),
            },
            "latency_ms": {
                "p50": format!("{:.2}", r.p50_block_us() as f64 / 1000.0),
                "p95": format!("{:.2}", r.p95_block_us() as f64 / 1000.0),
                "p99": format!("{:.2}", r.p99_block_us() as f64 / 1000.0),
            },
            "stage_breakdown_ms": {
                "source_read": format!("{:.1}", r.stages.source_read_us as f64 / 1000.0),
                "parse_decode": format!("{:.1}", r.stages.parse_us as f64 / 1000.0),
                "prevout_resolve": format!("{:.1}", r.stages.prevout_us as f64 / 1000.0),
                "flow_classify": format!("{:.1}", r.stages.flow_us as f64 / 1000.0),
                "sql_write": format!("{:.1}", r.stages.sql_write_us as f64 / 1000.0),
                "total": format!("{:.1}", total_us as f64 / 1000.0),
            },
            "elapsed_secs": format!("{:.2}", r.elapsed_secs),
            "nu7_headroom_multiple": format!("{:.1}", r.blocks_per_sec() / 0.04),
        })
    );
}
