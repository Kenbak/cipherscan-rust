-- Migration 017: cover the exact ZIP-318 migration predicate used by the
-- explorer's activity, summary, tier, cohort, and compact-scatter endpoints.
--
-- Online DDL: do not wrap this migration in a transaction. The partial index
-- contains only canonical Orchard -> Ironwood migration rows, keeping write
-- amplification bounded while allowing index-only range and tail reads.

CREATE INDEX CONCURRENTLY IF NOT EXISTS idx_transactions_ironwood_migration_query
ON public.transactions (block_height ASC, txid ASC)
INCLUDE (
    block_time,
    value_balance_ironwood,
    value_balance_orchard,
    ironwood_actions,
    orchard_actions,
    orchard_anchor,
    fee,
    expiry_height,
    locktime,
    is_coinbase
)
WHERE version = 6
  AND has_ironwood = true
  AND value_balance_orchard > 0
  AND value_balance_ironwood < 0
  AND vin_count = 0
  AND vout_count = 0;

