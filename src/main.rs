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

/// Show a specific block
fn show_block(config: &Config, height: u32) -> Result<(), String> {
    let zebra = ZebraState::open(config)?;

    let hash = zebra.get_block_hash(height)?;
    let mut hash_rev = hash;
    hash_rev.reverse();

    println!("📦 Block {}", height);
    println!("────────────────────────────────────────────────────────────");
    println!("   Hash: {}", hex::encode(&hash_rev));
    println!();

    // TODO: Show more block details

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
    let cookie = std::fs::read_to_string(cookie_file)
        .map_err(|e| format!("Failed to read cookie file: {}", e))?;
    let auth = BASE64.encode(format!("__cookie__:{}", cookie.trim()));
    println!("   Auth: __cookie__:{}...", &cookie.trim()[..10.min(cookie.len())]);
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
    let rpc_response = client
        .post(rpc_url)
        .header("Content-Type", "application/json")
        .header("Authorization", format!("Basic {}", auth))
        .json(&json!({
            "jsonrpc": "1.0",
            "id": "verify",
            "method": "getblock",
            "params": [block_hash, 2]  // verbosity 2 = include decoded txs
        }))
        .send()
        .await
        .map_err(|e| format!("RPC request failed: {}", e))?;

    let rpc_json: Value = rpc_response
        .json()
        .await
        .map_err(|e| format!("RPC response parse failed: {}", e))?;

    if let Some(error) = rpc_json.get("error") {
        return Err(format!("RPC error: {:?}", error));
    }

    let block = &rpc_json["result"];
    let tx_count = block["tx"].as_array().map(|a| a.len()).unwrap_or(0);

    println!("   Block {} has {} transactions", height, tx_count);
    println!();

    // Show first few transactions from RPC
    if let Some(txs) = block["tx"].as_array() {
        for (i, tx) in txs.iter().take(3).enumerate() {
            let txid = tx["txid"].as_str().unwrap_or("?");
            let version = tx["version"].as_i64().unwrap_or(0);
            let vin_count = tx["vin"].as_array().map(|a| a.len()).unwrap_or(0);
            let vout_count = tx["vout"].as_array().map(|a| a.len()).unwrap_or(0);

            // Shielded counts
            let vjoinsplit = tx["vjoinsplit"].as_array().map(|a| a.len()).unwrap_or(0);
            let vshielded_spend = tx["vShieldedSpend"].as_array().map(|a| a.len()).unwrap_or(0);
            let vshielded_output = tx["vShieldedOutput"].as_array().map(|a| a.len()).unwrap_or(0);
            let orchard_actions = tx["orchard"]["actions"].as_array().map(|a| a.len()).unwrap_or(0);

            // Value balances
            let value_balance = tx["valueBalance"].as_f64().unwrap_or(0.0);
            let orchard_balance = tx["orchard"]["valueBalance"].as_f64().unwrap_or(0.0);

            println!("   TX {}: {}", i, &txid[..16]);
            println!("      Version: v{}", version);
            println!("      Transparent: {} vin, {} vout", vin_count, vout_count);
            println!("      Sprout: {} joinsplits", vjoinsplit);
            println!("      Sapling: {} spends, {} outputs, balance: {:.8} ZEC",
                vshielded_spend, vshielded_output, value_balance);
            println!("      Orchard: {} actions, balance: {:.8} ZEC",
                orchard_actions, orchard_balance);

            // Try to get from RocksDB
            match zebra.get_tx_hash_by_loc(height, i as u16) {
                Ok(hash) => {
                    let mut rev = hash;
                    rev.reverse();
                    let rocks_txid = hex::encode(&rev);
                    if rocks_txid == txid {
                        println!("      ✅ RocksDB txid matches");
                    } else {
                        println!("      ❌ RocksDB txid mismatch: {}", &rocks_txid[..16]);
                    }
                }
                Err(e) => {
                    println!("      ⚠️  RocksDB: {}", e);
                }
            }

            // Try to get raw tx
            match zebra.get_transaction_by_loc(height, i as u16) {
                Ok(raw) => {
                    println!("      📦 Raw tx: {} bytes", raw.len());

                    // Parse header
                    if raw.len() >= 4 {
                        let header = u32::from_le_bytes([raw[0], raw[1], raw[2], raw[3]]);
                        let parsed_version = (header & 0x7FFFFFFF) as i32;
                        let overwintered = (header >> 31) == 1;
                        println!("      📋 Parsed: v{} overwintered={}", parsed_version, overwintered);

                        if parsed_version as i64 == version {
                            println!("      ✅ Version matches!");
                        } else {
                            println!("      ❌ Version mismatch (RPC says v{})", version);
                        }
                    }
                }
                Err(e) => {
                    println!("      ⚠️  Raw tx: {}", e);
                }
            }

            println!();
        }
    }

    println!("════════════════════════════════════════════════════════════");

    Ok(())
}
