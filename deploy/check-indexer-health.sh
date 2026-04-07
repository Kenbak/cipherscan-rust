#!/usr/bin/env bash
set -euo pipefail

INDEXER_BIN="${INDEXER_BIN:-/root/cipherscan-rust/target/release/cipherscan-indexer}"
STATE_DIR="${INDEXER_MONITOR_STATE_DIR:-/var/lib/cipherscan-rust-monitor}"
STATE_FILE="${STATE_DIR}/health-alert-state.env"
MAX_LAG="${INDEXER_MAX_LAG:-3}"
MAX_CONSECUTIVE_FAILURES="${INDEXER_MAX_CONSECUTIVE_FAILURES:-0}"
MAX_HEARTBEAT_AGE_SECONDS="${INDEXER_MAX_HEARTBEAT_AGE_SECONDS:-600}"
ALERT_COOLDOWN_SECONDS="${INDEXER_ALERT_COOLDOWN_SECONDS:-1800}"
INDEXER_SERVICE_NAME="${INDEXER_SERVICE_NAME:-cipherscan-rust.service}"
TELEGRAM_BOT_TOKEN="${TELEGRAM_BOT_TOKEN:-}"
TELEGRAM_CHAT_ID="${TELEGRAM_CHAT_ID:-}"
TELEGRAM_API_BASE="${TELEGRAM_API_BASE:-https://api.telegram.org}"
HOST_LABEL="$(hostname -f 2>/dev/null || hostname)"

mkdir -p "${STATE_DIR}"

previous_status="unknown"
previous_fingerprint=""
previous_alert_at=0

if [[ -f "${STATE_FILE}" ]]; then
  # shellcheck disable=SC1090
  source "${STATE_FILE}"
fi

if [[ -z "${TELEGRAM_BOT_TOKEN}" || -z "${TELEGRAM_CHAT_ID}" ]]; then
  echo "TELEGRAM_BOT_TOKEN and TELEGRAM_CHAT_ID must be set" >&2
  exit 2
fi

if [[ ! -x "${INDEXER_BIN}" ]]; then
  echo "Indexer binary is not executable: ${INDEXER_BIN}" >&2
  exit 2
fi

send_telegram() {
  local message="$1"

  curl -fsS -X POST \
    "${TELEGRAM_API_BASE}/bot${TELEGRAM_BOT_TOKEN}/sendMessage" \
    --data-urlencode "chat_id=${TELEGRAM_CHAT_ID}" \
    --data-urlencode "text=${message}" \
    --data-urlencode "disable_web_page_preview=true" \
    >/dev/null
}

persist_state() {
  local current_status="$1"
  local current_fingerprint="$2"
  local current_alert_at="$3"

  cat > "${STATE_FILE}" <<EOF
previous_status=${current_status}
previous_fingerprint=${current_fingerprint}
previous_alert_at=${current_alert_at}
EOF
}

now="$(date +%s)"
timestamp="$(date -u '+%Y-%m-%d %H:%M:%S UTC')"

service_status="$(systemctl is-active "${INDEXER_SERVICE_NAME}" 2>&1 || true)"

if [[ "${service_status}" == "active" ]]; then
  health_output="$("${INDEXER_BIN}" health --max-lag "${MAX_LAG}" --max-consecutive-failures "${MAX_CONSECUTIVE_FAILURES}" --max-heartbeat-age "${MAX_HEARTBEAT_AGE_SECONDS}" --json 2>&1)" || health_rc=$?
  health_rc="${health_rc:-0}"
else
  health_output="service ${INDEXER_SERVICE_NAME} is not active: ${service_status}"
  health_rc=1
fi

combined_output="$(printf 'service=%s\nhealth rc=%s\n%s\n' "${service_status}" "${health_rc}" "${health_output}")"
fingerprint="$(printf '%s' "${combined_output}" | shasum -a 256 | awk '{print $1}')"
summary="$(printf '%s' "${combined_output}" | tail -n 40 | cut -c1-3500)"

if [[ "${health_rc}" -eq 0 ]]; then
  if [[ "${previous_status}" == "unhealthy" ]]; then
    send_telegram "$(cat <<EOF
CipherScan indexer recovered
Host: ${HOST_LABEL}
Time: ${timestamp}
Lag threshold: ${MAX_LAG}
Failure threshold: ${MAX_CONSECUTIVE_FAILURES}
Heartbeat threshold seconds: ${MAX_HEARTBEAT_AGE_SECONDS}
EOF
)"
  fi

  persist_state "healthy" "${fingerprint}" "${now}"
  exit 0
fi

should_alert=0
if [[ "${previous_status}" != "unhealthy" ]]; then
  should_alert=1
elif [[ "${previous_fingerprint}" != "${fingerprint}" ]]; then
  should_alert=1
elif (( now - previous_alert_at >= ALERT_COOLDOWN_SECONDS )); then
  should_alert=1
fi

if (( should_alert == 1 )); then
  send_telegram "$(cat <<EOF
CipherScan indexer unhealthy
Host: ${HOST_LABEL}
Time: ${timestamp}
Lag threshold: ${MAX_LAG}
Failure threshold: ${MAX_CONSECUTIVE_FAILURES}
Heartbeat threshold seconds: ${MAX_HEARTBEAT_AGE_SECONDS}
Cooldown seconds: ${ALERT_COOLDOWN_SECONDS}

${summary}
EOF
)"
  persist_state "unhealthy" "${fingerprint}" "${now}"
else
  persist_state "unhealthy" "${previous_fingerprint}" "${previous_alert_at}"
fi

exit 1
