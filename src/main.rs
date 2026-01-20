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
