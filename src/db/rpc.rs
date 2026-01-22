//! Zebra JSON-RPC client
//!
//! Used for live mode to get real-time block updates.
//! Falls back from RocksDB secondary mode when that doesn't work.

use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Zebra RPC client
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
struct BlockchainInfo {
    blocks: u64,
    bestblockhash: String,
}

#[derive(Deserialize, Debug)]
pub struct BlockInfo {
    pub hash: String,
    pub height: u64,
    pub time: u64,
    pub tx: Vec<String>,
    #[serde(default)]
    pub previousblockhash: Option<String>,
    // Header fields
    #[serde(default)]
    pub version: i32,
    #[serde(default)]
    pub difficulty: f64,
    #[serde(default)]
    pub bits: String,
    #[serde(default)]
    pub nonce: String,
    #[serde(default)]
    pub merkleroot: String,
    #[serde(default)]
    pub finalsaplingroot: Option<String>,
    #[serde(default)]
    pub size: u64,
}

impl ZebraRpc {
    /// Create new RPC client from environment
    pub fn from_env() -> Result<Self, String> {
        let url = std::env::var("ZEBRA_RPC_URL")
            .unwrap_or_else(|_| "http://127.0.0.1:8232".to_string());

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

        let client = Client::new();

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
        self.call("getblock", vec![serde_json::json!(hash), serde_json::json!(1)])
            .await
    }

    /// Get block info by height
    pub async fn get_block_by_height(&self, height: u64) -> Result<BlockInfo, String> {
        let hash = self.get_block_hash(height).await?;
        self.get_block(&hash).await
    }

    /// Get raw transaction hex
    pub async fn get_raw_transaction_hex(&self, txid: &str) -> Result<String, String> {
        self.call(
            "getrawtransaction",
            vec![serde_json::json!(txid), serde_json::json!(0)],
        )
        .await
    }

    /// Get raw transaction verbose (JSON)
    pub async fn get_raw_transaction(&self, txid: &str) -> Result<serde_json::Value, String> {
        self.call(
            "getrawtransaction",
            vec![serde_json::json!(txid), serde_json::json!(1)],
        )
        .await
    }

    /// Get raw block hex
    pub async fn get_block_hex(&self, height: u64) -> Result<String, String> {
        let hash = self.get_block_hash(height).await?;
        self.call("getblock", vec![serde_json::json!(hash), serde_json::json!(0)])
            .await
    }
}
