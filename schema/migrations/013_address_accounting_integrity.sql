-- Intentionally reviewed migration for normalized, non-ownership pubkey exposure metadata.
CREATE TABLE IF NOT EXISTS transparent_key_exposures (
    txid TEXT NOT NULL,
    vout_index INTEGER NOT NULL,
    key_index INTEGER NOT NULL CHECK (key_index >= 0),
    pubkey_hex TEXT NOT NULL CHECK (length(pubkey_hex) IN (66, 130)),
    script_type TEXT NOT NULL,
    derived_address TEXT NOT NULL,
    created_at TIMESTAMP WITHOUT TIME ZONE NOT NULL DEFAULT NOW(),
    PRIMARY KEY (txid, vout_index, key_index),
    FOREIGN KEY (txid, vout_index)
        REFERENCES transaction_outputs (txid, vout_index)
        ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_transparent_key_exposures_derived
    ON transparent_key_exposures (derived_address);

COMMENT ON TABLE transparent_key_exposures IS
    'Disclosed script pubkeys; derived identifiers are analytics metadata, not ownership';

ALTER TABLE transparent_key_exposures OWNER TO zcash_user;
