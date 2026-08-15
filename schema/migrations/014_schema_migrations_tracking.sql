-- Migration 014: schema_migrations tracking table.
--
-- Migrations 001-013 were applied manually via `psql -f` with no tracking
-- table — a real gap surfaced during the 2026-08-15 hardening pass (and
-- indirectly during the 2026-07-16 rebuild, where the schema in git was
-- discovered to be missing Ironwood/block-root columns that had been
-- applied by hand in production but never committed). This migration adds
-- the tracking table and backfills it for 001-013, whose presence was
-- verified against live production schema objects (not assumed from
-- sequential numbering) before backfilling — see the verification queries
-- in the 2026-08-15 DR hardening notes for the exact checks run.
--
-- Convention going forward: after applying any new migration by hand,
-- insert a row here in the SAME `psql -f` session (or wrap both in one
-- transaction). `applied_at` for 001-013 is the backfill time, not the
-- true original apply time (which was never recorded) — noted explicitly
-- via `backfilled = true` rather than presented as if it were accurate.

CREATE TABLE IF NOT EXISTS public.schema_migrations (
    version      text PRIMARY KEY,
    description  text NOT NULL,
    applied_at   timestamptz NOT NULL DEFAULT now(),
    backfilled   boolean NOT NULL DEFAULT false
);

INSERT INTO public.schema_migrations (version, description, backfilled) VALUES
    ('001', 'Rust indexer support (initial schema for the Rust rewrite)', true),
    ('002', 'Address pagination indexes', true),
    ('003', 'Shielded tx query index + testnet address prefix fix (003_fix_testnet_addresses.js)', true),
    ('004', 'address_transactions table + historical backfill (004_backfill_address_transactions.js)', true),
    ('005', 'Cross-chain swaps table', true),
    ('006', 'Cross-chain materialized views', true),
    ('007', 'Cross-chain raw asset columns + privacy linkage analytics', true),
    ('008', 'Fork monitor nodes + reorg tracking (fork_events, orphaned_blocks, tip_reports)', true),
    ('009', 'Ironwood (NU6.3) accounting columns', true),
    ('010', 'Boundary pool snapshots', true),
    ('011', 'Orphaned transactions table', true),
    ('012', 'Transaction anchor columns (orchard_anchor, ironwood_anchor)', true),
    ('013', 'Address accounting integrity (transparent_key_exposures, has_shielded_data)', true),
    ('014', 'This migration: schema_migrations tracking table', false)
ON CONFLICT (version) DO NOTHING;
