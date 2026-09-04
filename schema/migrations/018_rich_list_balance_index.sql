-- Migration 018: accelerate the public rich-list top-N subqueries.
--
-- Production EXPLAIN ANALYZE showed the current ORDER BY balance DESC scans
-- and sorts the full positive-balance address set. This partial index keeps
-- the write/storage cost bounded to rows that can appear in the rich list.
--
-- Online DDL: the migration runner uses autocommit; do not wrap this file in
-- a transaction. Roll back with:
--   DROP INDEX CONCURRENTLY IF EXISTS public.idx_addresses_positive_balance_desc;

CREATE INDEX CONCURRENTLY IF NOT EXISTS idx_addresses_positive_balance_desc
ON public.addresses (balance DESC)
WHERE balance > 0;
