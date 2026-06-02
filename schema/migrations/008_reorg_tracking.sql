-- Migration 008: Reorg/uncle block tracking
-- Tracks orphaned blocks from chain reorganizations and external reports
--
-- Run: sudo -u postgres psql -d zcash_explorer_mainnet -f 008_reorg_tracking.sql

-- Fork events: each reorg is one event
CREATE TABLE IF NOT EXISTS fork_events (
    id SERIAL PRIMARY KEY,
    fork_height bigint NOT NULL,
    depth integer NOT NULL DEFAULT 1,
    canonical_tip bigint,
    orphaned_count integer NOT NULL DEFAULT 0,
    source text NOT NULL DEFAULT 'internal',
    description text,
    detected_at timestamp without time zone DEFAULT now(),
    resolved_at timestamp without time zone
);

CREATE INDEX IF NOT EXISTS idx_fork_events_height ON fork_events (fork_height DESC);
CREATE INDEX IF NOT EXISTS idx_fork_events_detected ON fork_events (detected_at DESC);

-- Orphaned blocks: individual blocks from losing forks
CREATE TABLE IF NOT EXISTS orphaned_blocks (
    id SERIAL PRIMARY KEY,
    height bigint NOT NULL,
    hash text NOT NULL UNIQUE,
    canonical_hash text,
    "timestamp" bigint,
    transaction_count integer DEFAULT 0,
    size integer DEFAULT 0,
    difficulty text,
    miner_address text,
    previous_block_hash text,
    fork_event_id integer REFERENCES fork_events(id) ON DELETE SET NULL,
    source text NOT NULL DEFAULT 'internal',
    reported_by text,
    consensus_valid boolean,
    detected_at timestamp without time zone DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_orphaned_blocks_height ON orphaned_blocks (height DESC);
CREATE INDEX IF NOT EXISTS idx_orphaned_blocks_detected ON orphaned_blocks (detected_at DESC);
CREATE INDEX IF NOT EXISTS idx_orphaned_blocks_fork_event ON orphaned_blocks (fork_event_id);

-- External tip reports: lightweight table for incoming hash reports from nodes
CREATE TABLE IF NOT EXISTS tip_reports (
    id SERIAL PRIMARY KEY,
    height bigint NOT NULL,
    hash text NOT NULL,
    node_id text,
    ip_hash text,
    is_match boolean,
    reported_at timestamp without time zone DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_tip_reports_height ON tip_reports (height DESC);
CREATE INDEX IF NOT EXISTS idx_tip_reports_reported ON tip_reports (reported_at DESC);
CREATE UNIQUE INDEX IF NOT EXISTS idx_tip_reports_unique_report ON tip_reports (height, hash, COALESCE(node_id, ''));
