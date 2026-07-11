-- Migration 006: Materialized views for /api/crosschain/* endpoints
-- Pre-computes expensive aggregations. Refreshed every 5 min by sync-crosschain-swaps.js cron job.

BEGIN;

CREATE MATERIALIZED VIEW IF NOT EXISTS mv_crosschain_summary AS
SELECT
  COUNT(*) FILTER (WHERE swap_created_at >= NOW() - INTERVAL '24 hours')::int AS swaps_24h,
  COALESCE(SUM(source_amount_usd) FILTER (WHERE swap_created_at >= NOW() - INTERVAL '24 hours'), 0)::float AS volume_24h,
  COUNT(*)::int AS swaps_all_time,
  COALESCE(SUM(source_amount_usd), 0)::float AS volume_all_time
FROM cross_chain_swaps
WHERE status = 'SUCCESS';

-- Single-row view; dummy unique index required for REFRESH CONCURRENTLY
CREATE UNIQUE INDEX IF NOT EXISTS mv_crosschain_summary_idx
  ON mv_crosschain_summary (swaps_all_time);

CREATE MATERIALIZED VIEW IF NOT EXISTS mv_crosschain_volume_24h AS
SELECT
  direction,
  CASE WHEN direction = 'inflow' THEN source_chain ELSE dest_chain END AS chain,
  CASE WHEN direction = 'inflow' THEN source_token ELSE dest_token END AS token,
  COALESCE(SUM(source_amount_usd), 0)::float AS volume_usd,
  COUNT(*)::int AS swap_count
FROM cross_chain_swaps
WHERE status = 'SUCCESS'
  AND swap_created_at >= NOW() - INTERVAL '24 hours'
GROUP BY direction, chain, token;

CREATE UNIQUE INDEX IF NOT EXISTS mv_crosschain_volume_24h_idx
  ON mv_crosschain_volume_24h (direction, chain, token);

CREATE MATERIALIZED VIEW IF NOT EXISTS mv_crosschain_latency AS
SELECT
  direction, chain,
  COUNT(*)::int AS swap_count,
  AVG(latency_min)::float AS avg_minutes,
  PERCENTILE_CONT(0.5) WITHIN GROUP (ORDER BY latency_min)::float AS median_minutes
FROM (
  SELECT
    ccs.direction,
    CASE WHEN ccs.direction = 'inflow' THEN ccs.source_chain ELSE ccs.dest_chain END AS chain,
    (t.block_time - EXTRACT(EPOCH FROM ccs.swap_created_at)) / 60.0 AS latency_min
  FROM cross_chain_swaps ccs
  JOIN transactions t ON t.txid = ccs.zec_txid
  WHERE ccs.status = 'SUCCESS'
    AND ccs.matched = true
    AND ccs.zec_txid IS NOT NULL
    AND (t.block_time - EXTRACT(EPOCH FROM ccs.swap_created_at)) > 0
    AND (t.block_time - EXTRACT(EPOCH FROM ccs.swap_created_at)) < 86400
) sub
GROUP BY direction, chain;

CREATE UNIQUE INDEX IF NOT EXISTS mv_crosschain_latency_idx
  ON mv_crosschain_latency (direction, chain);

CREATE MATERIALIZED VIEW IF NOT EXISTS mv_crosschain_trends AS
SELECT
  DATE_TRUNC('day', swap_created_at)::date AS day,
  direction,
  COUNT(*)::int AS swap_count,
  COALESCE(SUM(source_amount_usd), 0)::float AS volume_usd
FROM cross_chain_swaps
WHERE status = 'SUCCESS'
GROUP BY day, direction
ORDER BY day;

CREATE UNIQUE INDEX IF NOT EXISTS mv_crosschain_trends_idx
  ON mv_crosschain_trends (day, direction);

CREATE MATERIALIZED VIEW IF NOT EXISTS mv_crosschain_popular_pairs AS
SELECT
  CASE WHEN direction = 'inflow' THEN source_chain ELSE dest_chain END AS chain,
  CASE WHEN direction = 'inflow' THEN source_token ELSE dest_token END AS token,
  COUNT(*)::int AS swap_count
FROM cross_chain_swaps
WHERE status = 'SUCCESS'
  AND swap_created_at >= NOW() - INTERVAL '30 days'
  AND source_token NOT IN ('UNKNOWN_TOKEN', 'UNKNOWN', 'OTHER')
  AND dest_token NOT IN ('UNKNOWN_TOKEN', 'UNKNOWN', 'OTHER')
GROUP BY chain, token
ORDER BY swap_count DESC
LIMIT 100;

CREATE UNIQUE INDEX IF NOT EXISTS mv_crosschain_popular_pairs_idx
  ON mv_crosschain_popular_pairs (chain, token);

COMMIT;
