//! Indexer module - main indexing logic

mod blocks;
mod flows;
mod transactions;

pub use transactions::TransactionParser;

use crate::config::Config;
use crate::db::{PostgresWriter, ZebraState};
use crate::models::ShieldedFlow;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

fn checkpoint_progress_height(
    current_height: u32,
    end_height: u32,
    last_successful_height: Option<u32>,
) -> Option<u32> {
    if current_height % 100 == 0 || current_height == end_height {
        last_successful_height
    } else {
        None
    }
}

fn unix_timestamp_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

/// Main indexer orchestrator
pub struct Indexer {
    config: Config,
    zebra: ZebraState,
    postgres: PostgresWriter,
}

impl Indexer {
    const FAILURE_STATE_KEYS: [&'static str; 5] = [
        "last_failed_height",
        "last_failed_mode",
        "last_failed_error",
        "last_failed_at",
        "consecutive_failure_count",
    ];
    /// Create new indexer
    pub async fn new(config: Config) -> Result<Self, String> {
        let zebra = ZebraState::open(&config)?;
        let postgres = PostgresWriter::connect(&config.database_url)
            .await
            .map_err(|e| format!("PostgreSQL error: {}", e))?;

        Ok(Self {
            config,
            zebra,
            postgres,
        })
    }

    async fn record_failure(&self, mode: &str, height: u32, error: &str) -> Result<(), String> {
        let current_count = self
            .postgres
            .get_state("consecutive_failure_count")
            .await
            .map_err(|e| format!("Failure state read error: {}", e))?
            .and_then(|value| value.parse::<u32>().ok())
            .unwrap_or(0);

        let truncated_error: String = error.chars().take(500).collect();
        let timestamp = unix_timestamp_secs().to_string();
        let failure_count = current_count.saturating_add(1).to_string();

        self.postgres
            .update_checkpoint("last_failed_height", &height.to_string())
            .await
            .map_err(|e| format!("Failure state write error: {}", e))?;
        self.postgres
            .update_checkpoint("last_failed_mode", mode)
            .await
            .map_err(|e| format!("Failure state write error: {}", e))?;
        self.postgres
            .update_checkpoint("last_failed_error", &truncated_error)
            .await
            .map_err(|e| format!("Failure state write error: {}", e))?;
        self.postgres
            .update_checkpoint("last_failed_at", &timestamp)
            .await
            .map_err(|e| format!("Failure state write error: {}", e))?;
        self.postgres
            .update_checkpoint("consecutive_failure_count", &failure_count)
            .await
            .map_err(|e| format!("Failure state write error: {}", e))?;

        Ok(())
    }

    async fn clear_failure_state(&self) -> Result<(), String> {
        for key in Self::FAILURE_STATE_KEYS {
            self.postgres
                .delete_state(key)
                .await
                .map_err(|e| format!("Failure state cleanup error: {}", e))?;
        }

        Ok(())
    }

    async fn record_tip_heartbeat(&self, rpc_tip: u32) -> Result<(), String> {
        let now = unix_timestamp_secs().to_string();

        self.postgres
            .update_checkpoint("last_seen_rpc_tip", &rpc_tip.to_string())
            .await
            .map_err(|e| format!("Heartbeat write error: {}", e))?;
        self.postgres
            .update_checkpoint("last_tip_check_at", &now)
            .await
            .map_err(|e| format!("Heartbeat write error: {}", e))?;

        Ok(())
    }

    async fn record_success_heartbeat(&self) -> Result<(), String> {
        let now = unix_timestamp_secs().to_string();

        self.postgres
            .update_checkpoint("last_success_at", &now)
            .await
            .map_err(|e| format!("Heartbeat write error: {}", e))?;

        Ok(())
    }

    async fn has_active_failure_state(&self) -> Result<bool, String> {
        let failure_count = self
            .postgres
            .get_state("consecutive_failure_count")
            .await
            .map_err(|e| format!("Failure state read error: {}", e))?
            .and_then(|value| value.parse::<u32>().ok())
            .unwrap_or(0);

        Ok(failure_count > 0)
    }

    /// Index a single block and all its transactions
    async fn index_block(&self, height: u32) -> Result<(u32, u32), String> {
        // Get block hash
        let hash_bytes = self.zebra.get_block_hash(height)?;
        let mut hash_rev = hash_bytes;
        hash_rev.reverse();
        let block_hash = hex::encode(&hash_rev);

        // Get block header for timestamp and other fields
        let header = self.zebra.get_block_header(height)?;
        let block_time = header.time;

        // Get all transactions in block
        let raw_txs = self.zebra.iter_block_transactions(height)?;
        let tx_count = raw_txs.len() as u32;

        // Parse all transactions
        let mut transactions = Vec::with_capacity(raw_txs.len());
        let mut flows = Vec::new();

        for (tx_index, raw) in &raw_txs {
            let mut tx = TransactionParser::parse(raw, height, &block_hash, self.config.network)
                .map_err(|e| format!("Failed to parse tx {}:{}: {}", height, tx_index, e))?;

            // Resolve input addresses and values from previous outputs
            TransactionParser::resolve_inputs(&mut tx, &self.zebra);

            // Extract shielded flows
            let tx_flows = ShieldedFlow::from_transaction(&tx);
            flows.extend(tx_flows);
            transactions.push(tx);
        }

        // Write block, transactions, and flows atomically.
        let (_, flow_count) = self
            .postgres
            .batch_insert_with_header_and_flows(
                height,
                &block_hash,
                block_time,
                &transactions,
                &flows,
                &header,
            )
            .await
            .map_err(|e| format!("DB insert error: {}", e))?;

        Ok((tx_count, flow_count as u32))
    }

    /// Run backfill from start_height to end_height (or tip)
    pub async fn backfill(
        &self,
        start_height: Option<u32>,
        end_height: Option<u32>,
    ) -> Result<(), String> {
        let tip = self.zebra.get_tip_height()?;

        // If no start specified, resume from backfill checkpoint
        let start = match start_height {
            Some(h) => h,
            None => {
                let checkpoint = self
                    .postgres
                    .get_checkpoint_key("backfill_height")
                    .await
                    .map_err(|e| format!("Checkpoint error: {}", e))?
                    .unwrap_or(0);
                if checkpoint > 0 {
                    println!("📍 Resuming from checkpoint: {}", checkpoint);
                    checkpoint + 1 // Start from next block
                } else {
                    0
                }
            }
        };
        let end = end_height.unwrap_or(tip);

        println!("🚀 Starting backfill from {} to {}", start, end);
        println!("────────────────────────────────────────────────────────────");

        let overall_start = Instant::now();
        let mut current = start;
        let mut total_txs = 0u64;
        let mut total_flows = 0u64;
        let mut total_blocks = 0u64;
        let mut last_successful_height = start.checked_sub(1);
        let mut checkpointed_height = start.checked_sub(1);
        let mut failure_state_active = self.has_active_failure_state().await?;

        while current <= end {
            // Index single block
            match self.index_block(current).await {
                Ok((tx_count, flow_count)) => {
                    total_txs += tx_count as u64;
                    total_flows += flow_count as u64;
                    total_blocks += 1;
                    last_successful_height = Some(current);
                    self.record_success_heartbeat().await?;
                    if failure_state_active {
                        self.clear_failure_state().await?;
                        failure_state_active = false;
                    }
                }
                Err(e) => {
                    self.record_failure("backfill", current, &e).await?;

                    if let Some(last_success) = last_successful_height {
                        if Some(last_success) != checkpointed_height {
                            self.postgres
                                .update_checkpoint("backfill_height", &last_success.to_string())
                                .await
                                .map_err(|err| format!("Checkpoint error: {}", err))?;
                        }
                    }

                    return Err(format!("Backfill aborted at height {}: {}", current, e));
                }
            }

            if let Some(progress_height) =
                checkpoint_progress_height(current, end, last_successful_height)
            {
                let elapsed = overall_start.elapsed();
                let rate = if total_blocks > 0 {
                    total_blocks as f64 / elapsed.as_secs_f64().max(f64::EPSILON)
                } else {
                    0.0
                };
                let remaining_blocks = (end - current) as f64;
                let eta_secs = if rate > 0.0 {
                    remaining_blocks / rate
                } else {
                    0.0
                };

                println!(
                    "📦 {} / {} ({:.1}%) | {:.1} blk/s | txs:{} flows:{} | ETA: {:.0}s",
                    current,
                    end,
                    (current - start) as f64 / (end - start).max(1) as f64 * 100.0,
                    rate,
                    total_txs,
                    total_flows,
                    eta_secs
                );

                // Update backfill checkpoint to the last successfully indexed height.
                self.postgres
                    .update_checkpoint("backfill_height", &progress_height.to_string())
                    .await
                    .map_err(|e| format!("Checkpoint error: {}", e))?;
                checkpointed_height = Some(progress_height);
            }

            current += 1;
        }

        let elapsed = overall_start.elapsed();
        println!("────────────────────────────────────────────────────────────");
        println!("✅ Backfill complete!");
        println!("   Blocks: {}", total_blocks);
        println!("   Transactions: {}", total_txs);
        println!("   Flows: {}", total_flows);
        println!("   Time: {:.1}s", elapsed.as_secs_f64());
        println!(
            "   Rate: {:.1} blocks/s, {:.1} tx/s",
            total_blocks as f64 / elapsed.as_secs_f64(),
            total_txs as f64 / elapsed.as_secs_f64()
        );

        Ok(())
    }

    /// Index a single block from RPC (for live mode)
    async fn index_block_from_rpc(
        &self,
        rpc: &crate::db::ZebraRpc,
        height: u32,
    ) -> Result<(u32, u32), String> {
        // Get block info from RPC
        let block_info = rpc.get_block_by_height(height as u64).await?;
        let block_hash = block_info.hash.clone();
        let block_time = block_info.time;

        let tx_count = block_info.tx.len() as u32;
        let mut transactions = Vec::with_capacity(block_info.tx.len());
        let mut flows = Vec::new();

        // Get each transaction
        for (tx_index, txid) in block_info.tx.iter().enumerate() {
            let raw_hex = rpc.get_raw_transaction_hex(txid).await?;
            let raw_bytes =
                hex::decode(&raw_hex).map_err(|e| format!("Hex decode error: {}", e))?;

            let mut tx =
                TransactionParser::parse(&raw_bytes, height, &block_hash, self.config.network)
                    .map_err(|e| format!("Failed to parse tx {}:{}: {}", height, tx_index, e))?;

            // Resolve input values via RPC (for fee calculation)
            if !tx.is_coinbase() && !tx.vin.is_empty() {
                let mut total_input: i64 = 0;
                for input in &mut tx.vin {
                    if input.is_coinbase {
                        continue;
                    }
                    if let Ok(prev_tx_json) = rpc.get_raw_transaction(&input.txid).await {
                        if let Some(vout_array) =
                            prev_tx_json.get("vout").and_then(|v| v.as_array())
                        {
                            if let Some(prev_output) = vout_array.get(input.vout as usize) {
                                if let Some(value_zec) =
                                    prev_output.get("value").and_then(|v| v.as_f64())
                                {
                                    let value_zatoshi = (value_zec * 100_000_000.0) as i64;
                                    input.value = Some(value_zatoshi);
                                    total_input += value_zatoshi;
                                }
                                if let Some(script_pubkey) = prev_output.get("scriptPubKey") {
                                    if let Some(addresses) =
                                        script_pubkey.get("addresses").and_then(|a| a.as_array())
                                    {
                                        if let Some(addr) =
                                            addresses.first().and_then(|a| a.as_str())
                                        {
                                            input.address = Some(addr.to_string());
                                        }
                                    } else if let Some(addr) =
                                        script_pubkey.get("address").and_then(|a| a.as_str())
                                    {
                                        input.address = Some(addr.to_string());
                                    }
                                }
                            }
                        }
                    }
                }
                tx.transparent_value_in = total_input;
            }

            if !tx.is_coinbase() {
                let fee = tx.transparent_value_in - tx.transparent_value_out
                    + tx.sapling_value_balance
                    + tx.orchard_value_balance;
                if fee >= 0 {
                    tx.fee = Some(fee);
                }
            }

            let tx_flows = ShieldedFlow::from_transaction(&tx);
            flows.extend(tx_flows);
            transactions.push(tx);
        }

        // Create header from RPC block info
        let header = crate::db::ParsedBlockHeader {
            version: block_info.version,
            previous_block_hash: block_info.previousblockhash.clone().unwrap_or_default(),
            merkle_root: block_info.merkleroot.clone(),
            final_sapling_root: block_info.finalsaplingroot.clone().unwrap_or_default(),
            time: block_info.time,
            bits: block_info.bits.clone(),
            nonce: block_info.nonce.clone(),
            difficulty: block_info.difficulty,
            solution: String::new(), // Not returned by RPC, but not critical
        };

        // Write block, transactions, and flows atomically.
        let (_, flow_count) = self
            .postgres
            .batch_insert_with_header_and_flows(
                height,
                &block_hash,
                block_time,
                &transactions,
                &flows,
                &header,
            )
            .await
            .map_err(|e| format!("DB insert error: {}", e))?;

        Ok((tx_count, flow_count as u32))
    }

    /// Run live mode (follow chain tip)
    /// Uses gRPC streaming for instant block notifications when available,
    /// falls back to 30s JSON-RPC polling otherwise.
    pub async fn live(&self) -> Result<(), String> {
        use crate::db::grpc::proto::BlockHashAndHeight;
        use crate::db::{connect_chain_tip_stream, ZebraRpc};
        use tokio::time::Duration;
        use tonic::Streaming;

        println!("🔴 Starting live indexer...");
        println!("   Press Ctrl+C to stop");
        println!("────────────────────────────────────────────────────────────");

        let rpc = ZebraRpc::from_env()?;
        println!("   ✅ JSON-RPC client initialized");

        let grpc_url = self.config.zebra_grpc_url.clone();
        let mut grpc_stream: Option<Streaming<BlockHashAndHeight>> = None;
        let mut failure_state_active = self.has_active_failure_state().await?;

        if let Some(ref url) = grpc_url {
            println!("🔗 Connecting to Zebra gRPC at {}...", url);
            match connect_chain_tip_stream(url).await {
                Ok(stream) => {
                    grpc_stream = Some(stream);
                    println!("   ✅ gRPC connected — instant block notifications enabled");
                }
                Err(e) => {
                    println!("   ⚠️ gRPC unavailable ({}), using 30s polling", e);
                }
            }
        } else {
            println!("   ℹ️ ZEBRA_GRPC_URL not set — using 30s polling");
        }

        loop {
            // Wait for trigger: gRPC tip notification OR 30s polling timeout
            if let Some(ref mut stream) = grpc_stream {
                tokio::select! {
                    msg = stream.message() => {
                        match msg {
                            Ok(Some(tip)) => {
                                let hash = hex::encode(&tip.hash);
                                println!("📦 [gRPC] New block: {} ({}...)", tip.height, &hash[..16.min(hash.len())]);
                            }
                            Ok(None) => {
                                println!("⚠️ [gRPC] Stream ended, falling back to polling");
                                grpc_stream = None;
                            }
                            Err(e) => {
                                println!("⚠️ [gRPC] Stream error: {}, falling back to polling", e);
                                grpc_stream = None;
                            }
                        }
                    }
                    _ = tokio::time::sleep(Duration::from_secs(30)) => {
                        // Periodic poll even with gRPC, as a safety net
                    }
                }
            } else {
                tokio::time::sleep(Duration::from_secs(30)).await;

                // Periodically try to reconnect gRPC
                if let Some(ref url) = grpc_url {
                    if let Ok(stream) = connect_chain_tip_stream(url).await {
                        println!("   ✅ [gRPC] Reconnected");
                        grpc_stream = Some(stream);
                    }
                }
            }

            // Get authoritative tip from JSON-RPC
            let rpc_tip = match rpc.get_block_count().await {
                Ok(tip) => tip as u32,
                Err(e) => {
                    println!("   ⚠️ RPC error: {}", e);
                    tokio::time::sleep(Duration::from_secs(10)).await;
                    continue;
                }
            };

            self.record_tip_heartbeat(rpc_tip).await?;

            let last_indexed = self
                .postgres
                .get_checkpoint()
                .await
                .map_err(|e| format!("Checkpoint error: {}", e))?
                .unwrap_or(0);

            if rpc_tip > last_indexed {
                let blocks_behind = rpc_tip - last_indexed;
                println!(
                    "📥 New blocks: {} → {} ({} behind)",
                    last_indexed + 1,
                    rpc_tip,
                    blocks_behind
                );

                let mut last_success = last_indexed;

                for height in (last_indexed + 1)..=rpc_tip {
                    match self.index_block_from_rpc(&rpc, height).await {
                        Ok((tx_count, flow_count)) => {
                            println!(
                                "   ✅ Block {} | {} txs, {} flows",
                                height, tx_count, flow_count
                            );
                            last_success = height;
                            self.record_success_heartbeat().await?;
                            if failure_state_active {
                                self.clear_failure_state().await?;
                                failure_state_active = false;
                            }
                        }
                        Err(e) => {
                            self.record_failure("live", height, &e).await?;
                            failure_state_active = true;
                            println!("   ❌ Block {} error: {}", height, e);
                            break;
                        }
                    }
                }

                if last_success > last_indexed {
                    self.postgres
                        .update_checkpoint("last_indexed_height", &last_success.to_string())
                        .await
                        .map_err(|e| format!("Checkpoint error: {}", e))?;
                    println!("   ✅ Synced to block {}", last_success);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::checkpoint_progress_height;

    #[test]
    fn checkpoint_progress_uses_last_success_only() {
        assert_eq!(checkpoint_progress_height(100, 200, Some(99)), Some(99));
        assert_eq!(checkpoint_progress_height(200, 200, Some(198)), Some(198));
    }

    #[test]
    fn checkpoint_progress_skips_non_boundary_heights() {
        assert_eq!(checkpoint_progress_height(42, 200, Some(42)), None);
    }
}
