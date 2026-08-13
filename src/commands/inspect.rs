use crate::config::Config;
use crate::db::ZebraState;

/// Show a specific block with all its transactions
pub(crate) fn show_block(config: &Config, height: u32) -> Result<(), String> {
    use crate::indexer::TransactionParser;

    let zebra = ZebraState::open(config)?;

    let hash = zebra.get_block_hash(height)?;
    let block_hash = crate::util::display_hash(&hash);

    println!("📦 Block {}", height);
    println!("────────────────────────────────────────────────────────────");
    println!("   Hash: {}", block_hash);

    // Get all transactions in block
    let transactions = zebra.iter_block_transactions(height)?;
    println!("   Transactions: {}", transactions.len());
    println!();

    // Summary counters
    let mut total_transparent_out: i64 = 0;
    let mut total_orchard_actions: u32 = 0;
    let mut total_sapling_spends: u32 = 0;
    let mut total_sapling_outputs: u32 = 0;

    println!("   📋 Transaction Summary:");
    println!("   ─────────────────────────────────────────────────────────");

    for (idx, raw) in &transactions {
        match TransactionParser::parse(raw, height, &block_hash, config.network) {
            Ok(tx) => {
                total_transparent_out += tx.transparent_value_out;
                total_orchard_actions += tx.orchard_actions as u32;
                total_sapling_spends += tx.sapling_spends as u32;
                total_sapling_outputs += tx.sapling_outputs as u32;

                // Brief summary line
                let shielded =
                    if tx.orchard_actions > 0 || tx.sapling_spends > 0 || tx.sapling_outputs > 0 {
                        format!(
                            "🔒O:{} S:{}/{}",
                            tx.orchard_actions, tx.sapling_spends, tx.sapling_outputs
                        )
                    } else {
                        "".to_string()
                    };

                println!(
                    "   [{:3}] {} v{} | {} vout | {:.4} ZEC {}",
                    idx,
                    &tx.txid[..16],
                    tx.version,
                    tx.vout_count,
                    tx.transparent_value_out as f64 / 100_000_000.0,
                    shielded
                );
            }
            Err(e) => {
                println!("   [{:3}] ❌ Parse error: {}", idx, e);
            }
        }
    }

    println!();
    println!("   📊 Block Totals:");
    println!(
        "      Transparent value: {:.8} ZEC",
        total_transparent_out as f64 / 100_000_000.0
    );
    println!("      Orchard actions:   {}", total_orchard_actions);
    println!("      Sapling spends:    {}", total_sapling_spends);
    println!("      Sapling outputs:   {}", total_sapling_outputs);
    println!();

    Ok(())
}

/// Show a specific transaction parsed from RocksDB
pub(crate) fn show_transaction(config: &Config, height: u32, index: u16) -> Result<(), String> {
    use crate::indexer::TransactionParser;

    let zebra = ZebraState::open(config)?;

    // Get block hash
    let block_hash = { crate::util::display_hash(&zebra.get_block_hash(height)?) };

    // Get raw transaction
    let raw = zebra.get_transaction_by_loc(height, index)?;

    println!("📋 Transaction at {}:{}", height, index);
    println!("────────────────────────────────────────────────────────────");
    println!("   Raw size: {} bytes", raw.len());
    println!();

    // Parse using zebra-chain
    match TransactionParser::parse(&raw, height, &block_hash, config.network) {
        Ok(tx) => {
            println!("   ✅ Parsed successfully!");
            println!();
            println!("   TXID:       {}", tx.txid);
            println!("   Version:    v{}", tx.version);
            println!("   Lock time:  {}", tx.lock_time);
            if let Some(exp) = tx.expiry_height {
                println!("   Expiry:     {}", exp);
            }
            println!();
            println!("   📥 Transparent Inputs:  {}", tx.vin_count);
            println!("   📤 Transparent Outputs: {}", tx.vout_count);
            println!(
                "   💰 Value out: {} ZEC",
                tx.transparent_value_out as f64 / 100_000_000.0
            );
            println!();
            println!("   🔒 Shielded:");
            println!("      Sprout JoinSplits: {}", tx.joinsplit_count);
            println!("      Sapling Spends:    {}", tx.sapling_spends);
            println!("      Sapling Outputs:   {}", tx.sapling_outputs);
            println!("      Orchard Actions:   {}", tx.orchard_actions);
            println!();
            println!("   💱 Value Balances:");
            println!(
                "      Sapling: {} ZEC",
                tx.sapling_value_balance as f64 / 100_000_000.0
            );
            println!(
                "      Orchard: {} ZEC",
                tx.orchard_value_balance as f64 / 100_000_000.0
            );

            // Show transparent outputs
            if !tx.vout.is_empty() {
                println!();
                println!("   📤 Outputs:");
                for vout in &tx.vout {
                    let addr = vout.address.as_deref().unwrap_or("(unknown)");
                    println!(
                        "      [{}] {} ZEC → {}",
                        vout.n,
                        vout.value as f64 / 100_000_000.0,
                        addr
                    );
                }
            }
        }
        Err(e) => {
            println!("   ❌ Parse error: {}", e);

            // Show raw header for debugging
            if raw.len() >= 4 {
                let header = u32::from_le_bytes([raw[0], raw[1], raw[2], raw[3]]);
                let version = (header & 0x7FFFFFFF) as i32;
                let overwintered = (header >> 31) == 1;
                println!("   Header: v{}, overwintered={}", version, overwintered);
            }
        }
    }

    println!();
    println!("════════════════════════════════════════════════════════════");

    Ok(())
}
