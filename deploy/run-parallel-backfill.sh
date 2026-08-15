#!/usr/bin/env bash
set -Eeuo pipefail

if (( $# != 2 )); then
  echo "Usage: $0 <start-height> <end-height>" >&2
  exit 2
fi

readonly START_HEIGHT="$1"
readonly END_HEIGHT="$2"
readonly WORKERS="${WORKERS:-12}"
readonly CHUNK_SIZE="${CHUNK_SIZE:-50000}"
readonly ROOT="${CIPHERSCAN_ROOT:-/root/cipherscan-rust}"
readonly BINARY="${INDEXER_BINARY:-$ROOT/target/release/cipherscan-indexer}"
readonly LOG_DIR="${BACKFILL_LOG_DIR:-/var/log/cipherscan/backfill}"

if (( START_HEIGHT > END_HEIGHT )); then
  echo "Start height must not exceed end height" >&2
  exit 2
fi
if (( WORKERS < 1 || CHUNK_SIZE < 1 )); then
  echo "WORKERS and CHUNK_SIZE must be positive integers" >&2
  exit 2
fi
if systemctl is-active --quiet cipherscan-rust; then
  echo "Stop cipherscan-rust.service before starting parallel backfill" >&2
  exit 1
fi

# Hold the same lock cipherscan-rust.service takes (fd stays open for the
# rest of this script), so the live unit cannot start mid-backfill even if
# something else starts it concurrently. The is-active check above only
# catches the case where it is already running at invocation time.
readonly INDEXER_LOCK_FILE="${INDEXER_LOCK_FILE:-/run/cipherscan-indexer.lock}"
exec 9>"$INDEXER_LOCK_FILE"
if ! flock -n 9; then
  echo "Could not acquire $INDEXER_LOCK_FILE — cipherscan-rust.service (or another backfill) holds it" >&2
  exit 1
fi

set -a
# shellcheck disable=SC1091
source "$ROOT/.env"
set +a

mkdir -p "$LOG_DIR"
ranges_file="$(mktemp)"
trap 'rm -f "$ranges_file"' EXIT

for ((from = START_HEIGHT; from <= END_HEIGHT; from += CHUNK_SIZE)); do
  to=$((from + CHUNK_SIZE - 1))
  ((to > END_HEIGHT)) && to="$END_HEIGHT"
  printf '%s %s\n' "$from" "$to" >>"$ranges_file"
done

run_range() {
  local from="$1"
  local to="$2"
  local log="$LOG_DIR/${from}-${to}.log"
  echo "[$(date -Is)] starting $from-$to"
  "$BINARY" backfill --from "$from" --to "$to" >"$log" 2>&1
  echo "[$(date -Is)] completed $from-$to"
}
export -f run_range
export BINARY LOG_DIR

xargs -P "$WORKERS" -n 2 bash -c 'run_range "$1" "$2"' _ <"$ranges_file"

read -r indexed_count min_height max_height < <(
  psql "$DATABASE_URL" -At -F ' ' -c \
    "SELECT COUNT(*), MIN(height), MAX(height)
       FROM blocks
      WHERE height BETWEEN $START_HEIGHT AND $END_HEIGHT"
)
expected_count=$((END_HEIGHT - START_HEIGHT + 1))

if (( indexed_count != expected_count || min_height != START_HEIGHT || max_height != END_HEIGHT )); then
  echo "Coverage verification failed: count=$indexed_count min=$min_height max=$max_height" >&2
  exit 1
fi

psql "$DATABASE_URL" -v ON_ERROR_STOP=1 -c \
  "INSERT INTO indexer_state (key, value, updated_at)
   VALUES ('backfill_height', '$END_HEIGHT', NOW())
   ON CONFLICT (key) DO UPDATE
   SET value = EXCLUDED.value, updated_at = NOW()"

echo "Verified contiguous block coverage for $START_HEIGHT-$END_HEIGHT"
