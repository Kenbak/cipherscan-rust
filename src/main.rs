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
mod indexer;
mod models;

use clap::{Parser, Subcommand};
use serde::Serialize;
use std::time::{Instant, SystemTime, UNIX_EPOCH};
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
    Status {
        /// Emit machine-readable JSON
        #[arg(long)]
        json: bool,
    },

    /// Return non-zero when indexer health is degraded
    Health {
        /// Maximum acceptable lag behind the local chain tip
        #[arg(long, default_value = "3")]
        max_lag: u32,

        /// Maximum acceptable consecutive failures before unhealthy
        #[arg(long, default_value = "0")]
        max_consecutive_failures: u32,

        /// Maximum acceptable age for live heartbeat state in seconds
        #[arg(long, env = "INDEXER_MAX_HEARTBEAT_AGE_SECONDS", default_value = "600")]
        max_heartbeat_age: u64,

        /// Emit machine-readable JSON
        #[arg(long)]
        json: bool,
    },

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
        #[arg(
            long,
            env = "ZEBRA_RPC_COOKIE_FILE",
            default_value = "/root/.cache/zebra/.cookie"
        )]
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

    /// Compare Rust parsing with existing PostgreSQL data
    Compare {
        /// Number of transactions to sample
        #[arg(long, default_value = "50")]
        sample: usize,

        /// Start height for sampling
        #[arg(long, default_value = "3200000")]
        from_height: u32,

        /// PostgreSQL connection URL
        #[arg(long, env = "DATABASE_URL")]
        database_url: Option<String>,
    },

    /// Full validation: index into test DB, compare with prod, benchmark
    Validate {
        /// Production database URL
        #[arg(long, env = "DATABASE_URL")]
        prod_db: Option<String>,

        /// Test database URL (will be created/cleared)
        #[arg(long)]
        test_db: String,

        /// Start height for validation
        #[arg(long, default_value = "3200000")]
        from_height: u32,

        /// End height for validation
        #[arg(long, default_value = "3200100")]
        to_height: u32,
    },
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Load .env if present
    let _ = dotenvy::dotenv();

    // Initialize tracing
    tracing_subscriber::registry()
        .with(tracing_subscriber::fmt::layer())
        .with(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive("cipherscan_indexer=info".parse()?),
        )
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

    let suppress_banner = matches!(
        &cli.command,
        Commands::Status { json: true } | Commands::Health { json: true, .. }
    );

    if !suppress_banner {
        println!("════════════════════════════════════════════════════════════");
        println!("🚀 CipherScan Rust Indexer v0.1.0");
        println!("════════════════════════════════════════════════════════════");
        println!("📂 Zebra state: {:?}", config.zebra_state_path);
        println!("🌐 Network: {}", config.network_name());
        println!();
    }

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
        Commands::Status { json } => {
            show_status(&config, json).await?;
        }
        Commands::Health {
            max_lag,
            max_consecutive_failures,
            max_heartbeat_age,
            json,
        } => {
            check_health(
                &config,
                max_lag,
                max_consecutive_failures,
                max_heartbeat_age,
                json,
            )
            .await?;
        }
        Commands::Block { height } => {
            show_block(&config, height)?;
        }
        Commands::Verify {
            height,
            count,
            rpc_url,
            cookie_file,
        } => {
            verify_parsing(&config, height, count, &rpc_url, &cookie_file).await?;
        }
        Commands::Tx { height, index } => {
            show_transaction(&config, height, index)?;
        }
        Commands::Compare {
            sample,
            from_height,
            database_url,
        } => {
            let db_url = database_url.unwrap_or_else(|| config.database_url.clone());
            compare_with_postgres(&config, &db_url, sample, from_height).await?;
        }
        Commands::Validate {
            prod_db,
            test_db,
            from_height,
            to_height,
        } => {
            let prod_url = prod_db.unwrap_or_else(|| config.database_url.clone());
            validate_full(&config, &prod_url, &test_db, from_height, to_height).await?;
        }
    }

    Ok(())
}

/// Analyze database structure (original PoC functionality)
fn analyze_database(config: &Config) -> Result<(), String> {
    use rocksdb::{IteratorMode, Options, DB};

    let path = &config.zebra_state_path;

    // List column families
    println!("🔍 Listing column families...");
    let cf_names =
        DB::list_cf(&Options::default(), path).map_err(|e| format!("Failed to list CFs: {}", e))?;

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
                println!(
                    "   ✅ {:35} → {:>7} entries (sample: {}...)",
                    cf_name,
                    count,
                    &sample[..std::cmp::min(12, sample.len())]
                );
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
                    last_height =
                        ((key[0] as u32) << 16) | ((key[1] as u32) << 8) | (key[2] as u32);
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

/// Run backfill indexer (with PostgreSQL writes)
async fn run_backfill(config: &Config, from: Option<u32>, to: Option<u32>) -> Result<(), String> {
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

/// Run live indexer (with PostgreSQL writes)
async fn run_live(config: &Config) -> Result<(), String> {
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

    indexer.live().await
}

/// Show indexer status
#[derive(Debug, Serialize)]
struct FailureState {
    height: Option<u32>,
    mode: Option<String>,
    error: Option<String>,
    timestamp: Option<u64>,
    consecutive_failures: u32,
}

#[derive(Debug, Serialize)]
struct IndexerStatus {
    network: String,
    chain_tip: u32,
    chain_tip_source: String,
    block_count: u64,
    last_indexed_height: Option<u32>,
    backfill_height: Option<u32>,
    lag_blocks: Option<u32>,
    last_seen_rpc_tip: Option<u32>,
    last_tip_check_at: Option<u64>,
    last_success_at: Option<u64>,
    failure: FailureState,
}

#[derive(Debug)]
struct HealthAssessment {
    healthy: bool,
    reasons: Vec<String>,
}

fn parse_optional_u32(value: Option<String>) -> Option<u32> {
    value.and_then(|v| v.parse::<u32>().ok())
}

fn parse_optional_u64(value: Option<String>) -> Option<u64> {
    value.and_then(|v| v.parse::<u64>().ok())
}

fn unix_timestamp_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn assess_health(
    status: &IndexerStatus,
    max_lag: u32,
    max_consecutive_failures: u32,
    max_heartbeat_age: u64,
    now: u64,
) -> HealthAssessment {
    let mut reasons = Vec::new();

    if let Some(lag) = status.lag_blocks {
        if lag > max_lag {
            reasons.push(format!("lag {} exceeds threshold {}", lag, max_lag));
        }
    } else {
        reasons.push("live checkpoint missing".to_string());
    }

    if let Some(last_indexed) = status.last_indexed_height {
        if last_indexed > status.chain_tip {
            reasons.push(format!(
                "last indexed height {} exceeds chain tip {}",
                last_indexed, status.chain_tip
            ));
        }
    }

    if status.chain_tip_source != "rpc" {
        reasons.push(format!(
            "rpc tip unavailable; using {} fallback",
            status.chain_tip_source
        ));
    }

    match status.last_tip_check_at {
        Some(timestamp) => {
            let age = now.saturating_sub(timestamp);
            if age > max_heartbeat_age {
                reasons.push(format!(
                    "tip heartbeat age {}s exceeds threshold {}s",
                    age, max_heartbeat_age
                ));
            }
        }
        None => reasons.push("tip heartbeat missing".to_string()),
    }

    match status.last_success_at {
        Some(timestamp) => {
            let age = now.saturating_sub(timestamp);
            if age > max_heartbeat_age {
                reasons.push(format!(
                    "success heartbeat age {}s exceeds threshold {}s",
                    age, max_heartbeat_age
                ));
            }
        }
        None => reasons.push("success heartbeat missing".to_string()),
    }

    if status.failure.consecutive_failures > max_consecutive_failures {
        reasons.push(format!(
            "consecutive failures {} exceeds threshold {}",
            status.failure.consecutive_failures, max_consecutive_failures
        ));
    }

    HealthAssessment {
        healthy: reasons.is_empty(),
        reasons,
    }
}

async fn collect_status(config: &Config) -> Result<IndexerStatus, String> {
    let mut last_indexed_height = None;
    let mut backfill_height = None;
    let mut last_seen_rpc_tip = None;
    let mut last_tip_check_at = None;
    let mut last_success_at = None;
    let mut failure = FailureState {
        height: None,
        mode: None,
        error: None,
        timestamp: None,
        consecutive_failures: 0,
    };

    if !config.database_url.is_empty() {
        let postgres = crate::db::PostgresWriter::connect(&config.database_url)
            .await
            .map_err(|e| format!("PostgreSQL status error: {}", e))?;

        last_indexed_height = parse_optional_u32(
            postgres
                .get_state("last_indexed_height")
                .await
                .map_err(|e| format!("Status read error: {}", e))?,
        );
        backfill_height = parse_optional_u32(
            postgres
                .get_state("backfill_height")
                .await
                .map_err(|e| format!("Status read error: {}", e))?,
        );
        last_seen_rpc_tip = parse_optional_u32(
            postgres
                .get_state("last_seen_rpc_tip")
                .await
                .map_err(|e| format!("Status read error: {}", e))?,
        );
        last_tip_check_at = parse_optional_u64(
            postgres
                .get_state("last_tip_check_at")
                .await
                .map_err(|e| format!("Status read error: {}", e))?,
        );
        last_success_at = parse_optional_u64(
            postgres
                .get_state("last_success_at")
                .await
                .map_err(|e| format!("Status read error: {}", e))?,
        );

        failure.height = parse_optional_u32(
            postgres
                .get_state("last_failed_height")
                .await
                .map_err(|e| format!("Status read error: {}", e))?,
        );
        failure.mode = postgres
            .get_state("last_failed_mode")
            .await
            .map_err(|e| format!("Status read error: {}", e))?;
        failure.error = postgres
            .get_state("last_failed_error")
            .await
            .map_err(|e| format!("Status read error: {}", e))?;
        failure.timestamp = parse_optional_u64(
            postgres
                .get_state("last_failed_at")
                .await
                .map_err(|e| format!("Status read error: {}", e))?,
        );
        failure.consecutive_failures = parse_optional_u32(
            postgres
                .get_state("consecutive_failure_count")
                .await
                .map_err(|e| format!("Status read error: {}", e))?,
        )
        .unwrap_or(0);
    }

    let (chain_tip, chain_tip_source) = match crate::db::ZebraRpc::from_env() {
        Ok(rpc) => match rpc.get_block_count().await {
            Ok(tip) => (tip as u32, "rpc".to_string()),
            Err(_) => match last_seen_rpc_tip {
                Some(tip) => (tip, "state".to_string()),
                None => (
                    last_indexed_height.or(backfill_height).unwrap_or(0),
                    "checkpoint".to_string(),
                ),
            },
        },
        Err(_) => match last_seen_rpc_tip {
            Some(tip) => (tip, "state".to_string()),
            None => (
                last_indexed_height.or(backfill_height).unwrap_or(0),
                "checkpoint".to_string(),
            ),
        },
    };

    let lag_blocks = last_indexed_height.map(|indexed| chain_tip.saturating_sub(indexed));

    Ok(IndexerStatus {
        network: config.network_name().to_string(),
        chain_tip,
        chain_tip_source,
        block_count: chain_tip as u64 + 1,
        last_indexed_height,
        backfill_height,
        lag_blocks,
        last_seen_rpc_tip,
        last_tip_check_at,
        last_success_at,
        failure,
    })
}

async fn show_status(config: &Config, json: bool) -> Result<(), String> {
    let status = collect_status(config).await?;

    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&status)
                .map_err(|e| format!("Status serialization error: {}", e))?
        );
        return Ok(());
    }

    println!("📊 Indexer Status");
    println!("────────────────────────────────────────────────────────────");
    println!("   Network:           {}", status.network);
    println!("   Chain tip:         {}", status.chain_tip);
    println!("   Tip source:        {}", status.chain_tip_source);
    println!("   Block count:       {}", status.block_count);
    println!(
        "   Last indexed:      {}",
        status
            .last_indexed_height
            .map(|v| v.to_string())
            .unwrap_or_else(|| "unknown".to_string())
    );
    println!(
        "   Backfill checkpoint:{}",
        status
            .backfill_height
            .map(|v| format!(" {}", v))
            .unwrap_or_else(|| " unknown".to_string())
    );
    println!(
        "   Live lag:          {}",
        status
            .lag_blocks
            .map(|v| format!("{} blocks", v))
            .unwrap_or_else(|| "unknown".to_string())
    );
    println!(
        "   Last RPC tip:      {}",
        status
            .last_seen_rpc_tip
            .map(|v| v.to_string())
            .unwrap_or_else(|| "unknown".to_string())
    );
    println!(
        "   Last tip check:    {}",
        status
            .last_tip_check_at
            .map(|v| v.to_string())
            .unwrap_or_else(|| "unknown".to_string())
    );
    println!(
        "   Last success:      {}",
        status
            .last_success_at
            .map(|v| v.to_string())
            .unwrap_or_else(|| "unknown".to_string())
    );
    println!();

    if status.failure.consecutive_failures > 0 {
        println!("⚠️  Active failure");
        println!(
            "   Mode:              {}",
            status
                .failure
                .mode
                .clone()
                .unwrap_or_else(|| "unknown".to_string())
        );
        println!(
            "   Height:            {}",
            status
                .failure
                .height
                .map(|v| v.to_string())
                .unwrap_or_else(|| "unknown".to_string())
        );
        println!(
            "   Consecutive fails: {}",
            status.failure.consecutive_failures
        );
        println!(
            "   Last failure at:   {}",
            status
                .failure
                .timestamp
                .map(|v| v.to_string())
                .unwrap_or_else(|| "unknown".to_string())
        );
        println!(
            "   Error:             {}",
            status
                .failure
                .error
                .clone()
                .unwrap_or_else(|| "unknown".to_string())
        );
        println!();
    }

    println!("════════════════════════════════════════════════════════════");

    Ok(())
}

async fn check_health(
    config: &Config,
    max_lag: u32,
    max_consecutive_failures: u32,
    max_heartbeat_age: u64,
    json: bool,
) -> Result<(), String> {
    let status = collect_status(config).await?;
    let assessment = assess_health(
        &status,
        max_lag,
        max_consecutive_failures,
        max_heartbeat_age,
        unix_timestamp_secs(),
    );

    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "healthy": assessment.healthy,
                "reasons": assessment.reasons,
                "status": status,
            }))
            .map_err(|e| format!("Health serialization error: {}", e))?
        );
    } else if assessment.healthy {
        println!(
            "✅ Healthy | last indexed {} | tip {} | lag {}",
            status
                .last_indexed_height
                .map(|v| v.to_string())
                .unwrap_or_else(|| "unknown".to_string()),
            status.chain_tip,
            status
                .lag_blocks
                .map(|v| v.to_string())
                .unwrap_or_else(|| "unknown".to_string())
        );
    } else {
        println!("❌ Unhealthy");
        for reason in &assessment.reasons {
            println!("   - {}", reason);
        }
    }

    if assessment.healthy {
        Ok(())
    } else {
        Err(format!(
            "Indexer health check failed: {}",
            assessment.reasons.join("; ")
        ))
    }
}

#[cfg(test)]
mod health_tests {
    use super::{assess_health, FailureState, IndexerStatus};

    const NOW: u64 = 1_710_000_300;

    fn sample_status(lag_blocks: Option<u32>, consecutive_failures: u32) -> IndexerStatus {
        IndexerStatus {
            network: "mainnet".to_string(),
            chain_tip: 100,
            chain_tip_source: "rpc".to_string(),
            block_count: 101,
            last_indexed_height: Some(100u32.saturating_sub(lag_blocks.unwrap_or(0))),
            backfill_height: Some(100),
            lag_blocks,
            last_seen_rpc_tip: Some(100),
            last_tip_check_at: Some(NOW - 60),
            last_success_at: Some(NOW - 30),
            failure: FailureState {
                height: None,
                mode: None,
                error: None,
                timestamp: None,
                consecutive_failures,
            },
        }
    }

    #[test]
    fn health_passes_when_lag_and_failures_are_within_threshold() {
        let assessment = assess_health(&sample_status(Some(2), 0), 3, 0, 600, NOW);
        assert!(assessment.healthy);
        assert!(assessment.reasons.is_empty());
    }

    #[test]
    fn health_fails_when_lag_exceeds_threshold() {
        let assessment = assess_health(&sample_status(Some(5), 0), 3, 0, 600, NOW);
        assert!(!assessment.healthy);
        assert!(assessment
            .reasons
            .iter()
            .any(|reason| reason.contains("lag")));
    }

    #[test]
    fn health_fails_when_failure_count_exceeds_threshold() {
        let assessment = assess_health(&sample_status(Some(1), 2), 3, 0, 600, NOW);
        assert!(!assessment.healthy);
        assert!(assessment
            .reasons
            .iter()
            .any(|reason| reason.contains("consecutive failures")));
    }

    #[test]
    fn health_fails_when_indexed_height_exceeds_tip() {
        let mut status = sample_status(Some(0), 0);
        status.chain_tip = 90;
        status.last_indexed_height = Some(100);
        status.lag_blocks = Some(0);

        let assessment = assess_health(&status, 3, 0, 600, NOW);
        assert!(!assessment.healthy);
        assert!(assessment
            .reasons
            .iter()
            .any(|reason| reason.contains("exceeds chain tip")));
    }

    #[test]
    fn health_fails_when_rpc_tip_is_unavailable() {
        let mut status = sample_status(Some(0), 0);
        status.chain_tip_source = "state".to_string();

        let assessment = assess_health(&status, 3, 0, 600, NOW);
        assert!(!assessment.healthy);
        assert!(assessment
            .reasons
            .iter()
            .any(|reason| reason.contains("rpc tip unavailable")));
    }

    #[test]
    fn health_fails_when_tip_heartbeat_is_stale() {
        let mut status = sample_status(Some(0), 0);
        status.last_tip_check_at = Some(NOW - 601);

        let assessment = assess_health(&status, 3, 0, 600, NOW);
        assert!(!assessment.healthy);
        assert!(assessment
            .reasons
            .iter()
            .any(|reason| reason.contains("tip heartbeat age")));
    }

    #[test]
    fn health_fails_when_success_heartbeat_is_stale() {
        let mut status = sample_status(Some(0), 0);
        status.last_success_at = Some(NOW - 601);

        let assessment = assess_health(&status, 3, 0, 600, NOW);
        assert!(!assessment.healthy);
        assert!(assessment
            .reasons
            .iter()
            .any(|reason| reason.contains("success heartbeat age")));
    }
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

/// Verify parsing by comparing RocksDB data with Zebra RPC
async fn verify_parsing(
    config: &Config,
    start_height: u32,
    count: u32,
    rpc_url: &str,
    cookie_file: &str,
) -> Result<(), String> {
    use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
    use serde_json::{json, Value};

    println!("🔍 Verifying RocksDB parsing against RPC...");
    println!("   RPC URL: {}", rpc_url);
    println!("   Cookie file: {}", cookie_file);
    println!(
        "   Heights: {} to {}",
        start_height,
        start_height + count - 1
    );
    println!("────────────────────────────────────────────────────────────");

    // Read cookie for auth
    // Cookie file format: "__cookie__:password" (Zebra style)
    let cookie_content = std::fs::read_to_string(cookie_file)
        .map_err(|e| format!("Failed to read cookie file: {}", e))?;
    let cookie_trimmed = cookie_content.trim();

    // Use cookie content directly (already has __cookie__:password format)
    let auth = BASE64.encode(cookie_trimmed);
    println!(
        "   Auth: {}...{}",
        &cookie_trimmed[..15],
        &cookie_trimmed[cookie_trimmed.len() - 5..]
    );
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

        let rpc_hash = rpc_json["result"].as_str().unwrap_or("").to_string();

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
            return verify_transaction_simple(zebra, client, rpc_url, auth, height, &block_hash)
                .await;
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
            let rpc_txid = tx.as_str().or_else(|| tx["txid"].as_str()).unwrap_or("?");

            let rpc_txid_short = if rpc_txid.len() > 16 {
                &rpc_txid[..16]
            } else {
                rpc_txid
            };
            println!("   TX {}: {} (RPC)", i, rpc_txid_short);

            // Get txid from RocksDB and compare
            match zebra.get_tx_hash_by_loc(height, i as u16) {
                Ok(hash) => {
                    let mut rev = hash;
                    rev.reverse();
                    let rocks_txid = hex::encode(&rev);
                    let rocks_short = if rocks_txid.len() > 16 {
                        &rocks_txid[..16]
                    } else {
                        &rocks_txid
                    };

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
                        println!(
                            "      📋 {} bytes, v{}, overwintered={}",
                            raw.len(),
                            parsed_version,
                            overwintered
                        );
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
                        let rocks_short = if rocks_txid.len() > 16 {
                            &rocks_txid[..16]
                        } else {
                            &rocks_txid
                        };
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

/// Compare Rust parsing with existing PostgreSQL data
async fn compare_with_postgres(
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
            shielded_spends, shielded_outputs, orchard_actions,
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
        let pg_sapling_spends: Option<i32> = row.try_get("shielded_spends").ok();
        let pg_sapling_outputs: Option<i32> = row.try_get("shielded_outputs").ok();
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

        let block_hash = {
            let mut h = zebra.get_block_hash(height).unwrap_or([0u8; 32]);
            h.reverse();
            hex::encode(&h)
        };

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
            Ok(h) => {
                let mut rev = h;
                rev.reverse();
                hex::encode(&rev)
            }
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
        let pg_txid: String = row.get("txid");
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

        let block_hash = {
            let mut h = zebra.get_block_hash(height).unwrap_or([0u8; 32]);
            h.reverse();
            hex::encode(&h)
        };

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

/// Full validation: index into test DB, compare with prod, benchmark
async fn validate_full(
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
        let mut hash_rev = hash_bytes;
        hash_rev.reverse();
        let block_hash = hex::encode(&hash_rev);

        // Get all transactions
        let raw_txs = zebra.iter_block_transactions(height)?;
        let mut transactions = Vec::with_capacity(raw_txs.len());
        let mut all_flows = Vec::new();

        for (tx_index, raw) in &raw_txs {
            match TransactionParser::parse(raw, height, &block_hash, config.network) {
                Ok(mut tx) => {
                    // Resolve input addresses and values from previous outputs
                    TransactionParser::resolve_inputs(&mut tx, &zebra);

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
               shielded_spends, shielded_outputs, orchard_actions,
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
               shielded_spends, shielded_outputs, orchard_actions,
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
    let mut tx_nulls_checked = 0;

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
                            tx_nulls_checked += 1;
                        }
                        _ => {} // Match
                    }
                }};
            }

            compare_field!("version", i32);
            compare_field!("vin_count", i32);
            compare_field!("vout_count", i32);
            compare_field!("shielded_spends", i32);
            compare_field!("shielded_outputs", i32);
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
