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

        // Get all transactions in block
        let raw_txs = self.zebra.iter_block_transactions(height)?;
        let tx_count = raw_txs.len() as u32;

        // Parse all transactions
        let mut transactions = Vec::with_capacity(raw_txs.len());
        let mut flows = Vec::new();

        for (tx_index, raw) in &raw_txs {
            match TransactionParser::parse(raw, height, &block_hash) {
                Ok(tx) => {
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

        // Get block timestamp from first tx (coinbase) or estimate
        let block_time = if let Some(first_tx) = transactions.first() {
            // For now, use current time - we'd need to parse block header for actual time
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs()
        } else {
            0
        };

        // Write to PostgreSQL
        self.postgres.batch_insert(height, &block_hash, block_time, &transactions).await
            .map_err(|e| format!("DB insert error: {}", e))?;

        // Write flows
        for flow in &flows {
            self.postgres.upsert_flow(flow, block_time).await
                .map_err(|e| format!("Flow insert error: {}", e))?;
        }

        Ok((tx_count, flows.len() as u32))
    }

    /// Run backfill from start_height to end_height (or tip)
    pub async fn backfill(&self, start_height: Option<u32>, end_height: Option<u32>) -> Result<(), String> {
        let tip = self.zebra.get_tip_height()?;
        let start = start_height.unwrap_or_else(|| {
            // Resume from checkpoint if available
            0
        });
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

                // Update checkpoint
                self.postgres.update_checkpoint("last_indexed_height", &current.to_string()).await
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

    /// Run live mode (follow chain tip)
    pub async fn live(&self) -> Result<(), String> {
        println!("🔴 Starting live indexer...");
        println!("   Press Ctrl+C to stop");
        println!("────────────────────────────────────────────────────────────");

        loop {
            let tip = self.zebra.get_tip_height()?;
            let last_indexed = self.postgres.get_checkpoint().await
                .map_err(|e| format!("Checkpoint error: {}", e))?
                .unwrap_or(0);

            if tip > last_indexed {
                let blocks_behind = tip - last_indexed;
                println!("📥 New blocks detected: {} → {} ({} behind)", last_indexed + 1, tip, blocks_behind);

                for height in (last_indexed + 1)..=tip {
                    match self.index_block(height).await {
                        Ok((tx_count, flow_count)) => {
                            println!("   ✅ Block {} | {} txs, {} flows", height, tx_count, flow_count);
                        }
                        Err(e) => {
                            println!("   ❌ Block {} error: {}", height, e);
                        }
                    }
                }

                self.postgres.update_checkpoint("last_indexed_height", &tip.to_string()).await
                    .map_err(|e| format!("Checkpoint error: {}", e))?;

                println!("   ✅ Synced to block {}", tip);
            }

            // Wait before checking again (75 seconds = ~1 block time)
            tokio::time::sleep(tokio::time::Duration::from_secs(10)).await;
        }
    }
}
