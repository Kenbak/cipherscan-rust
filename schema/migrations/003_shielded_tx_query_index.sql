-- Migration: Add index for shielded transaction queries
-- 
-- The homepage and /api/tx/shielded endpoint filter on has_sapling/has_orchard
-- and ORDER BY block_height DESC. Without a targeted index, Postgres does a
-- sequential scan across millions of rows.
--
-- This partial index only covers rows that have shielded activity,
-- keeping it compact while making the query near-instant.

CREATE INDEX CONCURRENTLY IF NOT EXISTS idx_transactions_shielded_height
ON transactions(block_height DESC)
WHERE has_sapling = true OR has_orchard = true;

ANALYZE transactions;
