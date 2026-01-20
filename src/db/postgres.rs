//! PostgreSQL writer for indexed data
//!
//! Writes processed blockchain data to PostgreSQL for querying.

use sqlx::{PgPool, postgres::PgPoolOptions};
use crate::config::Config;
use crate::models::{Block, Transaction, ShieldedFlow};

/// PostgreSQL connection and writer
pub struct PostgresWriter {
    pool: PgPool,
    config: Config,
}

impl PostgresWriter {
    /// Connect to PostgreSQL
    pub async fn connect(config: &Config) -> Result<Self, sqlx::Error> {
        let pool = PgPoolOptions::new()
            .max_connections(10)
            .connect(&config.database_url)
            .await?;

        tracing::info!("Connected to PostgreSQL");

        Ok(Self {
            pool,
            config: config.clone(),
        })
    }

    /// Get the connection pool
    pub fn pool(&self) -> &PgPool {
        &self.pool
    }

    /// Get the last indexed block height
    pub async fn get_last_indexed_height(&self) -> Result<Option<u32>, sqlx::Error> {
        let result: Option<(i32,)> = sqlx::query_as(
            "SELECT MAX(height) FROM blocks"
        )
        .fetch_optional(&self.pool)
        .await?;

        Ok(result.map(|(h,)| h as u32))
    }

    /// Insert a batch of blocks
    pub async fn insert_blocks(&self, blocks: &[Block]) -> Result<u64, sqlx::Error> {
        if blocks.is_empty() {
            return Ok(0);
        }

        let mut tx = self.pool.begin().await?;
        let mut count = 0u64;

        for block in blocks {
            sqlx::query(
                r#"
                INSERT INTO blocks (
                    height, hash, version, merkle_root, time,
                    difficulty, nonce, solution, previous_block_hash,
                    tx_count, size, sapling_commitment_tree_size, orchard_commitment_tree_size
                ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13)
                ON CONFLICT (height) DO UPDATE SET
                    hash = EXCLUDED.hash,
                    tx_count = EXCLUDED.tx_count
                "#
            )
            .bind(block.height as i32)
            .bind(&block.hash)
            .bind(block.version)
            .bind(&block.merkle_root)
            .bind(block.time as i64)
            .bind(&block.difficulty)
            .bind(&block.nonce)
            .bind(&block.solution)
            .bind(&block.previous_block_hash)
            .bind(block.tx_count as i32)
            .bind(block.size as i32)
            .bind(block.sapling_tree_size.map(|s| s as i64))
            .bind(block.orchard_tree_size.map(|s| s as i64))
            .execute(&mut *tx)
            .await?;

            count += 1;
        }

        tx.commit().await?;
        Ok(count)
    }

    /// Insert a batch of transactions
    pub async fn insert_transactions(&self, txs: &[Transaction]) -> Result<u64, sqlx::Error> {
        if txs.is_empty() {
            return Ok(0);
        }

        let mut tx = self.pool.begin().await?;
        let mut count = 0u64;

        for transaction in txs {
            sqlx::query(
                r#"
                INSERT INTO transactions (
                    txid, block_height, block_hash, version, lock_time, expiry_height,
                    size, vin_count, vout_count,
                    sprout_joinsplit_count, sapling_spend_count, sapling_output_count,
                    orchard_action_count, fee, transparent_value_in, transparent_value_out,
                    sapling_value_balance, orchard_value_balance
                ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17, $18)
                ON CONFLICT (txid) DO NOTHING
                "#
            )
            .bind(&transaction.txid)
            .bind(transaction.block_height as i32)
            .bind(&transaction.block_hash)
            .bind(transaction.version)
            .bind(transaction.lock_time as i64)
            .bind(transaction.expiry_height.map(|h| h as i32))
            .bind(transaction.size as i32)
            .bind(transaction.vin_count as i16)
            .bind(transaction.vout_count as i16)
            .bind(transaction.joinsplit_count as i16)
            .bind(transaction.sapling_spends as i16)
            .bind(transaction.sapling_outputs as i16)
            .bind(transaction.orchard_actions as i16)
            .bind(transaction.fee.map(|f| f as i64))
            .bind(transaction.transparent_value_in as i64)
            .bind(transaction.transparent_value_out as i64)
            .bind(transaction.sapling_value_balance)
            .bind(transaction.orchard_value_balance)
            .execute(&mut *tx)
            .await?;

            count += 1;
        }

        tx.commit().await?;
        Ok(count)
    }

    /// Insert a batch of shielded flows
    pub async fn insert_flows(&self, flows: &[ShieldedFlow]) -> Result<u64, sqlx::Error> {
        if flows.is_empty() {
            return Ok(0);
        }

        let mut tx = self.pool.begin().await?;
        let mut count = 0u64;

        for flow in flows {
            sqlx::query(
                r#"
                INSERT INTO shielded_flows (
                    txid, flow_type, pool, amount, block_height,
                    transparent_addresses
                ) VALUES ($1, $2, $3, $4, $5, $6)
                ON CONFLICT (txid, flow_type, pool) DO NOTHING
                "#
            )
            .bind(&flow.txid)
            .bind(&flow.flow_type)
            .bind(&flow.pool)
            .bind(flow.amount)
            .bind(flow.block_height as i32)
            .bind(&flow.transparent_addresses)
            .execute(&mut *tx)
            .await?;

            count += 1;
        }

        tx.commit().await?;
        Ok(count)
    }

    /// Update indexer state (checkpoint)
    pub async fn update_checkpoint(&self, height: u32) -> Result<(), sqlx::Error> {
        sqlx::query(
            r#"
            INSERT INTO indexer_state (key, value)
            VALUES ('last_height', $1::text)
            ON CONFLICT (key) DO UPDATE SET value = EXCLUDED.value
            "#
        )
        .bind(height as i64)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    /// Get current checkpoint
    pub async fn get_checkpoint(&self) -> Result<Option<u32>, sqlx::Error> {
        let result: Option<(String,)> = sqlx::query_as(
            "SELECT value FROM indexer_state WHERE key = 'last_height'"
        )
        .fetch_optional(&self.pool)
        .await?;

        Ok(result.and_then(|(v,)| v.parse().ok()))
    }
}
