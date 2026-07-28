-- Migration 012: Transaction anchor roots for ZIP-318 compliance
--
-- Stores the per-transaction shared_anchor (Orchard note commitment tree root)
-- from Orchard and Ironwood shielded bundles. Enables verifying whether a
-- migration tx anchors to a 144-block boundary (ZIP-318 privacy requirement).
--
-- Run:
--   sudo -u postgres psql -v ON_ERROR_STOP=1 -d <dbname> -f 012_transaction_anchors.sql

-- Transaction-level anchors
ALTER TABLE transactions
    ADD COLUMN IF NOT EXISTS orchard_anchor TEXT,
    ADD COLUMN IF NOT EXISTS ironwood_anchor TEXT;

ALTER TABLE orphaned_transactions
    ADD COLUMN IF NOT EXISTS orchard_anchor TEXT,
    ADD COLUMN IF NOT EXISTS ironwood_anchor TEXT;

-- Index on blocks.final_orchard_root for fast anchor -> block height lookups
CREATE INDEX CONCURRENTLY IF NOT EXISTS idx_blocks_final_orchard_root
    ON blocks (final_orchard_root) WHERE final_orchard_root IS NOT NULL;

CREATE INDEX CONCURRENTLY IF NOT EXISTS idx_blocks_final_ironwood_root
    ON blocks (final_ironwood_root) WHERE final_ironwood_root IS NOT NULL;
