//! Receipt time of the local node's best-tip RPC response, independent of indexing.
//! This is not peer arrival time and does not observe every competing branch.
use super::ZebraRpc;
use sqlx::PgPool;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

pub const POLL_INTERVAL_MS: i32 = 1000;

pub async fn record_observation(
    pool: &PgPool,
    hash: &str,
    height: i64,
    received_at_ms: i64,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO block_observations (hash, height, first_seen_at, source, poll_interval_ms)
         VALUES ($1, $2, to_timestamp($3::double precision / 1000), 'local-node-rpc', $4)
         ON CONFLICT (hash) DO NOTHING",
    )
    .bind(hash)
    .bind(height)
    .bind(received_at_ms)
    .bind(POLL_INTERVAL_MS)
    .execute(pool)
    .await?;
    Ok(())
}

fn tip_identity(info: &serde_json::Value) -> Option<(&str, i64)> {
    let hash = info.get("bestblockhash")?.as_str()?;
    let height = info.get("blocks")?.as_i64()?;
    (height >= 0 && hash.len() == 64 && hash.bytes().all(|b| b.is_ascii_hexdigit()))
        .then_some((hash, height))
}

pub async fn observe_local_tip(rpc: ZebraRpc, pool: PgPool) {
    let mut last_recorded = String::new();
    loop {
        match rpc.get_blockchain_info().await {
            Ok(info) => {
                // Capture before any database work. No block/header clock is used.
                let received_at_ms = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .expect("system clock before Unix epoch")
                    .as_millis() as i64;
                if let Some((hash, height)) = tip_identity(&info) {
                    let hash = hash.to_ascii_lowercase();
                    if hash != last_recorded {
                        match record_observation(&pool, &hash, height, received_at_ms).await {
                            Ok(()) => last_recorded = hash,
                            Err(error) => {
                                tracing::warn!(%error, "tip observation was not persisted")
                            }
                        }
                    }
                } else {
                    tracing::warn!("local node returned an invalid tip identity");
                }
            }
            Err(error) => tracing::warn!(%error, "local tip observation RPC failed"),
        }
        tokio::time::sleep(Duration::from_millis(POLL_INTERVAL_MS as u64)).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn observation_requires_a_complete_tip_identity() {
        let hash = "a".repeat(64);
        let valid = serde_json::json!({"bestblockhash": hash, "blocks": 42});
        assert_eq!(tip_identity(&valid), Some((hash.as_str(), 42)));
        assert!(tip_identity(&serde_json::json!({"blocks": 42})).is_none());
        assert!(tip_identity(&serde_json::json!({"bestblockhash": "bad", "blocks": 42})).is_none());
        assert!(tip_identity(&serde_json::json!({"bestblockhash": hash, "blocks": -1})).is_none());
    }
}
