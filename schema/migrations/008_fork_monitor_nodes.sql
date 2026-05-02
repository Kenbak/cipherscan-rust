-- ============================================================================
-- MIGRATION 008: Fork Monitor Node Registry (persistent)
-- Date: 2026-05-02
-- ============================================================================
-- Stores voluntary node reports from the Fork Monitor.
-- Previously in-memory, now persisted so reports survive API restarts.
-- Expired rows are pruned on read (TTL-based).
-- ============================================================================

CREATE TABLE IF NOT EXISTS fork_monitor_nodes (
  name        TEXT PRIMARY KEY,
  tip         INTEGER NOT NULL,
  tip_hash    TEXT,
  sample_hashes JSONB DEFAULT '[]'::jsonb,
  peers       INTEGER,
  mining      BOOLEAN,
  ttl         TEXT NOT NULL DEFAULT '24h',
  reported_at BIGINT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_fork_monitor_nodes_reported_at
  ON fork_monitor_nodes (reported_at);
