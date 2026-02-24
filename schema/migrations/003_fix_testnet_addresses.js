#!/usr/bin/env node
/**
 * Fix testnet addresses: re-encode t1 → tm, t3 → t2
 *
 * The Rust indexer was using mainnet address prefixes on testnet.
 * This script decodes each address, swaps the 2-byte version prefix,
 * and re-encodes with the correct testnet prefix.
 *
 * Safe to run while the explorer is live (uses batched updates).
 * Idempotent — already-correct tm/t2 addresses are skipped.
 *
 * Usage:
 *   DATABASE_URL=postgres://user:pass@localhost/zcash_explorer_testnet node 003_fix_testnet_addresses.js
 */

const { Pool } = require('pg');
const crypto = require('crypto');

const DATABASE_URL = process.env.DATABASE_URL || 'postgres://localhost/zcash_explorer_testnet';
const BATCH_SIZE = 5000;

const pool = new Pool({ connectionString: DATABASE_URL, max: 3 });

function log(msg) {
  const ts = new Date().toISOString().slice(11, 19);
  console.log(`[${ts}] ${msg}`);
}

const BASE58_ALPHABET = '123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz';

function base58Decode(str) {
  let num = BigInt(0);
  for (const char of str) {
    const idx = BASE58_ALPHABET.indexOf(char);
    if (idx === -1) throw new Error(`Invalid base58 char: ${char}`);
    num = num * 58n + BigInt(idx);
  }

  // Count leading '1's (they map to 0x00 bytes)
  let leadingZeros = 0;
  for (const char of str) {
    if (char === '1') leadingZeros++;
    else break;
  }

  const hex = num.toString(16).padStart(2, '0');
  const bytes = Buffer.from(hex.length % 2 ? '0' + hex : hex, 'hex');

  return Buffer.concat([Buffer.alloc(leadingZeros), bytes]);
}

function base58Encode(buffer) {
  let num = BigInt('0x' + buffer.toString('hex'));
  let result = '';

  while (num > 0n) {
    const remainder = Number(num % 58n);
    num = num / 58n;
    result = BASE58_ALPHABET[remainder] + result;
  }

  // Leading zero bytes become '1'
  for (const byte of buffer) {
    if (byte === 0) result = '1' + result;
    else break;
  }

  return result;
}

function sha256(data) {
  return crypto.createHash('sha256').update(data).digest();
}

function reencodeAddress(address) {
  // Decode base58check: [2-byte prefix] [20-byte hash] [4-byte checksum]
  const decoded = base58Decode(address);
  if (decoded.length !== 26) return null; // 2 + 20 + 4

  const prefix = decoded.slice(0, 2);
  const hash = decoded.slice(2, 22);

  let newPrefix;
  if (prefix[0] === 0x1C && prefix[1] === 0xB8) {
    // t1 (mainnet P2PKH) → tm (testnet P2PKH)
    newPrefix = Buffer.from([0x1D, 0x25]);
  } else if (prefix[0] === 0x1C && prefix[1] === 0xBD) {
    // t3 (mainnet P2SH) → t2 (testnet P2SH)
    newPrefix = Buffer.from([0x1C, 0xBA]);
  } else {
    return null; // Already testnet or unknown
  }

  // Build new address with checksum
  const payload = Buffer.concat([newPrefix, hash]);
  const checksum = sha256(sha256(payload)).slice(0, 4);
  return base58Encode(Buffer.concat([payload, checksum]));
}

async function buildMapping() {
  log('Building address mapping...');

  // Get all unique t1/t3 addresses from transaction_outputs
  const result = await pool.query(`
    SELECT DISTINCT address FROM transaction_outputs WHERE address LIKE 't1%' OR address LIKE 't3%'
    UNION
    SELECT DISTINCT address FROM transaction_inputs WHERE address LIKE 't1%' OR address LIKE 't3%'
    UNION
    SELECT address FROM addresses WHERE address LIKE 't1%' OR address LIKE 't3%'
  `);

  const mapping = new Map();
  let skipped = 0;

  for (const row of result.rows) {
    const oldAddr = row.address;
    const newAddr = reencodeAddress(oldAddr);
    if (newAddr) {
      mapping.set(oldAddr, newAddr);
    } else {
      skipped++;
    }
  }

  log(`  ${mapping.size} addresses to fix, ${skipped} skipped`);
  return mapping;
}

async function fixTable(tableName, column, mapping, isPrimaryKey = false) {
  const entries = [...mapping.entries()];
  const total = entries.length;
  let updated = 0;

  log(`--- Fixing ${tableName}.${column} (${total} addresses) ---`);

  for (let i = 0; i < total; i += BATCH_SIZE) {
    const batch = entries.slice(i, i + BATCH_SIZE);
    const oldAddrs = batch.map(([old]) => old);
    const cases = batch.map(([old, newAddr], idx) => `WHEN $${idx * 2 + 1} THEN $${idx * 2 + 2}`).join(' ');
    const params = batch.flatMap(([old, newAddr]) => [old, newAddr]);

    if (isPrimaryKey) {
      // For PK columns, we need to insert new + delete old to avoid conflicts
      for (const [oldAddr, newAddr] of batch) {
        try {
          await pool.query(`
            INSERT INTO ${tableName} (${column}, balance, total_received, total_sent, tx_count, address_type, first_seen, last_seen, updated_at)
            SELECT $2, balance, total_received, total_sent, tx_count, address_type, first_seen, last_seen, NOW()
            FROM ${tableName} WHERE ${column} = $1
            ON CONFLICT (${column}) DO UPDATE SET
              balance = EXCLUDED.balance,
              total_received = EXCLUDED.total_received,
              total_sent = EXCLUDED.total_sent,
              tx_count = EXCLUDED.tx_count
          `, [oldAddr, newAddr]);
          await pool.query(`DELETE FROM ${tableName} WHERE ${column} = $1`, [oldAddr]);
          updated++;
        } catch (err) {
          log(`  Warning: failed to migrate ${oldAddr}: ${err.message}`);
        }
      }
    } else {
      // Simple UPDATE for non-PK columns
      await pool.query(`
        UPDATE ${tableName} SET ${column} = CASE ${column} ${cases} END
        WHERE ${column} = ANY($${params.length + 1})
      `, [...params, oldAddrs]);
      updated += batch.length;
    }

    if ((i / BATCH_SIZE + 1) % 10 === 0 || i + BATCH_SIZE >= total) {
      const pct = ((Math.min(i + BATCH_SIZE, total) / total) * 100).toFixed(1);
      log(`  ${tableName}: ${Math.min(i + BATCH_SIZE, total)}/${total} (${pct}%)`);
    }
  }

  log(`  Done: ${updated} rows updated`);
}

async function fixShieldedFlows(mapping) {
  log('--- Fixing shielded_flows.transparent_addresses ---');

  const result = await pool.query(`
    SELECT id, transparent_addresses FROM shielded_flows
    WHERE transparent_addresses IS NOT NULL AND array_length(transparent_addresses, 1) > 0
  `);

  let updated = 0;
  for (const row of result.rows) {
    const oldAddrs = row.transparent_addresses;
    const newAddrs = oldAddrs.map(a => mapping.get(a) || a);

    if (JSON.stringify(oldAddrs) !== JSON.stringify(newAddrs)) {
      await pool.query('UPDATE shielded_flows SET transparent_addresses = $1 WHERE id = $2', [newAddrs, row.id]);
      updated++;
    }
  }

  log(`  Done: ${updated} rows updated`);
}

async function main() {
  log('========================================');
  log('  TESTNET ADDRESS FIX (t1→tm, t3→t2)');
  log('========================================');
  log(`Database: ${DATABASE_URL.replace(/:[^:@]*@/, ':***@')}`);
  log('');

  try {
    await pool.query('SELECT 1');
    log('Database connected');
    log('');

    const mapping = await buildMapping();
    if (mapping.size === 0) {
      log('No addresses to fix — already correct!');
      return;
    }

    // Fix in dependency order: referenced tables first, then PK table
    await fixTable('transaction_outputs', 'address', mapping);
    await fixTable('transaction_inputs', 'address', mapping);
    await fixShieldedFlows(mapping);
    await fixTable('addresses', 'address', mapping, true);

    log('');
    log('========================================');
    log('  DONE — all addresses migrated');
    log('========================================');

    // Verify
    const check = await pool.query(`SELECT COUNT(*) as count FROM addresses WHERE address LIKE 't1%' OR address LIKE 't3%'`);
    const remaining = parseInt(check.rows[0].count);
    if (remaining > 0) {
      log(`  Warning: ${remaining} mainnet-style addresses still remain`);
    } else {
      log('  Verified: no t1/t3 addresses remaining');
    }

  } catch (err) {
    log(`ERROR: ${err.message}`);
    console.error(err);
    process.exit(1);
  } finally {
    await pool.end();
  }
}

main();
