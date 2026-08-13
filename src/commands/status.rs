use crate::config::Config;
use crate::util::{parse_optional_u32, parse_optional_u64, unix_timestamp_secs};
use serde::Serialize;

/// Show indexer status
#[derive(Debug, Serialize)]
pub(crate) struct FailureState {
    height: Option<u32>,
    mode: Option<String>,
    error: Option<String>,
    timestamp: Option<u64>,
    consecutive_failures: u32,
}

#[derive(Debug, Serialize)]
pub(crate) struct IndexerStatus {
    network: String,
    chain_tip: u32,
    chain_tip_source: String,
    block_count: u64,
    last_indexed_height: Option<u32>,
    backfill_height: Option<u32>,
    lag_blocks: Option<u32>,
    last_seen_rpc_tip: Option<u32>,
    last_tip_check_at: Option<u64>,
    last_success_at: Option<u64>,
    failure: FailureState,
}

#[derive(Debug)]
pub(crate) struct HealthAssessment {
    healthy: bool,
    reasons: Vec<String>,
}

fn assess_health(
    status: &IndexerStatus,
    max_lag: u32,
    max_consecutive_failures: u32,
    max_heartbeat_age: u64,
    now: u64,
) -> HealthAssessment {
    let mut reasons = Vec::new();

    if let Some(lag) = status.lag_blocks {
        if lag > max_lag {
            reasons.push(format!("lag {} exceeds threshold {}", lag, max_lag));
        }
    } else {
        reasons.push("live checkpoint missing".to_string());
    }

    if let Some(last_indexed) = status.last_indexed_height {
        if last_indexed > status.chain_tip {
            reasons.push(format!(
                "last indexed height {} exceeds chain tip {}",
                last_indexed, status.chain_tip
            ));
        }
    }

    if status.chain_tip_source != "rpc" {
        reasons.push(format!(
            "rpc tip unavailable; using {} fallback",
            status.chain_tip_source
        ));
    }

    match status.last_tip_check_at {
        Some(timestamp) => {
            let age = now.saturating_sub(timestamp);
            if age > max_heartbeat_age {
                reasons.push(format!(
                    "tip heartbeat age {}s exceeds threshold {}s",
                    age, max_heartbeat_age
                ));
            }
        }
        None => reasons.push("tip heartbeat missing".to_string()),
    }

    match status.last_success_at {
        Some(timestamp) => {
            let age = now.saturating_sub(timestamp);
            if age > max_heartbeat_age {
                reasons.push(format!(
                    "success heartbeat age {}s exceeds threshold {}s",
                    age, max_heartbeat_age
                ));
            }
        }
        None => reasons.push("success heartbeat missing".to_string()),
    }

    if status.failure.consecutive_failures > max_consecutive_failures {
        reasons.push(format!(
            "consecutive failures {} exceeds threshold {}",
            status.failure.consecutive_failures, max_consecutive_failures
        ));
    }

    HealthAssessment {
        healthy: reasons.is_empty(),
        reasons,
    }
}

async fn collect_status(config: &Config) -> Result<IndexerStatus, String> {
    let mut last_indexed_height = None;
    let mut backfill_height = None;
    let mut last_seen_rpc_tip = None;
    let mut last_tip_check_at = None;
    let mut last_success_at = None;
    let mut failure = FailureState {
        height: None,
        mode: None,
        error: None,
        timestamp: None,
        consecutive_failures: 0,
    };

    if !config.database_url.is_empty() {
        let postgres = crate::db::PostgresWriter::connect(&config.database_url)
            .await
            .map_err(|e| format!("PostgreSQL status error: {}", e))?;

        last_indexed_height = parse_optional_u32(
            postgres
                .get_state("last_indexed_height")
                .await
                .map_err(|e| format!("Status read error: {}", e))?,
        );
        backfill_height = parse_optional_u32(
            postgres
                .get_state("backfill_height")
                .await
                .map_err(|e| format!("Status read error: {}", e))?,
        );
        last_seen_rpc_tip = parse_optional_u32(
            postgres
                .get_state("last_seen_rpc_tip")
                .await
                .map_err(|e| format!("Status read error: {}", e))?,
        );
        last_tip_check_at = parse_optional_u64(
            postgres
                .get_state("last_tip_check_at")
                .await
                .map_err(|e| format!("Status read error: {}", e))?,
        );
        last_success_at = parse_optional_u64(
            postgres
                .get_state("last_success_at")
                .await
                .map_err(|e| format!("Status read error: {}", e))?,
        );

        failure.height = parse_optional_u32(
            postgres
                .get_state("last_failed_height")
                .await
                .map_err(|e| format!("Status read error: {}", e))?,
        );
        failure.mode = postgres
            .get_state("last_failed_mode")
            .await
            .map_err(|e| format!("Status read error: {}", e))?;
        failure.error = postgres
            .get_state("last_failed_error")
            .await
            .map_err(|e| format!("Status read error: {}", e))?;
        failure.timestamp = parse_optional_u64(
            postgres
                .get_state("last_failed_at")
                .await
                .map_err(|e| format!("Status read error: {}", e))?,
        );
        failure.consecutive_failures = parse_optional_u32(
            postgres
                .get_state("consecutive_failure_count")
                .await
                .map_err(|e| format!("Status read error: {}", e))?,
        )
        .unwrap_or(0);
    }

    let (chain_tip, chain_tip_source) = match crate::db::ZebraRpc::from_env() {
        Ok(rpc) => match rpc.get_block_count().await {
            Ok(tip) => (tip as u32, "rpc".to_string()),
            Err(_) => match last_seen_rpc_tip {
                Some(tip) => (tip, "state".to_string()),
                None => (
                    last_indexed_height.or(backfill_height).unwrap_or(0),
                    "checkpoint".to_string(),
                ),
            },
        },
        Err(_) => match last_seen_rpc_tip {
            Some(tip) => (tip, "state".to_string()),
            None => (
                last_indexed_height.or(backfill_height).unwrap_or(0),
                "checkpoint".to_string(),
            ),
        },
    };

    let lag_blocks = last_indexed_height.map(|indexed| chain_tip.saturating_sub(indexed));

    Ok(IndexerStatus {
        network: config.network_name().to_string(),
        chain_tip,
        chain_tip_source,
        block_count: chain_tip as u64 + 1,
        last_indexed_height,
        backfill_height,
        lag_blocks,
        last_seen_rpc_tip,
        last_tip_check_at,
        last_success_at,
        failure,
    })
}

pub(crate) async fn show_status(config: &Config, json: bool) -> Result<(), String> {
    let status = collect_status(config).await?;

    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&status)
                .map_err(|e| format!("Status serialization error: {}", e))?
        );
        return Ok(());
    }

    println!("📊 Indexer Status");
    println!("────────────────────────────────────────────────────────────");
    println!("   Network:           {}", status.network);
    println!("   Chain tip:         {}", status.chain_tip);
    println!("   Tip source:        {}", status.chain_tip_source);
    println!("   Block count:       {}", status.block_count);
    println!(
        "   Last indexed:      {}",
        status
            .last_indexed_height
            .map(|v| v.to_string())
            .unwrap_or_else(|| "unknown".to_string())
    );
    println!(
        "   Backfill checkpoint:{}",
        status
            .backfill_height
            .map(|v| format!(" {}", v))
            .unwrap_or_else(|| " unknown".to_string())
    );
    println!(
        "   Live lag:          {}",
        status
            .lag_blocks
            .map(|v| format!("{} blocks", v))
            .unwrap_or_else(|| "unknown".to_string())
    );
    println!(
        "   Last RPC tip:      {}",
        status
            .last_seen_rpc_tip
            .map(|v| v.to_string())
            .unwrap_or_else(|| "unknown".to_string())
    );
    println!(
        "   Last tip check:    {}",
        status
            .last_tip_check_at
            .map(|v| v.to_string())
            .unwrap_or_else(|| "unknown".to_string())
    );
    println!(
        "   Last success:      {}",
        status
            .last_success_at
            .map(|v| v.to_string())
            .unwrap_or_else(|| "unknown".to_string())
    );
    println!();

    if status.failure.consecutive_failures > 0 {
        println!("⚠️  Active failure");
        println!(
            "   Mode:              {}",
            status
                .failure
                .mode
                .clone()
                .unwrap_or_else(|| "unknown".to_string())
        );
        println!(
            "   Height:            {}",
            status
                .failure
                .height
                .map(|v| v.to_string())
                .unwrap_or_else(|| "unknown".to_string())
        );
        println!(
            "   Consecutive fails: {}",
            status.failure.consecutive_failures
        );
        println!(
            "   Last failure at:   {}",
            status
                .failure
                .timestamp
                .map(|v| v.to_string())
                .unwrap_or_else(|| "unknown".to_string())
        );
        println!(
            "   Error:             {}",
            status
                .failure
                .error
                .clone()
                .unwrap_or_else(|| "unknown".to_string())
        );
        println!();
    }

    println!("════════════════════════════════════════════════════════════");

    Ok(())
}

pub(crate) async fn check_health(
    config: &Config,
    max_lag: u32,
    max_consecutive_failures: u32,
    max_heartbeat_age: u64,
    json: bool,
) -> Result<(), String> {
    let status = collect_status(config).await?;
    let assessment = assess_health(
        &status,
        max_lag,
        max_consecutive_failures,
        max_heartbeat_age,
        unix_timestamp_secs(),
    );

    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "healthy": assessment.healthy,
                "reasons": assessment.reasons,
                "status": status,
            }))
            .map_err(|e| format!("Health serialization error: {}", e))?
        );
    } else if assessment.healthy {
        println!(
            "✅ Healthy | last indexed {} | tip {} | lag {}",
            status
                .last_indexed_height
                .map(|v| v.to_string())
                .unwrap_or_else(|| "unknown".to_string()),
            status.chain_tip,
            status
                .lag_blocks
                .map(|v| v.to_string())
                .unwrap_or_else(|| "unknown".to_string())
        );
    } else {
        println!("❌ Unhealthy");
        for reason in &assessment.reasons {
            println!("   - {}", reason);
        }
    }

    if assessment.healthy {
        Ok(())
    } else {
        Err(format!(
            "Indexer health check failed: {}",
            assessment.reasons.join("; ")
        ))
    }
}

#[cfg(test)]
mod health_tests {
    use super::{assess_health, FailureState, IndexerStatus};

    const NOW: u64 = 1_710_000_300;

    fn sample_status(lag_blocks: Option<u32>, consecutive_failures: u32) -> IndexerStatus {
        IndexerStatus {
            network: "mainnet".to_string(),
            chain_tip: 100,
            chain_tip_source: "rpc".to_string(),
            block_count: 101,
            last_indexed_height: Some(100u32.saturating_sub(lag_blocks.unwrap_or(0))),
            backfill_height: Some(100),
            lag_blocks,
            last_seen_rpc_tip: Some(100),
            last_tip_check_at: Some(NOW - 60),
            last_success_at: Some(NOW - 30),
            failure: FailureState {
                height: None,
                mode: None,
                error: None,
                timestamp: None,
                consecutive_failures,
            },
        }
    }

    #[test]
    fn health_passes_when_lag_and_failures_are_within_threshold() {
        let assessment = assess_health(&sample_status(Some(2), 0), 3, 0, 600, NOW);
        assert!(assessment.healthy);
        assert!(assessment.reasons.is_empty());
    }

    #[test]
    fn health_fails_when_lag_exceeds_threshold() {
        let assessment = assess_health(&sample_status(Some(5), 0), 3, 0, 600, NOW);
        assert!(!assessment.healthy);
        assert!(assessment
            .reasons
            .iter()
            .any(|reason| reason.contains("lag")));
    }

    #[test]
    fn health_fails_when_failure_count_exceeds_threshold() {
        let assessment = assess_health(&sample_status(Some(1), 2), 3, 0, 600, NOW);
        assert!(!assessment.healthy);
        assert!(assessment
            .reasons
            .iter()
            .any(|reason| reason.contains("consecutive failures")));
    }

    #[test]
    fn health_fails_when_indexed_height_exceeds_tip() {
        let mut status = sample_status(Some(0), 0);
        status.chain_tip = 90;
        status.last_indexed_height = Some(100);
        status.lag_blocks = Some(0);

        let assessment = assess_health(&status, 3, 0, 600, NOW);
        assert!(!assessment.healthy);
        assert!(assessment
            .reasons
            .iter()
            .any(|reason| reason.contains("exceeds chain tip")));
    }

    #[test]
    fn health_fails_when_rpc_tip_is_unavailable() {
        let mut status = sample_status(Some(0), 0);
        status.chain_tip_source = "state".to_string();

        let assessment = assess_health(&status, 3, 0, 600, NOW);
        assert!(!assessment.healthy);
        assert!(assessment
            .reasons
            .iter()
            .any(|reason| reason.contains("rpc tip unavailable")));
    }

    #[test]
    fn health_fails_when_tip_heartbeat_is_stale() {
        let mut status = sample_status(Some(0), 0);
        status.last_tip_check_at = Some(NOW - 601);

        let assessment = assess_health(&status, 3, 0, 600, NOW);
        assert!(!assessment.healthy);
        assert!(assessment
            .reasons
            .iter()
            .any(|reason| reason.contains("tip heartbeat age")));
    }

    #[test]
    fn health_fails_when_success_heartbeat_is_stale() {
        let mut status = sample_status(Some(0), 0);
        status.last_success_at = Some(NOW - 601);

        let assessment = assess_health(&status, 3, 0, 600, NOW);
        assert!(!assessment.healthy);
        assert!(assessment
            .reasons
            .iter()
            .any(|reason| reason.contains("success heartbeat age")));
    }
}
