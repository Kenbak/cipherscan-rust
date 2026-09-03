#!/usr/bin/env bash
# CipherScan NU7 Capacity Benchmark Runner
#
# Creates an isolated test database, runs the benchmark against real RocksDB
# state, and captures results. Safe to run on production — uses a separate
# database and does not touch production data.
#
# Usage:
#   bash deploy/run-benchmark.sh                    # Default: 1000 blocks, full pipeline
#   bash deploy/run-benchmark.sh --parse-only       # Skip DB writes, measure parse only
#   bash deploy/run-benchmark.sh --blocks 5000      # Custom block count
#   bash deploy/run-benchmark.sh --from 3500000     # Custom start height
#   bash deploy/run-benchmark.sh --dense             # Use a known dense block range
#
# Prerequisites:
#   - Zakura/Zebra RocksDB state available at $ZEBRA_STATE_PATH
#   - PostgreSQL running with superuser access (for createdb)
#   - cipherscan-indexer binary built (cargo build --release)

set -euo pipefail

BENCHMARK_DB="zcash_benchmark_$$"
BLOCKS=1000
FROM_HEIGHT=3200000
WARMUP=100
PARSE_ONLY=""
JSON=""
EXTRA_ARGS=""

while [[ $# -gt 0 ]]; do
    case $1 in
        --parse-only)   PARSE_ONLY="--parse-only"; shift ;;
        --json)         JSON="--json"; shift ;;
        --blocks)       BLOCKS="$2"; shift 2 ;;
        --from)         FROM_HEIGHT="$2"; shift 2 ;;
        --warmup)       WARMUP="$2"; shift 2 ;;
        --dense)
            # Block range with high transaction density
            FROM_HEIGHT=419200
            BLOCKS=500
            shift ;;
        *)              EXTRA_ARGS="$EXTRA_ARGS $1"; shift ;;
    esac
done

TO_HEIGHT=$((FROM_HEIGHT + BLOCKS - 1))
BINARY="./target/release/cipherscan-indexer"

if [[ ! -f "$BINARY" ]]; then
    echo "ERROR: Binary not found at $BINARY"
    echo "Run: cargo build --release"
    exit 1
fi

# Load environment
if [[ -f .env ]]; then
    set -a; source .env; set +a
fi

DB_USER="${PGUSER:-zcash_user}"
DB_HOST="${PGHOST:-127.0.0.1}"
DB_PORT="${PGPORT:-5432}"
BENCHMARK_URL="postgres://${DB_USER}@${DB_HOST}:${DB_PORT}/${BENCHMARK_DB}"

cleanup() {
    echo ""
    echo "Cleaning up benchmark database..."
    sudo -u postgres dropdb --if-exists "$BENCHMARK_DB" 2>/dev/null || true
}

if [[ -z "$PARSE_ONLY" ]]; then
    trap cleanup EXIT

    echo "Creating isolated benchmark database: $BENCHMARK_DB"
    sudo -u postgres createdb -O "$DB_USER" "$BENCHMARK_DB"

    # Apply schema
    sudo -u postgres psql -d "$BENCHMARK_DB" -f schema/postgres.sql -q
    echo "Schema applied."
else
    echo "Parse-only mode — no database needed."
    BENCHMARK_URL="unused"
fi

echo ""
echo "================================================================"
echo "  Running CipherScan NU7 Capacity Benchmark"
echo "================================================================"
echo "  Binary:  $BINARY"
echo "  Range:   $FROM_HEIGHT -> $TO_HEIGHT ($BLOCKS blocks)"
echo "  Warmup:  $WARMUP blocks"
echo "  Mode:    ${PARSE_ONLY:-full pipeline (source + parse + prevout + SQL write)}"
echo "  DB:      ${PARSE_ONLY:+(skipped)}${PARSE_ONLY:-$BENCHMARK_DB}"
echo "================================================================"
echo ""

RESULT_DIR="benchmark-results"
mkdir -p "$RESULT_DIR"
TIMESTAMP=$(date +%Y%m%d_%H%M%S)
RESULT_FILE="$RESULT_DIR/baseline_${TIMESTAMP}.json"

$BINARY benchmark \
    --benchmark-db "$BENCHMARK_URL" \
    --from "$FROM_HEIGHT" \
    --to "$TO_HEIGHT" \
    --warmup "$WARMUP" \
    $PARSE_ONLY \
    --json \
    $EXTRA_ARGS \
    | tee "$RESULT_FILE"

echo ""
echo "Results saved to: $RESULT_FILE"

# Also print human-readable output
if [[ -z "$JSON" ]]; then
    echo ""
    $BINARY benchmark \
        --benchmark-db "$BENCHMARK_URL" \
        --from "$FROM_HEIGHT" \
        --to "$TO_HEIGHT" \
        --warmup "$WARMUP" \
        $PARSE_ONLY \
        $EXTRA_ARGS
fi
