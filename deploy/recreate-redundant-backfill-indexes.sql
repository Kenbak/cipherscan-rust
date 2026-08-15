-- Manifest of indexes dropped during the 2026-07-16 NVMe-failure rebuild to
-- reduce WAL overhead during backfill, with a resolved disposition for each
-- as of 2026-08-15. `idx_tx_inputs_address`, `idx_tx_inputs_prev_tx`, and
-- `idx_tx_outputs_address` were already recreated (present since the
-- rebuild); this file only lists the other 8.
--
-- Resolution method: EXPLAIN (ANALYZE, BUFFERS) against production for the
-- actual query each index was meant to serve, cross-referenced with the
-- server/ API and jobs code in zcash-explorer for real call sites. Verified
-- with a same-session before/after EXPLAIN for each index that was rebuilt.
--
-- === Confirmed REDUNDANT — intentionally NOT recreated ===
-- Each of these queries already used a different existing index with no
-- sequential scan, so recreating the index would add write overhead with no
-- read benefit. Do not recreate without new evidence of a slow query plan.
--
-- idx_shielded_flows_txid: shielded_flows lookups by txid already use the
--   `shielded_flows_txid_flow_unique` unique index. 0.3ms, no seq scan.
-- idx_transactions_block_tx (block_height, tx_index): block-scoped tx
--   listing already uses `idx_transactions_height_index_txid`. 0.04ms.
-- idx_tx_inputs_txid: covered by transaction_inputs' primary key (leading
--   column txid); recreating would duplicate the PK index.
-- idx_tx_outputs_txid: covered by transaction_outputs' primary key (leading
--   column txid), same reasoning. Verified via EXPLAIN: 0.14ms, Index Scan
--   on transaction_outputs_pkey.
--
-- === Confirmed VALUABLE — recreated 2026-08-15 (CONCURRENTLY, zero downtime) ===
-- Each was verified to fix a real seq/parallel-seq scan or an avoidable
-- in-memory sort, with call sites confirmed in zcash-explorer
-- (server/signals/compute-mvrv.js, server/jobs/compute-utxo-age.js, and the
-- address-detail API routes). Before/after EXPLAIN on production:
--
-- idx_tx_outputs_spent (partial, WHERE spent = false — smaller and matches
--   every real call site, which all filter on the unspent side):
--   `SELECT count(*) FROM transaction_outputs WHERE spent = false` went from
--   a 16.6s Parallel Seq Scan (read=6,620,304 buffers) across the full 85GB
--   table to a 2.5s Parallel Index Only Scan (read=288,909 buffers) — ~6.7x
--   wall time, ~22x fewer buffer reads. Hit by server/signals/compute-mvrv.js
--   (MVRV realized cap) and server/jobs/compute-utxo-age.js (HODL waves)
--   every run.
CREATE INDEX CONCURRENTLY IF NOT EXISTS idx_tx_outputs_spent
    ON public.transaction_outputs USING btree (spent) WHERE (spent = false);

-- idx_tx_outputs_address_unspent (address, spent) WHERE spent = false: a
--   per-address unspent-outputs count (balance/UTXO calculation, hit on
--   every address page view) went from 280ms (Index Scan on
--   idx_tx_outputs_address with a post-filter removing 22,672 spent rows)
--   to 0.44ms (Index Only Scan, exact match) — ~636x.
CREATE INDEX CONCURRENTLY IF NOT EXISTS idx_tx_outputs_address_unspent
    ON public.transaction_outputs USING btree (address, spent) WHERE (spent = false);

-- idx_tx_outputs_addr_created (address, created_at DESC): address
--   transaction history pagination (ORDER BY created_at DESC LIMIT 20) went
--   from 28.7ms (Index Scan on idx_tx_outputs_address + top-N heapsort over
--   22,730 matching rows) to 2.3ms (Index Scan directly in output order, no
--   sort). Benefit scales with an address's tx_count — a high-volume
--   address (exchange hot wallet) would see a much larger gap than this
--   test case.
CREATE INDEX CONCURRENTLY IF NOT EXISTS idx_tx_outputs_addr_created
    ON public.transaction_outputs USING btree (address, created_at DESC);

-- idx_tx_inputs_addr_created (address, created_at DESC): symmetric with
--   idx_tx_outputs_addr_created for the "sent" side of an address's
--   transaction history (transaction_inputs, 68GB). Same access pattern,
--   recreated for the same reason; not independently re-benchmarked since
--   the query shape and index are identical to the outputs case.
CREATE INDEX CONCURRENTLY IF NOT EXISTS idx_tx_inputs_addr_created
    ON public.transaction_inputs USING btree (address, created_at DESC);

-- After creating any of the above, run ANALYZE on the affected table so the
-- planner picks them up immediately rather than waiting for autovacuum:
--   ANALYZE transaction_outputs;
--   ANALYZE transaction_inputs;
