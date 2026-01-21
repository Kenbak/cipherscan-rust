-- ============================================================================
-- MIGRATION 001: Support Rust Indexer + Privacy Analysis V2
-- Date: 2026-01-21
-- ============================================================================
-- This migration adds columns and tables needed for:
-- - Rust indexer (detailed shielded data)
-- - Address clustering (PRIVACY_ANALYSIS_V2.md section 1.2)
-- - Privacy scoring (RUST_INDEXER_V2.md section 3.3)
-- - Pool migration tracking
-- ============================================================================

-- ============================================================================
-- 1. TRANSACTIONS: Add missing shielded detail columns
-- ============================================================================

ALTER TABLE transactions ADD COLUMN IF NOT EXISTS expiry_height INTEGER;
ALTER TABLE transactions ADD COLUMN IF NOT EXISTS sapling_spend_count INTEGER DEFAULT 0;
ALTER TABLE transactions ADD COLUMN IF NOT EXISTS sapling_output_count INTEGER DEFAULT 0;
ALTER TABLE transactions ADD COLUMN IF NOT EXISTS sprout_joinsplit_count INTEGER DEFAULT 0;

-- Privacy score (0-100, computed during indexing)
ALTER TABLE transactions ADD COLUMN IF NOT EXISTS privacy_score SMALLINT;

-- Flow classification
ALTER TABLE transactions ADD COLUMN IF NOT EXISTS flow_type TEXT; -- 'shield', 'deshield', 'fully_shielded', 'pool_migration', 'mixed'

-- ============================================================================
-- 2. TRANSACTION_OUTPUTS: Add script type
-- ============================================================================

ALTER TABLE transaction_outputs ADD COLUMN IF NOT EXISTS script_type TEXT; -- 'pubkeyhash', 'scripthash', 'nulldata', 'nonstandard'

-- ============================================================================
-- 3. SHIELDED_FLOWS: Add detailed counts
-- ============================================================================

ALTER TABLE shielded_flows ADD COLUMN IF NOT EXISTS sapling_spend_count INTEGER DEFAULT 0;
ALTER TABLE shielded_flows ADD COLUMN IF NOT EXISTS sapling_output_count INTEGER DEFAULT 0;
ALTER TABLE shielded_flows ADD COLUMN IF NOT EXISTS orchard_action_count INTEGER DEFAULT 0;

-- Pool migration tracking (for Sapling ↔ Orchard migrations)
ALTER TABLE shielded_flows ADD COLUMN IF NOT EXISTS is_pool_migration BOOLEAN DEFAULT FALSE;
ALTER TABLE shielded_flows ADD COLUMN IF NOT EXISTS migration_from_pool TEXT; -- 'sapling' or 'orchard'
ALTER TABLE shielded_flows ADD COLUMN IF NOT EXISTS migration_to_pool TEXT;

-- ============================================================================
-- 4. ADDRESS_CLUSTERS: For privacy analysis (from PRIVACY_ANALYSIS_V2.md)
-- ============================================================================

CREATE TABLE IF NOT EXISTS address_clusters (
    id SERIAL PRIMARY KEY,
    cluster_id UUID NOT NULL,
    address TEXT NOT NULL UNIQUE,
    confidence FLOAT DEFAULT 1.0,
    heuristic TEXT, -- 'common_input', 'change', 'temporal', 'amount_fingerprint'
    first_seen_txid TEXT,
    created_at TIMESTAMPTZ DEFAULT NOW(),
    updated_at TIMESTAMPTZ DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_clusters_cluster_id ON address_clusters(cluster_id);
CREATE INDEX IF NOT EXISTS idx_clusters_address ON address_clusters(address);
CREATE INDEX IF NOT EXISTS idx_clusters_heuristic ON address_clusters(heuristic);

-- ============================================================================
-- 5. ADDRESS_RELATIONS: Track relationships between addresses
-- ============================================================================

CREATE TABLE IF NOT EXISTS address_relations (
    id SERIAL PRIMARY KEY,
    address_a TEXT NOT NULL,
    address_b TEXT NOT NULL,
    relation_type TEXT NOT NULL, -- 'common_input', 'change_output', 'temporal_pattern'
    confidence FLOAT NOT NULL DEFAULT 1.0,
    txid TEXT NOT NULL,
    block_height INTEGER,
    created_at TIMESTAMPTZ DEFAULT NOW(),
    UNIQUE(address_a, address_b, txid)
);

CREATE INDEX IF NOT EXISTS idx_relations_address_a ON address_relations(address_a);
CREATE INDEX IF NOT EXISTS idx_relations_address_b ON address_relations(address_b);
CREATE INDEX IF NOT EXISTS idx_relations_type ON address_relations(relation_type);
CREATE INDEX IF NOT EXISTS idx_relations_txid ON address_relations(txid);

-- ============================================================================
-- 6. INDEXER_STATE: Track Rust indexer progress
-- ============================================================================

CREATE TABLE IF NOT EXISTS indexer_state (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL,
    updated_at TIMESTAMPTZ DEFAULT NOW()
);

-- Example entries:
-- ('last_indexed_height', '3200000')
-- ('indexer_version', '0.1.0')
-- ('backfill_started_at', '2026-01-21T12:00:00Z')

-- ============================================================================
-- 7. IMPORTANT INDEXES (for performance)
-- ============================================================================

-- Address lookups on inputs/outputs
CREATE INDEX IF NOT EXISTS idx_tx_outputs_address ON transaction_outputs(address) WHERE address IS NOT NULL;
CREATE INDEX IF NOT EXISTS idx_tx_inputs_address ON transaction_inputs(address) WHERE address IS NOT NULL;

-- GIN index for array search on transparent_addresses
CREATE INDEX IF NOT EXISTS idx_shielded_flows_addresses ON shielded_flows USING GIN(transparent_addresses);

-- Flow type filtering
CREATE INDEX IF NOT EXISTS idx_tx_flow_type ON transactions(flow_type) WHERE flow_type IS NOT NULL;

-- Privacy score ranges
CREATE INDEX IF NOT EXISTS idx_tx_privacy_score ON transactions(privacy_score) WHERE privacy_score IS NOT NULL;

-- ============================================================================
-- 8. VERIFICATION
-- ============================================================================

DO $$
BEGIN
    RAISE NOTICE 'Migration 001 complete!';
    RAISE NOTICE 'New tables: address_clusters, address_relations, indexer_state';
    RAISE NOTICE 'New columns in transactions: expiry_height, sapling_spend_count, sapling_output_count, sprout_joinsplit_count, privacy_score, flow_type';
    RAISE NOTICE 'New columns in shielded_flows: sapling_spend_count, sapling_output_count, orchard_action_count, is_pool_migration, migration_from_pool, migration_to_pool';
END $$;
