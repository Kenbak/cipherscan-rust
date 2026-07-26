-- Migration 010: Boundary pool snapshots for historical pool balance tracking
--
-- Captures authoritative Zebra pool sizes at each 256-block boundary during
-- live indexing. Enables accurate historical pool balance queries without
-- proportional scaling approximations.
--
-- Run:
--   sudo -u postgres psql -v ON_ERROR_STOP=1 -d <dbname> -f 010_boundary_pool_snapshots.sql

CREATE TABLE IF NOT EXISTS boundary_pool_snapshots (
    boundary_height INTEGER PRIMARY KEY,
    block_time      BIGINT NOT NULL,
    orchard_zat     BIGINT NOT NULL,
    ironwood_zat    BIGINT NOT NULL,
    sapling_zat     BIGINT NOT NULL,
    sprout_zat      BIGINT NOT NULL,
    transparent_zat BIGINT,
    chain_supply_zat BIGINT,
    created_at      TIMESTAMP WITHOUT TIME ZONE DEFAULT NOW() NOT NULL
);

GRANT SELECT, INSERT, UPDATE, DELETE ON boundary_pool_snapshots TO zcash_user;
