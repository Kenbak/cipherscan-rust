#[path = "support/parser_report.rs"]
mod parser_report;

#[test]
fn historical_blocks_match_pre_migration_parser_and_node_hashes() {
    let fixtures = serde_json::from_str(include_str!("fixtures/parser-blocks.json")).unwrap();
    let expected: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/parser-expected.json")).unwrap();
    let actual = parser_report::report(&fixtures).unwrap();
    // Apply the same JSON decoding to both sides. serde_json's default f64
    // parser can differ by one ULP from the original difficulty calculation;
    // integer zatoshis and every other field remain exact.
    let actual: serde_json::Value =
        serde_json::from_slice(&serde_json::to_vec(&actual).unwrap()).unwrap();
    assert_json_equal(&actual, &expected, "blocks");
    let versions: std::collections::BTreeSet<_> = actual
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|b| b["transactions"].as_array().unwrap())
        .map(|tx| tx["version"].as_i64().unwrap())
        .collect();
    for version in [4, 5, 6] {
        assert!(versions.contains(&version), "missing v{version} fixture");
    }
}

fn assert_json_equal(actual: &serde_json::Value, expected: &serde_json::Value, path: &str) {
    use serde_json::Value;
    match (actual, expected) {
        (Value::Object(a), Value::Object(b)) => {
            assert_eq!(
                a.keys().collect::<Vec<_>>(),
                b.keys().collect::<Vec<_>>(),
                "{path}: keys"
            );
            for (key, value) in a {
                assert_json_equal(value, &b[key], &format!("{path}/{key}"));
            }
        }
        (Value::Array(a), Value::Array(b)) => {
            assert_eq!(a.len(), b.len(), "{path}: length");
            for (index, (a, b)) in a.iter().zip(b).enumerate() {
                assert_json_equal(a, b, &format!("{path}/{index}"));
            }
        }
        _ => assert_eq!(actual, expected, "parser mismatch at {path}"),
    }
}

/// Structural branch-ID fixtures, not consensus-valid NU7 activation blocks:
/// signatures/proofs are intentionally not regenerated when the ID is changed.
#[test]
fn nu7_branch_id_decodes_v5_and_v6_while_historical_v4_remains_covered() {
    use cipherscan_indexer::{config::Network, indexer::TransactionParser};
    use zakura_chain::{
        block::Block,
        serialization::{ZcashDeserialize, ZcashSerialize},
    };
    let fixtures: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/parser-blocks.json")).unwrap();
    let mut covered = std::collections::BTreeSet::new();
    for fixture in fixtures.as_array().unwrap() {
        let block = Block::zcash_deserialize(std::io::Cursor::new(
            hex::decode(fixture["hex"].as_str().unwrap()).unwrap(),
        ))
        .unwrap();
        for tx in &block.transactions {
            let mut bytes = Vec::new();
            tx.zcash_serialize(&mut bytes).unwrap();
            let version = u32::from_le_bytes(bytes[..4].try_into().unwrap()) & 0x7fff_ffff;
            if ![5, 6].contains(&version) || covered.contains(&version) {
                continue;
            }
            bytes[8..12].copy_from_slice(&0x7719_0ad9u32.to_le_bytes());
            let parsed =
                TransactionParser::parse(&bytes, 6_000_000, &"0".repeat(64), Network::Mainnet)
                    .unwrap();
            assert_eq!(parsed.version as u32, version);
            bytes[8..12].copy_from_slice(&0x1234_5678u32.to_le_bytes());
            assert!(
                TransactionParser::parse(&bytes, 6_000_000, &"0".repeat(64), Network::Mainnet)
                    .is_err()
            );
            covered.insert(version);
        }
    }
    assert_eq!(covered, [5, 6].into_iter().collect());
}
