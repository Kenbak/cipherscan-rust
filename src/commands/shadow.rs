//! Read-only full-block gRPC shadow verification.

use crate::config::Config;
use crate::db::{connect_chain_tip_stream, supervise_block_stream, BlockPayloadCache};
use std::time::{Duration, Instant};
use zebra_chain::block::Block;
use zebra_chain::serialization::ZcashDeserialize;

pub(crate) async fn verify_full_block_stream(
    config: &Config,
    event_count: u32,
    timeout_seconds: u64,
) -> Result<(), String> {
    if event_count == 0 {
        return Err("shadow event count must be non-zero".to_string());
    }
    let url = config
        .zebra_grpc_url
        .as_ref()
        .ok_or_else(|| "ZEBRA_GRPC_URL must be configured for shadow mode".to_string())?
        .clone();
    let cache = BlockPayloadCache::new(
        config.grpc_payload_cache_blocks,
        config.grpc_payload_cache_bytes,
    )?;
    tokio::spawn(supervise_block_stream(url.clone(), cache.clone()));
    let mut tip_stream = connect_chain_tip_stream(&url).await?;

    println!("Full-block gRPC shadow mode: no database writes");
    println!("Waiting for {event_count} canonical tip event(s), timeout {timeout_seconds}s");

    let started = Instant::now();
    let verification = async {
        let mut verified = 0;
        while verified < event_count {
            let tip = tip_stream
                .message()
                .await
                .map_err(|e| format!("ChainTipChange stream failed: {e}"))?
                .ok_or_else(|| "ChainTipChange stream ended".to_string())?;
            if tip.hash.len() != 32 {
                return Err(format!(
                    "ChainTipChange returned a {}-byte hash",
                    tip.hash.len()
                ));
            }
            let hash = hex::encode(tip.hash);
            let Some(encoded) = cache.take_wait(&hash, Duration::from_secs(10)).await else {
                println!(
                    "  height {} had no matching payload; treating it as an initial snapshot",
                    tip.height
                );
                continue;
            };
            let payload_age = encoded.received_at.elapsed();
            let block = Block::zcash_deserialize(&mut std::io::Cursor::new(encoded.data.as_ref()))
                .map_err(|e| format!("Failed to decode block {}: {e:?}", tip.height))?;
            let decoded_height = block
                .coinbase_height()
                .map(|height| height.0)
                .ok_or_else(|| format!("Block {} has no coinbase height", tip.height))?;
            if decoded_height != tip.height {
                return Err(format!(
                    "Full-block stream height mismatch: tip {}, payload {}",
                    tip.height, decoded_height
                ));
            }
            if block.hash().to_string() != hash {
                return Err(format!("Full-block stream hash mismatch at {}", tip.height));
            }

            verified += 1;
            println!(
                "  {verified}/{event_count}: height {} verified, {} bytes, payload age {:.1}ms",
                tip.height,
                encoded.data.len(),
                payload_age.as_secs_f64() * 1_000.0
            );
        }
        Ok(())
    };

    tokio::time::timeout(Duration::from_secs(timeout_seconds), verification)
        .await
        .map_err(|_| format!("Shadow verification timed out after {timeout_seconds}s"))??;
    println!(
        "Shadow verification passed in {:.1}s; production database was not accessed",
        started.elapsed().as_secs_f64()
    );
    Ok(())
}
