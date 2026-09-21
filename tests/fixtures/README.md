# Zakura parser compatibility fixtures

Captured on 2026-09-21 JST from CipherScan's own nodes using `getblockhash` and
`getblock` (verbosity 0 and 1). Mainnet was Zakura 1.3.0; testnet was 1.4.0.
Raw blocks and RPC transaction IDs are public chain data; no credentials are stored.

The 18 blocks cover both sides of Sapling, NU5, NU6.2, and NU6.3 activation on
mainnet and testnet, plus one recent block per network. They contain 70 transactions:
11 v3, 7 v4, 22 v5, and 30 v6, including 2 Sapling spends, 4 Sapling outputs,
18 Orchard actions, and 56 Ironwood actions.

`parser-expected.json` was generated with the unchanged parser at indexer commit
`115c40a`, using Zebra tag `v6.0.0` (crate 11.1.0, commit `bb41d69013ed`). The
baseline parser source is also identical to deployed mainnet commit `1f03081`.
The new parser is Zakura tag `v1.4.0` (crate 7.0.0, commit `1e36d1bb6a8a`).
Both Linux reports were byte-for-byte identical before saving these fixtures.

Run `cargo test --locked --test parser_compatibility`. The regression checks
full-block deserialization/serialization, complete input consumption, coinbase
height, RPC block/transaction hashes, all indexed header fields, and all raw
transaction fields (scripts, transparent outputs, shielded counts/balances/anchors).
The JSON comparison decodes both reports equally to avoid a one-ULP difference
from serde_json's default floating-point parser; zatoshi amounts remain integers.

Regenerate a report with:

```sh
cargo run --locked --release --example parser-fixture-report -- tests/fixtures/parser-blocks.json
```

Do not regenerate expected results from the candidate merely to make a failing
test pass. Establish an independently reviewed baseline and investigate each change.

These fixtures do not resolve historical prevouts or prove database/fee accounting,
stream recovery, or sustained capacity. Those require separate integration and
isolated replay checks. NU7 is not represented: add authoritative activation fixtures
when the supporting release and chain data are available. Keep historical v4 coverage.
