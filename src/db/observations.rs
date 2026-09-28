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

// serde_json retains integral i64 values exactly (including values above JS's
// safe-integer range). Only decimal integers are accepted; never round an RPC float.
fn signed_zat(value: &serde_json::Value) -> Option<i64> {
    value
        .as_i64()
        .or_else(|| value.as_str()?.parse::<i64>().ok())
}

pub async fn record_accounting(
    pool: &PgPool,
    info: &serde_json::Value,
    subsidy: Option<serde_json::Value>,
    received_at_ms: i64,
) -> Result<(), sqlx::Error> {
    let Some((hash, height)) = tip_identity(info) else {
        return Ok(());
    };
    let supply = signed_zat(&info["chainSupply"]["chainValueZat"]).filter(|n| {
        info["chainSupply"]["monitored"] == true && (0..=2_100_000_000_000_000).contains(n)
    });
    let pools: serde_json::Map<String, serde_json::Value> = info["valuePools"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|p| {
            let id = p["id"].as_str()?;
            let amount = signed_zat(&p["chainValueZat"])
                .filter(|n| p["monitored"] == true && (0..=2_100_000_000_000_000).contains(n));
            Some((
                id.to_owned(),
                amount
                    .map(|n| serde_json::Value::String(n.to_string()))
                    .unwrap_or(serde_json::Value::Null),
            ))
        })
        .collect();
    sqlx::query(
        "INSERT INTO node_accounting_observations
        (hash, height, observed_at, chain, nsm_balance_zat, circulating_supply_zat,
         pool_balances_zat, subsidy, upgrades, source, poll_interval_ms)
        VALUES ($1,$2,to_timestamp($3::double precision/1000),$4,$5,$6,$7,$8,$9,
                'getblockchaininfo', $10) ON CONFLICT (hash) DO NOTHING",
    )
    .bind(hash.to_ascii_lowercase())
    .bind(height)
    .bind(received_at_ms)
    .bind(info["chain"].as_str())
    .bind(signed_zat(&info["nsmValueBalanceZat"]))
    .bind(supply)
    .bind(serde_json::Value::Object(pools))
    .bind(subsidy)
    .bind(info.get("upgrades").cloned())
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
                            Ok(()) => {
                                // Only attach subsidy allocations while this exact tip is
                                // still canonical. NSM/pools/identity share one atomic RPC snapshot.
                                let subsidy = rpc.get_block_subsidy(height as u64).await.ok();
                                if rpc.get_block_hash(height as u64).await.ok().as_deref()
                                    != Some(&hash)
                                {
                                    continue;
                                }
                                match record_accounting(&pool, &info, subsidy, received_at_ms).await
                                {
                                    Ok(()) => last_recorded = hash,
                                    Err(error) => {
                                        tracing::warn!(%error, "accounting observation was not persisted")
                                    }
                                }
                            }
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
    fn accounting_preserves_signed_integers_without_float_rounding() {
        assert_eq!(
            signed_zat(&serde_json::json!(9_007_199_254_740_993_i64)),
            Some(9_007_199_254_740_993)
        );
        assert_eq!(
            signed_zat(&serde_json::json!("-9223372036854775808")),
            Some(i64::MIN)
        );
        assert_eq!(signed_zat(&serde_json::json!("9223372036854775808")), None);
        assert_eq!(signed_zat(&serde_json::json!(1.25)), None);
        assert_eq!(signed_zat(&serde_json::Value::Null), None);
    }
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
