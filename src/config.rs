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
