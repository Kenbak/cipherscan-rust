//! PostgreSQL writer for indexed data
//!
//! Writes processed blockchain data to PostgreSQL for querying.
//! Uses UPSERT (INSERT ON CONFLICT) to allow parallel backfill and live indexing.

use sqlx::{PgPool, postgres::PgPoolOptions};
use crate::config::Config;
use crate::models::{Transaction, TransparentOutput, TransparentInput, ShieldedFlow};

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
        let result: Option<(i64,)> = sqlx::query_as(
            "SELECT MAX(height) FROM blocks"
        )
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
            "#
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
    pub async fn upsert_transaction(&self, tx: &Transaction, block_time: u64) -> Result<(), sqlx::Error> {
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
            "#
        )
        .bind(&tx.txid)                                    // $1
        .bind(tx.block_height as i64)                      // $2
        .bind(&tx.block_hash)                              // $3
        .bind(block_time as i64)                           // $4
        .bind(tx.version)                                  // $5
        .bind(tx.lock_time as i64)                         // $6
        .bind(tx.size as i32)                              // $7
        .bind(tx.fee.unwrap_or(0))                         // $8
        .bind(tx.transparent_value_in)                     // $9
        .bind(tx.transparent_value_out)                    // $10
        .bind(tx.sapling_spends as i32)                    // $11
        .bind(tx.sapling_outputs as i32)                   // $12
        .bind(tx.orchard_actions as i32)                   // $13
        .bind(tx.sapling_value_balance + tx.orchard_value_balance) // $14 value_balance
        .bind(tx.sapling_value_balance)                    // $15
        .bind(tx.orchard_value_balance)                    // $16
        .bind(is_coinbase)                                 // $17
        .bind(has_sapling)                                 // $18
        .bind(has_orchard)                                 // $19
        .bind(has_sprout)                                  // $20
        .bind(tx.vin_count as i32)                         // $21
        .bind(tx.vout_count as i32)                        // $22
        .bind::<Option<i32>>(None)                         // $23 tx_index (not stored in our model yet)
        .bind(block_time as i64)                           // $24
        .bind(tx.expiry_height.map(|h| h as i32))          // $25
        .bind(tx.sapling_spends as i32)                    // $26
        .bind(tx.sapling_outputs as i32)                   // $27
        .bind(tx.joinsplit_count as i32)                   // $28
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    /// Insert transaction outputs (vout)
    pub async fn insert_outputs(&self, txid: &str, outputs: &[TransparentOutput]) -> Result<(), sqlx::Error> {
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
    pub async fn insert_inputs(&self, txid: &str, inputs: &[TransparentInput]) -> Result<(), sqlx::Error> {
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
    pub async fn upsert_flow(&self, flow: &ShieldedFlow, block_time: u64) -> Result<(), sqlx::Error> {
        sqlx::query(
            r#"
            INSERT INTO shielded_flows (
                txid, block_height, block_time, flow_type, amount_zat, pool,
                transparent_addresses, transparent_value_zat
            ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
            ON CONFLICT (txid, flow_type) DO UPDATE SET
                amount_zat = EXCLUDED.amount_zat,
                transparent_addresses = EXCLUDED.transparent_addresses
            "#
        )
        .bind(&flow.txid)
        .bind(flow.block_height as i32)
        .bind(block_time as i32)
        .bind(&flow.flow_type)
        .bind(flow.amount)
        .bind(&flow.pool)
        .bind(&flow.transparent_addresses)
        .bind(flow.amount)  // transparent_value_zat = amount for now
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
            "#
        )
        .bind(key)
        .bind(value)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    /// Get indexer state value
    pub async fn get_state(&self, key: &str) -> Result<Option<String>, sqlx::Error> {
        let result: Option<(String,)> = sqlx::query_as(
            "SELECT value FROM indexer_state WHERE key = $1"
        )
        .bind(key)
        .fetch_optional(&self.pool)
        .await?;

        Ok(result.map(|(v,)| v))
    }

    /// Get checkpoint (convenience method)
    pub async fn get_checkpoint(&self) -> Result<Option<u32>, sqlx::Error> {
        match self.get_state("last_indexed_height").await? {
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
        let mut db_tx = self.pool.begin().await?;
        let mut count = 0u64;

        // Insert block
        sqlx::query(
            r#"
            INSERT INTO blocks (height, hash, timestamp, transaction_count)
            VALUES ($1, $2, $3, $4)
            ON CONFLICT (height) DO UPDATE SET
                hash = EXCLUDED.hash,
                transaction_count = EXCLUDED.transaction_count
            "#
        )
        .bind(height as i64)
        .bind(hash)
        .bind(timestamp as i64)
        .bind(transactions.len() as i32)
        .execute(&mut *db_tx)
        .await?;

        // Insert transactions and their outputs
        for tx in transactions {
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
                    vin_count, vout_count, block_time
                ) VALUES (
                    $1, $2, $3, $4, $5, $6, $7, $8, $9, $10,
                    $11, $12, $13, $14, $15, $16, $17, $18, $19, $20, $21
                )
                ON CONFLICT (txid) DO NOTHING
                "#
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
            .execute(&mut *db_tx)
            .await?;

            // Insert outputs
            for output in &tx.vout {
                sqlx::query(
                    r#"
                    INSERT INTO transaction_outputs (txid, vout_index, value, address, script_type)
                    VALUES ($1, $2, $3, $4, $5)
                    ON CONFLICT DO NOTHING
                    "#
                )
                .bind(&tx.txid)
                .bind(output.n as i32)
                .bind(output.value)
                .bind(&output.address)
                .bind(&output.script_type)
                .execute(&mut *db_tx)
                .await?;
            }

            count += 1;
        }

        db_tx.commit().await?;
        Ok(count)
    }
}
