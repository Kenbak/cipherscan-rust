//! Offline compatibility report; never connects to a node or database.
#[path = "../tests/support/parser_report.rs"]
mod parser_report;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::args()
        .nth(1)
        .ok_or("usage: parser-fixture-report FIXTURES.json")?;
    let fixtures = serde_json::from_slice(&std::fs::read(path)?)?;
    println!(
        "{}",
        serde_json::to_string_pretty(&parser_report::report(&fixtures)?)?
    );
    Ok(())
}
