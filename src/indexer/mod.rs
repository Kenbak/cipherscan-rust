//! Indexer module - main indexing logic

mod blocks;
mod transactions;
mod flows;

pub use blocks::BlockIndexer;
pub use transactions::TransactionParser;
pub use flows::FlowAnalyzer;

use crate::config::Config;
use crate::db::{ZebraState, PostgresWriter};
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
        let postgres = PostgresWriter::connect(&config).await
            .map_err(|e| format!("PostgreSQL error: {}", e))?;
        
        Ok(Self {
            config,
            zebra,
            postgres,
        })
    }
    
    /// Run backfill from start_height to current tip
    pub async fn backfill(&self, start_height: Option<u32>) -> Result<(), String> {
        let tip = self.zebra.get_tip_height()?;
        let start = start_height.unwrap_or(0);
        
        tracing::info!("Starting backfill from {} to {}", start, tip);
        
        let batch_size = self.config.batch_size;
        let mut current = start;
        let overall_start = Instant::now();
        
        while current <= tip {
            let batch_end = std::cmp::min(current + batch_size as u32, tip);
            let batch_start = Instant::now();
            
            // Process batch
            let mut blocks_indexed = 0;
            let mut txs_indexed = 0;
            let mut flows_indexed = 0;
            
            for result in self.zebra.iter_blocks(current, batch_end) {
                match result {
                    Ok((height, hash)) => {
                        // TODO: Full block/tx parsing
                        blocks_indexed += 1;
                    }
                    Err(e) => {
                        tracing::warn!("Error at height {}: {}", current, e);
                    }
                }
            }
            
            let elapsed = batch_start.elapsed();
            let rate = blocks_indexed as f64 / elapsed.as_secs_f64();
            let total_elapsed = overall_start.elapsed();
            let overall_rate = (current - start) as f64 / total_elapsed.as_secs_f64();
            let remaining = (tip - current) as f64 / overall_rate;
            
            tracing::info!(
                "📦 {} → {} | {:.1} blk/s | ETA: {:.1}h | blocks:{} txs:{} flows:{}",
                current, batch_end, rate, remaining / 3600.0,
                blocks_indexed, txs_indexed, flows_indexed
            );
            
            // Update checkpoint
            self.postgres.update_checkpoint(batch_end).await
                .map_err(|e| format!("Checkpoint error: {}", e))?;
            
            current = batch_end + 1;
        }
        
        tracing::info!("✅ Backfill complete!");
        Ok(())
    }
    
    /// Run live mode (follow chain tip)
    pub async fn live(&self) -> Result<(), String> {
        tracing::info!("Starting live indexer...");
        
        loop {
            let tip = self.zebra.get_tip_height()?;
            let last_indexed = self.postgres.get_checkpoint().await
                .map_err(|e| format!("Checkpoint error: {}", e))?
                .unwrap_or(0);
            
            if tip > last_indexed {
                tracing::info!("New blocks: {} → {}", last_indexed + 1, tip);
                
                for result in self.zebra.iter_blocks(last_indexed + 1, tip) {
                    match result {
                        Ok((height, hash)) => {
                            // TODO: Full block/tx parsing
                            tracing::debug!("Indexed block {}", height);
                        }
                        Err(e) => {
                            tracing::error!("Error at height: {}", e);
                        }
                    }
                }
                
                self.postgres.update_checkpoint(tip).await
                    .map_err(|e| format!("Checkpoint error: {}", e))?;
            }
            
            // Wait before checking again
            tokio::time::sleep(tokio::time::Duration::from_secs(10)).await;
        }
    }
}
