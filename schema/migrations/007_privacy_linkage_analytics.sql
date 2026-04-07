-- Migration 007: Privacy linkage edges and batch clusters
-- Precomputed analytics tables for privacy-risk detection and visualization.

BEGIN;

CREATE OR REPLACE FUNCTION update_privacy_linkage_timestamp()
RETURNS TRIGGER AS $$
BEGIN
  NEW.updated_at = NOW();
  RETURN NEW;
END;
$$ LANGUAGE plpgsql;

CREATE OR REPLACE FUNCTION cleanup_expired_privacy_linkage()
RETURNS INTEGER AS $$
DECLARE
  deleted_edges INTEGER;
  deleted_clusters INTEGER;
BEGIN
  DELETE FROM privacy_linkage_edges WHERE expires_at < NOW();
  GET DIAGNOSTICS deleted_edges = ROW_COUNT;
  DELETE FROM privacy_batch_clusters WHERE expires_at < NOW();
  GET DIAGNOSTICS deleted_clusters = ROW_COUNT;
  RETURN deleted_edges + deleted_clusters;
END;
$$ LANGUAGE plpgsql;

CREATE TABLE IF NOT EXISTS privacy_linkage_edges (
  id SERIAL PRIMARY KEY,
  edge_hash VARCHAR(64) NOT NULL UNIQUE,
  edge_type VARCHAR(32) NOT NULL CHECK (edge_type IN ('PAIR_LINK', 'BATCH_LINK')),
  candidate_rank INTEGER NOT NULL DEFAULT 1 CHECK (candidate_rank >= 1),
  src_txid TEXT NOT NULL,
  src_block_height INTEGER,
  src_block_time INTEGER NOT NULL,
  src_amount_zat BIGINT NOT NULL,
  src_pool TEXT,
  dst_txid TEXT NOT NULL,
  dst_block_height INTEGER,
  dst_block_time INTEGER NOT NULL,
  dst_amount_zat BIGINT NOT NULL,
  dst_pool TEXT,
  anchor_txid TEXT,
  amount_diff_zat BIGINT NOT NULL DEFAULT 0,
  time_delta_seconds INTEGER NOT NULL,
  amount_rarity_score NUMERIC(6,2) NOT NULL DEFAULT 0,
  amount_weirdness_score NUMERIC(6,2) NOT NULL DEFAULT 0,
  timing_score NUMERIC(6,2) NOT NULL DEFAULT 0,
  recipient_reuse_score NUMERIC(6,2) NOT NULL DEFAULT 0,
  confidence_score INTEGER NOT NULL CHECK (confidence_score >= 0 AND confidence_score <= 100),
  confidence_margin INTEGER NOT NULL DEFAULT 0 CHECK (confidence_margin >= 0),
  ambiguity_score INTEGER NOT NULL DEFAULT 0 CHECK (ambiguity_score >= 0 AND ambiguity_score <= 100),
  warning_level VARCHAR(10) NOT NULL CHECK (warning_level IN ('HIGH', 'MEDIUM', 'LOW')),
  evidence JSONB NOT NULL DEFAULT '{}'::jsonb,
  detected_at TIMESTAMPTZ DEFAULT NOW(),
  updated_at TIMESTAMPTZ DEFAULT NOW(),
  expires_at TIMESTAMPTZ DEFAULT (NOW() + INTERVAL '90 days')
);

CREATE INDEX IF NOT EXISTS idx_privacy_linkage_src_txid
  ON privacy_linkage_edges(src_txid);
CREATE INDEX IF NOT EXISTS idx_privacy_linkage_dst_txid
  ON privacy_linkage_edges(dst_txid);
CREATE INDEX IF NOT EXISTS idx_privacy_linkage_anchor_txid
  ON privacy_linkage_edges(anchor_txid)
  WHERE anchor_txid IS NOT NULL;
CREATE INDEX IF NOT EXISTS idx_privacy_linkage_score
  ON privacy_linkage_edges(confidence_score DESC, dst_block_time DESC);
CREATE INDEX IF NOT EXISTS idx_privacy_linkage_warning
  ON privacy_linkage_edges(warning_level, dst_block_time DESC);
CREATE INDEX IF NOT EXISTS idx_privacy_linkage_rank
  ON privacy_linkage_edges(edge_type, candidate_rank, dst_block_time DESC);
CREATE INDEX IF NOT EXISTS idx_privacy_linkage_expires
  ON privacy_linkage_edges(expires_at);
CREATE INDEX IF NOT EXISTS idx_privacy_linkage_detected_at
  ON privacy_linkage_edges(detected_at DESC);
CREATE INDEX IF NOT EXISTS idx_privacy_linkage_evidence
  ON privacy_linkage_edges USING GIN (evidence);

DROP TRIGGER IF EXISTS privacy_linkage_edges_updated_at ON privacy_linkage_edges;
CREATE TRIGGER privacy_linkage_edges_updated_at
  BEFORE UPDATE ON privacy_linkage_edges
  FOR EACH ROW
  EXECUTE FUNCTION update_privacy_linkage_timestamp();

CREATE TABLE IF NOT EXISTS privacy_batch_clusters (
  id SERIAL PRIMARY KEY,
  cluster_hash VARCHAR(64) NOT NULL UNIQUE,
  cluster_type VARCHAR(32) NOT NULL CHECK (cluster_type IN ('BATCH_DESHIELD')),
  anchor_txid TEXT,
  anchor_block_height INTEGER,
  anchor_block_time INTEGER,
  anchor_amount_zat BIGINT,
  member_txids TEXT[] NOT NULL,
  member_count INTEGER NOT NULL CHECK (member_count >= 2),
  total_amount_zat BIGINT NOT NULL,
  representative_amount_zat BIGINT NOT NULL,
  first_tx_time INTEGER NOT NULL,
  last_tx_time INTEGER NOT NULL,
  time_span_seconds INTEGER NOT NULL,
  confidence_score INTEGER NOT NULL CHECK (confidence_score >= 0 AND confidence_score <= 100),
  confidence_margin INTEGER NOT NULL DEFAULT 0 CHECK (confidence_margin >= 0),
  ambiguity_score INTEGER NOT NULL DEFAULT 0 CHECK (ambiguity_score >= 0 AND ambiguity_score <= 100),
  warning_level VARCHAR(10) NOT NULL CHECK (warning_level IN ('HIGH', 'MEDIUM', 'LOW')),
  evidence JSONB NOT NULL DEFAULT '{}'::jsonb,
  detected_at TIMESTAMPTZ DEFAULT NOW(),
  updated_at TIMESTAMPTZ DEFAULT NOW(),
  expires_at TIMESTAMPTZ DEFAULT (NOW() + INTERVAL '90 days')
);

CREATE INDEX IF NOT EXISTS idx_privacy_batch_anchor_txid
  ON privacy_batch_clusters(anchor_txid)
  WHERE anchor_txid IS NOT NULL;
CREATE INDEX IF NOT EXISTS idx_privacy_batch_score
  ON privacy_batch_clusters(confidence_score DESC, first_tx_time DESC);
CREATE INDEX IF NOT EXISTS idx_privacy_batch_warning
  ON privacy_batch_clusters(warning_level, first_tx_time DESC);
CREATE INDEX IF NOT EXISTS idx_privacy_batch_first_time
  ON privacy_batch_clusters(first_tx_time DESC);
CREATE INDEX IF NOT EXISTS idx_privacy_batch_expires
  ON privacy_batch_clusters(expires_at);
CREATE INDEX IF NOT EXISTS idx_privacy_batch_member_txids
  ON privacy_batch_clusters USING GIN (member_txids);
CREATE INDEX IF NOT EXISTS idx_privacy_batch_evidence
  ON privacy_batch_clusters USING GIN (evidence);

DROP TRIGGER IF EXISTS privacy_batch_clusters_updated_at ON privacy_batch_clusters;
CREATE TRIGGER privacy_batch_clusters_updated_at
  BEFORE UPDATE ON privacy_batch_clusters
  FOR EACH ROW
  EXECUTE FUNCTION update_privacy_linkage_timestamp();

CREATE OR REPLACE VIEW high_risk_privacy_linkage_edges AS
SELECT
  id,
  edge_type,
  src_txid,
  dst_txid,
  anchor_txid,
  src_amount_zat / 100000000.0 AS src_amount_zec,
  dst_amount_zat / 100000000.0 AS dst_amount_zec,
  time_delta_seconds,
  confidence_score,
  confidence_margin,
  ambiguity_score,
  warning_level,
  candidate_rank,
  detected_at
FROM privacy_linkage_edges
WHERE expires_at > NOW()
  AND warning_level = 'HIGH'
  AND candidate_rank = 1
ORDER BY confidence_score DESC, detected_at DESC;

CREATE OR REPLACE VIEW high_risk_privacy_batch_clusters AS
SELECT
  id,
  cluster_type,
  anchor_txid,
  total_amount_zat / 100000000.0 AS total_amount_zec,
  representative_amount_zat / 100000000.0 AS representative_amount_zec,
  member_count,
  first_tx_time,
  last_tx_time,
  time_span_seconds,
  confidence_score,
  confidence_margin,
  ambiguity_score,
  warning_level,
  detected_at
FROM privacy_batch_clusters
WHERE expires_at > NOW()
  AND warning_level = 'HIGH'
ORDER BY confidence_score DESC, detected_at DESC;

GRANT ALL ON TABLE privacy_linkage_edges TO zcash_user;
GRANT ALL ON SEQUENCE privacy_linkage_edges_id_seq TO zcash_user;
GRANT ALL ON TABLE privacy_batch_clusters TO zcash_user;
GRANT ALL ON SEQUENCE privacy_batch_clusters_id_seq TO zcash_user;
GRANT ALL ON TABLE high_risk_privacy_linkage_edges TO zcash_user;
GRANT ALL ON TABLE high_risk_privacy_batch_clusters TO zcash_user;

COMMIT;
