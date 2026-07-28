//! Configuration for CipherScan Rust Indexer

use std::env;
use std::path::PathBuf;

/// Main configuration struct
#[derive(Debug, Clone)]
pub struct Config {
    /// Path to Zebra's RocksDB state
    pub zebra_state_path: PathBuf,

    /// PostgreSQL connection URL
    pub database_url: String,

    /// Batch size for PostgreSQL inserts
    pub batch_size: usize,

    /// Whether we're in mainnet or testnet
    pub network: Network,

    /// Maximum RocksDB open files (to avoid ulimit issues)
    pub max_open_files: i32,

    /// Zebra gRPC indexer URL (e.g. "http://127.0.0.1:8230")
    /// When set, enables instant block notifications instead of 30s polling
    pub zebra_grpc_url: Option<String>,

    /// Maximum reorg depth the indexer will handle automatically.
    /// Reorgs deeper than this require manual intervention (mainnet safety).
    /// Testnet should use a higher value since deep reorgs are routine.
    pub max_reorg_depth: u32,

    /// When set, archive raw block hex for blocks within ±500 of this height.
    /// Used to capture full block data around network upgrades (e.g. Ironwood).
    pub archive_window_height: Option<u32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Network {
    Mainnet,
    Testnet,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            zebra_state_path: PathBuf::from("/root/.cache/zebra/state/v27/mainnet"),
            database_url: String::from("postgres://localhost/zcash_explorer_mainnet"),
            batch_size: 1000,
            network: Network::Mainnet,
            max_open_files: 256,
            zebra_grpc_url: None,
            max_reorg_depth: 100,
            archive_window_height: None,
        }
    }
}

impl Config {
    /// Load configuration from environment variables
    pub fn from_env() -> Self {
        let mut config = Self::default();

        // Zebra state path
        if let Ok(path) = env::var("ZEBRA_STATE_PATH") {
            config.zebra_state_path = PathBuf::from(path);
        }

        // Database URL
        if let Ok(url) = env::var("DATABASE_URL") {
            config.database_url = url;
        }

        // Batch size
        if let Ok(size) = env::var("BATCH_SIZE") {
            if let Ok(n) = size.parse() {
                config.batch_size = n;
            }
        }

        // Network detection (from path or explicit)
        if let Ok(net) = env::var("NETWORK") {
            config.network = match net.to_lowercase().as_str() {
                "testnet" => Network::Testnet,
                _ => Network::Mainnet,
            };
        } else if config.zebra_state_path.to_string_lossy().contains("testnet") {
            config.network = Network::Testnet;
        }

        if let Ok(url) = env::var("ZEBRA_GRPC_URL") {
            let url = url.trim().to_string();
            if !url.is_empty() {
                config.zebra_grpc_url = Some(if url.starts_with("http") {
                    url
                } else {
                    format!("http://{}", url)
                });
            }
        }

        if let Ok(val) = env::var("MAX_REORG_DEPTH") {
            if let Ok(n) = val.parse::<u32>() {
                config.max_reorg_depth = n;
            }
        }

        if let Ok(val) = env::var("ARCHIVE_WINDOW_HEIGHT") {
            if let Ok(n) = val.parse::<u32>() {
                config.archive_window_height = Some(n);
            }
        }

        config
    }

    /// Get display name for the network
    pub fn network_name(&self) -> &'static str {
        match self.network {
            Network::Mainnet => "mainnet",
            Network::Testnet => "testnet",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = Config::default();
        assert_eq!(config.network, Network::Mainnet);
        assert_eq!(config.batch_size, 1000);
    }
}
