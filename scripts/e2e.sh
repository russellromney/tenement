#!/usr/bin/env bash
# End-to-end smoke test: start tenement, spawn an instance, hit it through the
# proxy, stop it. Exits non-zero on any failure. Designed to run in CI on a
# fresh checkout, but also runnable locally.
#
# Usage:
#   scripts/e2e.sh                       # builds the release binary if needed
#   TEN_BIN=./target/debug/ten scripts/e2e.sh   # use an existing binary
#   E2E_PORT=18080 scripts/e2e.sh        # override server port

set -euo pipefail

# ----------------------------------------------------------------------------
# Config
# ----------------------------------------------------------------------------
REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
PORT="${E2E_PORT:-18080}"
DOMAIN="localhost"
SERVICE="hello"
INSTANCE_ID="world"
INSTANCE="${SERVICE}:${INSTANCE_ID}"

WORK_DIR="$(mktemp -d -t tenement-e2e.XXXXXX)"
DATA_DIR="${WORK_DIR}/data"
SERVE_DIR="${WORK_DIR}/serve"
SERVER_LOG="${WORK_DIR}/server.log"
SERVER_PID=""

mkdir -p "${DATA_DIR}" "${SERVE_DIR}"

# ----------------------------------------------------------------------------
# Helpers
# ----------------------------------------------------------------------------
log() { printf '\n[e2e] %s\n' "$*"; }
fail() { printf '\n[e2e] FAIL: %s\n' "$*" >&2; exit 1; }

cleanup() {
  local rc=$?
  set +e
  if [[ -n "${SERVER_PID}" ]] && kill -0 "${SERVER_PID}" 2>/dev/null; then
    log "stopping server (pid ${SERVER_PID})"
    kill "${SERVER_PID}" 2>/dev/null
    # Give it a moment to drain; then SIGKILL if still alive.
    for _ in 1 2 3 4 5; do
      kill -0 "${SERVER_PID}" 2>/dev/null || break
      sleep 0.2
    done
    kill -9 "${SERVER_PID}" 2>/dev/null
    wait "${SERVER_PID}" 2>/dev/null
  fi
  if [[ ${rc} -ne 0 && -f "${SERVER_LOG}" ]]; then
    printf '\n[e2e] server log:\n'
    sed 's/^/  | /' "${SERVER_LOG}" >&2
  fi
  rm -rf "${WORK_DIR}"
  exit ${rc}
}
trap cleanup EXIT INT TERM

wait_for() {
  # wait_for <description> <timeout-seconds> <command...>
  local desc="$1" timeout="$2"; shift 2
  local deadline=$(( $(date +%s) + timeout ))
  while (( $(date +%s) < deadline )); do
    if "$@" >/dev/null 2>&1; then return 0; fi
    sleep 0.2
  done
  fail "timed out after ${timeout}s waiting for: ${desc}"
}

# ----------------------------------------------------------------------------
# Locate / build binary
# ----------------------------------------------------------------------------
if [[ -n "${TEN_BIN:-}" ]]; then
  # Resolve to absolute path; we cd into the temp dir later.
  if [[ "${TEN_BIN}" = /* ]]; then
    TEN="${TEN_BIN}"
  else
    TEN="$(cd "$(dirname "${TEN_BIN}")" && pwd)/$(basename "${TEN_BIN}")"
  fi
  [[ -x "${TEN}" ]] || fail "TEN_BIN=${TEN_BIN} is not executable"
else
  TEN="${REPO_ROOT}/target/release/ten"
  if [[ ! -x "${TEN}" ]]; then
    log "building release binary (cargo build --release -p tenement-cli)"
    (cd "${REPO_ROOT}" && cargo build --release -p tenement-cli)
  fi
fi
# All CLI commands target the local server we start below. Set via env so we
# don't have to thread --server through every invocation.
export TENEMENT_SERVER="http://127.0.0.1:${PORT}"
log "binary: ${TEN}"
log "server: ${TENEMENT_SERVER}"
log "work dir: ${WORK_DIR}"

# ----------------------------------------------------------------------------
# Test fixture: tiny python HTTP server as the spawned service
# ----------------------------------------------------------------------------
cat > "${SERVE_DIR}/server.sh" <<'SH'
#!/usr/bin/env bash
# Minimal HTTP service used by e2e.sh. Reads $PORT (set by tenement).
set -euo pipefail
exec python3 -m http.server "${PORT:-8000}" --bind 127.0.0.1
SH
chmod +x "${SERVE_DIR}/server.sh"

cat > "${SERVE_DIR}/index.html" <<'HTML'
e2e-ok
HTML

cat > "${SERVE_DIR}/tenement.toml" <<EOF
[settings]
data_dir = "${DATA_DIR}"

[service.${SERVICE}]
command = "./server.sh"
health = "/"
isolation = "process"
EOF

cd "${SERVE_DIR}"

# ----------------------------------------------------------------------------
# 1. Generate admin token (writes \${DATA_DIR}/api_token for CLI auto-read)
# ----------------------------------------------------------------------------
log "generating admin token"
"${TEN}" --data-dir "${DATA_DIR}" token-gen >/dev/null
[[ -f "${DATA_DIR}/api_token" ]] || fail "expected ${DATA_DIR}/api_token to exist"

# ----------------------------------------------------------------------------
# 2. Start server in background
# ----------------------------------------------------------------------------
log "starting server on :${PORT}"
"${TEN}" --data-dir "${DATA_DIR}" serve --port "${PORT}" --domain "${DOMAIN}" \
  >"${SERVER_LOG}" 2>&1 &
SERVER_PID=$!

wait_for "GET /health -> 200" 30 \
  curl -fsS "http://127.0.0.1:${PORT}/health"

# ----------------------------------------------------------------------------
# 3. Spawn instance
# ----------------------------------------------------------------------------
log "spawning ${INSTANCE}"
"${TEN}" --data-dir "${DATA_DIR}" spawn "${INSTANCE}"

wait_for "instance to appear in ps" 10 \
  bash -c "'${TEN}' --data-dir '${DATA_DIR}' ps | grep -q '${INSTANCE_ID}'"

# ----------------------------------------------------------------------------
# 4. Hit it via the proxy (subdomain Host header)
# ----------------------------------------------------------------------------
HOST_HEADER="${INSTANCE_ID}.${SERVICE}.${DOMAIN}"
log "GET / via proxy (Host: ${HOST_HEADER})"
wait_for "proxy returns 200" 30 \
  curl -fsS -H "Host: ${HOST_HEADER}" "http://127.0.0.1:${PORT}/"

body="$(curl -fsS -H "Host: ${HOST_HEADER}" "http://127.0.0.1:${PORT}/index.html")"
if ! grep -q "e2e-ok" <<<"${body}"; then
  fail "expected proxied response to contain 'e2e-ok', got: ${body}"
fi
log "proxy response OK"

# ----------------------------------------------------------------------------
# 5. Stop instance, verify gone
# ----------------------------------------------------------------------------
log "stopping ${INSTANCE}"
"${TEN}" --data-dir "${DATA_DIR}" stop "${INSTANCE}"

wait_for "instance to disappear from ps" 10 \
  bash -c "! '${TEN}' --data-dir '${DATA_DIR}' ps | grep -q '${INSTANCE_ID}'"

log "PASS"
