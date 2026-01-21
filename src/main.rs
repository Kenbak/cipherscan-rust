//! CipherScan Rust Indexer
//!
//! Fast indexer that reads directly from Zebra's RocksDB state database.
//! ~100-1000x faster than JSON-RPC for backfills.
//!
//! Usage:
//!   cargo run --release -- analyze      # Analyze database structure
//!   cargo run --release -- backfill     # Index from start to tip
//!   cargo run --release -- live         # Follow chain tip
//!   cargo run --release -- status       # Show indexer status

mod config;
mod db;
mod models;
mod indexer;

use clap::{Parser, Subcommand};
use std::time::Instant;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

use crate::config::Config;
use crate::db::ZebraState;

/// CipherScan Rust Indexer - High-performance Zcash blockchain indexer
#[derive(Parser)]
#[command(name = "cipherscan-indexer")]
#[command(version = "0.1.0")]
#[command(about = "Fast Zcash indexer reading directly from Zebra's RocksDB")]
struct Cli {
    /// Path to Zebra state directory
    #[arg(long, env = "ZEBRA_STATE_PATH")]
    zebra_path: Option<String>,

    /// PostgreSQL connection URL
    #[arg(long, env = "DATABASE_URL")]
    database_url: Option<String>,

    /// Batch size for database operations
    #[arg(long, default_value = "1000")]
    batch_size: usize,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Analyze Zebra's RocksDB structure
    Analyze,

    /// Run backfill from genesis (or checkpoint) to current tip
    Backfill {
        /// Start from specific height (overrides checkpoint)
        #[arg(long)]
        from: Option<u32>,

        /// Stop at specific height
        #[arg(long)]
        to: Option<u32>,
    },

    /// Run live indexer (follow chain tip)
    Live,

    /// Show indexer status
    Status,

    /// Decode and show specific block
    Block {
        /// Block height to show
        height: u32,
    },

    /// Verify parsing by comparing RocksDB data with RPC
    Verify {
        /// Block height to verify
        #[arg(long, default_value = "1000000")]
        height: u32,

        /// Number of blocks to verify
        #[arg(long, default_value = "10")]
        count: u32,

        /// Zebra RPC URL
        #[arg(long, env = "ZEBRA_RPC_URL", default_value = "http://127.0.0.1:8232")]
        rpc_url: String,

        /// Cookie file path for auth
        #[arg(long, env = "ZEBRA_RPC_COOKIE_FILE", default_value = "/root/.cache/zebra/.cookie")]
        cookie_file: String,
    },

    /// Parse and display a transaction from RocksDB
    Tx {
        /// Block height
        height: u32,

        /// Transaction index within block
        #[arg(default_value = "0")]
        index: u16,
    },
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Load .env if present
    let _ = dotenvy::dotenv();

    // Initialize tracing
    tracing_subscriber::registry()
        .with(tracing_subscriber::fmt::layer())
        .with(tracing_subscriber::EnvFilter::from_default_env()
            .add_directive("cipherscan_indexer=info".parse()?))
        .init();

    let cli = Cli::parse();

    // Build config
    let mut config = Config::from_env();
    if let Some(path) = cli.zebra_path {
        config.zebra_state_path = path.into();
    }
    if let Some(url) = cli.database_url {
        config.database_url = url;
    }
    config.batch_size = cli.batch_size;

    println!("════════════════════════════════════════════════════════════");
    println!("🚀 CipherScan Rust Indexer v0.1.0");
    println!("════════════════════════════════════════════════════════════");
    println!("📂 Zebra state: {:?}", config.zebra_state_path);
    println!("🌐 Network: {}", config.network_name());
    println!();

    match cli.command {
        Commands::Analyze => {
            analyze_database(&config)?;
        }
        Commands::Backfill { from, to } => {
            run_backfill(&config, from, to).await?;
        }
        Commands::Live => {
            run_live(&config).await?;
        }
        Commands::Status => {
            show_status(&config).await?;
        }
        Commands::Block { height } => {
            show_block(&config, height)?;
        }
        Commands::Verify { height, count, rpc_url, cookie_file } => {
            verify_parsing(&config, height, count, &rpc_url, &cookie_file).await?;
        }
        Commands::Tx { height, index } => {
            show_transaction(&config, height, index)?;
        }
    }

    Ok(())
}

/// Analyze database structure (original PoC functionality)
fn analyze_database(config: &Config) -> Result<(), String> {
    use rocksdb::{DB, Options, IteratorMode};

    let path = &config.zebra_state_path;

    // List column families
    println!("🔍 Listing column families...");
    let cf_names = DB::list_cf(&Options::default(), path)
        .map_err(|e| format!("Failed to list CFs: {}", e))?;

    println!("   Found {} column families:", cf_names.len());
    for cf in &cf_names {
        println!("      - {}", cf);
    }

    // Open with column families
    let mut opts = Options::default();
    opts.set_error_if_exists(false);
    opts.create_if_missing(false);
    opts.set_max_open_files(config.max_open_files);

    println!("\n🔓 Opening RocksDB with column families (read-only)...");
    let start = Instant::now();

    let db = DB::open_cf_for_read_only(&opts, path, &cf_names, false)
        .map_err(|e| format!("Failed to open RocksDB: {}", e))?;

    println!("✅ RocksDB opened in {:?}", start.elapsed());
    println!("\n📊 Analyzing column families...");
    println!("────────────────────────────────────────────────────────────");

    for cf_name in &cf_names {
        if let Some(cf) = db.cf_handle(cf_name.as_str()) {
            let iter = db.iterator_cf(cf, IteratorMode::Start);
            let mut count = 0;
            let mut sample_key: Option<String> = None;

            for item in iter {
                match item {
                    Ok((key, _value)) => {
                        count += 1;
                        if sample_key.is_none() && !key.is_empty() {
                            sample_key = Some(hex::encode(&key[..std::cmp::min(16, key.len())]));
                        }
                        if count >= 100000 {
                            break;
                        }
                    }
                    Err(_) => break,
                }
            }

            let sample = sample_key.unwrap_or_else(|| "N/A".to_string());
            if count > 0 {
                println!("   ✅ {:35} → {:>7} entries (sample: {}...)",
                    cf_name, count, &sample[..std::cmp::min(12, sample.len())]);
            } else {
                println!("   ⬚ {:35} → empty", cf_name);
            }
        }
    }

    // Show chain tip
    println!();
    if let Some(cf) = db.cf_handle("hash_by_height") {
        let mut last_height = 0u32;
        for item in db.iterator_cf(cf, IteratorMode::End) {
            if let Ok((key, _)) = item {
                if key.len() >= 3 {
                    last_height = ((key[0] as u32) << 16) | ((key[1] as u32) << 8) | (key[2] as u32);
                }
                break;
            }
        }
        println!("📈 Chain tip height: {}", last_height);
    }

    println!("\n════════════════════════════════════════════════════════════");
    println!("✅ Analysis complete!");
    println!("════════════════════════════════════════════════════════════");

    Ok(())
}

/// Run backfill indexer
async fn run_backfill(config: &Config, from: Option<u32>, to: Option<u32>) -> Result<(), String> {
    let zebra = ZebraState::open(config)?;

    let tip = zebra.get_tip_height()?;
    let start_height = from.unwrap_or(0);
    let end_height = to.unwrap_or(tip);

    println!("📊 Backfill: {} → {} ({} blocks)",
        start_height, end_height, end_height - start_height + 1);
    println!();

    let batch_size = config.batch_size;
    let mut current = start_height;
    let overall_start = Instant::now();
    let mut total_blocks = 0u64;

    while current <= end_height {
        let batch_end = std::cmp::min(current + batch_size as u32 - 1, end_height);
        let batch_start = Instant::now();

        let mut blocks_in_batch = 0u32;

        for result in zebra.iter_blocks(current, batch_end) {
            match result {
                Ok((height, _hash)) => {
                    blocks_in_batch += 1;
                    total_blocks += 1;
                }
                Err(e) => {
                    tracing::warn!("Error at {}: {}", current, e);
                }
            }
        }

        let elapsed = batch_start.elapsed();
        let rate = blocks_in_batch as f64 / elapsed.as_secs_f64();
        let total_elapsed = overall_start.elapsed();
        let overall_rate = total_blocks as f64 / total_elapsed.as_secs_f64();
        let remaining = (end_height - batch_end) as f64 / overall_rate;
        let progress = (batch_end - start_height) as f64 / (end_height - start_height) as f64 * 100.0;

        println!("📦 {} → {} | {:.1}% | {:.0} blk/s | ETA: {:.1}h",
            current, batch_end, progress, rate, remaining / 3600.0);

        current = batch_end + 1;
    }

    let total_time = overall_start.elapsed();
    println!();
    println!("════════════════════════════════════════════════════════════");
    println!("✅ Backfill complete!");
    println!("   Total blocks: {}", total_blocks);
    println!("   Total time: {:?}", total_time);
    println!("   Average rate: {:.0} blocks/sec", total_blocks as f64 / total_time.as_secs_f64());
    println!("════════════════════════════════════════════════════════════");

    Ok(())
}

/// Run live indexer
async fn run_live(config: &Config) -> Result<(), String> {
    let zebra = ZebraState::open(config)?;

    println!("🔄 Starting live indexer (Ctrl+C to stop)...");
    println!();

    let mut last_height = zebra.get_tip_height()?;
    println!("📈 Starting at height: {}", last_height);

    loop {
        tokio::time::sleep(tokio::time::Duration::from_secs(10)).await;

        let current_tip = zebra.get_tip_height()?;

        if current_tip > last_height {
            let new_blocks = current_tip - last_height;
            println!("📦 New blocks: {} → {} (+{})", last_height + 1, current_tip, new_blocks);

            for result in zebra.iter_blocks(last_height + 1, current_tip) {
                match result {
                    Ok((height, hash)) => {
                        let mut hash_rev = hash;
                        hash_rev.reverse();
                        tracing::debug!("Block {}: {}", height, hex::encode(&hash_rev[..8]));
                    }
                    Err(e) => {
                        tracing::error!("Error: {}", e);
                    }
                }
            }

            last_height = current_tip;
        }
    }
}

/// Show indexer status
async fn show_status(config: &Config) -> Result<(), String> {
    let zebra = ZebraState::open(config)?;
    let stats = zebra.get_stats();

    println!("📊 Indexer Status");
    println!("────────────────────────────────────────────────────────────");
    println!("   Network:     {}", stats.network);
    println!("   Chain tip:   {}", stats.tip_height);
    println!("   Block count: {}", stats.block_count);
    println!();

    // TODO: Show PostgreSQL status when connected

    println!("════════════════════════════════════════════════════════════");

    Ok(())
}

/// Show a specific block with all its transactions
fn show_block(config: &Config, height: u32) -> Result<(), String> {
    use crate::indexer::TransactionParser;

    let zebra = ZebraState::open(config)?;

    let hash = zebra.get_block_hash(height)?;
    let mut hash_rev = hash;
    hash_rev.reverse();
    let block_hash = hex::encode(&hash_rev);

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
        match TransactionParser::parse(raw, height, &block_hash) {
            Ok(tx) => {
                total_transparent_out += tx.transparent_value_out;
                total_orchard_actions += tx.orchard_actions as u32;
                total_sapling_spends += tx.sapling_spends as u32;
                total_sapling_outputs += tx.sapling_outputs as u32;

                // Brief summary line
                let shielded = if tx.orchard_actions > 0 || tx.sapling_spends > 0 || tx.sapling_outputs > 0 {
                    format!("🔒O:{} S:{}/{}",
                        tx.orchard_actions,
                        tx.sapling_spends,
                        tx.sapling_outputs
                    )
                } else {
                    "".to_string()
                };

                println!("   [{:3}] {} v{} | {} vout | {:.4} ZEC {}",
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
    println!("      Transparent value: {:.8} ZEC", total_transparent_out as f64 / 100_000_000.0);
    println!("      Orchard actions:   {}", total_orchard_actions);
    println!("      Sapling spends:    {}", total_sapling_spends);
    println!("      Sapling outputs:   {}", total_sapling_outputs);
    println!();

    Ok(())
}

/// Show a specific transaction parsed from RocksDB
fn show_transaction(config: &Config, height: u32, index: u16) -> Result<(), String> {
    use crate::indexer::TransactionParser;

    let zebra = ZebraState::open(config)?;

    // Get block hash
    let block_hash = {
        let mut h = zebra.get_block_hash(height)?;
        h.reverse();
        hex::encode(&h)
    };

    // Get raw transaction
    let raw = zebra.get_transaction_by_loc(height, index)?;

    println!("📋 Transaction at {}:{}", height, index);
    println!("────────────────────────────────────────────────────────────");
    println!("   Raw size: {} bytes", raw.len());
    println!();

    // Parse using zebra-chain
    match TransactionParser::parse(&raw, height, &block_hash) {
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
            println!("   💰 Value out: {} ZEC", tx.transparent_value_out as f64 / 100_000_000.0);
            println!();
            println!("   🔒 Shielded:");
            println!("      Sprout JoinSplits: {}", tx.joinsplit_count);
            println!("      Sapling Spends:    {}", tx.sapling_spends);
            println!("      Sapling Outputs:   {}", tx.sapling_outputs);
            println!("      Orchard Actions:   {}", tx.orchard_actions);
            println!();
            println!("   💱 Value Balances:");
            println!("      Sapling: {} ZEC", tx.sapling_value_balance as f64 / 100_000_000.0);
            println!("      Orchard: {} ZEC", tx.orchard_value_balance as f64 / 100_000_000.0);

            // Show transparent outputs
            if !tx.vout.is_empty() {
                println!();
                println!("   📤 Outputs:");
                for vout in &tx.vout {
                    let addr = vout.address.as_deref().unwrap_or("(unknown)");
                    println!("      [{}] {} ZEC → {}",
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

/// Verify parsing by comparing RocksDB data with Zebra RPC
async fn verify_parsing(config: &Config, start_height: u32, count: u32, rpc_url: &str, cookie_file: &str) -> Result<(), String> {
    use serde_json::{json, Value};
    use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};

    println!("🔍 Verifying RocksDB parsing against RPC...");
    println!("   RPC URL: {}", rpc_url);
    println!("   Cookie file: {}", cookie_file);
    println!("   Heights: {} to {}", start_height, start_height + count - 1);
    println!("────────────────────────────────────────────────────────────");

    // Read cookie for auth
    // Cookie file format: "__cookie__:password" (Zebra style)
    let cookie_content = std::fs::read_to_string(cookie_file)
        .map_err(|e| format!("Failed to read cookie file: {}", e))?;
    let cookie_trimmed = cookie_content.trim();

    // Use cookie content directly (already has __cookie__:password format)
    let auth = BASE64.encode(cookie_trimmed);
    println!("   Auth: {}...{}", &cookie_trimmed[..15], &cookie_trimmed[cookie_trimmed.len()-5..]);
    println!();

    let zebra = ZebraState::open(config)?;
    let client = reqwest::Client::new();

    let mut matches = 0;
    let mut mismatches = 0;

    for height in start_height..start_height + count {
        // Get hash from RocksDB
        let rocks_hash = match zebra.get_block_hash(height) {
            Ok(h) => {
                let mut rev = h;
                rev.reverse();
                hex::encode(&rev)
            }
            Err(e) => {
                println!("   ❌ Height {}: RocksDB error - {}", height, e);
                mismatches += 1;
                continue;
            }
        };

        // Get hash from RPC
        let rpc_response = client
            .post(rpc_url)
            .header("Content-Type", "application/json")
            .header("Authorization", format!("Basic {}", auth))
            .json(&json!({
                "jsonrpc": "1.0",
                "id": "verify",
                "method": "getblockhash",
                "params": [height]
            }))
            .send()
            .await
            .map_err(|e| format!("RPC request failed: {}", e))?;

        let rpc_json: Value = rpc_response
            .json()
            .await
            .map_err(|e| format!("RPC response parse failed: {}", e))?;

        let rpc_hash = rpc_json["result"]
            .as_str()
            .unwrap_or("")
            .to_string();

        if rocks_hash == rpc_hash {
            println!("   ✅ Height {:>8}: {}", height, &rocks_hash[..16]);
            matches += 1;
        } else {
            println!("   ❌ Height {:>8}: MISMATCH", height);
            println!("      RocksDB: {}", rocks_hash);
            println!("      RPC:     {}", rpc_hash);
            mismatches += 1;
        }
    }

    println!();
    println!("════════════════════════════════════════════════════════════");
    println!("📊 Verification Results:");
    println!("   ✅ Matches:    {}", matches);
    println!("   ❌ Mismatches: {}", mismatches);

    if mismatches == 0 {
        println!();
        println!("   🎉 All block hashes verified successfully!");
    }

    println!("════════════════════════════════════════════════════════════");

    // Now verify a transaction if we had matches
    if matches > 0 {
        println!();
        verify_transaction(&zebra, &client, rpc_url, &auth, start_height).await?;
    }

    Ok(())
}

/// Verify transaction parsing
async fn verify_transaction(
    zebra: &ZebraState,
    client: &reqwest::Client,
    rpc_url: &str,
    auth: &str,
    height: u32,
) -> Result<(), String> {
    use serde_json::{json, Value};

    println!("🔍 Verifying transaction parsing at height {}...", height);
    println!("────────────────────────────────────────────────────────────");

    // Get block hash
    let block_hash = {
        let mut h = zebra.get_block_hash(height)?;
        h.reverse();
        hex::encode(&h)
    };

    // Get block from RPC to see transactions
    // Zebra uses verbosity 1 for decoded txs (not 2 like zcashd)
    let rpc_response = client
        .post(rpc_url)
        .header("Content-Type", "application/json")
        .header("Authorization", format!("Basic {}", auth))
        .json(&json!({
            "jsonrpc": "1.0",
            "id": "verify",
            "method": "getblock",
            "params": [block_hash, 1]  // verbosity 1 = include decoded txs in Zebra
        }))
        .send()
        .await
        .map_err(|e| format!("RPC request failed: {}", e))?;

    let rpc_json: Value = rpc_response
        .json()
        .await
        .map_err(|e| format!("RPC response parse failed: {}", e))?;

    // Check for errors (null means not found or other issue)
    if let Some(error) = rpc_json.get("error") {
        if !error.is_null() {
            println!("   ⚠️  RPC error, trying with verbosity 0...");
            // Fallback to simpler block info
            return verify_transaction_simple(zebra, client, rpc_url, auth, height, &block_hash).await;
        }
    }

    let block = &rpc_json["result"];
    if block.is_null() {
        println!("   ⚠️  Block data null, trying with verbosity 0...");
        return verify_transaction_simple(zebra, client, rpc_url, auth, height, &block_hash).await;
    }

    let tx_count = block["tx"].as_array().map(|a| a.len()).unwrap_or(0);

    println!("   Block {} has {} transactions", height, tx_count);
    println!();

    // Show first few transactions from RPC
    // Zebra verbosity 1 returns tx as array of txid strings, not objects
    if let Some(txs) = block["tx"].as_array() {
        for (i, tx) in txs.iter().take(3).enumerate() {
            // Zebra returns txid as string directly, zcashd returns object with txid field
            let rpc_txid = tx.as_str()
                .or_else(|| tx["txid"].as_str())
                .unwrap_or("?");

            let rpc_txid_short = if rpc_txid.len() > 16 { &rpc_txid[..16] } else { rpc_txid };
            println!("   TX {}: {} (RPC)", i, rpc_txid_short);

            // Get txid from RocksDB and compare
            match zebra.get_tx_hash_by_loc(height, i as u16) {
                Ok(hash) => {
                    let mut rev = hash;
                    rev.reverse();
                    let rocks_txid = hex::encode(&rev);
                    let rocks_short = if rocks_txid.len() > 16 { &rocks_txid[..16] } else { &rocks_txid };

                    if rocks_txid == rpc_txid {
                        println!("      ✅ RocksDB matches: {}", rocks_short);
                    } else {
                        println!("      ❌ MISMATCH!");
                        println!("         RPC:     {}", rpc_txid);
                        println!("         RocksDB: {}", rocks_txid);
                    }
                }
                Err(e) => {
                    println!("      ⚠️  RocksDB: {}", e);
                }
            }

            // Try to get raw tx and show parsed info
            match zebra.get_transaction_by_loc(height, i as u16) {
                Ok(raw) => {
                    // Parse header
                    if raw.len() >= 4 {
                        let header = u32::from_le_bytes([raw[0], raw[1], raw[2], raw[3]]);
                        let parsed_version = (header & 0x7FFFFFFF) as i32;
                        let overwintered = (header >> 31) == 1;
                        println!("      📋 {} bytes, v{}, overwintered={}", raw.len(), parsed_version, overwintered);
                    } else {
                        println!("      📋 {} bytes", raw.len());
                    }
                }
                Err(e) => {
                    println!("      ⚠️  Raw tx error: {}", e);
                }
            }

            println!();
        }
    }

    println!("════════════════════════════════════════════════════════════");

    Ok(())
}

/// Simple transaction verification fallback (verbosity 0)
async fn verify_transaction_simple(
    zebra: &ZebraState,
    client: &reqwest::Client,
    rpc_url: &str,
    auth: &str,
    height: u32,
    block_hash: &str,
) -> Result<(), String> {
    use serde_json::{json, Value};

    // Get block with verbosity 0 (just tx hashes)
    let rpc_response = client
        .post(rpc_url)
        .header("Content-Type", "application/json")
        .header("Authorization", format!("Basic {}", auth))
        .json(&json!({
            "jsonrpc": "1.0",
            "id": "verify",
            "method": "getblock",
            "params": [block_hash, 0]
        }))
        .send()
        .await
        .map_err(|e| format!("RPC request failed: {}", e))?;

    let rpc_json: Value = rpc_response
        .json()
        .await
        .map_err(|e| format!("RPC response parse failed: {}", e))?;

    if let Some(error) = rpc_json.get("error") {
        if !error.is_null() {
            return Err(format!("RPC error: {:?}", error));
        }
    }

    let block = &rpc_json["result"];

    // With verbosity 0, result is just a hex string of the block
    if let Some(hex_data) = block.as_str() {
        println!("   📦 Block data: {} bytes (hex)", hex_data.len() / 2);

        // Try to get first transaction from RocksDB
        for i in 0..3u16 {
            match zebra.get_tx_hash_by_loc(height, i) {
                Ok(hash) => {
                    let mut rev = hash;
                    rev.reverse();
                    println!("   TX {}: {} (from RocksDB)", i, hex::encode(&rev));
                }
                Err(_) => break,
            }
        }
    } else if let Some(txs) = block["tx"].as_array() {
        println!("   Block {} has {} transactions", height, txs.len());

        for (i, tx) in txs.iter().take(3).enumerate() {
            let txid = tx.as_str().unwrap_or("?");
            let txid_short = if txid.len() > 16 { &txid[..16] } else { txid };
            println!("   TX {}: {} (from RPC)", i, txid_short);

            // Compare with RocksDB
            match zebra.get_tx_hash_by_loc(height, i as u16) {
                Ok(hash) => {
                    let mut rev = hash;
                    rev.reverse();
                    let rocks_txid = hex::encode(&rev);
                    if rocks_txid == txid {
                        println!("      ✅ Matches RocksDB");
                    } else {
                        let rocks_short = if rocks_txid.len() > 16 { &rocks_txid[..16] } else { &rocks_txid };
                        println!("      ❌ RocksDB has: {}", rocks_short);
                    }
                }
                Err(e) => {
                    println!("      ⚠️  RocksDB: {}", e);
                }
            }
        }
    }

    println!();
    println!("════════════════════════════════════════════════════════════");

    Ok(())
}
