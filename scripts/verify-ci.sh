#!/usr/bin/env bash
set -euo pipefail

for command_name in initdb pg_ctl psql createdb cargo; do
    if ! command -v "$command_name" >/dev/null 2>&1; then
        echo "ERROR: $command_name is required for the local CI-equivalent check" >&2
        exit 1
    fi
done

test_port="${TEST_PG_PORT:-55432}"
if ! [[ "$test_port" =~ ^[0-9]+$ ]] || (( test_port < 1024 || test_port > 65535 )); then
    echo "ERROR: TEST_PG_PORT must be an unprivileged TCP port" >&2
    exit 1
fi

test_cluster="$(mktemp -d "${TMPDIR:-/tmp}/cipherscan-pg.XXXXXX")"
cleanup() {
    pg_ctl -D "$test_cluster" -m fast stop >/dev/null 2>&1 || true
    case "$test_cluster" in
        "${TMPDIR:-/tmp}"/cipherscan-pg.*) rm -rf "$test_cluster" ;;
        *) echo "WARNING: refusing to remove unexpected temporary path: $test_cluster" >&2 ;;
    esac
}
trap cleanup EXIT INT TERM

initdb -D "$test_cluster" -U postgres --auth=trust >/dev/null
pg_ctl -D "$test_cluster" -o "-p $test_port -k $test_cluster" -w start >/dev/null

psql -h "$test_cluster" -p "$test_port" -U postgres -d postgres -v ON_ERROR_STOP=1 \
    -c "CREATE ROLE zcash_user LOGIN PASSWORD 'zcash_test' NOSUPERUSER NOCREATEDB NOCREATEROLE;" >/dev/null
createdb -h "$test_cluster" -p "$test_port" -U postgres cipherscan_ci
psql -h "$test_cluster" -p "$test_port" -U postgres -d cipherscan_ci \
    -v ON_ERROR_STOP=1 -f schema/postgres.sql >/dev/null

export DATABASE_URL="postgres://postgres@localhost:$test_port/cipherscan_ci"
bash deploy/apply-migrations.sh
second_run="$(bash deploy/apply-migrations.sh)"
printf '%s\n' "$second_run"
grep -q 'All migrations already applied' <<< "$second_run"

export DATABASE_URL="postgres://zcash_user:zcash_test@localhost:$test_port/cipherscan_ci"
cargo build --locked --release
cargo test --locked --release --all-targets -- --test-threads=4
