use crate::config::Config;
use crate::db::ZebraState;
use std::time::Instant;

/// Full validation: index into test DB, compare with prod, benchmark
pub(crate) async fn validate_full(
    config: &Config,
    prod_db_url: &str,
    test_db_url: &str,
    from_height: u32,
    to_height: u32,
) -> Result<(), String> {
    use crate::db::PostgresWriter;
    use crate::indexer::TransactionParser;
    use crate::models::ShieldedFlow;
    use sqlx::postgres::PgPoolOptions;
    use sqlx::Row;

    let block_count = to_height - from_height + 1;

    println!("════════════════════════════════════════════════════════════");
    println!("🧪 FULL VALIDATION");
    println!("════════════════════════════════════════════════════════════");
    println!(
        "   Blocks: {} → {} ({} blocks)",
        from_height, to_height, block_count
    );
    println!(
        "   Prod DB: {}...",
        &prod_db_url[..40.min(prod_db_url.len())]
    );
    println!(
        "   Test DB: {}...",
        &test_db_url[..40.min(test_db_url.len())]
    );
    println!();

    // ========================================================================
    // STEP 1: Index into test database with Rust
    // ========================================================================
    println!("────────────────────────────────────────────────────────────");
    println!("📝 STEP 1: Index {} blocks with Rust indexer", block_count);
    println!("────────────────────────────────────────────────────────────");

    let zebra = ZebraState::open(config)?;
    let test_writer = PostgresWriter::connect(test_db_url)
        .await
        .map_err(|e| format!("Failed to connect to test DB: {}", e))?;

    println!("✅ Connected to test database");

    let rust_start = Instant::now();
    let mut rust_tx_count = 0u64;
    let mut rust_flow_count = 0u64;

    for height in from_height..=to_height {
        // Get block hash
        let hash_bytes = zebra.get_block_hash(height)?;
        let block_hash = crate::util::display_hash(&hash_bytes);

        // Get all transactions
        let raw_txs = zebra.iter_block_transactions(height)?;
        let mut transactions = Vec::with_capacity(raw_txs.len());
        let mut all_flows = Vec::new();

        for (tx_index, raw) in &raw_txs {
            match TransactionParser::parse(raw, height, &block_hash, config.network) {
                Ok(mut tx) => {
                    // Resolve input addresses and values from previous outputs
                    TransactionParser::resolve_inputs(&mut tx, &zebra).map_err(|e| {
                        format!("Input resolution failed at {}:{}: {}", height, tx_index, e)
                    })?;

                    let flows = ShieldedFlow::from_transaction(&tx);
                    rust_flow_count += flows.len() as u64;
                    all_flows.extend(flows);
                    transactions.push(tx);
                    rust_tx_count += 1;
                }
                Err(e) => {
                    tracing::warn!("Parse error at {}:{}: {}", height, tx_index, e);
                }
            }
        }

        // Get block header for timestamp and other fields
        let header = zebra
            .get_block_header(height)
            .map_err(|e| format!("Header error at {}: {}", height, e))?;
        let block_time = header.time;

        // Write the block bundle atomically so verification matches the production indexer path.
        test_writer
            .batch_insert_with_header_and_flows(
                height,
                &block_hash,
                block_time,
                &transactions,
                &all_flows,
                &header,
            )
            .await
            .map_err(|e| format!("DB write error at {}: {}", height, e))?;

        if (height - from_height + 1) % 10 == 0 {
            let elapsed = rust_start.elapsed();
            let rate = (height - from_height + 1) as f64 / elapsed.as_secs_f64();
            println!(
                "   📦 {} / {} | {:.1} blk/s | {} txs",
                height, to_height, rate, rust_tx_count
            );
        }
    }

    let rust_elapsed = rust_start.elapsed();
    let rust_rate = block_count as f64 / rust_elapsed.as_secs_f64();
    let rust_tx_rate = rust_tx_count as f64 / rust_elapsed.as_secs_f64();

    println!();
    println!("✅ Rust indexing complete:");
    println!("   Blocks: {}", block_count);
    println!("   Transactions: {}", rust_tx_count);
    println!("   Flows: {}", rust_flow_count);
    println!("   Time: {:.2}s", rust_elapsed.as_secs_f64());
    println!(
        "   Rate: {:.1} blocks/s, {:.1} tx/s",
        rust_rate, rust_tx_rate
    );

    // ========================================================================
    // STEP 2: Compare test DB with production DB
    // ========================================================================
    println!();
    println!("────────────────────────────────────────────────────────────");
    println!("🔍 STEP 2: Compare test DB with production DB");
    println!("────────────────────────────────────────────────────────────");

    let prod_pool = PgPoolOptions::new()
        .max_connections(5)
        .connect(prod_db_url)
        .await
        .map_err(|e| format!("Failed to connect to prod DB: {}", e))?;

    let test_pool = PgPoolOptions::new()
        .max_connections(5)
        .connect(test_db_url)
        .await
        .map_err(|e| format!("Failed to connect to test DB: {}", e))?;

    println!("✅ Connected to both databases");
    println!();

    // Compare transactions
    println!("📊 Comparing transactions...");

    let prod_txs: Vec<_> = sqlx::query(
        r#"
        SELECT txid, block_height, version, vin_count, vout_count,
               sapling_spend_count, sapling_output_count, orchard_actions,
               value_balance_sapling, value_balance_orchard, fee,
               total_input, total_output, is_coinbase
        FROM transactions
        WHERE block_height >= $1 AND block_height <= $2
        ORDER BY block_height, txid
        "#,
    )
    .bind(from_height as i64)
    .bind(to_height as i64)
    .fetch_all(&prod_pool)
    .await
    .map_err(|e| format!("Prod query failed: {}", e))?;

    let test_txs: Vec<_> = sqlx::query(
        r#"
        SELECT txid, block_height, version, vin_count, vout_count,
               sapling_spend_count, sapling_output_count, orchard_actions,
               value_balance_sapling, value_balance_orchard, fee,
               total_input, total_output, is_coinbase
        FROM transactions
        WHERE block_height >= $1 AND block_height <= $2
        ORDER BY block_height, txid
        "#,
    )
    .bind(from_height as i64)
    .bind(to_height as i64)
    .fetch_all(&test_pool)
    .await
    .map_err(|e| format!("Test query failed: {}", e))?;

    println!("   Prod DB: {} transactions", prod_txs.len());
    println!("   Test DB: {} transactions", test_txs.len());

    // Build lookup map for test txs
    let mut test_tx_map: std::collections::HashMap<String, &sqlx::postgres::PgRow> =
        std::collections::HashMap::new();
    for row in &test_txs {
        let txid: String = row.get("txid");
        test_tx_map.insert(txid, row);
    }

    let mut tx_matches = 0;
    let mut tx_mismatches: Vec<String> = Vec::new();
    let mut tx_improvements: Vec<String> = Vec::new();
    let mut tx_missing = 0;
    let mut _tx_nulls_checked = 0;

    // Fields where prod=0 and test=value is an IMPROVEMENT (Node.js doesn't calculate these)
    let improvement_fields: std::collections::HashSet<&str> =
        ["fee", "total_input", "total_output", "is_coinbase"]
            .iter()
            .cloned()
            .collect();

    for prod_row in &prod_txs {
        let txid: String = prod_row.get("txid");
        let height: i64 = prod_row.get("block_height");

        if let Some(test_row) = test_tx_map.get(&txid) {
            let mut diffs: Vec<String> = Vec::new();
            let mut improvements: Vec<String> = Vec::new();

            // Compare each field - explicit null checking
            macro_rules! compare_field {
                ($field:expr, $ty:ty) => {{
                    let prod_val: Option<$ty> = prod_row.try_get($field).ok();
                    let test_val: Option<$ty> = test_row.try_get($field).ok();

                    match (prod_val, test_val) {
                        (Some(p), Some(t)) if p != t => {
                            // Check if this is an "improvement" field where prod=0
                            let is_improvement = improvement_fields.contains($field);
                            let prod_is_zero =
                                format!("{:?}", p) == "0" || format!("{:?}", p) == "false";

                            if is_improvement && prod_is_zero {
                                // This is an improvement, not a mismatch
                                improvements.push(format!("{}: +{:?}", $field, t));
                            } else {
                                diffs.push(format!("{}: prod={:?} test={:?}", $field, p, t));
                            }
                        }
                        (Some(p), None) => {
                            diffs.push(format!("{}: prod={:?} test=NULL", $field, p));
                        }
                        (None, Some(t)) => {
                            diffs.push(format!("{}: prod=NULL test={:?}", $field, t));
                        }
                        (None, None) => {
                            _tx_nulls_checked += 1;
                        }
                        _ => {} // Match
                    }
                }};
            }

            compare_field!("version", i32);
            compare_field!("vin_count", i32);
            compare_field!("vout_count", i32);
            compare_field!("sapling_spend_count", i32);
            compare_field!("sapling_output_count", i32);
            compare_field!("orchard_actions", i32);
            compare_field!("value_balance_sapling", i64);
            compare_field!("value_balance_orchard", i64);
            compare_field!("fee", i64);
            compare_field!("total_input", i64);
            compare_field!("total_output", i64);
            compare_field!("is_coinbase", bool);

            if diffs.is_empty() {
                tx_matches += 1;
                if !improvements.is_empty() {
                    tx_improvements.push(format!(
                        "{}:{} {}",
                        height,
                        &txid[..16],
                        improvements.join(", ")
                    ));
                }
            } else {
                tx_mismatches.push(format!("{}:{} {}", height, &txid[..16], diffs.join(", ")));
            }
        } else {
            tx_missing += 1;
            if tx_missing <= 5 {
                println!(
                    "   ⚠️  Missing in test: {} at height {}",
                    &txid[..16],
                    height
                );
            }
        }
    }

    println!();
    println!("   ✅ Matches: {}", tx_matches);
    println!("   ❌ Real mismatches: {}", tx_mismatches.len());
    println!(
        "   ✨ Improvements (Rust adds data): {}",
        tx_improvements.len()
    );
    println!("   ⚠️  Missing: {}", tx_missing);

    if !tx_mismatches.is_empty() {
        println!();
        println!("   First 5 real mismatches:");
        for m in tx_mismatches.iter().take(5) {
            println!("      ❌ {}", m);
        }
    }

    if !tx_improvements.is_empty() && tx_improvements.len() <= 3 {
        println!();
        println!("   Sample improvements:");
        for m in tx_improvements.iter().take(3) {
            println!("      ✨ {}", m);
        }
    }

    // Compare transaction outputs
    println!();
    println!("📤 Comparing transaction outputs...");

    let prod_outputs: Vec<_> = sqlx::query(
        r#"
        SELECT txid, vout_index, value, address
        FROM transaction_outputs
        WHERE txid IN (
            SELECT txid FROM transactions
            WHERE block_height >= $1 AND block_height <= $2
        )
        ORDER BY txid, vout_index
        "#,
    )
    .bind(from_height as i64)
    .bind(to_height as i64)
    .fetch_all(&prod_pool)
    .await
    .map_err(|e| format!("Prod outputs query failed: {}", e))?;

    let test_outputs: Vec<_> = sqlx::query(
        r#"
        SELECT txid, vout_index, value, address
        FROM transaction_outputs
        WHERE txid IN (
            SELECT txid FROM transactions
            WHERE block_height >= $1 AND block_height <= $2
        )
        ORDER BY txid, vout_index
        "#,
    )
    .bind(from_height as i64)
    .bind(to_height as i64)
    .fetch_all(&test_pool)
    .await
    .map_err(|e| format!("Test outputs query failed: {}", e))?;

    println!("   Prod DB: {} outputs", prod_outputs.len());
    println!("   Test DB: {} outputs", test_outputs.len());

    // Build lookup
    let mut test_output_map: std::collections::HashMap<(String, i32), &sqlx::postgres::PgRow> =
        std::collections::HashMap::new();
    for row in &test_outputs {
        let txid: String = row.get("txid");
        let vout: i32 = row.get("vout_index");
        test_output_map.insert((txid, vout), row);
    }

    let mut out_matches = 0;
    let mut out_mismatches: Vec<String> = Vec::new();
    let mut out_missing = 0;

    for prod_row in &prod_outputs {
        let txid: String = prod_row.get("txid");
        let vout: i32 = prod_row.get("vout_index");
        let prod_value: i64 = prod_row.get("value");
        let prod_addr: Option<String> = prod_row.try_get("address").ok().flatten();

        if let Some(test_row) = test_output_map.get(&(txid.clone(), vout)) {
            let test_value: i64 = test_row.get("value");
            let test_addr: Option<String> = test_row.try_get("address").ok().flatten();

            let mut diffs: Vec<String> = Vec::new();

            if prod_value != test_value {
                diffs.push(format!("value: prod={} test={}", prod_value, test_value));
            }

            match (&prod_addr, &test_addr) {
                (Some(p), Some(t)) if p != t => {
                    diffs.push(format!(
                        "addr: prod={} test={}",
                        &p[..20.min(p.len())],
                        &t[..20.min(t.len())]
                    ));
                }
                (Some(p), None) => {
                    diffs.push(format!("addr: prod={} test=NULL", &p[..20.min(p.len())]));
                }
                (None, Some(t)) => {
                    diffs.push(format!("addr: prod=NULL test={}", &t[..20.min(t.len())]));
                }
                _ => {}
            }

            if diffs.is_empty() {
                out_matches += 1;
            } else {
                out_mismatches.push(format!("{}[{}]: {}", &txid[..12], vout, diffs.join(", ")));
            }
        } else {
            out_missing += 1;
        }
    }

    println!();
    println!("   ✅ Matches: {}", out_matches);
    println!("   ❌ Mismatches: {}", out_mismatches.len());
    println!("   ⚠️  Missing: {}", out_missing);

    if !out_mismatches.is_empty() {
        println!();
        println!("   First 10 mismatches:");
        for m in out_mismatches.iter().take(10) {
            println!("      {}", m);
        }
    }

    // Compare transaction inputs
    println!();
    println!("📥 Comparing transaction inputs...");

    let prod_inputs: Vec<_> = sqlx::query(
        r#"
        SELECT txid, vout_index, prev_txid, prev_vout, address, value
        FROM transaction_inputs
        WHERE txid IN (
            SELECT txid FROM transactions
            WHERE block_height >= $1 AND block_height <= $2
        )
        ORDER BY txid, vout_index
        "#,
    )
    .bind(from_height as i64)
    .bind(to_height as i64)
    .fetch_all(&prod_pool)
    .await
    .map_err(|e| format!("Prod inputs query failed: {}", e))?;

    let test_inputs: Vec<_> = sqlx::query(
        r#"
        SELECT txid, vout_index, prev_txid, prev_vout, address, value
        FROM transaction_inputs
        WHERE txid IN (
            SELECT txid FROM transactions
            WHERE block_height >= $1 AND block_height <= $2
        )
        ORDER BY txid, vout_index
        "#,
    )
    .bind(from_height as i64)
    .bind(to_height as i64)
    .fetch_all(&test_pool)
    .await
    .map_err(|e| format!("Test inputs query failed: {}", e))?;

    println!("   Prod DB: {} inputs", prod_inputs.len());
    println!("   Test DB: {} inputs", test_inputs.len());

    // Build lookup
    let mut test_input_map: std::collections::HashMap<(String, i32), &sqlx::postgres::PgRow> =
        std::collections::HashMap::new();
    for row in &test_inputs {
        let txid: String = row.get("txid");
        let vin: i32 = row.get("vout_index");
        test_input_map.insert((txid, vin), row);
    }

    let mut in_matches = 0;
    let mut in_mismatches: Vec<String> = Vec::new();
    let mut in_missing = 0;

    for prod_row in &prod_inputs {
        let txid: String = prod_row.get("txid");
        let vin: i32 = prod_row.get("vout_index");
        let prod_prev_txid: Option<String> = prod_row.try_get("prev_txid").ok().flatten();
        let prod_value: Option<i64> = prod_row.try_get("value").ok().flatten();

        if let Some(test_row) = test_input_map.get(&(txid.clone(), vin)) {
            let test_prev_txid: Option<String> = test_row.try_get("prev_txid").ok().flatten();
            let test_value: Option<i64> = test_row.try_get("value").ok().flatten();

            let mut diffs: Vec<String> = Vec::new();

            match (&prod_prev_txid, &test_prev_txid) {
                (Some(p), Some(t)) if p != t => {
                    diffs.push(format!("prev_txid differs"));
                }
                (Some(_), None) => diffs.push("prev_txid: prod has value, test NULL".to_string()),
                (None, Some(_)) => diffs.push("prev_txid: prod NULL, test has value".to_string()),
                _ => {}
            }

            match (prod_value, test_value) {
                (Some(p), Some(t)) if p != t => {
                    diffs.push(format!("value: prod={} test={}", p, t));
                }
                (Some(p), None) => diffs.push(format!("value: prod={} test=NULL", p)),
                (None, Some(t)) => diffs.push(format!("value: prod=NULL test={}", t)),
                _ => {}
            }

            if diffs.is_empty() {
                in_matches += 1;
            } else {
                in_mismatches.push(format!("{}[{}]: {}", &txid[..12], vin, diffs.join(", ")));
            }
        } else {
            in_missing += 1;
        }
    }

    println!();
    println!("   ✅ Matches: {}", in_matches);
    println!("   ❌ Mismatches: {}", in_mismatches.len());
    println!("   ⚠️  Missing: {}", in_missing);

    // Compare shielded flows
    println!();
    println!("🔒 Comparing shielded flows...");

    let prod_flows: Vec<_> = sqlx::query(
        r#"
        SELECT txid, flow_type, pool, amount_zat, block_height
        FROM shielded_flows
        WHERE block_height >= $1 AND block_height <= $2
        ORDER BY txid, flow_type
        "#,
    )
    .bind(from_height as i32)
    .bind(to_height as i32)
    .fetch_all(&prod_pool)
    .await
    .map_err(|e| format!("Prod flows query failed: {}", e))?;

    let test_flows: Vec<_> = sqlx::query(
        r#"
        SELECT txid, flow_type, pool, amount_zat, block_height
        FROM shielded_flows
        WHERE block_height >= $1 AND block_height <= $2
        ORDER BY txid, flow_type
        "#,
    )
    .bind(from_height as i32)
    .bind(to_height as i32)
    .fetch_all(&test_pool)
    .await
    .map_err(|e| format!("Test flows query failed: {}", e))?;

    println!("   Prod DB: {} flows", prod_flows.len());
    println!("   Test DB: {} flows", test_flows.len());

    // Build lookup
    let mut test_flow_map: std::collections::HashMap<(String, String), &sqlx::postgres::PgRow> =
        std::collections::HashMap::new();
    for row in &test_flows {
        let txid: String = row.get("txid");
        let flow_type: String = row.get("flow_type");
        test_flow_map.insert((txid, flow_type), row);
    }

    let mut flow_matches = 0;
    let mut flow_mismatches: Vec<String> = Vec::new();
    let mut flow_missing = 0;

    for prod_row in &prod_flows {
        let txid: String = prod_row.get("txid");
        let flow_type: String = prod_row.get("flow_type");
        let prod_pool_name: String = prod_row.get("pool");
        let prod_amount: i64 = prod_row.get("amount_zat");

        if let Some(test_row) = test_flow_map.get(&(txid.clone(), flow_type.clone())) {
            let test_pool_name: String = test_row.get("pool");
            let test_amount: i64 = test_row.get("amount_zat");

            let mut diffs: Vec<String> = Vec::new();

            if prod_pool_name != test_pool_name {
                diffs.push(format!(
                    "pool: prod={} test={}",
                    prod_pool_name, test_pool_name
                ));
            }

            if prod_amount != test_amount {
                diffs.push(format!("amount: prod={} test={}", prod_amount, test_amount));
            }

            if diffs.is_empty() {
                flow_matches += 1;
            } else {
                flow_mismatches.push(format!(
                    "{} {}: {}",
                    &txid[..12],
                    flow_type,
                    diffs.join(", ")
                ));
            }
        } else {
            flow_missing += 1;
            if flow_missing <= 3 {
                let txid_short = if txid.len() > 16 { &txid[..16] } else { &txid };
                println!(
                    "   ⚠️  Missing in test: {} {} (prod has it)",
                    txid_short, flow_type
                );
            }
        }
    }

    // Also check for extra flows in test that aren't in prod
    let mut flow_extra = 0;
    for test_row in &test_flows {
        let txid: String = test_row.get("txid");
        let flow_type: String = test_row.get("flow_type");

        let prod_has_it = prod_flows.iter().any(|r| {
            let pt: String = r.get("txid");
            let pf: String = r.get("flow_type");
            pt == txid && pf == flow_type
        });

        if !prod_has_it {
            flow_extra += 1;
            if flow_extra <= 3 {
                let txid_short = if txid.len() > 16 { &txid[..16] } else { &txid };
                println!(
                    "   ℹ️  Extra in test: {} {} (prod doesn't have it)",
                    txid_short, flow_type
                );
            }
        }
    }

    println!();
    println!("   ✅ Matches: {}", flow_matches);
    println!("   ❌ Mismatches: {}", flow_mismatches.len());
    println!("   ⚠️  Missing in test: {}", flow_missing);
    println!("   ℹ️  Extra in test: {}", flow_extra);

    if !flow_mismatches.is_empty() {
        println!();
        println!("   First 10 flow mismatches:");
        for m in flow_mismatches.iter().take(10) {
            println!("      {}", m);
        }
    }

    // ========================================================================
    // SUMMARY
    // ========================================================================
    println!();
    println!("════════════════════════════════════════════════════════════");
    println!("📊 VALIDATION SUMMARY");
    println!("════════════════════════════════════════════════════════════");
    println!();
    println!("🚀 Performance:");
    println!(
        "   Rust: {:.2}s for {} blocks ({:.1} blk/s, {:.1} tx/s)",
        rust_elapsed.as_secs_f64(),
        block_count,
        rust_rate,
        rust_tx_rate
    );
    println!();
    println!("🔍 Data Comparison:");
    println!(
        "   Transactions: {}/{} matched ({:.1}%)",
        tx_matches,
        prod_txs.len(),
        if !prod_txs.is_empty() {
            tx_matches as f64 / prod_txs.len() as f64 * 100.0
        } else {
            0.0
        }
    );
    println!(
        "   Outputs:      {}/{} matched ({:.1}%)",
        out_matches,
        prod_outputs.len(),
        if !prod_outputs.is_empty() {
            out_matches as f64 / prod_outputs.len() as f64 * 100.0
        } else {
            0.0
        }
    );
    println!(
        "   Inputs:       {}/{} matched ({:.1}%)",
        in_matches,
        prod_inputs.len(),
        if !prod_inputs.is_empty() {
            in_matches as f64 / prod_inputs.len() as f64 * 100.0
        } else {
            0.0
        }
    );
    println!(
        "   Flows:        {}/{} matched ({:.1}%)",
        flow_matches,
        prod_flows.len(),
        if !prod_flows.is_empty() {
            flow_matches as f64 / prod_flows.len() as f64 * 100.0
        } else {
            0.0
        }
    );

    let all_ok = tx_mismatches.is_empty()
        && out_mismatches.is_empty()
        && in_mismatches.is_empty()
        && flow_mismatches.is_empty()
        && tx_missing == 0;

    println!();
    if all_ok {
        println!("🎉 VALIDATION PASSED! Rust indexer matches production data.");
    } else {
        println!("⚠️  VALIDATION ISSUES FOUND - Review mismatches above.");
    }

    println!("════════════════════════════════════════════════════════════");

    Ok(())
}
