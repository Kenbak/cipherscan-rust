use crate::config::Config;
use crate::db::ZebraState;
use std::time::Instant;

/// Run backfill indexer (with PostgreSQL writes)
pub(crate) async fn run_backfill(
    config: &Config,
    from: Option<u32>,
    to: Option<u32>,
) -> Result<(), String> {
    use crate::indexer::Indexer;

    // Check if DATABASE_URL is configured
    if config.database_url.is_empty() {
        return Err(
            "DATABASE_URL not configured. Set it in .env or pass --database-url".to_string(),
        );
    }

    println!("🔗 Connecting to PostgreSQL...");

    let indexer = Indexer::new(config.clone()).await?;

    println!("✅ Connected to PostgreSQL");
    println!();

    indexer.backfill(from, to).await
}

/// Run metadata-only backfill (locktime, expiry_height, sapling/sprout counts).
/// Reads raw txs from RocksDB, parses with the fixed parser, and issues targeted
/// UPDATEs on the transactions table. Does NOT touch outputs, inputs, address_transactions, or flows.
pub(crate) async fn run_backfill_metadata(
    config: &Config,
    from: u32,
    to: Option<u32>,
    batch_size: u32,
) -> Result<(), String> {
    use crate::db::PostgresWriter;
    use crate::indexer::TransactionParser;

    if config.database_url.is_empty() {
        return Err(
            "DATABASE_URL not configured. Set it in .env or pass --database-url".to_string(),
        );
    }

    let zebra = ZebraState::open(config)?;
    let postgres = PostgresWriter::connect(&config.database_url)
        .await
        .map_err(|e| format!("PostgreSQL error: {}", e))?;

    let tip = zebra.get_tip_height()?;
    let end = to.unwrap_or(tip);

    let checkpoint_key = "metadata_backfill_height";
    let start = match postgres
        .get_checkpoint_key(checkpoint_key)
        .await
        .map_err(|e| format!("Checkpoint read error: {}", e))?
    {
        Some(saved) if saved >= from => {
            println!(
                "📍 Resuming metadata backfill from checkpoint: {}",
                saved + 1
            );
            saved + 1
        }
        _ => from,
    };

    if start > end {
        println!(
            "✅ Metadata backfill already complete (start {} > end {})",
            start, end
        );
        return Ok(());
    }

    let total_blocks = end - start + 1;
    println!(
        "🔧 Metadata backfill: {} → {} ({} blocks, batch={})",
        start, end, total_blocks, batch_size
    );
    println!("   Only updating: locktime, expiry_height, sapling_spend_count,");
    println!("                  sapling_output_count, sprout_joinsplit_count, has_sprout");
    println!("────────────────────────────────────────────────────────────");

    let overall_start = Instant::now();
    let mut total_txs_updated = 0u64;
    let mut total_txs_parsed = 0u64;
    let mut current = start;

    while current <= end {
        let batch_end = (current + batch_size - 1).min(end);
        let mut batch_txs = Vec::new();

        for height in current..=batch_end {
            let hash_bytes = match zebra.get_block_hash(height) {
                Ok(h) => h,
                Err(e) => {
                    tracing::warn!("Skipping height {} (no block hash): {}", height, e);
                    continue;
                }
            };
            let block_hash = crate::util::display_hash(&hash_bytes);

            let raw_txs = match zebra.iter_block_transactions(height) {
                Ok(txs) => txs,
                Err(e) => {
                    tracing::warn!("Skipping height {} (no txs): {}", height, e);
                    continue;
                }
            };

            for (_tx_index, raw) in &raw_txs {
                match TransactionParser::parse(raw, height, &block_hash, config.network) {
                    Ok(tx) => {
                        total_txs_parsed += 1;
                        batch_txs.push(tx);
                    }
                    Err(e) => {
                        tracing::warn!("Parse error at {}:{}: {}", height, _tx_index, e);
                    }
                }
            }
        }

        let updated = postgres
            .batch_update_metadata(&batch_txs)
            .await
            .map_err(|e| {
                format!(
                    "DB update error at heights {}-{}: {}",
                    current, batch_end, e
                )
            })?;
        total_txs_updated += updated;

        postgres
            .update_checkpoint(checkpoint_key, &batch_end.to_string())
            .await
            .map_err(|e| format!("Checkpoint error: {}", e))?;

        let elapsed = overall_start.elapsed();
        let blocks_done = batch_end - start + 1;
        let rate = blocks_done as f64 / elapsed.as_secs_f64().max(f64::EPSILON);
        let remaining = (end - batch_end) as f64;
        let eta_secs = if rate > 0.0 { remaining / rate } else { 0.0 };

        println!(
            "📦 {} / {} ({:.1}%) | {:.0} blk/s | txs updated: {} | ETA: {:.0}s",
            batch_end,
            end,
            blocks_done as f64 / total_blocks as f64 * 100.0,
            rate,
            total_txs_updated,
            eta_secs
        );

        current = batch_end + 1;
    }

    let elapsed = overall_start.elapsed();
    println!("────────────────────────────────────────────────────────────");
    println!("✅ Metadata backfill complete!");
    println!("   Blocks scanned: {}", total_blocks);
    println!("   Transactions parsed: {}", total_txs_parsed);
    println!("   Rows updated: {}", total_txs_updated);
    println!("   Time: {:.1}s", elapsed.as_secs_f64());
    println!(
        "   Rate: {:.0} blocks/s, {:.0} tx/s",
        total_blocks as f64 / elapsed.as_secs_f64(),
        total_txs_parsed as f64 / elapsed.as_secs_f64()
    );

    Ok(())
}

/// Backfill anchor roots for transactions that were indexed before anchors were stored.
/// Queries transactions missing anchors, fetches raw bytes via RPC, parses, and batch-updates.
pub(crate) async fn run_backfill_anchors(
    config: &Config,
    v6_only: bool,
    batch_size: usize,
) -> Result<(), String> {
    use crate::db::PostgresWriter;
    use crate::indexer::TransactionParser;
    use sqlx::postgres::PgPoolOptions;
    use sqlx::Row;

    if config.database_url.is_empty() {
        return Err(
            "DATABASE_URL not configured. Set it in .env or pass --database-url".to_string(),
        );
    }

    let rpc = crate::db::ZebraRpc::from_env()
        .map_err(|e| format!("RPC not configured (needed for raw tx fetch): {}", e))?;
    let postgres = PostgresWriter::connect(&config.database_url)
        .await
        .map_err(|e| format!("PostgreSQL error: {}", e))?;

    let pool = PgPoolOptions::new()
        .max_connections(3)
        .connect(&config.database_url)
        .await
        .map_err(|e| format!("Query pool error: {}", e))?;

    let version_filter = if v6_only {
        "AND version = 6"
    } else {
        "AND version >= 5"
    };

    let query = format!(
        r#"SELECT txid, block_height, block_hash
           FROM transactions
           WHERE orchard_anchor IS NULL
             AND (has_orchard = true OR has_ironwood = true)
             {version_filter}
           ORDER BY block_height ASC"#
    );

    let rows: Vec<_> = sqlx::query(&query)
        .fetch_all(&pool)
        .await
        .map_err(|e| format!("Query error: {}", e))?;

    let total = rows.len();
    if total == 0 {
        println!("✅ No transactions need anchor backfill.");
        return Ok(());
    }

    println!(
        "🔧 Anchor backfill: {} transactions (batch={}, v6_only={})",
        total, batch_size, v6_only
    );
    println!("────────────────────────────────────────────────────────────");

    let overall_start = Instant::now();
    let mut updated_total = 0u64;
    let mut errors = 0u64;

    for (chunk_idx, chunk) in rows.chunks(batch_size).enumerate() {
        let mut updates: Vec<(String, Option<String>, Option<String>)> = Vec::new();

        for row in chunk {
            let txid: String = row.get("txid");
            let height: i64 = row.get("block_height");
            let block_hash: String = row.get("block_hash");

            match rpc.get_raw_transaction_hex(&txid).await {
                Ok(raw_hex) => {
                    let raw_bytes = match hex::decode(&raw_hex) {
                        Ok(b) => b,
                        Err(e) => {
                            tracing::warn!("Hex decode error for {}: {}", &txid[..16], e);
                            errors += 1;
                            continue;
                        }
                    };

                    match TransactionParser::parse(
                        &raw_bytes,
                        height as u32,
                        &block_hash,
                        config.network,
                    ) {
                        Ok(tx) => {
                            updates.push((txid, tx.orchard_anchor, tx.ironwood_anchor));
                        }
                        Err(e) => {
                            tracing::warn!("Parse error for {} at {}: {}", &txid[..16], height, e);
                            errors += 1;
                        }
                    }
                }
                Err(e) => {
                    tracing::warn!("RPC error for {}: {}", &txid[..16], e);
                    errors += 1;
                }
            }
        }

        let batch_updated = postgres
            .batch_update_anchors(&updates)
            .await
            .map_err(|e| format!("DB update error: {}", e))?;
        updated_total += batch_updated;

        let done = (chunk_idx + 1) * batch_size;
        let done = done.min(total);
        let elapsed = overall_start.elapsed();
        let rate = done as f64 / elapsed.as_secs_f64().max(f64::EPSILON);
        let eta = (total - done) as f64 / rate;

        println!(
            "📦 {}/{} ({:.1}%) | {:.0} tx/s | updated: {} | errors: {} | ETA: {:.0}s",
            done,
            total,
            done as f64 / total as f64 * 100.0,
            rate,
            updated_total,
            errors,
            eta
        );
    }

    let elapsed = overall_start.elapsed();
    println!("────────────────────────────────────────────────────────────");
    println!("✅ Anchor backfill complete!");
    println!("   Transactions processed: {}", total);
    println!("   Rows updated: {}", updated_total);
    println!("   Errors: {}", errors);
    println!("   Time: {:.1}s", elapsed.as_secs_f64());

    Ok(())
}

/// Reclassify nonstandard outputs as P2PK or bare-multisig by re-parsing raw scripts.
pub(crate) async fn run_backfill_scripts(config: &Config, batch_size: u32) -> Result<(), String> {
    use crate::db::PostgresWriter;
    use crate::indexer::TransactionParser;
    use sqlx::postgres::PgPoolOptions;
    use sqlx::Row;

    if config.database_url.is_empty() {
        return Err(
            "DATABASE_URL not configured. Set it in .env or pass --database-url".to_string(),
        );
    }

    let zebra = ZebraState::open(config)?;
    let _postgres = PostgresWriter::connect(&config.database_url)
        .await
        .map_err(|e| format!("PostgreSQL error: {}", e))?;

    let pool = PgPoolOptions::new()
        .max_connections(3)
        .connect(&config.database_url)
        .await
        .map_err(|e| format!("Query pool error: {}", e))?;

    // Include already-classified rows so migration 013 can normalize their
    // address semantics and populate every disclosed pubkey exposure.
    let rows: Vec<_> = sqlx::query(
        r#"SELECT o.txid, o.vout_index, t.block_height
           FROM transaction_outputs o
           JOIN transactions t ON o.txid = t.txid
           WHERE (o.script_type = 'nonstandard' AND o.address IS NULL)
              OR o.script_type IN ('pubkey', 'multisig')
           ORDER BY t.block_height ASC"#,
    )
    .fetch_all(&pool)
    .await
    .map_err(|e| format!("Query error: {}", e))?;

    let total = rows.len();
    if total == 0 {
        println!("✅ No nonstandard outputs to reclassify.");
        return Ok(());
    }

    println!(
        "🔧 Script backfill: {} nonstandard outputs to re-parse (batch={})",
        total, batch_size
    );
    println!("────────────────────────────────────────────────────────────");

    let overall_start = Instant::now();
    let mut reclassified = 0u64;
    let mut errors = 0u64;

    for (chunk_idx, chunk) in rows.chunks(batch_size as usize).enumerate() {
        let mut db_tx = pool
            .begin()
            .await
            .map_err(|e| format!("Transaction start error: {}", e))?;

        for row in chunk {
            let txid: String = row.get("txid");
            let vout_index: i32 = row.get("vout_index");
            let block_height: i64 = row.get("block_height");

            // Look up the raw transaction from Zebra RocksDB
            let raw_txs = match zebra.iter_block_transactions(block_height as u32) {
                Ok(txs) => txs,
                Err(_) => {
                    errors += 1;
                    continue;
                }
            };

            let mut found = false;
            for (_tx_idx, raw) in &raw_txs {
                let block_hash_bytes = match zebra.get_block_hash(block_height as u32) {
                    Ok(h) => h,
                    Err(_) => continue,
                };
                let block_hash = crate::util::display_hash(&block_hash_bytes);

                match TransactionParser::parse(
                    raw,
                    block_height as u32,
                    &block_hash,
                    config.network,
                ) {
                    Ok(tx) => {
                        if tx.txid == txid {
                            if let Some(output) = tx.vout.iter().find(|o| o.n == vout_index as u32)
                            {
                                if output.script_type == "pubkey"
                                    || output.script_type == "multisig"
                                {
                                    if output.pubkey_exposures.is_empty() {
                                        errors += 1;
                                        found = true;
                                        break;
                                    }
                                    // Populate canonical script/exposure metadata first.
                                    // The targeted integrity repair clears legacy synthetic
                                    // addresses atomically with activity/summary correction.
                                    sqlx::query(
                                        r#"UPDATE transaction_outputs
                                           SET script_type = $1, script_pubkey = $2
                                           WHERE txid = $3 AND vout_index = $4"#,
                                    )
                                    .bind(&output.script_type)
                                    .bind(&output.script_pub_key)
                                    .bind(&txid)
                                    .bind(vout_index)
                                    .execute(&mut *db_tx)
                                    .await
                                    .map_err(|e| format!("Update error: {}", e))?;

                                    sqlx::query(
                                        "DELETE FROM transparent_key_exposures \
                                         WHERE txid=$1 AND vout_index=$2",
                                    )
                                    .bind(&txid)
                                    .bind(vout_index)
                                    .execute(&mut *db_tx)
                                    .await
                                    .map_err(|e| format!("Exposure reset error: {}", e))?;
                                    for exposure in &output.pubkey_exposures {
                                        sqlx::query(
                                            r#"INSERT INTO transparent_key_exposures
                                               (txid, vout_index, key_index, pubkey_hex, script_type, derived_address)
                                               VALUES ($1, $2, $3, $4, $5, $6)
                                               ON CONFLICT (txid, vout_index, key_index) DO UPDATE SET
                                                   pubkey_hex = EXCLUDED.pubkey_hex,
                                                   script_type = EXCLUDED.script_type,
                                                   derived_address = EXCLUDED.derived_address"#
                                        )
                                        .bind(&txid)
                                        .bind(vout_index)
                                        .bind(exposure.pubkey_index as i32)
                                        .bind(&exposure.pubkey_hex)
                                        .bind(&output.script_type)
                                        .bind(&exposure.derived_p2pkh)
                                        .execute(&mut *db_tx)
                                        .await
                                        .map_err(|e| format!("Exposure upsert error: {}", e))?;
                                    }

                                    reclassified += 1;
                                }
                            }
                            found = true;
                            break;
                        }
                    }
                    Err(_) => continue,
                }
            }
            if !found {
                errors += 1;
            }
        }

        db_tx
            .commit()
            .await
            .map_err(|e| format!("Commit error: {}", e))?;

        let done = ((chunk_idx + 1) * batch_size as usize).min(total);
        let elapsed = overall_start.elapsed();
        let rate = done as f64 / elapsed.as_secs_f64().max(f64::EPSILON);
        let eta = (total - done) as f64 / rate;

        println!(
            "📦 {}/{} ({:.1}%) | {:.0} outputs/s | reclassified: {} | errors: {} | ETA: {:.0}s",
            done,
            total,
            done as f64 / total as f64 * 100.0,
            rate,
            reclassified,
            errors,
            eta
        );
    }

    let elapsed = overall_start.elapsed();
    println!("────────────────────────────────────────────────────────────");
    println!("✅ Script backfill complete!");
    println!("   Outputs processed: {}", total);
    println!("   Reclassified: {} (P2PK/multisig)", reclassified);
    println!("   Errors: {}", errors);
    println!("   Time: {:.1}s", elapsed.as_secs_f64());

    if errors != 0 {
        return Err(format!(
            "script exposure backfill failed with {errors} unresolved outputs"
        ));
    }
    Ok(())
}
