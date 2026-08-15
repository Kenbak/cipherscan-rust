use crate::config::Config;
use crate::db::ZebraState;

/// Verify parsing by comparing RocksDB data with Zebra RPC
pub(crate) async fn verify_parsing(
    config: &Config,
    start_height: u32,
    count: u32,
    rpc_url: &str,
    cookie_file: &str,
) -> Result<(), String> {
    use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
    use serde_json::{json, Value};

    println!("🔍 Verifying RocksDB parsing against RPC...");
    println!("   RPC URL: {}", rpc_url);
    println!("   Cookie file: {}", cookie_file);
    println!(
        "   Heights: {} to {}",
        start_height,
        start_height + count - 1
    );
    println!("────────────────────────────────────────────────────────────");

    // Read cookie for auth
    // Cookie file format: "__cookie__:password" (Zebra style)
    let cookie_content = std::fs::read_to_string(cookie_file)
        .map_err(|e| format!("Failed to read cookie file: {}", e))?;
    let cookie_trimmed = cookie_content.trim();

    // Use cookie content directly (already has __cookie__:password format)
    let auth = BASE64.encode(cookie_trimmed);
    println!("   Auth: [cookie loaded, {} bytes]", cookie_trimmed.len());
    println!();

    let zebra = ZebraState::open(config)?;
    let client = reqwest::Client::new();

    let mut matches = 0;
    let mut mismatches = 0;

    for height in start_height..start_height + count {
        // Get hash from RocksDB
        let rocks_hash = match zebra.get_block_hash(height) {
            Ok(h) => crate::util::display_hash(&h),
            Err(e) => {
                println!("   ❌ Height {}: RocksDB error - {}", height, e);
                mismatches += 1;
                continue;
            }
        };

        // Get hash from RPC
        let rpc_response = client
            .post(rpc_url)
            .header("Content-Type", "application/json")
            .header("Authorization", format!("Basic {}", auth))
            .json(&json!({
                "jsonrpc": "1.0",
                "id": "verify",
                "method": "getblockhash",
                "params": [height]
            }))
            .send()
            .await
            .map_err(|e| format!("RPC request failed: {}", e))?;

        let rpc_json: Value = rpc_response
            .json()
            .await
            .map_err(|e| format!("RPC response parse failed: {}", e))?;

        let rpc_hash = rpc_json["result"].as_str().unwrap_or("").to_string();

        if rocks_hash == rpc_hash {
            println!("   ✅ Height {:>8}: {}", height, &rocks_hash[..16]);
            matches += 1;
        } else {
            println!("   ❌ Height {:>8}: MISMATCH", height);
            println!("      RocksDB: {}", rocks_hash);
            println!("      RPC:     {}", rpc_hash);
            mismatches += 1;
        }
    }

    println!();
    println!("════════════════════════════════════════════════════════════");
    println!("📊 Verification Results:");
    println!("   ✅ Matches:    {}", matches);
    println!("   ❌ Mismatches: {}", mismatches);

    if mismatches == 0 {
        println!();
        println!("   🎉 All block hashes verified successfully!");
    }

    println!("════════════════════════════════════════════════════════════");

    // Now verify a transaction if we had matches
    if matches > 0 {
        println!();
        verify_transaction(&zebra, &client, rpc_url, &auth, start_height).await?;
    }

    Ok(())
}

/// Verify transaction parsing
async fn verify_transaction(
    zebra: &ZebraState,
    client: &reqwest::Client,
    rpc_url: &str,
    auth: &str,
    height: u32,
) -> Result<(), String> {
    use serde_json::{json, Value};

    println!("🔍 Verifying transaction parsing at height {}...", height);
    println!("────────────────────────────────────────────────────────────");

    // Get block hash
    let block_hash = { crate::util::display_hash(&zebra.get_block_hash(height)?) };

    // Get block from RPC to see transactions
    // Zebra uses verbosity 1 for decoded txs (not 2 like zcashd)
    let rpc_response = client
        .post(rpc_url)
        .header("Content-Type", "application/json")
        .header("Authorization", format!("Basic {}", auth))
        .json(&json!({
            "jsonrpc": "1.0",
            "id": "verify",
            "method": "getblock",
            "params": [block_hash, 1]  // verbosity 1 = include decoded txs in Zebra
        }))
        .send()
        .await
        .map_err(|e| format!("RPC request failed: {}", e))?;

    let rpc_json: Value = rpc_response
        .json()
        .await
        .map_err(|e| format!("RPC response parse failed: {}", e))?;

    // Check for errors (null means not found or other issue)
    if let Some(error) = rpc_json.get("error") {
        if !error.is_null() {
            println!("   ⚠️  RPC error, trying with verbosity 0...");
            // Fallback to simpler block info
            return verify_transaction_simple(zebra, client, rpc_url, auth, height, &block_hash)
                .await;
        }
    }

    let block = &rpc_json["result"];
    if block.is_null() {
        println!("   ⚠️  Block data null, trying with verbosity 0...");
        return verify_transaction_simple(zebra, client, rpc_url, auth, height, &block_hash).await;
    }

    let tx_count = block["tx"].as_array().map(|a| a.len()).unwrap_or(0);

    println!("   Block {} has {} transactions", height, tx_count);
    println!();

    // Show first few transactions from RPC
    // Zebra verbosity 1 returns tx as array of txid strings, not objects
    if let Some(txs) = block["tx"].as_array() {
        for (i, tx) in txs.iter().take(3).enumerate() {
            // Zebra returns txid as string directly, zcashd returns object with txid field
            let rpc_txid = tx.as_str().or_else(|| tx["txid"].as_str()).unwrap_or("?");

            let rpc_txid_short = if rpc_txid.len() > 16 {
                &rpc_txid[..16]
            } else {
                rpc_txid
            };
            println!("   TX {}: {} (RPC)", i, rpc_txid_short);

            // Get txid from RocksDB and compare
            match zebra.get_tx_hash_by_loc(height, i as u16) {
                Ok(hash) => {
                    let rocks_txid = crate::util::display_hash(&hash);
                    let rocks_short = if rocks_txid.len() > 16 {
                        &rocks_txid[..16]
                    } else {
                        &rocks_txid
                    };

                    if rocks_txid == rpc_txid {
                        println!("      ✅ RocksDB matches: {}", rocks_short);
                    } else {
                        println!("      ❌ MISMATCH!");
                        println!("         RPC:     {}", rpc_txid);
                        println!("         RocksDB: {}", rocks_txid);
                    }
                }
                Err(e) => {
                    println!("      ⚠️  RocksDB: {}", e);
                }
            }

            // Try to get raw tx and show parsed info
            match zebra.get_transaction_by_loc(height, i as u16) {
                Ok(raw) => {
                    // Parse header
                    if raw.len() >= 4 {
                        let header = u32::from_le_bytes([raw[0], raw[1], raw[2], raw[3]]);
                        let parsed_version = (header & 0x7FFFFFFF) as i32;
                        let overwintered = (header >> 31) == 1;
                        println!(
                            "      📋 {} bytes, v{}, overwintered={}",
                            raw.len(),
                            parsed_version,
                            overwintered
                        );
                    } else {
                        println!("      📋 {} bytes", raw.len());
                    }
                }
                Err(e) => {
                    println!("      ⚠️  Raw tx error: {}", e);
                }
            }

            println!();
        }
    }

    println!("════════════════════════════════════════════════════════════");

    Ok(())
}

/// Simple transaction verification fallback (verbosity 0)
async fn verify_transaction_simple(
    zebra: &ZebraState,
    client: &reqwest::Client,
    rpc_url: &str,
    auth: &str,
    height: u32,
    block_hash: &str,
) -> Result<(), String> {
    use serde_json::{json, Value};

    // Get block with verbosity 0 (just tx hashes)
    let rpc_response = client
        .post(rpc_url)
        .header("Content-Type", "application/json")
        .header("Authorization", format!("Basic {}", auth))
        .json(&json!({
            "jsonrpc": "1.0",
            "id": "verify",
            "method": "getblock",
            "params": [block_hash, 0]
        }))
        .send()
        .await
        .map_err(|e| format!("RPC request failed: {}", e))?;

    let rpc_json: Value = rpc_response
        .json()
        .await
        .map_err(|e| format!("RPC response parse failed: {}", e))?;

    if let Some(error) = rpc_json.get("error") {
        if !error.is_null() {
            return Err(format!("RPC error: {:?}", error));
        }
    }

    let block = &rpc_json["result"];

    // With verbosity 0, result is just a hex string of the block
    if let Some(hex_data) = block.as_str() {
        println!("   📦 Block data: {} bytes (hex)", hex_data.len() / 2);

        // Try to get first transaction from RocksDB
        for i in 0..3u16 {
            match zebra.get_tx_hash_by_loc(height, i) {
                Ok(hash) => {
                    println!(
                        "   TX {}: {} (from RocksDB)",
                        i,
                        crate::util::display_hash(&hash)
                    );
                }
                Err(_) => break,
            }
        }
    } else if let Some(txs) = block["tx"].as_array() {
        println!("   Block {} has {} transactions", height, txs.len());

        for (i, tx) in txs.iter().take(3).enumerate() {
            let txid = tx.as_str().unwrap_or("?");
            let txid_short = if txid.len() > 16 { &txid[..16] } else { txid };
            println!("   TX {}: {} (from RPC)", i, txid_short);

            // Compare with RocksDB
            match zebra.get_tx_hash_by_loc(height, i as u16) {
                Ok(hash) => {
                    let rocks_txid = crate::util::display_hash(&hash);
                    if rocks_txid == txid {
                        println!("      ✅ Matches RocksDB");
                    } else {
                        let rocks_short = if rocks_txid.len() > 16 {
                            &rocks_txid[..16]
                        } else {
                            &rocks_txid
                        };
                        println!("      ❌ RocksDB has: {}", rocks_short);
                    }
                }
                Err(e) => {
                    println!("      ⚠️  RocksDB: {}", e);
                }
            }
        }
    }

    println!();
    println!("════════════════════════════════════════════════════════════");

    Ok(())
}
