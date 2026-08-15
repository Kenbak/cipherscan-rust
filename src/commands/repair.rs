use crate::config::Config;

/// Repair fees for transactions where ironwood_value_balance != 0.
/// These were computed without including the Ironwood pool balance.
/// Correct formula: fee = transparent_in + sapling_vb + orchard_vb + ironwood_vb - transparent_out
pub(crate) async fn repair_ironwood_fees(
    config: &Config,
    batch_size: usize,
    dry_run: bool,
) -> Result<(), String> {
    use crate::db::PostgresWriter;

    if config.database_url.is_empty() {
        return Err("DATABASE_URL not configured".to_string());
    }

    let postgres = PostgresWriter::connect(&config.database_url)
        .await
        .map_err(|e| format!("PostgreSQL error: {}", e))?;

    println!("🔧 Repair: recalculating fees for Ironwood transactions");
    if dry_run {
        println!("   (DRY RUN — no changes will be written)");
    }
    println!("────────────────────────────────────────────────────────────");

    // Find all transactions with nonzero ironwood_value_balance
    let rows: Vec<_> = sqlx::query(
        r#"SELECT txid, fee,
                  COALESCE(value_balance_sapling, 0) AS sapling_vb,
                  COALESCE(value_balance_orchard, 0) AS orchard_vb,
                  COALESCE(value_balance_ironwood, 0) AS ironwood_vb,
                  COALESCE(transparent_value_in, 0) AS t_in,
                  COALESCE(transparent_value_out, 0) AS t_out
           FROM transactions
           WHERE value_balance_ironwood IS NOT NULL
             AND value_balance_ironwood != 0
           ORDER BY block_height ASC"#,
    )
    .fetch_all(postgres.pool())
    .await
    .map_err(|e| format!("Query error: {}", e))?;

    let total = rows.len();
    println!(
        "   Found {} transactions with nonzero ironwood_value_balance",
        total
    );

    if total == 0 {
        println!("   ✅ Nothing to repair");
        return Ok(());
    }

    let mut updated = 0u64;
    let mut skipped = 0u64;

    for chunk in rows.chunks(batch_size) {
        let mut db_tx = postgres
            .pool()
            .begin()
            .await
            .map_err(|e| format!("Transaction begin error: {}", e))?;

        for row in chunk {
            use sqlx::Row;
            let txid: &str = row.get("txid");
            let old_fee: Option<i64> = row.get("fee");
            let sapling_vb: i64 = row.get("sapling_vb");
            let orchard_vb: i64 = row.get("orchard_vb");
            let ironwood_vb: i64 = row.get("ironwood_vb");
            let t_in: i64 = row.get("t_in");
            let t_out: i64 = row.get("t_out");

            let correct_fee = t_in + sapling_vb + orchard_vb + ironwood_vb - t_out;
            let new_fee = if correct_fee >= 0 {
                Some(correct_fee)
            } else {
                None
            };

            if new_fee == old_fee {
                skipped += 1;
                continue;
            }

            if dry_run {
                println!(
                    "   WOULD FIX {}: fee {} → {}",
                    &txid[..16.min(txid.len())],
                    old_fee.map_or("NULL".to_string(), |f| f.to_string()),
                    new_fee.map_or("NULL".to_string(), |f| f.to_string()),
                );
            } else {
                sqlx::query("UPDATE transactions SET fee = $1 WHERE txid = $2")
                    .bind(new_fee)
                    .bind(txid)
                    .execute(&mut *db_tx)
                    .await
                    .map_err(|e| format!("Update error for {}: {}", txid, e))?;
            }
            updated += 1;
        }

        if !dry_run {
            db_tx
                .commit()
                .await
                .map_err(|e| format!("Commit error: {}", e))?;
        }
    }

    println!("────────────────────────────────────────────────────────────");
    println!(
        "   ✅ {} fees {}, {} already correct",
        updated,
        if dry_run {
            "would be updated"
        } else {
            "updated"
        },
        skipped,
    );
    Ok(())
}
