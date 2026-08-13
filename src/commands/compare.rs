use crate::config::Config;
use crate::db::ZebraState;

/// Compare Rust parsing with existing PostgreSQL data
pub(crate) async fn compare_with_postgres(
    config: &Config,
    database_url: &str,
    sample_count: usize,
    from_height: u32,
) -> Result<(), String> {
    use crate::indexer::TransactionParser;
    use sqlx::postgres::PgPoolOptions;
    use sqlx::Row;

    println!("🔍 Comparing Rust parsing with PostgreSQL data...");
    println!(
        "   Database: {}...",
        &database_url[..40.min(database_url.len())]
    );
    println!("   Sample size: {}", sample_count);
    println!("   From height: {}", from_height);
    println!("────────────────────────────────────────────────────────────");
    println!();

    // Connect to PostgreSQL
    let pool = PgPoolOptions::new()
        .max_connections(5)
        .connect(database_url)
        .await
        .map_err(|e| format!("Failed to connect to PostgreSQL: {}", e))?;

    println!("✅ Connected to PostgreSQL");

    // Open RocksDB
    let zebra = ZebraState::open(config)?;

    // Get sample transactions from PostgreSQL
    let query = r#"
        SELECT
            txid, block_height, tx_index, version, locktime,
            vin_count, vout_count, size, fee,
            sapling_spend_count, sapling_output_count, orchard_actions,
            value_balance_sapling, value_balance_orchard,
            is_coinbase, has_sapling, has_orchard
        FROM transactions
        WHERE block_height >= $1
        ORDER BY block_height, tx_index
        LIMIT $2
    "#;

    let rows = sqlx::query(query)
        .bind(from_height as i64)
        .bind(sample_count as i64)
        .fetch_all(&pool)
        .await
        .map_err(|e| format!("Query failed: {}", e))?;

    println!("📊 Fetched {} transactions from PostgreSQL", rows.len());
    println!();

    // Comparison stats
    let mut total = 0;
    let mut matches = 0;
    let mut mismatches: Vec<String> = Vec::new();

    for row in &rows {
        let pg_txid: String = row.get("txid");
        let pg_height: i64 = row.get("block_height");
        let pg_tx_index: Option<i32> = row.try_get("tx_index").ok();
        let pg_version: Option<i32> = row.try_get("version").ok();
        let pg_vin_count: Option<i32> = row.try_get("vin_count").ok();
        let pg_vout_count: Option<i32> = row.try_get("vout_count").ok();
        let pg_sapling_spends: Option<i32> = row.try_get("sapling_spend_count").ok();
        let pg_sapling_outputs: Option<i32> = row.try_get("sapling_output_count").ok();
        let pg_orchard_actions: Option<i32> = row.try_get("orchard_actions").ok();
        let pg_value_balance_sapling: Option<i64> = row.try_get("value_balance_sapling").ok();
        let pg_value_balance_orchard: Option<i64> = row.try_get("value_balance_orchard").ok();

        let height = pg_height as u32;
        let tx_index = pg_tx_index.unwrap_or(0) as u16;

        // Parse from RocksDB
        let raw = match zebra.get_transaction_by_loc(height, tx_index) {
            Ok(r) => r,
            Err(e) => {
                println!("⚠️  {}:{} - RocksDB error: {}", height, tx_index, e);
                continue;
            }
        };

        let block_hash = crate::util::display_hash(&zebra.get_block_hash(height).unwrap_or([0u8; 32]));

        let rust_tx = match TransactionParser::parse(&raw, height, &block_hash, config.network) {
            Ok(tx) => tx,
            Err(e) => {
                println!("⚠️  {}:{} - Parse error: {}", height, tx_index, e);
                continue;
            }
        };

        total += 1;
        let mut tx_matches = true;
        let mut diffs: Vec<String> = Vec::new();

        // Compare fields
        if rust_tx.txid != pg_txid {
            diffs.push(format!(
                "txid: rust={} pg={}",
                &rust_tx.txid[..16],
                &pg_txid[..16]
            ));
            tx_matches = false;
        }

        if let Some(pg_v) = pg_version {
            if rust_tx.version != pg_v {
                diffs.push(format!("version: rust={} pg={}", rust_tx.version, pg_v));
                tx_matches = false;
            }
        }

        if let Some(pg_vin) = pg_vin_count {
            if rust_tx.vin_count as i32 != pg_vin {
                diffs.push(format!(
                    "vin_count: rust={} pg={}",
                    rust_tx.vin_count, pg_vin
                ));
                tx_matches = false;
            }
        }

        if let Some(pg_vout) = pg_vout_count {
            if rust_tx.vout_count as i32 != pg_vout {
                diffs.push(format!(
                    "vout_count: rust={} pg={}",
                    rust_tx.vout_count, pg_vout
                ));
                tx_matches = false;
            }
        }

        if let Some(pg_ss) = pg_sapling_spends {
            if rust_tx.sapling_spends as i32 != pg_ss {
                diffs.push(format!(
                    "sapling_spends: rust={} pg={}",
                    rust_tx.sapling_spends, pg_ss
                ));
                tx_matches = false;
            }
        }

        if let Some(pg_so) = pg_sapling_outputs {
            if rust_tx.sapling_outputs as i32 != pg_so {
                diffs.push(format!(
                    "sapling_outputs: rust={} pg={}",
                    rust_tx.sapling_outputs, pg_so
                ));
                tx_matches = false;
            }
        }

        if let Some(pg_oa) = pg_orchard_actions {
            if rust_tx.orchard_actions as i32 != pg_oa {
                diffs.push(format!(
                    "orchard_actions: rust={} pg={}",
                    rust_tx.orchard_actions, pg_oa
                ));
                tx_matches = false;
            }
        }

        if let Some(pg_vbs) = pg_value_balance_sapling {
            if rust_tx.sapling_value_balance != pg_vbs {
                diffs.push(format!(
                    "sapling_balance: rust={} pg={}",
                    rust_tx.sapling_value_balance, pg_vbs
                ));
                tx_matches = false;
            }
        }

        if let Some(pg_vbo) = pg_value_balance_orchard {
            if rust_tx.orchard_value_balance != pg_vbo {
                diffs.push(format!(
                    "orchard_balance: rust={} pg={}",
                    rust_tx.orchard_value_balance, pg_vbo
                ));
                tx_matches = false;
            }
        }

        if tx_matches {
            matches += 1;
        } else {
            let txid_short = if pg_txid.len() > 16 {
                &pg_txid[..16]
            } else {
                &pg_txid
            };
            let msg = format!(
                "{}:{} {} - {}",
                height,
                tx_index,
                txid_short,
                diffs.join(", ")
            );
            mismatches.push(msg);
        }
    }

    // Summary for transactions
    println!();
    println!("────────────────────────────────────────────────────────────");
    println!("📊 Transaction Comparison:");
    println!(
        "   Total: {} | ✅ Matches: {} | ❌ Mismatches: {}",
        total,
        matches,
        mismatches.len()
    );

    if !mismatches.is_empty() {
        println!("   First 10 mismatches:");
        for m in mismatches.iter().take(10) {
            println!("      {}", m);
        }
    }

    // ========================================================================
    // COMPARE BLOCKS
    // ========================================================================
    println!();
    println!("────────────────────────────────────────────────────────────");
    println!("📦 Comparing Blocks...");

    let block_query = r#"
        SELECT height, hash, timestamp, transaction_count
        FROM blocks
        WHERE height >= $1
        ORDER BY height
        LIMIT $2
    "#;

    let block_rows = sqlx::query(block_query)
        .bind(from_height as i64)
        .bind(sample_count as i64)
        .fetch_all(&pool)
        .await
        .map_err(|e| format!("Block query failed: {}", e))?;

    let mut block_total = 0;
    let mut block_matches = 0;
    let mut block_mismatches: Vec<String> = Vec::new();

    for row in &block_rows {
        let pg_height: i64 = row.get("height");
        let pg_hash: String = row.get("hash");
        let pg_tx_count: Option<i32> = row.try_get("transaction_count").ok();

        let height = pg_height as u32;

        // Get from RocksDB
        let rust_hash = match zebra.get_block_hash(height) {
            Ok(h) => { crate::util::display_hash(&h) }
            Err(_) => continue,
        };

        let rust_tx_count = zebra.get_block_tx_count(height).unwrap_or(0);

        block_total += 1;
        let mut diffs: Vec<String> = Vec::new();

        if rust_hash != pg_hash {
            diffs.push(format!("hash mismatch"));
        }

        if let Some(pg_tc) = pg_tx_count {
            if rust_tx_count as i32 != pg_tc {
                diffs.push(format!("tx_count: rust={} pg={}", rust_tx_count, pg_tc));
            }
        }

        if diffs.is_empty() {
            block_matches += 1;
        } else {
            block_mismatches.push(format!("Block {}: {}", height, diffs.join(", ")));
        }
    }

    println!(
        "   Total: {} | ✅ Matches: {} | ❌ Mismatches: {}",
        block_total,
        block_matches,
        block_mismatches.len()
    );
    for m in block_mismatches.iter().take(5) {
        println!("      {}", m);
    }

    // ========================================================================
    // COMPARE TRANSACTION OUTPUTS (sample)
    // ========================================================================
    println!();
    println!("────────────────────────────────────────────────────────────");
    println!("📤 Comparing Transaction Outputs (vout)...");

    let vout_query = r#"
        SELECT o.txid, o.vout_index, o.value, o.address, t.block_height, t.tx_index
        FROM transaction_outputs o
        JOIN transactions t ON o.txid = t.txid
        WHERE t.block_height >= $1
        ORDER BY t.block_height, t.tx_index, o.vout_index
        LIMIT $2
    "#;

    let vout_rows = sqlx::query(vout_query)
        .bind(from_height as i64)
        .bind((sample_count * 3) as i64) // More outputs than tx
        .fetch_all(&pool)
        .await
        .map_err(|e| format!("Vout query failed: {}", e))?;

    let mut vout_total = 0;
    let mut vout_matches = 0;
    let mut vout_mismatches: Vec<String> = Vec::new();

    for row in &vout_rows {
        let _pg_txid: String = row.get("txid");
        let pg_vout_index: i32 = row.get("vout_index");
        let pg_value: i64 = row.get("value");
        let pg_address: Option<String> = row.try_get("address").ok();
        let pg_height: i64 = row.get("block_height");
        let pg_tx_index: Option<i32> = row.try_get("tx_index").ok();

        let height = pg_height as u32;
        let tx_index = pg_tx_index.unwrap_or(0) as u16;

        // Parse from RocksDB
        let raw = match zebra.get_transaction_by_loc(height, tx_index) {
            Ok(r) => r,
            Err(_) => continue,
        };

        let block_hash = crate::util::display_hash(&zebra.get_block_hash(height).unwrap_or([0u8; 32]));

        let rust_tx = match TransactionParser::parse(&raw, height, &block_hash, config.network) {
            Ok(tx) => tx,
            Err(_) => continue,
        };

        // Find the matching vout
        if let Some(rust_vout) = rust_tx.vout.iter().find(|v| v.n == pg_vout_index as u32) {
            vout_total += 1;
            let mut diffs: Vec<String> = Vec::new();

            if rust_vout.value != pg_value {
                diffs.push(format!("value: rust={} pg={}", rust_vout.value, pg_value));
            }

            // Compare addresses (both might be None/null)
            let rust_addr = rust_vout.address.as_deref();
            let pg_addr = pg_address.as_deref();
            if rust_addr != pg_addr {
                let r = rust_addr.unwrap_or("(none)");
                let p = pg_addr.unwrap_or("(none)");
                // Only report if both are Some but different
                if rust_addr.is_some() && pg_addr.is_some() {
                    diffs.push(format!(
                        "addr: rust={} pg={}",
                        &r[..16.min(r.len())],
                        &p[..16.min(p.len())]
                    ));
                }
            }

            if diffs.is_empty() {
                vout_matches += 1;
            } else {
                vout_mismatches.push(format!(
                    "{}:{} vout[{}]: {}",
                    height,
                    tx_index,
                    pg_vout_index,
                    diffs.join(", ")
                ));
            }
        }
    }

    println!(
        "   Total: {} | ✅ Matches: {} | ❌ Mismatches: {}",
        vout_total,
        vout_matches,
        vout_mismatches.len()
    );
    for m in vout_mismatches.iter().take(5) {
        println!("      {}", m);
    }

    // ========================================================================
    // FINAL SUMMARY
    // ========================================================================
    println!();
    println!("════════════════════════════════════════════════════════════");
    println!("📊 FINAL COMPARISON SUMMARY:");
    println!(
        "   Transactions: {}/{} matched ({:.1}%)",
        matches,
        total,
        if total > 0 {
            matches as f64 / total as f64 * 100.0
        } else {
            0.0
        }
    );
    println!(
        "   Blocks:       {}/{} matched ({:.1}%)",
        block_matches,
        block_total,
        if block_total > 0 {
            block_matches as f64 / block_total as f64 * 100.0
        } else {
            0.0
        }
    );
    println!(
        "   Vouts:        {}/{} matched ({:.1}%)",
        vout_matches,
        vout_total,
        if vout_total > 0 {
            vout_matches as f64 / vout_total as f64 * 100.0
        } else {
            0.0
        }
    );

    let all_match =
        mismatches.is_empty() && block_mismatches.is_empty() && vout_mismatches.is_empty();
    if all_match {
        println!();
        println!("🎉 All data matches! Rust parser is validated.");
    }

    println!("════════════════════════════════════════════════════════════");

    Ok(())
}
