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
