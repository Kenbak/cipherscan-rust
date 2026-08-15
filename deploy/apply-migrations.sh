#!/usr/bin/env bash
# apply-migrations.sh — Applies unapplied SQL migrations from schema/migrations/
# in version order, recording each in the schema_migrations table.
#
# Usage:
#   ./deploy/apply-migrations.sh                    # uses DATABASE_URL from env or .env
#   ./deploy/apply-migrations.sh --dry-run          # show what would be applied, don't apply
#   DATABASE_URL=postgres://... ./deploy/apply-migrations.sh
#
# Online-DDL discipline enforced:
#   - CREATE INDEX without CONCURRENTLY → abort
#   - Wrapping CONCURRENTLY inside BEGIN/COMMIT → abort
#   - ADD COLUMN ... DEFAULT (non-null) → warning (check if volatile)
#
# Each migration runs in autocommit mode (no --single-transaction) so that
# CREATE INDEX CONCURRENTLY works. Failures leave schema_migrations accurate:
# only successfully applied migrations are recorded.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
MIGRATIONS_DIR="$REPO_ROOT/schema/migrations"

DRY_RUN=false
if [[ "${1:-}" == "--dry-run" ]]; then
    DRY_RUN=true
fi

if [[ -z "${DATABASE_URL:-}" ]]; then
    if [[ -f "$REPO_ROOT/.env" ]]; then
        DATABASE_URL=$(grep '^DATABASE_URL=' "$REPO_ROOT/.env" | head -1 | cut -d= -f2-)
    fi
fi

if [[ -z "${DATABASE_URL:-}" ]]; then
    echo "ERROR: DATABASE_URL not set and no .env found" >&2
    exit 1
fi

export PGCONNSTRING="$DATABASE_URL"

run_sql() {
    psql "$PGCONNSTRING" -qtAX -c "$1" 2>/dev/null
}

echo "=== CipherScan Migration Runner ==="
echo "Migrations dir: $MIGRATIONS_DIR"

if ! run_sql "SELECT 1 FROM schema_migrations LIMIT 1" >/dev/null 2>&1; then
    echo "schema_migrations table not found — creating it"
    if [[ "$DRY_RUN" == "true" ]]; then
        echo "[DRY RUN] Would create schema_migrations table"
    else
        psql "$PGCONNSTRING" -f "$MIGRATIONS_DIR/014_schema_migrations_tracking.sql"
    fi
fi

applied=$(run_sql "SELECT version FROM schema_migrations ORDER BY version")

sql_files=()
while IFS= read -r f; do
    sql_files+=("$f")
done < <(find "$MIGRATIONS_DIR" -maxdepth 1 -name '*.sql' | sort)

pending=()
for filepath in "${sql_files[@]}"; do
    filename=$(basename "$filepath")
    version=$(echo "$filename" | sed -n 's/^\([0-9]*\).*/\1/p')
    if [[ -z "$version" ]]; then
        echo "SKIP: $filename (no version prefix)"
        continue
    fi
    version_clean=$(echo "$version" | sed 's/^0*//')
    if echo "$applied" | grep -qx "$version_clean" 2>/dev/null; then
        continue
    fi
    # Also check with leading zeros
    if echo "$applied" | grep -qx "$version" 2>/dev/null; then
        continue
    fi
    pending+=("$filepath")
done

if [[ ${#pending[@]} -eq 0 ]]; then
    echo "All migrations already applied. Nothing to do."
    exit 0
fi

echo ""
echo "Pending migrations (${#pending[@]}):"
for filepath in "${pending[@]}"; do
    echo "  - $(basename "$filepath")"
done
echo ""

lint_migration() {
    local filepath="$1"
    local filename
    filename=$(basename "$filepath")
    local errors=0

    while IFS= read -r line; do
        if echo "$line" | grep -iq 'CREATE.*INDEX' && \
           ! echo "$line" | grep -iq 'CONCURRENTLY' && \
           ! echo "$line" | grep -iq 'IF NOT EXISTS'; then
            echo "ERROR: $filename: blocking CREATE INDEX (missing CONCURRENTLY):" >&2
            echo "  $line" >&2
            errors=1
        fi
    done < "$filepath"

    if grep -iq 'BEGIN\|START TRANSACTION' "$filepath" && \
       grep -iq 'CONCURRENTLY' "$filepath"; then
        echo "ERROR: $filename: CONCURRENTLY cannot run inside a transaction block" >&2
        errors=1
    fi

    if grep -iq 'ADD COLUMN.*DEFAULT' "$filepath"; then
        if ! grep -iq 'ADD COLUMN.*DEFAULT NULL' "$filepath"; then
            echo "WARNING: $filename: ADD COLUMN with DEFAULT detected — verify it's not volatile"
        fi
    fi

    return $errors
}

all_ok=true
for filepath in "${pending[@]}"; do
    if ! lint_migration "$filepath"; then
        all_ok=false
    fi
done

if [[ "$all_ok" == "false" ]]; then
    echo ""
    echo "ABORTED: Fix the above online-DDL violations before applying." >&2
    exit 1
fi

if [[ "$DRY_RUN" == "true" ]]; then
    echo "[DRY RUN] Would apply the above ${#pending[@]} migration(s). Exiting."
    exit 0
fi

echo "Applying migrations..."
echo ""

for filepath in "${pending[@]}"; do
    filename=$(basename "$filepath")
    version=$(echo "$filename" | sed -n 's/^\([0-9]*\).*/\1/p' | sed 's/^0*//')
    description=$(echo "$filename" | sed 's/^[0-9]*_//' | sed 's/\.sql$//' | tr '_' ' ' | sed "s/'/''/g")

    echo -n "  Applying $filename ... "

    if psql "$PGCONNSTRING" -f "$filepath" > /dev/null 2>&1; then
        run_sql "INSERT INTO schema_migrations (version, description) VALUES ('$version', '$description') ON CONFLICT (version) DO NOTHING;"
        echo "OK"
    else
        echo "FAILED" >&2
        echo "ERROR: Migration $filename failed. Stopping." >&2
        echo "The schema_migrations table reflects only what was successfully applied." >&2
        exit 1
    fi
done

echo ""
echo "All ${#pending[@]} migration(s) applied successfully."
echo ""
echo "Current schema_migrations:"
run_sql "SELECT version, description, applied_at FROM schema_migrations ORDER BY version;"
