//! Indexer module - main indexing logic

mod blocks;
mod transactions;
mod flows;

pub use blocks::BlockIndexer;
pub use transactions::TransactionParser;
pub use flows::FlowAnalyzer;

use crate::config::Config;
use crate::db::{ZebraState, PostgresWriter};
use crate::models::ShieldedFlow;
use std::time::Instant;

/// Main indexer orchestrator
pub struct Indexer {
    config: Config,
    zebra: ZebraState,
    postgres: PostgresWriter,
}

impl Indexer {
    /// Create new indexer
    pub async fn new(config: Config) -> Result<Self, String> {
        let zebra = ZebraState::open(&config)?;
        let postgres = PostgresWriter::connect(&config.database_url).await
            .map_err(|e| format!("PostgreSQL error: {}", e))?;

        Ok(Self {
            config,
            zebra,
            postgres,
        })
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
            match TransactionParser::parse(raw, height, &block_hash) {
                Ok(mut tx) => {
                    // Resolve input addresses and values from previous outputs
                    TransactionParser::resolve_inputs(&mut tx, &self.zebra);

                    // Extract shielded flows
                    let tx_flows = ShieldedFlow::from_transaction(&tx);
                    flows.extend(tx_flows);
                    transactions.push(tx);
                }
                Err(e) => {
                    tracing::warn!("Failed to parse tx {}:{}: {}", height, tx_index, e);
                }
            }
        }

        // Write to PostgreSQL (blocks, transactions, outputs, inputs)
        self.postgres.batch_insert_with_header(height, &block_hash, block_time, &transactions, &header).await
            .map_err(|e| format!("DB insert error: {}", e))?;

        // Write flows
        let flow_count = self.postgres.batch_insert_flows(&flows, block_time).await
            .map_err(|e| format!("Flow insert error: {}", e))?;

        Ok((tx_count, flow_count as u32))
    }

    /// Run backfill from start_height to end_height (or tip)
    pub async fn backfill(&self, start_height: Option<u32>, end_height: Option<u32>) -> Result<(), String> {
        let tip = self.zebra.get_tip_height()?;
        
        // If no start specified, resume from backfill checkpoint
        let start = match start_height {
            Some(h) => h,
            None => {
                let checkpoint = self.postgres.get_checkpoint_key("backfill_height").await
                    .map_err(|e| format!("Checkpoint error: {}", e))?
                    .unwrap_or(0);
                if checkpoint > 0 {
                    println!("📍 Resuming from checkpoint: {}", checkpoint);
                    checkpoint + 1  // Start from next block
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

        while current <= end {
            let batch_start = Instant::now();

            // Index single block
            match self.index_block(current).await {
                Ok((tx_count, flow_count)) => {
                    total_txs += tx_count as u64;
                    total_flows += flow_count as u64;
                    total_blocks += 1;
                }
                Err(e) => {
                    tracing::error!("Error at height {}: {}", current, e);
                }
            }

            // Progress every 100 blocks
            if current % 100 == 0 || current == end {
                let elapsed = overall_start.elapsed();
                let rate = total_blocks as f64 / elapsed.as_secs_f64();
                let remaining_blocks = (end - current) as f64;
                let eta_secs = remaining_blocks / rate;

                println!(
                    "📦 {} / {} ({:.1}%) | {:.1} blk/s | txs:{} flows:{} | ETA: {:.0}s",
                    current, end,
                    (current - start) as f64 / (end - start).max(1) as f64 * 100.0,
                    rate, total_txs, total_flows, eta_secs
                );

                // Update backfill checkpoint (separate from live checkpoint)
                self.postgres.update_checkpoint("backfill_height", &current.to_string()).await
                    .map_err(|e| format!("Checkpoint error: {}", e))?;
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
        println!("   Rate: {:.1} blocks/s, {:.1} tx/s",
            total_blocks as f64 / elapsed.as_secs_f64(),
            total_txs as f64 / elapsed.as_secs_f64()
        );

        Ok(())
    }

    /// Index a single block from RPC (for live mode)
    async fn index_block_from_rpc(&self, rpc: &crate::db::ZebraRpc, height: u32) -> Result<(u32, u32), String> {
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
            let raw_bytes = hex::decode(&raw_hex)
                .map_err(|e| format!("Hex decode error: {}", e))?;

            match TransactionParser::parse(&raw_bytes, height, &block_hash) {
                Ok(mut tx) => {
                    // Resolve input values via RPC (for fee calculation)
                    if !tx.is_coinbase() && !tx.vin.is_empty() {
                        let mut total_input: i64 = 0;
                        for input in &mut tx.vin {
                            if input.is_coinbase {
                                continue;
                            }
                            if let Ok(prev_tx_json) = rpc.get_raw_transaction(&input.txid).await {
                                if let Some(vout_array) = prev_tx_json.get("vout").and_then(|v| v.as_array()) {
                                    if let Some(prev_output) = vout_array.get(input.vout as usize) {
                                        // Get value (in ZEC, convert to zatoshi)
                                        if let Some(value_zec) = prev_output.get("value").and_then(|v| v.as_f64()) {
                                            let value_zatoshi = (value_zec * 100_000_000.0) as i64;
                                            input.value = Some(value_zatoshi);
                                            total_input += value_zatoshi;
                                        }
                                        // Get address
                                        if let Some(script_pubkey) = prev_output.get("scriptPubKey") {
                                            if let Some(addresses) = script_pubkey.get("addresses").and_then(|a| a.as_array()) {
                                                if let Some(addr) = addresses.first().and_then(|a| a.as_str()) {
                                                    input.address = Some(addr.to_string());
                                                }
                                            } else if let Some(addr) = script_pubkey.get("address").and_then(|a| a.as_str()) {
                                                input.address = Some(addr.to_string());
                                            }
                                        }
                                    }
                                }
                            }
                        }
                        tx.transparent_value_in = total_input;
                        tx.fee = Some(total_input - tx.transparent_value_out);
                    }

                    let tx_flows = ShieldedFlow::from_transaction(&tx);
                    flows.extend(tx_flows);
                    transactions.push(tx);
                }
                Err(e) => {
                    tracing::warn!("Failed to parse tx {}:{}: {}", height, tx_index, e);
                }
            }
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

        // Write to PostgreSQL
        self.postgres.batch_insert_with_header(height, &block_hash, block_time, &transactions, &header).await
            .map_err(|e| format!("DB insert error: {}", e))?;

        let flow_count = self.postgres.batch_insert_flows(&flows, block_time).await
            .map_err(|e| format!("Flow insert error: {}", e))?;

        Ok((tx_count, flow_count as u32))
    }

    /// Run live mode (follow chain tip)
    /// Uses RPC for everything - more reliable than RocksDB secondary mode
    pub async fn live(&self) -> Result<(), String> {
        use crate::db::ZebraRpc;

        println!("🔴 Starting live indexer (RPC mode)...");
        println!("   Press Ctrl+C to stop");
        println!("────────────────────────────────────────────────────────────");

        // Initialize RPC client
        let rpc = ZebraRpc::from_env()?;
        println!("   ✅ RPC client initialized");

        loop {
            // Get tip from RPC
            let rpc_tip = match rpc.get_block_count().await {
                Ok(tip) => tip as u32,
                Err(e) => {
                    println!("   ⚠️ RPC error: {}", e);
                    tokio::time::sleep(tokio::time::Duration::from_secs(10)).await;
                    continue;
                }
            };

            let last_indexed = self.postgres.get_checkpoint().await
                .map_err(|e| format!("Checkpoint error: {}", e))?
                .unwrap_or(0);

            if rpc_tip > last_indexed {
                let blocks_behind = rpc_tip - last_indexed;
                println!("📥 New blocks: {} → {} ({} behind)", last_indexed + 1, rpc_tip, blocks_behind);

                for height in (last_indexed + 1)..=rpc_tip {
                    match self.index_block_from_rpc(&rpc, height).await {
                        Ok((tx_count, flow_count)) => {
                            println!("   ✅ Block {} | {} txs, {} flows", height, tx_count, flow_count);
                        }
                        Err(e) => {
                            println!("   ❌ Block {} error: {}", height, e);
                            // Don't continue if we can't index a block
                            break;
                        }
                    }
                }

                // Update checkpoint to highest successfully indexed
                let new_checkpoint = std::cmp::min(rpc_tip, last_indexed + blocks_behind);
                self.postgres.update_checkpoint("last_indexed_height", &new_checkpoint.to_string()).await
                    .map_err(|e| format!("Checkpoint error: {}", e))?;

                println!("   ✅ Synced to block {}", new_checkpoint);
            }

            // Wait before checking again (~75s = average block time)
            tokio::time::sleep(tokio::time::Duration::from_secs(30)).await;
        }
    }
}
