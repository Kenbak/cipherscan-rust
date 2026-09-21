//! Zebra JSON-RPC client
//!
//! Used for live mode to get real-time block updates.
//! Falls back from RocksDB secondary mode when that doesn't work.

use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::time::Duration;

/// Zebra RPC client
#[derive(Clone)]
pub struct ZebraRpc {
    client: Client,
    url: String,
    auth: Option<(String, String)>,
}

#[derive(Serialize)]
struct RpcRequest<'a> {
    jsonrpc: &'a str,
    id: &'a str,
    method: &'a str,
    params: Vec<serde_json::Value>,
}

#[derive(Deserialize, Debug)]
struct RpcResponse<T> {
    result: Option<T>,
    error: Option<RpcError>,
}

#[derive(Deserialize, Debug)]
struct RpcError {
    code: i32,
    message: String,
}

#[derive(Deserialize, Debug)]
pub struct BlockInfo {
    pub hash: String,
    pub time: u64,
    #[serde(default)]
    pub difficulty: f64,
    #[serde(default)]
    pub finalsaplingroot: Option<String>,
    #[serde(default)]
    pub finalorchardroot: Option<String>,
    #[serde(default)]
    pub finalironwoodroot: Option<String>,
}

impl ZebraRpc {
    /// Create new RPC client from environment
    pub fn from_env() -> Result<Self, String> {
        let url =
            std::env::var("ZEBRA_RPC_URL").unwrap_or_else(|_| "http://127.0.0.1:8232".to_string());

        // Try cookie file first
        let auth = if let Ok(cookie_path) = std::env::var("ZEBRA_RPC_COOKIE_FILE") {
            let path = PathBuf::from(&cookie_path);
            if path.exists() {
                match std::fs::read_to_string(&path) {
                    Ok(cookie) => {
                        let parts: Vec<&str> = cookie.trim().split(':').collect();
                        if parts.len() == 2 {
                            Some((parts[0].to_string(), parts[1].to_string()))
                        } else {
                            None
                        }
                    }
                    Err(_) => None,
                }
            } else {
                None
            }
        } else if let (Ok(user), Ok(pass)) = (
            std::env::var("ZEBRA_RPC_USER"),
            std::env::var("ZEBRA_RPC_PASS"),
        ) {
            Some((user, pass))
        } else {
            None
        };

        let client = Client::builder()
            .timeout(Duration::from_secs(30))
            .connect_timeout(Duration::from_secs(10))
            .build()
            .map_err(|e| format!("Failed to build HTTP client: {}", e))?;

        Ok(Self { client, url, auth })
    }

    /// Make an RPC call
    async fn call<T: for<'de> Deserialize<'de>>(
        &self,
        method: &str,
        params: Vec<serde_json::Value>,
    ) -> Result<T, String> {
        let request = RpcRequest {
            jsonrpc: "1.0",
            id: "cipherscan",
            method,
            params,
        };

        let mut req = self.client.post(&self.url).json(&request);

        if let Some((user, pass)) = &self.auth {
            req = req.basic_auth(user, Some(pass));
        }

        let response = req
            .send()
            .await
            .map_err(|e| format!("RPC request failed: {}", e))?;

        let rpc_response: RpcResponse<T> = response
            .json()
            .await
            .map_err(|e| format!("RPC parse failed: {}", e))?;

        if let Some(err) = rpc_response.error {
            return Err(format!("RPC error {}: {}", err.code, err.message));
        }

        rpc_response
            .result
            .ok_or_else(|| "RPC returned no result".to_string())
    }

    /// Get current blockchain height
    pub async fn get_block_count(&self) -> Result<u64, String> {
        self.call("getblockcount", vec![]).await
    }

    /// Get block hash at height
    pub async fn get_block_hash(&self, height: u64) -> Result<String, String> {
        self.call("getblockhash", vec![serde_json::json!(height)])
            .await
    }

    /// Get block info by hash
    pub async fn get_block(&self, hash: &str) -> Result<BlockInfo, String> {
        self.call(
            "getblock",
            vec![serde_json::json!(hash), serde_json::json!(1)],
        )
        .await
    }

    /// Get block info by height
    pub async fn get_block_by_height(&self, height: u64) -> Result<BlockInfo, String> {
        let hash = self.get_block_hash(height).await?;
        self.get_block(&hash).await
    }

    /// Get full raw block as hex (verbosity 0)
    pub async fn get_raw_block_hex(&self, hash: &str) -> Result<String, String> {
        self.call(
            "getblock",
            vec![serde_json::json!(hash), serde_json::json!(0)],
        )
        .await
    }

    /// Get raw transaction hex
    pub async fn get_raw_transaction_hex(&self, txid: &str) -> Result<String, String> {
        self.call(
            "getrawtransaction",
            vec![serde_json::json!(txid), serde_json::json!(0)],
        )
        .await
    }

    /// Valid fork tips let the optional block stream skip historical branches.
    pub async fn get_fork_tip_hashes(&self) -> Result<Vec<Vec<u8>>, String> {
        let tips: Vec<ChainTip> = self.call("getchaintips", vec![]).await?;
        fork_tip_hashes(tips)
    }

    /// Get blockchain info (includes valuePools with authoritative pool balances)
    pub async fn get_blockchain_info(&self) -> Result<serde_json::Value, String> {
        self.call("getblockchaininfo", vec![]).await
    }
}

#[derive(Deserialize)]
struct ChainTip {
    hash: String,
    status: String,
}

fn fork_tip_hashes(tips: Vec<ChainTip>) -> Result<Vec<Vec<u8>>, String> {
    let mut hashes = Vec::new();
    for tip in tips.into_iter().filter(|tip| tip.status == "valid-fork") {
        let hash = hex::decode(tip.hash).map_err(|e| format!("Invalid fork tip hash: {e}"))?;
        if hash.len() != 32 {
            return Err("Invalid fork tip hash length".to_string());
        }
        if !hashes.contains(&hash) {
            hashes.push(hash);
        }
    }
    // Reserve one slot for the fresh canonical tip obtained through gRPC.
    if hashes.len() >= zakura_chain::parameters::MAX_NON_FINALIZED_CHAIN_FORKS {
        return Err("Too many fork tips for the block stream".to_string());
    }
    Ok(hashes)
}

#[cfg(test)]
mod fork_tip_tests {
    use super::*;

    #[test]
    fn seeds_valid_forks_without_stale_active_or_invalid_tips() {
        let tips = serde_json::from_value(serde_json::json!([
            {"status":"active", "hash":"01".repeat(32)},
            {"status":"valid-fork", "hash":"02".repeat(32)},
            {"status":"valid-fork", "hash":"02".repeat(32)},
            {"status":"invalid", "hash":"03".repeat(32)}
        ]))
        .unwrap();
        assert_eq!(fork_tip_hashes(tips).unwrap(), vec![vec![2; 32]]);
    }

    #[test]
    fn refuses_malformed_or_excessive_fork_tips() {
        assert!(fork_tip_hashes(vec![ChainTip {
            hash: "00".repeat(31),
            status: "valid-fork".into()
        }])
        .is_err());
        let tips = (0..zakura_chain::parameters::MAX_NON_FINALIZED_CHAIN_FORKS)
            .map(|i| ChainTip {
                hash: hex::encode([i as u8; 32]),
                status: "valid-fork".into(),
            })
            .collect();
        assert!(fork_tip_hashes(tips).is_err());
    }
}
