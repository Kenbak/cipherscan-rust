# Zakura node and parser rollout

The parser uses `zakura-chain` 7.0.0 from the exact Zakura node `v1.4.0` tag,
with the resolved source commit recorded in Cargo.lock. These crate and node
version numbers are separate. This release is not an NU7 compatibility claim.
Historical v4 decoding remains required even after a future network disables new v4 transactions.

## Verification

- `cargo fmt --all -- --check`
- `cargo clippy --locked --release --all-targets -- -D warnings`
- `cargo test --locked --release` with `DATABASE_URL` pointing **only** at a
  disposable database initialized from `schema/postgres.sql` and migrations.
  The integration tests delete their test rows; never use a production URL.
- `tests/fixtures/README.md` records the independent historical parser baseline.
- Replay finalized blocks into a separate database with `benchmark`. Stay more
  than the node's non-finalized window behind its tip when reading RocksDB.
  Compare against the previous binary on the same range, including resolved fees.

The existing RocksDB replay harness does not fetch interpreted pool roots from
RPC. Its `final_sapling_root` can contain header commitment bytes and its Orchard
root can be null. This affects the previous parser too. Do not treat those replay
fields as an authoritative pool-root comparison or use the benchmark to overwrite
production history. Live indexing obtains final pool roots from node RPC metadata.

## Optional full-block stream

`shadow` verifies payload hashes and heights without database writes. It requires
both `ZEBRA_GRPC_URL` and working JSON-RPC settings (`ZEBRA_RPC_URL`, and
`ZEBRA_RPC_COOKIE_FILE` when authentication is enabled).

Zakura's subscription accepts known chain tips. The client supplies all valid fork
tips from `getchaintips`, plus a fresh canonical gRPC tip, on every connection.
Seeding only the canonical tip can replay historical fork branches and immediately
fill the server's listener buffer. Invalid hashes or too many tips fail the
subscription; live RPC catch-up remains authoritative for any missed payloads.

Keep `ENABLE_FULL_BLOCK_GRPC` at its current setting during the parser migration.
Enabling this optimization is a separate operational decision after shadow and
recovery checks. Chain-tip notifications and RPC block ingestion work independently.

## Staging and rollback

1. Upgrade the testnet node; verify the unchanged indexer first.
2. Validate the candidate parser offline and in a disposable database, then testnet live.
3. Upgrade mainnet standby and verify matching canonical block hashes, RPC,
   notifications, and rollback before changing the archive primary.
4. Upgrade the primary node first, verify the existing indexer, then install the
   candidate indexer. Retain the prior binaries, configuration, and node snapshot.
5. After a mainnet node restart, synchronize lightwalletd's configured credentials
   with the new cookie and restart dependent services. Never log cookie contents.
6. Verify indexed tip, failure state, API health, and lightwalletd after deployment.

No production SQL migration is required by the parser switch. Local release
branches must be incorporated before a subsequent normal main-branch deployment,
so automation does not restore an older parser. Keep deployed artifact hashes and
exact source commits in the private operational wiki.

NU7 activation fixtures, finalized network/height timing rules, NSM accounting,
and sustained end-to-end load/recovery certification remain separate work.
