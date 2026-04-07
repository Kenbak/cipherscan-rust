//! PostgreSQL writer for indexed data
//!
//! Writes processed blockchain data to PostgreSQL for querying.
//! Uses UPSERT (INSERT ON CONFLICT) to allow parallel backfill and live indexing.

use crate::models::{ShieldedFlow, Transaction, TransparentInput, TransparentOutput};
use sqlx::{postgres::PgPoolOptions, PgPool};

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

    /// Get the connection pool
    pub fn pool(&self) -> &PgPool {
        &self.pool
    }

    /// Get the last indexed block height
    pub async fn get_last_indexed_height(&self) -> Result<Option<u32>, sqlx::Error> {
        let result: Option<(i64,)> = sqlx::query_as("SELECT MAX(height) FROM blocks")
            .fetch_optional(&self.pool)
            .await?;

        Ok(result.and_then(|(h,)| Some(h as u32)))
    }

    /// Insert or update a block (matches actual schema)
    pub async fn upsert_block(
        &self,
        height: u32,
        hash: &str,
        timestamp: u64,
        tx_count: u32,
        size: Option<u32>,
    ) -> Result<(), sqlx::Error> {
        sqlx::query(
            r#"
            INSERT INTO blocks (height, hash, timestamp, transaction_count, size)
            VALUES ($1, $2, $3, $4, $5)
            ON CONFLICT (height) DO UPDATE SET
                hash = EXCLUDED.hash,
                timestamp = EXCLUDED.timestamp,
                transaction_count = EXCLUDED.transaction_count,
                size = COALESCE(EXCLUDED.size, blocks.size)
            "#,
        )
        .bind(height as i64)
        .bind(hash)
        .bind(timestamp as i64)
        .bind(tx_count as i32)
        .bind(size.map(|s| s as i32))
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    /// Insert or update a transaction (matches actual schema)
    pub async fn upsert_transaction(
        &self,
        tx: &Transaction,
        block_time: u64,
    ) -> Result<(), sqlx::Error> {
        // Determine flags
        let has_sapling = tx.sapling_spends > 0 || tx.sapling_outputs > 0;
        let has_orchard = tx.orchard_actions > 0;
        let has_sprout = tx.joinsplit_count > 0;
        let is_coinbase = tx.vin.first().map(|v| v.is_coinbase).unwrap_or(false);

        sqlx::query(
            r#"
            INSERT INTO transactions (
                txid, block_height, block_hash, timestamp, version, locktime,
                size, fee, total_input, total_output,
                shielded_spends, shielded_outputs, orchard_actions,
                value_balance, value_balance_sapling, value_balance_orchard,
                is_coinbase, has_sapling, has_orchard, has_sprout,
                vin_count, vout_count, tx_index, block_time,
                expiry_height, sapling_spend_count, sapling_output_count, sprout_joinsplit_count
            ) VALUES (
                $1, $2, $3, $4, $5, $6, $7, $8, $9, $10,
                $11, $12, $13, $14, $15, $16, $17, $18, $19, $20,
                $21, $22, $23, $24, $25, $26, $27, $28
            )
            ON CONFLICT (txid) DO UPDATE SET
                block_height = EXCLUDED.block_height,
                expiry_height = EXCLUDED.expiry_height,
                sapling_spend_count = EXCLUDED.sapling_spend_count,
                sapling_output_count = EXCLUDED.sapling_output_count,
                sprout_joinsplit_count = EXCLUDED.sprout_joinsplit_count
            "#,
        )
        .bind(&tx.txid) // $1
        .bind(tx.block_height as i64) // $2
        .bind(&tx.block_hash) // $3
        .bind(block_time as i64) // $4
        .bind(tx.version) // $5
        .bind(tx.lock_time as i64) // $6
        .bind(tx.size as i32) // $7
        .bind(tx.fee.unwrap_or(0)) // $8
        .bind(tx.transparent_value_in) // $9
        .bind(tx.transparent_value_out) // $10
        .bind(tx.sapling_spends as i32) // $11
        .bind(tx.sapling_outputs as i32) // $12
        .bind(tx.orchard_actions as i32) // $13
        .bind(tx.sapling_value_balance + tx.orchard_value_balance) // $14 value_balance
        .bind(tx.sapling_value_balance) // $15
        .bind(tx.orchard_value_balance) // $16
        .bind(is_coinbase) // $17
        .bind(has_sapling) // $18
        .bind(has_orchard) // $19
        .bind(has_sprout) // $20
        .bind(tx.vin_count as i32) // $21
        .bind(tx.vout_count as i32) // $22
        .bind::<Option<i32>>(None) // $23 tx_index (not stored in our model yet)
        .bind(block_time as i64) // $24
        .bind(tx.expiry_height.map(|h| h as i32)) // $25
        .bind(tx.sapling_spends as i32) // $26
        .bind(tx.sapling_outputs as i32) // $27
        .bind(tx.joinsplit_count as i32) // $28
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    /// Insert transaction outputs (vout)
    pub async fn insert_outputs(
        &self,
        txid: &str,
        outputs: &[TransparentOutput],
    ) -> Result<(), sqlx::Error> {
        for output in outputs {
            sqlx::query(
                r#"
                INSERT INTO transaction_outputs (txid, vout_index, value, address, script_pubkey, script_type)
                VALUES ($1, $2, $3, $4, $5, $6)
                ON CONFLICT (txid, vout_index) DO UPDATE SET
                    value = EXCLUDED.value,
                    address = EXCLUDED.address,
                    script_type = EXCLUDED.script_type
                "#
            )
            .bind(txid)
            .bind(output.n as i32)
            .bind(output.value)
            .bind(&output.address)
            .bind(&output.script_pub_key)
            .bind(&output.script_type)
            .execute(&self.pool)
            .await?;
        }

        Ok(())
    }

    /// Insert transaction inputs (vin)
    pub async fn insert_inputs(
        &self,
        txid: &str,
        inputs: &[TransparentInput],
    ) -> Result<(), sqlx::Error> {
        for (i, input) in inputs.iter().enumerate() {
            if input.is_coinbase {
                // Skip coinbase inputs or insert with special handling
                continue;
            }

            sqlx::query(
                r#"
                INSERT INTO transaction_inputs (txid, vout_index, prev_txid, prev_vout, address, value)
                VALUES ($1, $2, $3, $4, $5, $6)
                ON CONFLICT DO NOTHING
                "#
            )
            .bind(txid)
            .bind(i as i32)
            .bind(&input.txid)
            .bind(input.vout as i32)
            .bind(&input.address)
            .bind(input.value)
            .execute(&self.pool)
            .await?;
        }

        Ok(())
    }

    /// Insert or update a shielded flow (matches actual schema)
    pub async fn upsert_flow(
        &self,
        flow: &ShieldedFlow,
        block_time: u64,
    ) -> Result<(), sqlx::Error> {
        sqlx::query(
            r#"
            INSERT INTO shielded_flows (
                txid, block_height, block_time, flow_type, amount_zat, pool,
                transparent_addresses, transparent_value_zat
            ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
            ON CONFLICT (txid, flow_type) DO UPDATE SET
                amount_zat = EXCLUDED.amount_zat,
                transparent_addresses = EXCLUDED.transparent_addresses
            "#,
        )
        .bind(&flow.txid)
        .bind(flow.block_height as i32)
        .bind(block_time as i32)
        .bind(&flow.flow_type)
        .bind(flow.amount)
        .bind(&flow.pool)
        .bind(&flow.transparent_addresses)
        .bind(flow.amount) // transparent_value_zat = amount for now
        .execute(&self.pool)
        .await?;

        Ok(())
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

    /// Batch insert for better performance (transactions in a DB transaction)
    pub async fn batch_insert(
        &self,
        height: u32,
        hash: &str,
        timestamp: u64,
        transactions: &[Transaction],
    ) -> Result<u64, sqlx::Error> {
        // Use default header (for backwards compatibility)
        let header = crate::db::ParsedBlockHeader {
            version: 4,
            previous_block_hash: String::new(),
            merkle_root: String::new(),
            final_sapling_root: String::new(),
            time: timestamp,
            bits: String::new(),
            difficulty: 0.0,
            nonce: String::new(),
            solution: String::new(),
        };
        self.batch_insert_with_header(height, hash, timestamp, transactions, &header)
            .await
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

        // Insert block with all header fields
        sqlx::query(
            r#"
            INSERT INTO blocks (
                height, hash, timestamp, transaction_count, total_fees,
                version, merkle_root, final_sapling_root, bits, nonce, solution,
                difficulty, previous_block_hash, size, miner_address
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15)
            ON CONFLICT (height) DO UPDATE SET
                hash = EXCLUDED.hash,
                transaction_count = EXCLUDED.transaction_count,
                total_fees = EXCLUDED.total_fees,
                version = EXCLUDED.version,
                merkle_root = EXCLUDED.merkle_root,
                final_sapling_root = EXCLUDED.final_sapling_root,
                bits = EXCLUDED.bits,
                nonce = EXCLUDED.nonce,
                solution = EXCLUDED.solution,
                difficulty = EXCLUDED.difficulty,
                previous_block_hash = EXCLUDED.previous_block_hash,
                size = EXCLUDED.size,
                miner_address = EXCLUDED.miner_address
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
        .bind(&header.bits)
        .bind(&header.nonce)
        .bind(&header.solution)
        .bind(header.difficulty)
        .bind(&header.previous_block_hash)
        .bind(block_size)
        .bind(&miner_address)
        .execute(&mut *db_tx)
        .await?;

        // Insert transactions and their outputs
        for (tx_idx, tx) in transactions.iter().enumerate() {
            // Insert transaction
            let has_sapling = tx.sapling_spends > 0 || tx.sapling_outputs > 0;
            let has_orchard = tx.orchard_actions > 0;
            let is_coinbase = tx.vin.first().map(|v| v.is_coinbase).unwrap_or(false);

            sqlx::query(
                r#"
                INSERT INTO transactions (
                    txid, block_height, block_hash, timestamp, version, locktime,
                    size, fee, total_input, total_output,
                    shielded_spends, shielded_outputs, orchard_actions,
                    value_balance_sapling, value_balance_orchard,
                    is_coinbase, has_sapling, has_orchard,
                    vin_count, vout_count, block_time, tx_index
                ) VALUES (
                    $1, $2, $3, $4, $5, $6, $7, $8, $9, $10,
                    $11, $12, $13, $14, $15, $16, $17, $18, $19, $20, $21, $22
                )
                ON CONFLICT (txid) DO UPDATE SET
                    block_height = EXCLUDED.block_height,
                    block_hash = EXCLUDED.block_hash,
                    fee = EXCLUDED.fee,
                    total_input = EXCLUDED.total_input,
                    total_output = EXCLUDED.total_output,
                    is_coinbase = EXCLUDED.is_coinbase,
                    tx_index = EXCLUDED.tx_index
                "#,
            )
            .bind(&tx.txid)
            .bind(tx.block_height as i64)
            .bind(&tx.block_hash)
            .bind(timestamp as i64)
            .bind(tx.version)
            .bind(tx.lock_time as i64)
            .bind(tx.size as i32)
            .bind(tx.fee.unwrap_or(0))
            .bind(tx.transparent_value_in)
            .bind(tx.transparent_value_out)
            .bind(tx.sapling_spends as i32)
            .bind(tx.sapling_outputs as i32)
            .bind(tx.orchard_actions as i32)
            .bind(tx.sapling_value_balance)
            .bind(tx.orchard_value_balance)
            .bind(is_coinbase)
            .bind(has_sapling)
            .bind(has_orchard)
            .bind(tx.vin_count as i32)
            .bind(tx.vout_count as i32)
            .bind(timestamp as i64)
            .bind(tx_idx as i32) // $22 tx_index
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
                        entry.1 += output.value; // value_out
                    }
                }
                for input in &tx.vin {
                    if input.is_coinbase {
                        continue;
                    }
                    if let Some(ref addr) = input.address {
                        let entry = addr_map.entry(addr.as_str()).or_insert((0, 0));
                        entry.0 += input.value.unwrap_or(0); // value_in
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

        let flow_count = self.insert_flows_tx(&mut db_tx, flows, timestamp).await?;

        // Update addresses table (aggregate per-address for this block)
        self.update_addresses_for_block(&mut db_tx, transactions, timestamp)
            .await?;

        db_tx.commit().await?;
        Ok((count, flow_count))
    }

    /// Batch insert with full block header info.
    pub async fn batch_insert_with_header(
        &self,
        height: u32,
        hash: &str,
        timestamp: u64,
        transactions: &[Transaction],
        header: &crate::db::ParsedBlockHeader,
    ) -> Result<u64, sqlx::Error> {
        let (count, _) = self
            .batch_insert_with_header_and_flows(height, hash, timestamp, transactions, &[], header)
            .await?;
        Ok(count)
    }

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
                    entry.total_received += output.value;
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
                        entry.total_sent += value;
                        entry.txids.insert(tx.txid.clone());
                    }
                }
            }
        }

        // Upsert each address
        for (address, stats) in &addr_map {
            let tx_count = stats.txids.len() as i64;
            let balance_delta = stats.total_received - stats.total_sent;

            sqlx::query(
                r#"
                INSERT INTO addresses (address, balance, total_received, total_sent, tx_count, first_seen, last_seen, address_type)
                VALUES ($1, $2, $3, $4, $5, $6, $6, 'transparent')
                ON CONFLICT (address) DO UPDATE SET
                    balance = addresses.balance + $2,
                    total_received = addresses.total_received + $3,
                    total_sent = addresses.total_sent + $4,
                    tx_count = addresses.tx_count + $5,
                    last_seen = $6,
                    updated_at = NOW()
                "#
            )
            .bind(address)             // $1
            .bind(balance_delta)       // $2 balance delta
            .bind(stats.total_received) // $3
            .bind(stats.total_sent)     // $4
            .bind(tx_count)             // $5
            .bind(block_time as i64)    // $6
            .execute(&mut **db_tx)
            .await?;
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

        let mut count = 0u64;

        for flow in flows {
            sqlx::query(
                r#"
                INSERT INTO shielded_flows (
                    txid, block_height, block_time, flow_type, amount_zat, pool,
                    transparent_addresses, transparent_value_zat
                ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
                ON CONFLICT (txid, flow_type) DO UPDATE SET
                    amount_zat = EXCLUDED.amount_zat,
                    transparent_addresses = EXCLUDED.transparent_addresses
                "#,
            )
            .bind(&flow.txid)
            .bind(flow.block_height as i32)
            .bind(block_time as i32)
            .bind(&flow.flow_type)
            .bind(flow.amount)
            .bind(&flow.pool)
            .bind(&flow.transparent_addresses)
            .bind(flow.amount)
            .execute(&mut **db_tx)
            .await?;

            count += 1;
        }

        Ok(count)
    }

    /// Batch insert flows.
    pub async fn batch_insert_flows(
        &self,
        flows: &[ShieldedFlow],
        block_time: u64,
    ) -> Result<u64, sqlx::Error> {
        let mut db_tx = self.pool.begin().await?;
        let count = self.insert_flows_tx(&mut db_tx, flows, block_time).await?;
        db_tx.commit().await?;
        Ok(count)
    }
}
