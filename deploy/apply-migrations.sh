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

psql_cmd() {
    if [[ "${MIGRATIONS_AS_POSTGRES:-false}" == "true" ]]; then
        sudo -u postgres psql "$PGCONNSTRING" "$@"
    else
        psql "$PGCONNSTRING" "$@"
    fi
}

run_sql() {
    psql_cmd -v ON_ERROR_STOP=1 -qtAX -c "$1" 2>/dev/null
}

echo "=== CipherScan Migration Runner ==="
echo "Migrations dir: $MIGRATIONS_DIR"

if ! run_sql "SELECT 1 FROM schema_migrations LIMIT 1" >/dev/null 2>&1; then
    echo "schema_migrations table not found — creating it"
    if [[ "$DRY_RUN" == "true" ]]; then
        echo "[DRY RUN] Would create schema_migrations table"
    else
        psql_cmd -v ON_ERROR_STOP=1 -X \
            < "$MIGRATIONS_DIR/014_schema_migrations_tracking.sql"
    fi
fi

applied=$(run_sql "SELECT version FROM schema_migrations ORDER BY version")

sql_files=()
while IFS= read -r f; do
    sql_files+=("$f")
done < <(find "$MIGRATIONS_DIR" -maxdepth 1 -name '*.sql' | sort)

# Versions 001-013 predate this runner and intentionally include a handful of
# historical same-number files that migration 014 backfilled as one audited
# baseline. Every runner-managed version must map to exactly one SQL file.
managed_version_numbers=()
managed_version_files=()
for filepath in "${sql_files[@]}"; do
    filename=$(basename "$filepath")
    version=$(echo "$filename" | sed -n 's/^\([0-9]*\).*/\1/p')
    [[ -n "$version" ]] || continue
    version_number=$((10#$version))
    if (( version_number >= 14 )); then
        for i in "${!managed_version_numbers[@]}"; do
            if [[ "${managed_version_numbers[$i]}" == "$version_number" ]]; then
                echo "ERROR: duplicate managed migration version $version_number:" >&2
                echo "  ${managed_version_files[$i]}" >&2
                echo "  $filename" >&2
                exit 1
            fi
        done
        managed_version_numbers+=("$version_number")
        managed_version_files+=("$filename")
    fi
done

pending=()
for filepath in "${sql_files[@]}"; do
    filename=$(basename "$filepath")
    version=$(echo "$filename" | sed -n 's/^\([0-9]*\).*/\1/p')
    if [[ -z "$version" ]]; then
        echo "SKIP: $filename (no version prefix)"
        continue
    fi
    version_clean="$((10#$version))"
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
    python3 "$SCRIPT_DIR/lint-migration.py" "$1"
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
    # Persist the zero-padded filename prefix as the canonical identifier.
    # Lookup above remains compatible with older rows recorded as 15/17/etc.
    version=$(echo "$filename" | sed -n 's/^\([0-9]*\).*/\1/p')
    description=$(echo "$filename" | sed 's/^[0-9]*_//' | sed 's/\.sql$//' | tr '_' ' ' | sed "s/'/''/g")

    echo -n "  Applying $filename ... "

    # Let the invoking (deployment) user open the repository file, then send
    # SQL over stdin. The postgres OS account intentionally cannot traverse
    # /root, but still owns only the database process executing the DDL.
    if psql_cmd -v ON_ERROR_STOP=1 -X < "$filepath"; then
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
