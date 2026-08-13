use crate::config::Config;
use std::time::Instant;

/// Analyze database structure (original PoC functionality)
pub(crate) fn analyze_database(config: &Config) -> Result<(), String> {
    use rocksdb::{IteratorMode, Options, DB};

    let path = &config.zebra_state_path;

    // List column families
    println!("🔍 Listing column families...");
    let cf_names =
        DB::list_cf(&Options::default(), path).map_err(|e| format!("Failed to list CFs: {}", e))?;

    println!("   Found {} column families:", cf_names.len());
    for cf in &cf_names {
        println!("      - {}", cf);
    }

    // Open with column families
    let mut opts = Options::default();
    opts.set_error_if_exists(false);
    opts.create_if_missing(false);
    opts.set_max_open_files(config.max_open_files);

    println!("\n🔓 Opening RocksDB with column families (read-only)...");
    let start = Instant::now();

    let db = DB::open_cf_for_read_only(&opts, path, &cf_names, false)
        .map_err(|e| format!("Failed to open RocksDB: {}", e))?;

    println!("✅ RocksDB opened in {:?}", start.elapsed());
    println!("\n📊 Analyzing column families...");
    println!("────────────────────────────────────────────────────────────");

    for cf_name in &cf_names {
        if let Some(cf) = db.cf_handle(cf_name.as_str()) {
            let iter = db.iterator_cf(cf, IteratorMode::Start);
            let mut count = 0;
            let mut sample_key: Option<String> = None;

            for item in iter {
                match item {
                    Ok((key, _value)) => {
                        count += 1;
                        if sample_key.is_none() && !key.is_empty() {
                            sample_key = Some(hex::encode(&key[..std::cmp::min(16, key.len())]));
                        }
                        if count >= 100000 {
                            break;
                        }
                    }
                    Err(_) => break,
                }
            }

            let sample = sample_key.unwrap_or_else(|| "N/A".to_string());
            if count > 0 {
                println!(
                    "   ✅ {:35} → {:>7} entries (sample: {}...)",
                    cf_name,
                    count,
                    &sample[..std::cmp::min(12, sample.len())]
                );
            } else {
                println!("   ⬚ {:35} → empty", cf_name);
            }
        }
    }

    // Show chain tip
    println!();
    if let Some(cf) = db.cf_handle("hash_by_height") {
        let mut last_height = 0u32;
        for item in db.iterator_cf(cf, IteratorMode::End) {
            if let Ok((key, _)) = item {
                if key.len() >= 3 {
                    last_height =
                        ((key[0] as u32) << 16) | ((key[1] as u32) << 8) | (key[2] as u32);
                }
                break;
            }
        }
        println!("📈 Chain tip height: {}", last_height);
    }

    println!("\n════════════════════════════════════════════════════════════");
    println!("✅ Analysis complete!");
    println!("════════════════════════════════════════════════════════════");

    Ok(())
}
