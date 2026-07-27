//! PostgreSQL writer for indexed data
//!
//! Writes processed blockchain data to PostgreSQL for querying.
//! Uses UPSERT (INSERT ON CONFLICT) to allow parallel backfill and live indexing.

use crate::models::{ShieldedFlow, Transaction};
use sqlx::{postgres::PgPoolOptions, PgPool, Postgres, QueryBuilder};

/// PostgreSQL connection and writer
pub struct PostgresWriter {
    pool: PgPool,
}

impl PostgresWriter {
    /// Connect to PostgreSQL
    pub async fn connect(database_url: &str) -> Result<Self, sqlx::Error> {
        let pool = PgPoolOptions::new()
            .max_connections(10)
            .connect(database_url)
            .await?;

        tracing::info!("Connected to PostgreSQL");

        Ok(Self { pool })
    }

    /// Update indexer state (checkpoint)
    pub async fn update_checkpoint(&self, key: &str, value: &str) -> Result<(), sqlx::Error> {
        sqlx::query(
            r#"
            INSERT INTO indexer_state (key, value, updated_at)
            VALUES ($1, $2, NOW())
            ON CONFLICT (key) DO UPDATE SET
                value = EXCLUDED.value,
                updated_at = NOW()
            "#,
        )
        .bind(key)
        .bind(value)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    /// Get indexer state value
    pub async fn get_state(&self, key: &str) -> Result<Option<String>, sqlx::Error> {
        let result: Option<(String,)> =
            sqlx::query_as("SELECT value FROM indexer_state WHERE key = $1")
                .bind(key)
                .fetch_optional(&self.pool)
                .await?;

        Ok(result.map(|(v,)| v))
    }

    /// Delete an indexer state value when it no longer applies.
    pub async fn delete_state(&self, key: &str) -> Result<(), sqlx::Error> {
        sqlx::query("DELETE FROM indexer_state WHERE key = $1")
            .bind(key)
            .execute(&self.pool)
            .await?;

        Ok(())
    }

    /// Get checkpoint (convenience method)
    pub async fn get_checkpoint(&self) -> Result<Option<u32>, sqlx::Error> {
        match self.get_state("last_indexed_height").await? {
            Some(v) => Ok(v.parse().ok()),
            None => Ok(None),
        }
    }

    /// Get checkpoint by specific key
    pub async fn get_checkpoint_key(&self, key: &str) -> Result<Option<u32>, sqlx::Error> {
        match self.get_state(key).await? {
            Some(v) => Ok(v.parse().ok()),
            None => Ok(None),
        }
    }

    /// Batch insert with full block header info and optional flows in one DB transaction.
    pub async fn batch_insert_with_header_and_flows(
        &self,
        height: u32,
        hash: &str,
        timestamp: u64,
        transactions: &[Transaction],
        flows: &[ShieldedFlow],
        header: &crate::db::ParsedBlockHeader,
    ) -> Result<(u64, u64), sqlx::Error> {
        let mut db_tx = self.pool.begin().await?;
        let mut count = 0u64;

        // Calculate block-level aggregates
        let total_fees: i64 = transactions.iter().filter_map(|tx| tx.fee).sum();

        // Block size = sum of all tx sizes + header size (~1487 bytes for Zcash)
        // Header: 4 (version) + 32 (prev_hash) + 32 (merkle) + 32 (reserved) + 4 (time)
        //       + 4 (bits) + 32 (nonce) + 3 (solution length) + 1344 (solution) = ~1487
        const HEADER_SIZE: i32 = 1487;
        let tx_sizes: i32 = transactions.iter().map(|tx| tx.size as i32).sum();
        let block_size = tx_sizes + HEADER_SIZE;

        // Miner address = first output of coinbase transaction
        let miner_address: Option<String> = transactions.first().and_then(|coinbase| {
            if coinbase.vin.first().map(|v| v.is_coinbase).unwrap_or(false) {
                coinbase.vout.first().and_then(|out| out.address.clone())
            } else {
                None
            }
        });

        // Coinbase hex = script_sig of the coinbase input (miner's embedded data)
        let coinbase_hex: Option<String> = transactions.first().and_then(|coinbase| {
            coinbase.vin.first().and_then(|input| {
                if input.is_coinbase {
                    input.script_sig.clone()
                } else {
                    None
                }
            })
        });

        // Insert block with all header fields
        sqlx::query(
            r#"
            INSERT INTO blocks (
                height, hash, timestamp, transaction_count, total_fees,
                version, merkle_root, final_sapling_root, final_orchard_root,
                bits, nonce, solution,
                difficulty, previous_block_hash, size, miner_address, coinbase_hex,
                final_ironwood_root
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17, $18)
            ON CONFLICT (height) DO UPDATE SET
                hash = EXCLUDED.hash,
                transaction_count = EXCLUDED.transaction_count,
                total_fees = EXCLUDED.total_fees,
                version = EXCLUDED.version,
                merkle_root = EXCLUDED.merkle_root,
                final_sapling_root = EXCLUDED.final_sapling_root,
                final_orchard_root = EXCLUDED.final_orchard_root,
                bits = EXCLUDED.bits,
                nonce = EXCLUDED.nonce,
                solution = EXCLUDED.solution,
                difficulty = EXCLUDED.difficulty,
                previous_block_hash = EXCLUDED.previous_block_hash,
                size = EXCLUDED.size,
                miner_address = EXCLUDED.miner_address,
                coinbase_hex = EXCLUDED.coinbase_hex,
                final_ironwood_root = EXCLUDED.final_ironwood_root
            "#,
        )
        .bind(height as i64)
        .bind(hash)
        .bind(timestamp as i64)
        .bind(transactions.len() as i32)
        .bind(total_fees)
        .bind(header.version)
        .bind(&header.merkle_root)
        .bind(&header.final_sapling_root)
        .bind(&header.final_orchard_root)
        .bind(&header.bits)
        .bind(&header.nonce)
        .bind(&header.solution)
        .bind(header.difficulty)
        .bind(&header.previous_block_hash)
        .bind(block_size)
        .bind(&miner_address)
        .bind(&coinbase_hex)
        .bind(&header.final_ironwood_root)
        .execute(&mut *db_tx)
        .await?;

        let use_rowwise_writes = std::env::var_os("CIPHERSCAN_ROWWISE_WRITES").is_some();
        if !use_rowwise_writes {
            // Collapse the transaction, input, output, and address-activity writes
            // into chunked multi-row UPSERTs. The previous row-wise path incurred
            // thousands of local client/server round trips for transparent blocks.
            self.bulk_insert_transaction_data(&mut db_tx, transactions, timestamp)
                .await?;
            count = transactions.len() as u64;
        }

        // Keep an emergency rollback path while the bulk implementation is being
        // introduced. Setting CIPHERSCAN_ROWWISE_WRITES restores the old behavior.
        if use_rowwise_writes {
            count = 0;
            for (tx_idx, tx) in transactions.iter().enumerate() {
                // Insert transaction
                let has_sapling = tx.sapling_spends > 0 || tx.sapling_outputs > 0;
                let has_orchard = tx.orchard_actions > 0;
                let has_ironwood = tx.ironwood_actions > 0;
                let is_coinbase = tx.vin.first().map(|v| v.is_coinbase).unwrap_or(false);

                let has_sprout = tx.joinsplit_count > 0;

                sqlx::query(
                    r#"
                INSERT INTO transactions (
                    txid, block_height, block_hash, version, locktime,
                    size, fee, total_input, total_output,
                    shielded_spends, shielded_outputs, orchard_actions,
                    value_balance_sapling, value_balance_orchard,
                    is_coinbase, has_sapling, has_orchard,
                    vin_count, vout_count, block_time, tx_index,
                    ironwood_actions, value_balance_ironwood, has_ironwood, value_balance,
                    expiry_height, sapling_spend_count, sapling_output_count,
                    sprout_joinsplit_count, has_sprout
                ) VALUES (
                    $1, $2, $3, $4, $5, $6, $7, $8, $9,
                    $10, $11, $12, $13, $14, $15, $16, $17, $18, $19, $20, $21,
                    $22, $23, $24, $25,
                    $26, $27, $28, $29, $30
                )
                ON CONFLICT (txid) DO UPDATE SET
                    block_height = EXCLUDED.block_height,
                    block_hash = EXCLUDED.block_hash,
                    fee = EXCLUDED.fee,
                    total_input = EXCLUDED.total_input,
                    total_output = EXCLUDED.total_output,
                    is_coinbase = EXCLUDED.is_coinbase,
                    tx_index = EXCLUDED.tx_index,
                    ironwood_actions = EXCLUDED.ironwood_actions,
                    value_balance_ironwood = EXCLUDED.value_balance_ironwood,
                    has_ironwood = EXCLUDED.has_ironwood,
                    value_balance = EXCLUDED.value_balance,
                    locktime = EXCLUDED.locktime,
                    expiry_height = EXCLUDED.expiry_height,
                    sapling_spend_count = EXCLUDED.sapling_spend_count,
                    sapling_output_count = EXCLUDED.sapling_output_count,
                    sprout_joinsplit_count = EXCLUDED.sprout_joinsplit_count,
                    has_sprout = EXCLUDED.has_sprout
                "#,
                )
                .bind(&tx.txid) // $1
                .bind(tx.block_height as i64) // $2
                .bind(&tx.block_hash) // $3
                .bind(tx.version) // $4
                .bind(tx.lock_time as i64) // $5
                .bind(tx.size as i32) // $6
                .bind(tx.fee.unwrap_or(0)) // $7
                .bind(tx.transparent_value_in) // $8
                .bind(tx.transparent_value_out) // $9
                .bind(tx.sapling_spends as i32) // $10
                .bind(tx.sapling_outputs as i32) // $11
                .bind(tx.orchard_actions as i32) // $12
                .bind(tx.sapling_value_balance) // $13
                .bind(tx.orchard_value_balance) // $14
                .bind(is_coinbase) // $15
                .bind(has_sapling) // $16
                .bind(has_orchard) // $17
                .bind(tx.vin_count as i32) // $18
                .bind(tx.vout_count as i32) // $19
                .bind(timestamp as i64) // $20
                .bind(tx_idx as i32) // $21
                .bind(tx.ironwood_actions as i32) // $22
                .bind(tx.ironwood_value_balance) // $23
                .bind(has_ironwood) // $24
                .bind(
                    tx.sapling_value_balance + tx.orchard_value_balance + tx.ironwood_value_balance,
                ) // $25
                .bind(tx.expiry_height.map(|h| h as i32)) // $26
                .bind(tx.sapling_spends as i32) // $27
                .bind(tx.sapling_outputs as i32) // $28
                .bind(tx.joinsplit_count as i32) // $29
                .bind(has_sprout) // $30
                .execute(&mut *db_tx)
                .await?;

                // Insert outputs
                for output in &tx.vout {
                    sqlx::query(
                        r#"
                    INSERT INTO transaction_outputs (txid, vout_index, value, address, script_type)
                    VALUES ($1, $2, $3, $4, $5)
                    ON CONFLICT (txid, vout_index) DO UPDATE SET
                        value = EXCLUDED.value,
                        address = EXCLUDED.address,
                        script_type = EXCLUDED.script_type
                    "#,
                    )
                    .bind(&tx.txid)
                    .bind(output.n as i32)
                    .bind(output.value)
                    .bind(&output.address)
                    .bind(&output.script_type)
                    .execute(&mut *db_tx)
                    .await?;
                }

                // Insert inputs (skip coinbase)
                for (i, input) in tx.vin.iter().enumerate() {
                    if input.is_coinbase {
                        continue;
                    }

                    sqlx::query(
                    r#"
                    INSERT INTO transaction_inputs (txid, vout_index, prev_txid, prev_vout, address, value)
                    VALUES ($1, $2, $3, $4, $5, $6)
                    ON CONFLICT (txid, vout_index) DO UPDATE SET
                        address = EXCLUDED.address,
                        value = EXCLUDED.value
                    "#
                )
                .bind(&tx.txid)
                .bind(i as i32)
                .bind(&input.txid)
                .bind(input.vout as i32)
                .bind(&input.address)
                .bind(input.value)
                .execute(&mut *db_tx)
                .await?;
                }

                // Insert into address_transactions (denormalized lookup table)
                {
                    use std::collections::HashMap;
                    let mut addr_map: HashMap<&str, (i64, i64)> = HashMap::new();

                    for output in &tx.vout {
                        if let Some(ref addr) = output.address {
                            let entry = addr_map.entry(addr.as_str()).or_insert((0, 0));
                            entry.1 = entry.1.checked_add(output.value).unwrap_or(i64::MAX);
                        }
                    }
                    for input in &tx.vin {
                        if input.is_coinbase {
                            continue;
                        }
                        if let Some(ref addr) = input.address {
                            let entry = addr_map.entry(addr.as_str()).or_insert((0, 0));
                            entry.0 = entry.0.checked_add(input.value.unwrap_or(0)).unwrap_or(i64::MAX);
                        }
                    }

                    for (addr, (val_in, val_out)) in &addr_map {
                        sqlx::query(
                        r#"
                        INSERT INTO address_transactions (address, txid, block_height, tx_index, block_time, is_input, is_output, value_in, value_out)
                        VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
                        ON CONFLICT (address, block_height, tx_index, txid)
                        DO UPDATE SET is_input = EXCLUDED.is_input OR address_transactions.is_input,
                                      is_output = EXCLUDED.is_output OR address_transactions.is_output,
                                      value_in = EXCLUDED.value_in,
                                      value_out = EXCLUDED.value_out
                        "#
                    )
                    .bind(addr)
                    .bind(&tx.txid)
                    .bind(tx.block_height as i32)
                    .bind(tx_idx as i32)
                    .bind(timestamp as i64)
                    .bind(*val_in > 0)
                    .bind(*val_out > 0)
                    .bind(*val_in)
                    .bind(*val_out)
                    .execute(&mut *db_tx)
                    .await?;
                    }
                }

                count += 1;
            }
        }

        let flow_count = self.insert_flows_tx(&mut db_tx, flows, timestamp).await?;

        // Update addresses table (aggregate per-address for this block)
        self.update_addresses_for_block(&mut db_tx, transactions, timestamp)
            .await?;

        db_tx.commit().await?;
        Ok((count, flow_count))
    }

    async fn bulk_insert_transaction_data(
        &self,
        db_tx: &mut sqlx::Transaction<'_, Postgres>,
        transactions: &[Transaction],
        timestamp: u64,
    ) -> Result<(), sqlx::Error> {
        const TX_CHUNK_SIZE: usize = 2_000;
        for (chunk_index, chunk) in transactions.chunks(TX_CHUNK_SIZE).enumerate() {
            let tx_index_offset = chunk_index * TX_CHUNK_SIZE;
            let mut query = QueryBuilder::<Postgres>::new(
                r#"
                INSERT INTO transactions (
                    txid, block_height, block_hash, version, locktime,
                    size, fee, total_input, total_output,
                    shielded_spends, shielded_outputs, orchard_actions,
                    value_balance_sapling, value_balance_orchard,
                    is_coinbase, has_sapling, has_orchard,
                    vin_count, vout_count, block_time, tx_index,
                    ironwood_actions, value_balance_ironwood, has_ironwood, value_balance,
                    expiry_height, sapling_spend_count, sapling_output_count,
                    sprout_joinsplit_count, has_sprout
                ) "#,
            );
            query.push_values(chunk.iter().enumerate(), |mut row, (index, tx)| {
                let has_sapling = tx.sapling_spends > 0 || tx.sapling_outputs > 0;
                let has_orchard = tx.orchard_actions > 0;
                let has_ironwood = tx.ironwood_actions > 0;
                let has_sprout = tx.joinsplit_count > 0;
                let is_coinbase = tx.vin.first().map(|vin| vin.is_coinbase).unwrap_or(false);
                let value_balance =
                    tx.sapling_value_balance + tx.orchard_value_balance + tx.ironwood_value_balance;

                row.push_bind(&tx.txid)
                    .push_bind(tx.block_height as i64)
                    .push_bind(&tx.block_hash)
                    .push_bind(tx.version)
                    .push_bind(tx.lock_time as i64)
                    .push_bind(tx.size as i32)
                    .push_bind(tx.fee.unwrap_or(0))
                    .push_bind(tx.transparent_value_in)
                    .push_bind(tx.transparent_value_out)
                    .push_bind(tx.sapling_spends as i32)
                    .push_bind(tx.sapling_outputs as i32)
                    .push_bind(tx.orchard_actions as i32)
                    .push_bind(tx.sapling_value_balance)
                    .push_bind(tx.orchard_value_balance)
                    .push_bind(is_coinbase)
                    .push_bind(has_sapling)
                    .push_bind(has_orchard)
                    .push_bind(tx.vin_count as i32)
                    .push_bind(tx.vout_count as i32)
                    .push_bind(timestamp as i64)
                    .push_bind((tx_index_offset + index) as i32)
                    .push_bind(tx.ironwood_actions as i32)
                    .push_bind(tx.ironwood_value_balance)
                    .push_bind(has_ironwood)
                    .push_bind(value_balance)
                    .push_bind(tx.expiry_height.map(|height| height as i32))
                    .push_bind(tx.sapling_spends as i32)
                    .push_bind(tx.sapling_outputs as i32)
                    .push_bind(tx.joinsplit_count as i32)
                    .push_bind(has_sprout);
            });
            query.push(
                r#"
                ON CONFLICT (txid) DO UPDATE SET
                    block_height = EXCLUDED.block_height,
                    block_hash = EXCLUDED.block_hash,
                    fee = EXCLUDED.fee,
                    total_input = EXCLUDED.total_input,
                    total_output = EXCLUDED.total_output,
                    is_coinbase = EXCLUDED.is_coinbase,
                    tx_index = EXCLUDED.tx_index,
                    ironwood_actions = EXCLUDED.ironwood_actions,
                    value_balance_ironwood = EXCLUDED.value_balance_ironwood,
                    has_ironwood = EXCLUDED.has_ironwood,
                    value_balance = EXCLUDED.value_balance,
                    locktime = EXCLUDED.locktime,
                    expiry_height = EXCLUDED.expiry_height,
                    sapling_spend_count = EXCLUDED.sapling_spend_count,
                    sapling_output_count = EXCLUDED.sapling_output_count,
                    sprout_joinsplit_count = EXCLUDED.sprout_joinsplit_count,
                    has_sprout = EXCLUDED.has_sprout
                "#,
            );
            query.build().execute(&mut **db_tx).await?;
        }

        let outputs: Vec<_> = transactions
            .iter()
            .flat_map(|tx| tx.vout.iter().map(move |output| (&tx.txid, output)))
            .collect();
        const OUTPUT_CHUNK_SIZE: usize = 10_000;
        for chunk in outputs.chunks(OUTPUT_CHUNK_SIZE) {
            let mut query = QueryBuilder::<Postgres>::new(
                "INSERT INTO transaction_outputs (txid, vout_index, value, address, script_type) ",
            );
            query.push_values(chunk, |mut row, (txid, output)| {
                row.push_bind(*txid)
                    .push_bind(output.n as i32)
                    .push_bind(output.value)
                    .push_bind(&output.address)
                    .push_bind(&output.script_type);
            });
            query.push(
                r#"
                ON CONFLICT (txid, vout_index) DO UPDATE SET
                    value = EXCLUDED.value,
                    address = EXCLUDED.address,
                    script_type = EXCLUDED.script_type
                "#,
            );
            query.build().execute(&mut **db_tx).await?;
        }

        let inputs: Vec<_> = transactions
            .iter()
            .flat_map(|tx| {
                tx.vin
                    .iter()
                    .enumerate()
                    .filter(|(_, input)| !input.is_coinbase)
                    .map(move |(index, input)| (&tx.txid, index, input))
            })
            .collect();
        const INPUT_CHUNK_SIZE: usize = 10_000;
        for chunk in inputs.chunks(INPUT_CHUNK_SIZE) {
            let mut query = QueryBuilder::<Postgres>::new(
                "INSERT INTO transaction_inputs (txid, vout_index, prev_txid, prev_vout, address, value) ",
            );
            query.push_values(chunk, |mut row, (txid, index, input)| {
                row.push_bind(*txid)
                    .push_bind(*index as i32)
                    .push_bind(&input.txid)
                    .push_bind(input.vout as i32)
                    .push_bind(&input.address)
                    .push_bind(input.value);
            });
            query.push(
                r#"
                ON CONFLICT (txid, vout_index) DO UPDATE SET
                    address = EXCLUDED.address,
                    value = EXCLUDED.value
                "#,
            );
            query.build().execute(&mut **db_tx).await?;
        }

        struct AddressTransactionRow<'a> {
            address: &'a str,
            txid: &'a str,
            block_height: i32,
            tx_index: i32,
            value_in: i64,
            value_out: i64,
        }

        let mut address_transactions = Vec::new();
        for (tx_index, tx) in transactions.iter().enumerate() {
            use std::collections::HashMap;
            let mut addresses: HashMap<&str, (i64, i64)> = HashMap::new();

            for output in &tx.vout {
                if let Some(address) = output.address.as_deref() {
                    let e = addresses.entry(address).or_insert((0, 0));
                    e.1 = e.1.checked_add(output.value).unwrap_or(i64::MAX);
                }
            }
            for input in &tx.vin {
                if !input.is_coinbase {
                    if let Some(address) = input.address.as_deref() {
                        let e = addresses.entry(address).or_insert((0, 0));
                        e.0 = e.0.checked_add(input.value.unwrap_or(0)).unwrap_or(i64::MAX);
                    }
                }
            }

            address_transactions.extend(addresses.into_iter().map(
                |(address, (value_in, value_out))| AddressTransactionRow {
                    address,
                    txid: &tx.txid,
                    block_height: tx.block_height as i32,
                    tx_index: tx_index as i32,
                    value_in,
                    value_out,
                },
            ));
        }

        const ADDRESS_TX_CHUNK_SIZE: usize = 7_000;
        for chunk in address_transactions.chunks(ADDRESS_TX_CHUNK_SIZE) {
            let mut query = QueryBuilder::<Postgres>::new(
                r#"
                INSERT INTO address_transactions (
                    address, txid, block_height, tx_index, block_time,
                    is_input, is_output, value_in, value_out
                ) "#,
            );
            query.push_values(chunk, |mut row, item| {
                row.push_bind(item.address)
                    .push_bind(item.txid)
                    .push_bind(item.block_height)
                    .push_bind(item.tx_index)
                    .push_bind(timestamp as i64)
                    .push_bind(item.value_in > 0)
                    .push_bind(item.value_out > 0)
                    .push_bind(item.value_in)
                    .push_bind(item.value_out);
            });
            query.push(
                r#"
                ON CONFLICT (address, block_height, tx_index, txid)
                DO UPDATE SET
                    is_input = EXCLUDED.is_input OR address_transactions.is_input,
                    is_output = EXCLUDED.is_output OR address_transactions.is_output,
                    value_in = EXCLUDED.value_in,
                    value_out = EXCLUDED.value_out
                "#,
            );
            query.build().execute(&mut **db_tx).await?;
        }

        Ok(())
    }

    /// Batch insert with full block header info.
    /// Update the addresses summary table for all addresses in a block's transactions
    async fn update_addresses_for_block(
        &self,
        db_tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
        transactions: &[Transaction],
        block_time: u64,
    ) -> Result<(), sqlx::Error> {
        use std::collections::HashMap;

        // Aggregate: address -> (total_received, total_sent, set of txids)
        struct AddrStats {
            total_received: i64,
            total_sent: i64,
            txids: std::collections::HashSet<String>,
        }

        let mut addr_map: HashMap<String, AddrStats> = HashMap::new();

        for tx in transactions {
            // Outputs = received
            for output in &tx.vout {
                if let Some(ref address) = output.address {
                    let entry = addr_map
                        .entry(address.clone())
                        .or_insert_with(|| AddrStats {
                            total_received: 0,
                            total_sent: 0,
                            txids: std::collections::HashSet::new(),
                        });
                    entry.total_received = entry.total_received
                        .checked_add(output.value)
                        .unwrap_or(i64::MAX);
                    entry.txids.insert(tx.txid.clone());
                }
            }

            // Inputs = sent
            for input in &tx.vin {
                if input.is_coinbase {
                    continue;
                }
                if let Some(ref address) = input.address {
                    if let Some(value) = input.value {
                        let entry = addr_map
                            .entry(address.clone())
                            .or_insert_with(|| AddrStats {
                                total_received: 0,
                                total_sent: 0,
                                txids: std::collections::HashSet::new(),
                            });
                        entry.total_sent = entry.total_sent
                            .checked_add(value)
                            .unwrap_or(i64::MAX);
                        entry.txids.insert(tx.txid.clone());
                    }
                }
            }
        }

        const ADDRESS_CHUNK_SIZE: usize = 8_000;
        let mut address_rows: Vec<_> = addr_map.iter().collect();
        // Concurrent backfill workers can touch popular addresses in different
        // blocks. A stable lock order prevents cross-block UPSERT deadlocks.
        address_rows.sort_unstable_by(|(left, _), (right, _)| left.cmp(right));
        for chunk in address_rows.chunks(ADDRESS_CHUNK_SIZE) {
            let mut query = QueryBuilder::<Postgres>::new(
                r#"
                INSERT INTO addresses (
                    address, balance, total_received, total_sent,
                    tx_count, first_seen, last_seen, address_type
                ) "#,
            );
            query.push_values(chunk, |mut row, (address, stats)| {
                row.push_bind(*address)
                    .push_bind(stats.total_received - stats.total_sent)
                    .push_bind(stats.total_received)
                    .push_bind(stats.total_sent)
                    .push_bind(stats.txids.len() as i64)
                    .push_bind(block_time as i64)
                    .push_bind(block_time as i64)
                    .push_bind("transparent");
            });
            query.push(
                r#"
                ON CONFLICT (address) DO UPDATE SET
                    balance = addresses.balance + EXCLUDED.balance,
                    total_received = addresses.total_received + EXCLUDED.total_received,
                    total_sent = addresses.total_sent + EXCLUDED.total_sent,
                    tx_count = addresses.tx_count + EXCLUDED.tx_count,
                    first_seen = LEAST(addresses.first_seen, EXCLUDED.first_seen),
                    last_seen = GREATEST(addresses.last_seen, EXCLUDED.last_seen),
                    updated_at = NOW()
                "#,
            );
            query.build().execute(&mut **db_tx).await?;
        }

        Ok(())
    }

    async fn insert_flows_tx(
        &self,
        db_tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
        flows: &[ShieldedFlow],
        block_time: u64,
    ) -> Result<u64, sqlx::Error> {
        if flows.is_empty() {
            return Ok(0);
        }

        const FLOW_CHUNK_SIZE: usize = 8_000;
        for chunk in flows.chunks(FLOW_CHUNK_SIZE) {
            let mut query = QueryBuilder::<Postgres>::new(
                r#"
                INSERT INTO shielded_flows (
                    txid, block_height, block_time, flow_type, amount_zat, pool,
                    transparent_addresses, transparent_value_zat
                ) "#,
            );
            query.push_values(chunk, |mut row, flow| {
                row.push_bind(&flow.txid)
                    .push_bind(flow.block_height as i32)
                    .push_bind(block_time as i32)
                    .push_bind(&flow.flow_type)
                    .push_bind(flow.amount)
                    .push_bind(&flow.pool)
                    .push_bind(&flow.transparent_addresses)
                    .push_bind(flow.amount);
            });
            query.push(
                r#"
                ON CONFLICT (txid, flow_type) DO UPDATE SET
                    amount_zat = EXCLUDED.amount_zat,
                    transparent_addresses = EXCLUDED.transparent_addresses
                "#,
            );
            query.build().execute(&mut **db_tx).await?;
        }

        Ok(flows.len() as u64)
    }

    /// Batch-update only the metadata columns for a set of transactions.
    /// Used by the backfill-metadata subcommand to fill in expiry_height, locktime,
    /// sapling/sprout counts without touching outputs, inputs, or flows.
    pub async fn batch_update_metadata(
        &self,
        txs: &[crate::models::Transaction],
    ) -> Result<u64, sqlx::Error> {
        if txs.is_empty() {
            return Ok(0);
        }

        let mut db_tx = self.pool.begin().await?;
        let mut updated = 0u64;

        for tx in txs {
            let has_sprout = tx.joinsplit_count > 0;

            let result = sqlx::query(
                r#"
                UPDATE transactions SET
                    locktime = $2,
                    expiry_height = $3,
                    sapling_spend_count = $4,
                    sapling_output_count = $5,
                    sprout_joinsplit_count = $6,
                    has_sprout = $7
                WHERE txid = $1
                "#,
            )
            .bind(&tx.txid) // $1
            .bind(tx.lock_time as i64) // $2
            .bind(tx.expiry_height.map(|h| h as i32)) // $3
            .bind(tx.sapling_spends as i32) // $4
            .bind(tx.sapling_outputs as i32) // $5
            .bind(tx.joinsplit_count as i32) // $6
            .bind(has_sprout) // $7
            .execute(&mut *db_tx)
            .await?;

            updated += result.rows_affected();
        }

        db_tx.commit().await?;
        Ok(updated)
    }

    /// Get the stored block hash at a given height, if any.
    pub async fn get_block_hash_at_height(
        &self,
        height: u32,
    ) -> Result<Option<String>, sqlx::Error> {
        let result: Option<(String,)> = sqlx::query_as("SELECT hash FROM blocks WHERE height = $1")
            .bind(height as i64)
            .fetch_optional(&self.pool)
            .await?;
        Ok(result.map(|(h,)| h))
    }

    /// Roll back all data from `fork_height` onward, archiving orphaned blocks.
    ///
    /// Uses txid-indexed deletes for performance on large tables.
    /// Reverses address balance deltas before deleting transaction data.
    pub async fn rollback_from_height(
        &self,
        fork_height: u32,
        description: &str,
    ) -> Result<u32, sqlx::Error> {
        let mut db_tx = self.pool.begin().await?;

        // Count blocks being rolled back
        let orphan_count: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM blocks WHERE height >= $1")
            .bind(fork_height as i64)
            .fetch_one(&mut *db_tx)
            .await?;
        let orphan_count = orphan_count.0 as u32;

        if orphan_count == 0 {
            db_tx.rollback().await?;
            return Ok(0);
        }

        // Record fork event
        let fork_event_id: (i32,) = sqlx::query_as(
            r#"INSERT INTO fork_events (fork_height, depth, canonical_tip, orphaned_count, source, description, detected_at, resolved_at)
               VALUES ($1, $2, $1, $3, 'indexer', $4, NOW(), NULL)
               RETURNING id"#
        )
        .bind(fork_height as i64)
        .bind(orphan_count as i32)
        .bind(orphan_count as i32)
        .bind(description)
        .fetch_one(&mut *db_tx)
        .await?;

        // Archive orphaned blocks (include roots for anchor debugging + coinbase for miner fingerprinting)
        sqlx::query(
            r#"INSERT INTO orphaned_blocks (height, hash, timestamp, transaction_count, size, difficulty, miner_address, previous_block_hash, final_sapling_root, final_orchard_root, coinbase_hex, fork_event_id, source, first_indexed_at)
               SELECT b.height, b.hash, b.timestamp, b.transaction_count, b.size, b.difficulty::text, b.miner_address, b.previous_block_hash, b.final_sapling_root, b.final_orchard_root, b.coinbase_hex, $1, 'indexer', b.created_at
               FROM blocks b WHERE b.height >= $2
               ON CONFLICT (hash) DO NOTHING"#
        )
        .bind(fork_event_id.0)
        .bind(fork_height as i64)
        .execute(&mut *db_tx)
        .await?;

        // Archive orphaned transactions before deletion
        sqlx::query(
            r#"INSERT INTO orphaned_transactions (
                   txid, block_height, block_hash, "timestamp", tx_index, version, locktime,
                   expiry_height, size, fee, is_coinbase,
                   vin_count, vout_count, total_input, total_output,
                   has_sapling, has_orchard, has_sprout, has_ironwood, has_shielded_data,
                   sapling_spend_count, sapling_output_count, orchard_actions, ironwood_actions,
                   sprout_joinsplit_count,
                   value_balance, value_balance_sapling, value_balance_orchard, value_balance_ironwood,
                   flow_type, privacy_score, fork_event_id, first_indexed_at
               )
               SELECT
                   t.txid, t.block_height, t.block_hash, t.block_time, t.tx_index, t.version, t.locktime,
                   t.expiry_height, t.size, t.fee, t.is_coinbase,
                   t.vin_count, t.vout_count, t.total_input, t.total_output,
                   t.has_sapling, t.has_orchard, t.has_sprout, t.has_ironwood, t.has_shielded_data,
                   t.sapling_spend_count, t.sapling_output_count, t.orchard_actions, t.ironwood_actions,
                   t.sprout_joinsplit_count,
                   t.value_balance, t.value_balance_sapling, t.value_balance_orchard, t.value_balance_ironwood,
                   t.flow_type, t.privacy_score, $1, t.created_at
               FROM transactions t WHERE t.block_height >= $2
               ON CONFLICT (txid, block_hash) DO NOTHING"#
        )
        .bind(fork_event_id.0)
        .bind(fork_height as i64)
        .execute(&mut *db_tx)
        .await?;

        // Archive transparent inputs for orphaned transactions
        sqlx::query(
            r#"INSERT INTO orphaned_transaction_inputs (txid, block_hash, vout_index, prev_txid, prev_vout, address, value, coinbase)
               SELECT ti.txid, t.block_hash, ti.vout_index, ti.prev_txid, ti.prev_vout, ti.address, ti.value, ti.coinbase
               FROM transaction_inputs ti
               JOIN transactions t ON ti.txid = t.txid
               WHERE t.block_height >= $1"#
        )
        .bind(fork_height as i64)
        .execute(&mut *db_tx)
        .await?;

        // Archive transparent outputs for orphaned transactions
        sqlx::query(
            r#"INSERT INTO orphaned_transaction_outputs (txid, block_hash, vout_index, value, address, script_type)
               SELECT to2.txid, t.block_hash, to2.vout_index, to2.value, to2.address, to2.script_type
               FROM transaction_outputs to2
               JOIN transactions t ON to2.txid = t.txid
               WHERE t.block_height >= $1"#
        )
        .bind(fork_height as i64)
        .execute(&mut *db_tx)
        .await?;

        // Reverse address balance deltas using txid index (fast path)
        sqlx::query(
            r#"UPDATE addresses SET
                   balance = addresses.balance - sub.net_delta,
                   total_received = GREATEST(0, addresses.total_received - sub.total_in),
                   total_sent = GREATEST(0, addresses.total_sent - sub.total_out),
                   tx_count = GREATEST(0, addresses.tx_count - sub.tx_count)
               FROM (
                   SELECT address,
                          SUM(value_out) as total_in,
                          SUM(value_in) as total_out,
                          SUM(value_out - value_in) as net_delta,
                          COUNT(DISTINCT txid) as tx_count
                   FROM address_transactions
                   WHERE txid IN (SELECT txid FROM transactions WHERE block_height >= $1)
                   GROUP BY address
               ) sub
               WHERE addresses.address = sub.address"#,
        )
        .bind(fork_height as i32)
        .execute(&mut *db_tx)
        .await?;

        // Delete dependent rows using txid index (fast)
        sqlx::query(
            "DELETE FROM address_transactions WHERE txid IN (SELECT txid FROM transactions WHERE block_height >= $1)"
        )
        .bind(fork_height as i64)
        .execute(&mut *db_tx)
        .await?;

        sqlx::query(
            "DELETE FROM shielded_flows WHERE txid IN (SELECT txid FROM transactions WHERE block_height >= $1)"
        )
        .bind(fork_height as i64)
        .execute(&mut *db_tx)
        .await?;

        sqlx::query(
            "DELETE FROM transaction_inputs WHERE txid IN (SELECT txid FROM transactions WHERE block_height >= $1)"
        )
        .bind(fork_height as i64)
        .execute(&mut *db_tx)
        .await?;

        sqlx::query(
            "DELETE FROM transaction_outputs WHERE txid IN (SELECT txid FROM transactions WHERE block_height >= $1)"
        )
        .bind(fork_height as i64)
        .execute(&mut *db_tx)
        .await?;

        // Delete transactions and blocks
        sqlx::query("DELETE FROM transactions WHERE block_height >= $1")
            .bind(fork_height as i64)
            .execute(&mut *db_tx)
            .await?;

        sqlx::query("DELETE FROM blocks WHERE height >= $1")
            .bind(fork_height as i64)
            .execute(&mut *db_tx)
            .await?;

        // Reset checkpoint
        let new_checkpoint = fork_height.saturating_sub(1);
        sqlx::query(
            r#"INSERT INTO indexer_state (key, value, updated_at) VALUES ('last_indexed_height', $1, NOW())
               ON CONFLICT (key) DO UPDATE SET value = EXCLUDED.value, updated_at = NOW()"#
        )
        .bind(new_checkpoint.to_string())
        .execute(&mut *db_tx)
        .await?;

        db_tx.commit().await?;
        Ok(orphan_count)
    }

    /// After re-indexing replacement blocks, backfill canonical_hash on orphaned_blocks
    /// and remove false orphans (where the orphaned hash matches the new canonical hash,
    /// which happens during double-reorgs: A->B->A).
    pub async fn finalize_orphans_after_reindex(
        &self,
        fork_height: u32,
        last_height: u32,
    ) -> Result<(), sqlx::Error> {
        // Backfill canonical_hash from the (now re-indexed) blocks table
        sqlx::query(
            r#"UPDATE orphaned_blocks ob
               SET canonical_hash = b.hash
               FROM blocks b
               WHERE ob.height = b.height
               AND ob.canonical_hash IS NULL
               AND ob.height >= $1 AND ob.height <= $2"#,
        )
        .bind(fork_height as i64)
        .bind(last_height as i64)
        .execute(&self.pool)
        .await?;

        // Remove false orphans: if orphaned hash == canonical hash, it was a double-reorg
        let removed = sqlx::query(
            r#"DELETE FROM orphaned_blocks
               WHERE canonical_hash IS NOT NULL
               AND hash = canonical_hash
               AND height >= $1 AND height <= $2"#,
        )
        .bind(fork_height as i64)
        .bind(last_height as i64)
        .execute(&self.pool)
        .await?;

        if removed.rows_affected() > 0 {
            tracing::info!(
                "Cleaned {} false orphan(s) from double-reorg",
                removed.rows_affected()
            );
        }

        Ok(())
    }

    /// Insert a boundary pool snapshot (authoritative Zebra pool sizes at a 256-block boundary).
    /// Uses ON CONFLICT to allow idempotent re-inserts (e.g. after reorg re-indexing).
    pub async fn insert_boundary_pool_snapshot(
        &self,
        boundary_height: u32,
        block_time: i64,
        orchard_zat: i64,
        ironwood_zat: i64,
        sapling_zat: i64,
        sprout_zat: i64,
        transparent_zat: Option<i64>,
        chain_supply_zat: Option<i64>,
    ) -> Result<(), sqlx::Error> {
        sqlx::query(
            r#"INSERT INTO boundary_pool_snapshots
                 (boundary_height, block_time, orchard_zat, ironwood_zat,
                  sapling_zat, sprout_zat, transparent_zat, chain_supply_zat)
               VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
               ON CONFLICT (boundary_height)
               DO UPDATE SET
                 block_time = EXCLUDED.block_time,
                 orchard_zat = EXCLUDED.orchard_zat,
                 ironwood_zat = EXCLUDED.ironwood_zat,
                 sapling_zat = EXCLUDED.sapling_zat,
                 sprout_zat = EXCLUDED.sprout_zat,
                 transparent_zat = EXCLUDED.transparent_zat,
                 chain_supply_zat = EXCLUDED.chain_supply_zat"#,
        )
        .bind(boundary_height as i64)
        .bind(block_time)
        .bind(orchard_zat)
        .bind(ironwood_zat)
        .bind(sapling_zat)
        .bind(sprout_zat)
        .bind(transparent_zat)
        .bind(chain_supply_zat)
        .execute(&self.pool)
        .await?;
        Ok(())
    }
}
