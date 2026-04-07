-- Migration 007: Store raw upstream asset identifiers for diagnostic traceability
-- These columns preserve the exact originAsset / destinationAsset strings
-- returned by the NEAR Intents API so token misclassification is detectable
-- without re-querying the upstream API.

BEGIN;

ALTER TABLE cross_chain_swaps ADD COLUMN IF NOT EXISTS raw_origin_asset TEXT;
ALTER TABLE cross_chain_swaps ADD COLUMN IF NOT EXISTS raw_dest_asset TEXT;

COMMIT;
