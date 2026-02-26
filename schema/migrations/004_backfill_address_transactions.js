#!/usr/bin/env node
/**
 * Backfill address_transactions table from existing transaction_inputs/outputs.
 *
 * Usage:
 *   DATABASE_URL=postgres://user:pass@localhost/db node 004_backfill_address_transactions.js
 *
 * This script:
 *   1. Inserts from transaction_outputs (is_output = true, value_out)
 *   2. Upserts from transaction_inputs (is_input = true, value_in)
 *   3. Both are done in batches by block_height to avoid memory issues
 *
 * Safe to re-run (uses ON CONFLICT upserts).
 */

const { Pool } = require('pg');

const DATABASE_URL = process.env.DATABASE_URL;
if (!DATABASE_URL) {
  console.error('ERROR: Set DATABASE_URL environment variable');
  process.exit(1);
}

const pool = new Pool({ connectionString: DATABASE_URL, max: 3 });
const BATCH_SIZE = 10000;

function log(msg) {
  const ts = new Date().toISOString().slice(11, 19);
  console.log(`[${ts}] ${msg}`);
}

function formatDuration(ms) {
  const s = Math.floor(ms / 1000);
  if (s < 60) return `${s}s`;
  const m = Math.floor(s / 60);
  return `${m}m ${s % 60}s`;
}

async function main() {
  log('========================================');
  log('  BACKFILL address_transactions');
  log('========================================');
  log('');

  await pool.query('SELECT 1');
  log('Database connected');

  // Get block height range
  const { rows: [{ min_height, max_height }] } = await pool.query(
    'SELECT MIN(block_height) as min_height, MAX(block_height) as max_height FROM transactions WHERE block_height > 0'
  );
  const minHeight = parseInt(min_height);
  const maxHeight = parseInt(max_height);
  log(`Block range: ${minHeight.toLocaleString()} → ${maxHeight.toLocaleString()}`);
  log('');

  // Step 1: Insert from transaction_outputs
  log('--- Step 1: Inserting from transaction_outputs ---');
  const step1Start = Date.now();
  let totalOutputRows = 0;

  for (let h = minHeight; h <= maxHeight; h += BATCH_SIZE) {
    const batchEnd = Math.min(h + BATCH_SIZE - 1, maxHeight);
    const result = await pool.query(`
      INSERT INTO address_transactions (address, txid, block_height, tx_index, block_time, is_output, value_out)
      SELECT
        o.address,
        o.txid,
        t.block_height,
        COALESCE(t.tx_index, 0),
        t.block_time,
        true,
        COALESCE(SUM(o.value), 0)
      FROM transaction_outputs o
      JOIN transactions t ON o.txid = t.txid
      WHERE o.address IS NOT NULL
        AND t.block_height >= $1 AND t.block_height <= $2
      GROUP BY o.address, o.txid, t.block_height, t.tx_index, t.block_time
      ON CONFLICT (address, block_height, tx_index, txid)
      DO UPDATE SET is_output = true, value_out = EXCLUDED.value_out
    `, [h, batchEnd]);

    totalOutputRows += result.rowCount;
    const pct = ((batchEnd - minHeight) / (maxHeight - minHeight) * 100).toFixed(1);
    if ((h - minHeight) % (BATCH_SIZE * 10) === 0 || batchEnd === maxHeight) {
      log(`  outputs: block ${batchEnd.toLocaleString()} (${pct}%) — ${totalOutputRows.toLocaleString()} rows`);
    }
  }

  log(`  ✓ Outputs done: ${totalOutputRows.toLocaleString()} rows (${formatDuration(Date.now() - step1Start)})`);
  log('');

  // Step 2: Upsert from transaction_inputs
  log('--- Step 2: Upserting from transaction_inputs ---');
  const step2Start = Date.now();
  let totalInputRows = 0;

  for (let h = minHeight; h <= maxHeight; h += BATCH_SIZE) {
    const batchEnd = Math.min(h + BATCH_SIZE - 1, maxHeight);
    const result = await pool.query(`
      INSERT INTO address_transactions (address, txid, block_height, tx_index, block_time, is_input, value_in)
      SELECT
        i.address,
        i.txid,
        t.block_height,
        COALESCE(t.tx_index, 0),
        t.block_time,
        true,
        COALESCE(SUM(i.value), 0)
      FROM transaction_inputs i
      JOIN transactions t ON i.txid = t.txid
      WHERE i.address IS NOT NULL
        AND t.block_height >= $1 AND t.block_height <= $2
      GROUP BY i.address, i.txid, t.block_height, t.tx_index, t.block_time
      ON CONFLICT (address, block_height, tx_index, txid)
      DO UPDATE SET is_input = true, value_in = EXCLUDED.value_in
    `, [h, batchEnd]);

    totalInputRows += result.rowCount;
    const pct = ((batchEnd - minHeight) / (maxHeight - minHeight) * 100).toFixed(1);
    if ((h - minHeight) % (BATCH_SIZE * 10) === 0 || batchEnd === maxHeight) {
      log(`  inputs: block ${batchEnd.toLocaleString()} (${pct}%) — ${totalInputRows.toLocaleString()} rows`);
    }
  }

  log(`  ✓ Inputs done: ${totalInputRows.toLocaleString()} rows (${formatDuration(Date.now() - step2Start)})`);
  log('');

  // Step 3: Update stats
  log('--- Step 3: Analyzing table ---');
  await pool.query('ANALYZE address_transactions');
  log('  ✓ ANALYZE complete');
  log('');

  // Summary
  const { rows: [{ total_rows }] } = await pool.query('SELECT COUNT(*) as total_rows FROM address_transactions');
  const { rows: [{ total_addresses }] } = await pool.query('SELECT COUNT(DISTINCT address) as total_addresses FROM address_transactions');

  log('========================================');
  log('  SUMMARY');
  log('========================================');
  log(`  Total rows: ${parseInt(total_rows).toLocaleString()}`);
  log(`  Unique addresses: ${parseInt(total_addresses).toLocaleString()}`);
  log(`  Total time: ${formatDuration(Date.now() - step1Start)}`);
  log('');

  await pool.end();
}

main().catch(err => {
  console.error('FATAL:', err);
  process.exit(1);
});
