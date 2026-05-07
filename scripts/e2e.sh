#!/usr/bin/env bash
# End-to-end coverage test for tenement.
#
# Runs many scenarios against a single `ten serve` instance, each exercising
# a different user-visible feature: basic spawn/proxy/stop, wake-on-request,
# weighted multi-instance routing, restart-on-crash, logs round-trip,
# the rest of the CLI surface, blue/green deploy + route, and one real
# example app (python-fastapi).
#
# Usage:
#   scripts/e2e.sh                              # builds release binary if needed
#   TEN_BIN=./target/debug/ten scripts/e2e.sh   # use an existing binary
#   E2E_PORT=18080 scripts/e2e.sh               # override server port
#   E2E_SKIP_FASTAPI=1 scripts/e2e.sh           # skip the uv/fastapi scenario

set -euo pipefail

# ----------------------------------------------------------------------------
# Config
# ----------------------------------------------------------------------------
REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
PORT="${E2E_PORT:-18080}"
DOMAIN="localhost"

WORK_DIR="$(mktemp -d -t tenement-e2e.XXXXXX)"
DATA_DIR="${WORK_DIR}/data"
SERVE_DIR="${WORK_DIR}/serve"
SERVER_LOG="${WORK_DIR}/server.log"
SERVER_PID=""

mkdir -p "${DATA_DIR}" "${SERVE_DIR}"

PASSED=()
FAILED=()
CURRENT_SCENARIO=""

# ----------------------------------------------------------------------------
# Helpers
# ----------------------------------------------------------------------------
log()    { printf '\n[e2e] %s\n' "$*"; }
banner() { printf '\n========== %s ==========\n' "$*"; }
fail()   { printf '\n[e2e] FAIL (%s): %s\n' "${CURRENT_SCENARIO:-?}" "$*" >&2; exit 1; }

cleanup() {
  local rc=$?
  set +e
  if [[ -n "${SERVER_PID}" ]] && kill -0 "${SERVER_PID}" 2>/dev/null; then
    log "stopping server (pid ${SERVER_PID})"
    kill "${SERVER_PID}" 2>/dev/null
    for _ in 1 2 3 4 5 6 7 8 9 10; do
      kill -0 "${SERVER_PID}" 2>/dev/null || break
      sleep 0.2
    done
    kill -9 "${SERVER_PID}" 2>/dev/null
    wait "${SERVER_PID}" 2>/dev/null
  fi

  printf '\n========== SUMMARY ==========\n'
  printf 'Passed: %d\n' "${#PASSED[@]}"
  if (( ${#PASSED[@]} > 0 )); then
    for s in "${PASSED[@]}"; do printf '  + %s\n' "$s"; done
  fi
  printf 'Failed: %d\n' "${#FAILED[@]}"
  if (( ${#FAILED[@]} > 0 )); then
    for s in "${FAILED[@]}"; do printf '  - %s\n' "$s"; done
  fi

  if [[ ${rc} -ne 0 || ${#FAILED[@]} -gt 0 ]]; then
    if [[ -f "${SERVER_LOG}" ]]; then
      printf '\n[e2e] server log (last 200 lines):\n'
      tail -n 200 "${SERVER_LOG}" | sed 's/^/  | /' >&2
    fi
    rm -rf "${WORK_DIR}"
    exit $(( rc != 0 ? rc : 1 ))
  fi

  rm -rf "${WORK_DIR}"
  exit 0
}
trap cleanup EXIT INT TERM

# wait_for <description> <timeout-seconds> <command...>
wait_for() {
  local desc="$1" timeout="$2"; shift 2
  local deadline=$(( $(date +%s) + timeout ))
  while (( $(date +%s) < deadline )); do
    if "$@" >/dev/null 2>&1; then return 0; fi
    sleep 0.2
  done
  fail "timed out after ${timeout}s waiting for: ${desc}"
}

# Run a scenario function; record pass/fail and continue to the next.
scenario() {
  local name="$1" fn="$2"
  CURRENT_SCENARIO="${name}"
  banner "${name}"
  if (set -e; "${fn}"); then
    PASSED+=("${name}")
    log "${name}: PASS"
  else
    FAILED+=("${name}")
    log "${name}: FAIL"
  fi
  CURRENT_SCENARIO=""
}

# ----------------------------------------------------------------------------
# Locate / build binary
# ----------------------------------------------------------------------------
if [[ -n "${TEN_BIN:-}" ]]; then
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
export TENEMENT_SERVER="http://127.0.0.1:${PORT}"
log "binary:   ${TEN}"
log "server:   ${TENEMENT_SERVER}"
log "work dir: ${WORK_DIR}"

# ----------------------------------------------------------------------------
# Test fixtures
# ----------------------------------------------------------------------------

# Simple HTTP server (uses python3, preinstalled on GHA Linux runners).
cat > "${SERVE_DIR}/simple.sh" <<'SH'
#!/usr/bin/env bash
set -euo pipefail
exec python3 -m http.server "${PORT:-8000}" --bind 127.0.0.1
SH
chmod +x "${SERVE_DIR}/simple.sh"

# Identifiable HTTP server: returns "id=<INSTANCE_ID> pid=<PID>".
# Used by weighted-routing and restart-on-crash scenarios.
cat > "${SERVE_DIR}/identify.sh" <<'SH'
#!/usr/bin/env bash
set -euo pipefail
exec python3 -u -c '
import http.server, os, sys
PORT = int(os.environ["PORT"])
INSTANCE_ID = os.environ.get("INSTANCE_ID", "?")
PID = os.getpid()
print(f"started id={INSTANCE_ID} pid={PID} port={PORT}", flush=True)
class H(http.server.BaseHTTPRequestHandler):
    def do_GET(self):
        self.send_response(200)
        self.send_header("Content-Type", "text/plain")
        self.end_headers()
        self.wfile.write(f"id={INSTANCE_ID} pid={PID}\n".encode())
    def do_POST(self):
        n = int(self.headers.get("Content-Length", "0"))
        body = self.rfile.read(n).decode()
        self.send_response(200)
        self.send_header("Content-Type", "text/plain")
        self.end_headers()
        self.wfile.write(f"echo id={INSTANCE_ID} body={body}\n".encode())
    def log_message(self, *a, **kw): pass
http.server.HTTPServer(("127.0.0.1", PORT), H).serve_forever()
'
SH
chmod +x "${SERVE_DIR}/identify.sh"

# Static file with known marker, used by the basic scenario.
echo "e2e-ok" > "${SERVE_DIR}/index.html"

# Build the multi-service tenement.toml.
# Use short health intervals + zero backoff so restart tests don't take forever.
cat > "${SERVE_DIR}/tenement.toml" <<EOF
[settings]
data_dir = "${DATA_DIR}"
health_check_interval = 1
backoff_base_ms = 0

[service.basic]
command = "./simple.sh"
health = "/"
isolation = "process"

[service.wake]
command = "./simple.sh"
health = "/"
isolation = "process"

[service.weighted]
command = "./identify.sh"
health = "/"
isolation = "process"
[service.weighted.env]
INSTANCE_ID = "{id}"

[service.crash]
command = "./identify.sh"
health = "/"
isolation = "process"
restart = "on-failure"
[service.crash.env]
INSTANCE_ID = "{id}"

[service.logs]
command = "./identify.sh"
health = "/"
isolation = "process"
[service.logs.env]
INSTANCE_ID = "{id}"

[service.bg]
command = "./identify.sh"
health = "/"
isolation = "process"
[service.bg.env]
INSTANCE_ID = "{id}"
EOF

# Optionally append the python-fastapi service if uv is available and the
# example exists. We pull deps via `uv run --with` so we don't need to build
# the example's pyproject (which is configured for a real package layout).
if [[ -z "${E2E_SKIP_FASTAPI:-}" ]] && command -v uv >/dev/null 2>&1 \
    && [[ -f "${REPO_ROOT}/examples/python-fastapi/app.py" ]]; then
  log "fastapi example will run (uv present)"
  cp "${REPO_ROOT}/examples/python-fastapi/app.py" "${SERVE_DIR}/fastapi_app.py"
  # Pre-warm uv's cache so the spawned process starts fast on first request.
  log "pre-warming uv cache (fastapi/uvicorn)"
  uv run --quiet --with 'fastapi>=0.109.0' --with 'uvicorn>=0.27.0' \
    python -c 'import fastapi, uvicorn' \
    || fail "uv pre-warm failed"
  cat >> "${SERVE_DIR}/tenement.toml" <<EOF

[service.fastapi]
command = "uv run --with fastapi>=0.109.0 --with uvicorn>=0.27.0 python fastapi_app.py"
health = "/health"
isolation = "process"
startup_timeout = 60
EOF
  E2E_FASTAPI=1
else
  log "fastapi example will be skipped (uv missing or example absent)"
  E2E_FASTAPI=0
fi

cd "${SERVE_DIR}"

# ----------------------------------------------------------------------------
# Bootstrap: token, server, readiness
# ----------------------------------------------------------------------------
log "generating admin token"
"${TEN}" --data-dir "${DATA_DIR}" token-gen >/dev/null
[[ -f "${DATA_DIR}/api_token" ]] || fail "expected ${DATA_DIR}/api_token to exist"

log "starting server on :${PORT}"
"${TEN}" --data-dir "${DATA_DIR}" serve --port "${PORT}" --domain "${DOMAIN}" \
  >"${SERVER_LOG}" 2>&1 &
SERVER_PID=$!

wait_for "GET /health -> 200" 30 \
  curl -fsS "http://127.0.0.1:${PORT}/health"

# ----------------------------------------------------------------------------
# Scenario helpers
# ----------------------------------------------------------------------------

# Run `ten` with the test data dir.
ten() { "${TEN}" --data-dir "${DATA_DIR}" "$@"; }

# Curl through the proxy with a Host header.
proxy_curl() {
  local host="$1"; shift
  curl -fsS -H "Host: ${host}" "http://127.0.0.1:${PORT}$@"
}

# Wait for an instance to appear / disappear from `ten ps`.
wait_for_in_ps()    { wait_for "in ps: $1"     10 bash -c "ten ps | grep -q -- '$1'"; }
wait_for_not_in_ps(){ wait_for "not in ps: $1" 10 bash -c "! ten ps | grep -q -- '$1'"; }

# ----------------------------------------------------------------------------
# Scenario 1 — basic spawn / proxy / stop
# ----------------------------------------------------------------------------
test_basic() {
  ten spawn basic:one
  wait_for_in_ps "basic:one"

  wait_for "proxy 200 (basic)" 30 \
    curl -fsS -H "Host: one.basic.${DOMAIN}" "http://127.0.0.1:${PORT}/"

  local body; body="$(proxy_curl "one.basic.${DOMAIN}" /index.html)"
  grep -q "e2e-ok" <<<"${body}" || fail "expected 'e2e-ok' in body, got: ${body}"

  ten stop basic:one
  wait_for_not_in_ps "basic:one"
}

# ----------------------------------------------------------------------------
# Scenario 2 — wake-on-request (proxy auto-spawns a stopped instance)
# ----------------------------------------------------------------------------
test_wake_on_request() {
  # Confirm starting state: no wake instance running.
  if ten ps | grep -q "wake:"; then
    fail "expected no 'wake:' instances at start"
  fi

  # Hit the proxy; it should wake the instance.
  wait_for "wake proxy 200" 30 \
    curl -fsS -H "Host: cold.wake.${DOMAIN}" "http://127.0.0.1:${PORT}/"

  wait_for_in_ps "wake:cold"

  ten stop wake:cold
  wait_for_not_in_ps "wake:cold"
}

# ----------------------------------------------------------------------------
# Scenario 3 — weighted routing distributes across multiple instances
# ----------------------------------------------------------------------------
test_weighted_multi_instance() {
  ten spawn weighted:a
  ten spawn weighted:b
  wait_for_in_ps "weighted:a"
  wait_for_in_ps "weighted:b"

  # Wait until both backends respond directly before testing weighted route.
  wait_for "weighted:a serving" 15 \
    curl -fsS -H "Host: a.weighted.${DOMAIN}" "http://127.0.0.1:${PORT}/"
  wait_for "weighted:b serving" 15 \
    curl -fsS -H "Host: b.weighted.${DOMAIN}" "http://127.0.0.1:${PORT}/"

  # Hit the weighted form (no id) many times; both ids should appear.
  local seen_a=0 seen_b=0 i
  for i in $(seq 1 40); do
    local resp
    resp="$(proxy_curl "weighted.${DOMAIN}" /)" || continue
    case "${resp}" in
      *"id=a"*) seen_a=$((seen_a+1));;
      *"id=b"*) seen_b=$((seen_b+1));;
    esac
  done
  log "weighted distribution: a=${seen_a} b=${seen_b} (out of 40)"
  (( seen_a > 0 )) || fail "instance 'a' never selected by weighted routing"
  (( seen_b > 0 )) || fail "instance 'b' never selected by weighted routing"

  # Skew weights and verify the response distribution shifts.
  ten weight weighted:a 0
  ten weight weighted:b 100
  local skew_a=0 skew_b=0
  for i in $(seq 1 20); do
    local resp; resp="$(proxy_curl "weighted.${DOMAIN}" /)" || continue
    case "${resp}" in
      *"id=a"*) skew_a=$((skew_a+1));;
      *"id=b"*) skew_b=$((skew_b+1));;
    esac
  done
  log "after weight a=0 b=100: a=${skew_a} b=${skew_b} (out of 20)"
  (( skew_a == 0 )) || fail "instance 'a' should receive 0 traffic with weight=0, got ${skew_a}"
  (( skew_b > 0 )) || fail "instance 'b' should receive traffic with weight=100"

  ten stop weighted:a
  ten stop weighted:b
  wait_for_not_in_ps "weighted:a"
  wait_for_not_in_ps "weighted:b"
}

# ----------------------------------------------------------------------------
# Scenario 4 — restart-on-crash (process killed → tenement respawns)
# ----------------------------------------------------------------------------
test_restart_on_crash() {
  ten spawn crash:one
  wait_for_in_ps "crash:one"
  wait_for "crash:one serving" 15 \
    curl -fsS -H "Host: one.crash.${DOMAIN}" "http://127.0.0.1:${PORT}/"

  local before; before="$(proxy_curl "one.crash.${DOMAIN}" /)"
  local pid_before; pid_before="$(grep -oE 'pid=[0-9]+' <<<"${before}" | cut -d= -f2)"
  [[ -n "${pid_before}" ]] || fail "could not parse pid from: ${before}"
  log "before crash: pid=${pid_before}"

  log "killing pid ${pid_before}"
  kill -9 "${pid_before}" 2>/dev/null || fail "kill -9 ${pid_before} failed"

  # Tenement detects the dead process at the next health check
  # (interval=1s) and respawns. Allow generous time for CI.
  wait_for "post-crash 200" 30 \
    curl -fsS -H "Host: one.crash.${DOMAIN}" "http://127.0.0.1:${PORT}/"

  local after; after="$(proxy_curl "one.crash.${DOMAIN}" /)"
  local pid_after; pid_after="$(grep -oE 'pid=[0-9]+' <<<"${after}" | cut -d= -f2)"
  log "after crash: pid=${pid_after}"
  [[ -n "${pid_after}" ]] || fail "could not parse pid post-restart from: ${after}"
  [[ "${pid_after}" != "${pid_before}" ]] || fail "pid did not change after kill — restart did not fire"

  ten stop crash:one
  wait_for_not_in_ps "crash:one"
}

# ----------------------------------------------------------------------------
# Scenario 5 — logs round-trip (stdout from spawned process is queryable)
# ----------------------------------------------------------------------------
test_logs() {
  ten spawn logs:one
  wait_for_in_ps "logs:one"

  # The identify.sh wrapper prints "started id=... pid=... port=..." on boot.
  # Wait for the batch flusher (250ms) and a margin, then query.
  sleep 2
  local out; out="$(ten logs logs:one --limit 50)" || fail "ten logs failed: ${out}"
  grep -q "started id=one" <<<"${out}" || {
    printf '%s\n' "${out}" >&2
    fail "expected 'started id=one' in logs output"
  }

  # Search filter should narrow the query.
  local search; search="$(ten logs logs:one --search started --limit 50)"
  grep -q "started" <<<"${search}" || fail "search=started returned nothing"

  ten stop logs:one
  wait_for_not_in_ps "logs:one"
}

# ----------------------------------------------------------------------------
# Scenario 6 — remaining CLI surface (health, restart, config, init,
#              POST through proxy)
# ----------------------------------------------------------------------------
test_cli_surface() {
  # config: prints loaded services
  local cfg; cfg="$(ten config)"
  grep -q "basic" <<<"${cfg}" || fail "ten config did not list 'basic' service"

  # health + restart on a running instance
  ten spawn basic:cli
  wait_for_in_ps "basic:cli"
  wait_for "health 200" 15 \
    curl -fsS -H "Host: cli.basic.${DOMAIN}" "http://127.0.0.1:${PORT}/"

  local h; h="$(ten health basic:cli)"
  grep -qE "(healthy|ok|up)" <<<"${h}" \
    || fail "ten health output did not look healthy: ${h}"

  ten restart basic:cli
  # After restart the instance id stays in ps; just verify it still serves.
  wait_for "post-restart 200" 30 \
    curl -fsS -H "Host: cli.basic.${DOMAIN}" "http://127.0.0.1:${PORT}/"

  ten stop basic:cli
  wait_for_not_in_ps "basic:cli"

  # POST through the proxy (we've only verified GET so far).
  ten spawn weighted:post
  wait_for_in_ps "weighted:post"
  wait_for "post backend up" 15 \
    curl -fsS -H "Host: post.weighted.${DOMAIN}" "http://127.0.0.1:${PORT}/"
  local post_resp
  post_resp="$(curl -fsS -X POST -d 'hello' \
    -H "Host: post.weighted.${DOMAIN}" "http://127.0.0.1:${PORT}/")"
  grep -q "echo id=post body=hello" <<<"${post_resp}" \
    || fail "POST through proxy failed, got: ${post_resp}"
  ten stop weighted:post
  wait_for_not_in_ps "weighted:post"

  # `ten init` scaffolds a new project in an empty dir.
  local init_dir="${WORK_DIR}/init-test"
  mkdir -p "${init_dir}"
  (cd "${init_dir}" && ten init --name demo --command "./run.sh" >/dev/null)
  [[ -f "${init_dir}/tenement.toml" ]] || fail "ten init did not create tenement.toml"
  grep -q "service.demo" "${init_dir}/tenement.toml" \
    || fail "scaffolded tenement.toml missing [service.demo]"
}

# ----------------------------------------------------------------------------
# Scenario 7 — blue/green deploy + route
# ----------------------------------------------------------------------------
test_deploy_route() {
  # `ten deploy` spawns an instance and waits for it to become healthy.
  ten deploy bg:v1 --weight 100 --timeout 30
  wait_for_in_ps "bg:v1"
  wait_for "v1 serving" 15 \
    curl -fsS -H "Host: v1.bg.${DOMAIN}" "http://127.0.0.1:${PORT}/"

  ten deploy bg:v2 --weight 0 --timeout 30
  wait_for_in_ps "bg:v2"
  wait_for "v2 serving" 15 \
    curl -fsS -H "Host: v2.bg.${DOMAIN}" "http://127.0.0.1:${PORT}/"

  # Pre-route: weighted form should hit only v1 (v2 has weight 0).
  local pre_v1=0 pre_v2=0
  for _ in $(seq 1 10); do
    local r; r="$(proxy_curl "bg.${DOMAIN}" /)" || continue
    case "${r}" in
      *"id=v1"*) pre_v1=$((pre_v1+1));;
      *"id=v2"*) pre_v2=$((pre_v2+1));;
    esac
  done
  log "pre-route: v1=${pre_v1} v2=${pre_v2}"
  (( pre_v1 > 0 ))   || fail "v1 should receive traffic before route"
  (( pre_v2 == 0 ))  || fail "v2 should receive 0 traffic before route, got ${pre_v2}"

  # Atomic blue/green swap.
  ten route bg --from v1 --to v2

  local post_v1=0 post_v2=0
  for _ in $(seq 1 10); do
    local r; r="$(proxy_curl "bg.${DOMAIN}" /)" || continue
    case "${r}" in
      *"id=v1"*) post_v1=$((post_v1+1));;
      *"id=v2"*) post_v2=$((post_v2+1));;
    esac
  done
  log "post-route: v1=${post_v1} v2=${post_v2}"
  (( post_v1 == 0 )) || fail "v1 should receive 0 traffic after route, got ${post_v1}"
  (( post_v2 > 0 ))  || fail "v2 should receive traffic after route"

  ten stop bg:v1
  ten stop bg:v2
  wait_for_not_in_ps "bg:v1"
  wait_for_not_in_ps "bg:v2"
}

# ----------------------------------------------------------------------------
# Scenario 8 — real example: python-fastapi (skipped if uv missing)
# ----------------------------------------------------------------------------
test_fastapi_example() {
  if [[ "${E2E_FASTAPI}" != "1" ]]; then
    log "skipping (uv not present or example missing)"
    return 0
  fi
  ten spawn fastapi:prod
  wait_for_in_ps "fastapi:prod"

  wait_for "fastapi /health 200" 60 \
    curl -fsS -H "Host: prod.fastapi.${DOMAIN}" "http://127.0.0.1:${PORT}/health"

  local body; body="$(proxy_curl "prod.fastapi.${DOMAIN}" /)"
  grep -q "FastAPI" <<<"${body}" \
    || fail "expected FastAPI message in body, got: ${body}"

  local item; item="$(proxy_curl "prod.fastapi.${DOMAIN}" /items/42)"
  grep -q '"item_id":42' <<<"${item}" \
    || fail "expected item_id=42 in /items/42, got: ${item}"

  ten stop fastapi:prod
  wait_for_not_in_ps "fastapi:prod"
}

# ----------------------------------------------------------------------------
# Run scenarios
# ----------------------------------------------------------------------------
scenario "basic"                test_basic
scenario "wake-on-request"      test_wake_on_request
scenario "weighted-multi"       test_weighted_multi_instance
scenario "restart-on-crash"     test_restart_on_crash
scenario "logs"                 test_logs
scenario "cli-surface"          test_cli_surface
scenario "deploy-route"         test_deploy_route
scenario "fastapi-example"      test_fastapi_example

if (( ${#FAILED[@]} > 0 )); then
  exit 1
fi
log "ALL PASS"
