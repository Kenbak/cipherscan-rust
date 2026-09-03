#!/usr/bin/env bash
# CipherScan NU7 Capacity Benchmark Runner
#
# Creates a disposable database, validates its schema, runs one benchmark pass,
# and captures the result. Full-pipeline runs on a host that also runs the
# production indexer require an explicit --allow-shared-host acknowledgement:
# a separate database still shares PostgreSQL WAL, CPU, memory, and storage.
#
# Usage:
#   bash deploy/run-benchmark.sh --parse-only
#   bash deploy/run-benchmark.sh --blocks 5000 --from 3500000
#   bash deploy/run-benchmark.sh --runs 5 --allow-shared-host
#
# Prerequisites:
#   - Zakura/Zebra RocksDB state available at $ZEBRA_STATE_PATH
#   - PostgreSQL running with superuser access (for createdb)
#   - cipherscan-indexer binary built (cargo build --release)

set -euo pipefail

BLOCKS=1000
FROM_HEIGHT=3200000
WARMUP=100
RUNS=3
PARSE_ONLY=false
JSON=true
ALLOW_SHARED_HOST=false
LABEL="ordinary"
VERIFY=true

while [[ $# -gt 0 ]]; do
    case $1 in
        --parse-only)   PARSE_ONLY=true; shift ;;
        --json)         JSON=true; shift ;;
        --human)        JSON=false; shift ;;
        --blocks)       BLOCKS="$2"; shift 2 ;;
        --from)         FROM_HEIGHT="$2"; shift 2 ;;
        --warmup)       WARMUP="$2"; shift 2 ;;
        --runs)         RUNS="$2"; shift 2 ;;
        --label)        LABEL="$2"; shift 2 ;;
        --allow-shared-host) ALLOW_SHARED_HOST=true; shift ;;
        --no-verify)     VERIFY=false; shift ;;
        *)
            echo "ERROR: Unknown argument: $1" >&2
            exit 2
            ;;
    esac
done

if ! [[ "$BLOCKS" =~ ^[1-9][0-9]*$ && "$RUNS" =~ ^[1-9][0-9]*$ && "$WARMUP" =~ ^[0-9]+$ ]]; then
    echo "ERROR: --blocks and --runs must be positive integers; --warmup must be non-negative" >&2
    exit 2
fi

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

if [[ "$PARSE_ONLY" == "false" && -z "${DATABASE_URL:-}" ]]; then
    echo "ERROR: DATABASE_URL must be configured for full-pipeline runs" >&2
    exit 1
fi

if [[ "$PARSE_ONLY" == "false" && "$ALLOW_SHARED_HOST" == "false" ]] \
    && command -v systemctl >/dev/null \
    && systemctl is-active --quiet cipherscan-rust.service 2>/dev/null; then
    echo "ERROR: Production indexer is active on this host." >&2
    echo "Use dedicated benchmark hardware, stop the production service, or pass" >&2
    echo "--allow-shared-host after confirming an off-peak window and health limits." >&2
    exit 1
fi

RESULT_DIR="benchmark-results"
mkdir -p "$RESULT_DIR"

active_db=""
cleanup() {
    if [[ -n "$active_db" ]]; then
        sudo -u postgres dropdb --if-exists "$active_db" >/dev/null 2>&1 || true
    fi
}
trap cleanup EXIT INT TERM

replace_database_name() {
    DATABASE_URL="$DATABASE_URL" DB_NAME="$1" python3 - <<'PY'
import os
from urllib.parse import urlsplit, urlunsplit

parts = urlsplit(os.environ["DATABASE_URL"])
if parts.scheme not in {"postgres", "postgresql"}:
    raise SystemExit("DATABASE_URL must use postgres:// or postgresql://")
print(urlunsplit((parts.scheme, parts.netloc, "/" + os.environ["DB_NAME"], parts.query, parts.fragment)))
PY
}

for run in $(seq 1 "$RUNS"); do
    timestamp=$(date -u +%Y%m%dT%H%M%SZ)
    extension="txt"
    format_args=()
    if [[ "$JSON" == "true" ]]; then
        extension="json"
        format_args=(--json)
    fi
    result_file="$RESULT_DIR/${LABEL}_${FROM_HEIGHT}_${TO_HEIGHT}_run${run}_${timestamp}.${extension}"
    benchmark_url="unused"
    mode_args=(--parse-only)

    if [[ "$PARSE_ONLY" == "false" ]]; then
        active_db="zcash_benchmark_$$_${run}"
        db_user=$(psql "$DATABASE_URL" -qtAX -c "SELECT current_user")
        sudo -u postgres createdb -O "$db_user" "$active_db"
        benchmark_url=$(replace_database_name "$active_db")

        sudo -u postgres psql -d "$active_db" -v ON_ERROR_STOP=1 -X -q \
            -f schema/postgres.sql
        test "$(sudo -u postgres psql -d "$active_db" -X -qtAX \
            -c "SELECT to_regclass('public.transparent_key_exposures')::text")" \
            = "transparent_key_exposures"
        test "$(sudo -u postgres psql -d "$active_db" -X -qtAX \
            -c "SELECT EXISTS (
                SELECT 1 FROM pg_constraint
                WHERE conrelid = 'public.transaction_outputs'::regclass
                  AND conname = 'transaction_outputs_pkey'
            )")" = "t"
        mode_args=()
    fi

    echo "Running ${LABEL} benchmark ${run}/${RUNS}: ${FROM_HEIGHT}-${TO_HEIGHT}" >&2
    "$BINARY" benchmark \
        --benchmark-db "$benchmark_url" \
        --from "$FROM_HEIGHT" \
        --to "$TO_HEIGHT" \
        --warmup "$WARMUP" \
        "${mode_args[@]}" \
        "${format_args[@]}" \
        | tee "$result_file"

    if [[ "$PARSE_ONLY" == "false" && "$VERIFY" == "true" ]]; then
        verification_file="${result_file%.*}.verify.txt"
        "$BINARY" validate \
            --prod-db "$DATABASE_URL" \
            --test-db "$benchmark_url" \
            --from-height "$FROM_HEIGHT" \
            --to-height "$TO_HEIGHT" \
            | tee "$verification_file"
        echo "Verification: $verification_file" >&2
    fi

    echo "Result: $result_file" >&2
    cleanup
    active_db=""
done
