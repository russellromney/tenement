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
        print(f"req id={INSTANCE_ID} pid={PID} path={self.path}", flush=True)
        self.send_response(200)
        self.send_header("Content-Type", "text/plain")
        self.end_headers()
        self.wfile.write(f"id={INSTANCE_ID} pid={PID}\n".encode())
    def do_POST(self):
        n = int(self.headers.get("Content-Length", "0"))
        body = self.rfile.read(n).decode()
        print(f"req id={INSTANCE_ID} pid={PID} POST body={body}", flush=True)
        self.send_response(200)
        self.send_header("Content-Type", "text/plain")
        self.end_headers()
        self.wfile.write(f"echo id={INSTANCE_ID} body={body}\n".encode())
    # Suppress the default access log; we emit our own line above.
    def log_message(self, *a, **kw): pass
http.server.HTTPServer(("127.0.0.1", PORT), H).serve_forever()
'
SH
chmod +x "${SERVE_DIR}/identify.sh"

# Same as identify.sh but /health returns 500. Used by the unhealthy-detection
# scenario. / returns 200 so spawn (which only waits for socket-up) succeeds.
cat > "${SERVE_DIR}/identify_unhealthy.sh" <<'SH'
#!/usr/bin/env bash
set -euo pipefail
exec python3 -u -c '
import http.server, os
PORT = int(os.environ["PORT"])
class H(http.server.BaseHTTPRequestHandler):
    def do_GET(self):
        if self.path == "/health":
            self.send_response(500)
            self.end_headers()
            self.wfile.write(b"intentionally bad\n")
        else:
            self.send_response(200)
            self.end_headers()
            self.wfile.write(b"ok\n")
    def log_message(self, *a, **kw): pass
http.server.HTTPServer(("127.0.0.1", PORT), H).serve_forever()
'
SH
chmod +x "${SERVE_DIR}/identify_unhealthy.sh"

# Persist marker server: writes a marker file (only if absent) into the
# per-instance data dir, then serves HTTP. Used by the storage-persistence
# scenarios.
cat > "${SERVE_DIR}/identify_persist.sh" <<'SH'
#!/usr/bin/env bash
set -euo pipefail
: "${MARKER_PATH:?MARKER_PATH must be set}"
mkdir -p "$(dirname "$MARKER_PATH")"
if [[ ! -f "$MARKER_PATH" ]]; then
  echo "first-write-$$" > "$MARKER_PATH"
fi
exec python3 -m http.server "${PORT:-8000}" --bind 127.0.0.1
SH
chmod +x "${SERVE_DIR}/identify_persist.sh"

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

# Idle scale-to-zero: 2s idle window
[service.idle]
command = "./simple.sh"
health = "/"
isolation = "process"
idle_timeout = 2

# Tenant-token scope: instances have ids that match a tenant token.
[service.tenant]
command = "./identify.sh"
health = "/"
isolation = "process"
[service.tenant.env]
INSTANCE_ID = "{id}"

# Health endpoint that always returns 500.
[service.bad]
command = "./identify_unhealthy.sh"
health = "/health"
isolation = "process"

# Storage persistence: data_dir kept after stop.
[service.persist]
command = "./identify_persist.sh"
health = "/"
isolation = "process"
storage_persist = true
[service.persist.env]
MARKER_PATH = "{data_dir}/{name}/{id}/marker.txt"

# Storage NOT persisted (default is true; we explicitly opt out here so the
# cleanup-on-stop path is exercised).
[service.ephemeral]
command = "./identify_persist.sh"
health = "/"
isolation = "process"
storage_persist = false
[service.ephemeral.env]
MARKER_PATH = "{data_dir}/{name}/{id}/marker.txt"
EOF

# Optionally append the python-fastapi service if uv is available and the
# example exists. We pull deps via `uv run --with` so we don't need to build
# the example's pyproject (which is configured for a real package layout).
if [[ -z "${E2E_SKIP_FASTAPI:-}" ]] && command -v uv >/dev/null 2>&1 \
    && [[ -f "${REPO_ROOT}/examples/python-fastapi/app.py" ]]; then
  log "fastapi example will run (uv present)"
  # Run from a copy of the unmodified example so we can exercise the
  # README-documented setup ('uv sync' + 'uv run python app.py') without
  # mutating the example tree.
  FASTAPI_DIR="${WORK_DIR}/fastapi"
  mkdir -p "${FASTAPI_DIR}"
  cp "${REPO_ROOT}/examples/python-fastapi/app.py" "${FASTAPI_DIR}/"
  cp "${REPO_ROOT}/examples/python-fastapi/pyproject.toml" "${FASTAPI_DIR}/"
  # Use the runner's preinstalled python3 to avoid uv downloading a fresh
  # interpreter on cold CI (can stall for minutes silently).
  PY="$(command -v python3)"
  log "uv sync (proves examples/python-fastapi pyproject is healthy)"
  (cd "${FASTAPI_DIR}" && uv sync --python "${PY}") \
    || fail "uv sync failed in fastapi example"
  cat >> "${SERVE_DIR}/tenement.toml" <<EOF

[service.fastapi]
command = "uv run --python ${PY} python app.py"
workdir = "${FASTAPI_DIR}"
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

# Wait for an instance to appear / disappear from `ten ps`. Inlined (rather
# than delegating to wait_for) because the helper is a shell function and
# shell functions are not visible to subshells spawned via `bash -c`.
wait_for_in_ps() {
  local id="$1" deadline=$(( $(date +%s) + 15 ))
  while (( $(date +%s) < deadline )); do
    if ten ps 2>/dev/null | grep -q -- "$id"; then return 0; fi
    sleep 0.2
  done
  fail "timed out after 15s waiting for in ps: $id"
}
wait_for_not_in_ps() {
  local id="$1" deadline=$(( $(date +%s) + 15 ))
  while (( $(date +%s) < deadline )); do
    if ! ten ps 2>/dev/null | grep -q -- "$id"; then return 0; fi
    sleep 0.2
  done
  fail "timed out after 15s waiting for not in ps: $id"
}

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

  # The proxy now blocks briefly while the hypervisor's health checker
  # respawns the dead process, so a single request right after the kill
  # should still succeed (no 502s for the client) — provided we give it
  # a generous timeout for the whole round-trip.
  log "single curl right after kill — proxy must wait, not 502"
  local after
  after="$(curl -fsS --max-time 30 \
    -H "Host: one.crash.${DOMAIN}" "http://127.0.0.1:${PORT}/")" \
    || fail "single curl after kill returned non-2xx — proxy retry didn't kick in"

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
# Scenario 9 — idle timeout (scale-to-zero) + wake-back-up
# ----------------------------------------------------------------------------
test_idle_timeout() {
  ten spawn idle:one
  wait_for_in_ps "idle:one"
  wait_for "idle backend serving" 15 \
    curl -fsS -H "Host: one.idle.${DOMAIN}" "http://127.0.0.1:${PORT}/"

  # idle_timeout = 2s and health_check_interval = 1s, so the idle reaper
  # should stop the instance within ~3-4s. Give CI a generous window.
  log "waiting for idle reaper to stop the instance (idle_timeout=2s)"
  local deadline=$(( $(date +%s) + 20 ))
  while (( $(date +%s) < deadline )); do
    if ! ten ps 2>/dev/null | grep -q "idle:one"; then
      break
    fi
    sleep 0.5
  done
  if ten ps 2>/dev/null | grep -q "idle:one"; then
    fail "idle reaper did not stop idle:one within 20s"
  fi
  log "instance was reaped; verifying wake-on-request brings it back"

  # A subsequent request should wake it.
  wait_for "post-idle wake 200" 30 \
    curl -fsS -H "Host: one.idle.${DOMAIN}" "http://127.0.0.1:${PORT}/"
  wait_for_in_ps "idle:one"
  ten stop idle:one
  wait_for_not_in_ps "idle:one"
}

# ----------------------------------------------------------------------------
# Scenario 10 — tenant tokens (multi-tenancy scope)
# ----------------------------------------------------------------------------
test_tenant_tokens() {
  # Spawn two instances belonging to different "tenants" (id == tenant id).
  ten spawn tenant:alice
  ten spawn tenant:bob
  wait_for_in_ps "tenant:alice"
  wait_for_in_ps "tenant:bob"

  # Mint a tenant token scoped to "alice" and use it directly via curl
  # (so we don't override the auto-saved admin token in api_token).
  local alice_token
  alice_token="$(ten token-gen --tenant alice 2>&1 \
    | awk '/^  /{print $1; exit}')"
  [[ -n "${alice_token}" ]] || fail "could not parse alice tenant token"

  # Admin sees both, alice sees only alice.
  local admin_list; admin_list="$(ten ps)"
  grep -q "tenant:alice" <<<"${admin_list}" || fail "admin should see alice"
  grep -q "tenant:bob"   <<<"${admin_list}" || fail "admin should see bob"

  local alice_list
  alice_list="$(curl -fsS -H "Authorization: Bearer ${alice_token}" \
    "http://127.0.0.1:${PORT}/api/instances")" \
    || fail "alice token rejected"
  grep -q "tenant:alice" <<<"${alice_list}" \
    || fail "alice should see her own instance"
  if grep -q "tenant:bob" <<<"${alice_list}"; then
    fail "alice should NOT see bob's instance — got: ${alice_list}"
  fi

  # Tenant tokens cannot deploy (admin-only). Expect non-2xx.
  local code
  code="$(curl -s -o /dev/null -w '%{http_code}' \
    -X POST -H "Authorization: Bearer ${alice_token}" \
    -H "Content-Type: application/json" \
    -d '{"process":"tenant","id":"alice","weight":100,"timeout_secs":30}' \
    "http://127.0.0.1:${PORT}/api/deploy")"
  [[ "${code}" =~ ^4 ]] \
    || fail "tenant deploy should be rejected (4xx), got ${code}"

  ten stop tenant:alice
  ten stop tenant:bob
  wait_for_not_in_ps "tenant:alice"
  wait_for_not_in_ps "tenant:bob"
}

# ----------------------------------------------------------------------------
# Scenario 11 — health-check failure marks instance unhealthy
# ----------------------------------------------------------------------------
test_unhealthy_detection() {
  ten spawn bad:one
  wait_for_in_ps "bad:one"

  # Wait for at least one health-check cycle (interval=1s) plus a buffer.
  # consecutive_failures threshold may take 2-3 cycles before health flips
  # to "unhealthy".
  local deadline=$(( $(date +%s) + 15 )) status=""
  while (( $(date +%s) < deadline )); do
    status="$(ten health bad:one 2>&1 || true)"
    if grep -qiE "unhealthy|degraded|unknown" <<<"${status}"; then
      break
    fi
    sleep 0.5
  done
  if ! grep -qiE "unhealthy|degraded|unknown" <<<"${status}"; then
    printf '%s\n' "${status}" >&2
    fail "expected 'bad:one' to be marked unhealthy"
  fi
  log "bad:one health: ${status}"

  ten stop bad:one
  wait_for_not_in_ps "bad:one"
}

# ----------------------------------------------------------------------------
# Scenario 12 — storage persistence (and its negative)
# ----------------------------------------------------------------------------
test_storage_persistence() {
  # Persistent: data_dir kept after stop.
  ten spawn persist:p1
  wait_for_in_ps "persist:p1"
  wait_for "persist backend serving" 15 \
    curl -fsS -H "Host: p1.persist.${DOMAIN}" "http://127.0.0.1:${PORT}/"
  local marker="${DATA_DIR}/persist/p1/marker.txt"
  wait_for "marker file written" 10 test -f "${marker}"
  local first_value; first_value="$(cat "${marker}")"
  log "persist marker (first run): ${first_value}"

  ten stop persist:p1
  wait_for_not_in_ps "persist:p1"
  [[ -f "${marker}" ]] \
    || fail "with storage_persist=true, marker file should remain after stop"

  # Spawn again — the script only writes the marker if it doesn't exist,
  # so it should preserve the original value.
  ten spawn persist:p1
  wait_for_in_ps "persist:p1"
  wait_for "persist backend (2nd run) serving" 15 \
    curl -fsS -H "Host: p1.persist.${DOMAIN}" "http://127.0.0.1:${PORT}/"
  local second_value; second_value="$(cat "${marker}")"
  log "persist marker (second run): ${second_value}"
  [[ "${first_value}" == "${second_value}" ]] \
    || fail "persisted marker changed across stop+spawn (was '${first_value}', now '${second_value}')"
  ten stop persist:p1
  wait_for_not_in_ps "persist:p1"

  # Negative: ephemeral service (storage_persist defaults to false).
  ten spawn ephemeral:e1
  wait_for_in_ps "ephemeral:e1"
  wait_for "ephemeral backend serving" 15 \
    curl -fsS -H "Host: e1.ephemeral.${DOMAIN}" "http://127.0.0.1:${PORT}/"
  local emarker="${DATA_DIR}/ephemeral/e1/marker.txt"
  wait_for "ephemeral marker written" 10 test -f "${emarker}"
  ten stop ephemeral:e1
  wait_for_not_in_ps "ephemeral:e1"
  if [[ -f "${emarker}" ]]; then
    fail "with storage_persist=false (default), data dir should be cleaned up after stop"
  fi
}

# ----------------------------------------------------------------------------
# Scenario 13 — log streaming via SSE (`/api/logs/stream`)
# ----------------------------------------------------------------------------
test_log_streaming() {
  ten spawn logs:stream
  wait_for_in_ps "logs:stream"
  wait_for "logs:stream serving" 15 \
    curl -fsS -H "Host: stream.logs.${DOMAIN}" "http://127.0.0.1:${PORT}/"

  local token; token="$(cat "${DATA_DIR}/api_token")"
  local out_file="${WORK_DIR}/sse-out.log"
  : > "${out_file}"

  # SSE delivers events as they happen — start the stream first, then
  # poke the backend a few times to generate fresh log lines, then check
  # what arrived.
  curl -fsS -N --max-time 6 \
    -H "Authorization: Bearer ${token}" \
    "http://127.0.0.1:${PORT}/api/logs/stream?process=logs&id=stream" \
    >"${out_file}" 2>&1 &
  local sse_pid=$!
  sleep 1
  for _ in 1 2 3 4 5; do
    curl -fsS -H "Host: stream.logs.${DOMAIN}" \
      "http://127.0.0.1:${PORT}/" >/dev/null 2>&1 || true
    sleep 0.2
  done
  wait "${sse_pid}" 2>/dev/null || true

  if ! grep -q "id=stream" "${out_file}"; then
    sed 's/^/  | /' "${out_file}" >&2
    fail "expected SSE stream to deliver log lines containing 'id=stream'"
  fi

  ten stop logs:stream
  wait_for_not_in_ps "logs:stream"
}

# ----------------------------------------------------------------------------
# Scenario 14 — auth edge cases (no token, bad token, valid token)
# ----------------------------------------------------------------------------
test_auth_edges() {
  local code
  # No token → 401.
  code="$(curl -s -o /dev/null -w '%{http_code}' \
    "http://127.0.0.1:${PORT}/api/instances")"
  [[ "${code}" == "401" ]] \
    || fail "expected 401 for unauthenticated /api/instances, got ${code}"

  # Bad token → 401.
  code="$(curl -s -o /dev/null -w '%{http_code}' \
    -H "Authorization: Bearer not-a-real-token-deadbeef" \
    "http://127.0.0.1:${PORT}/api/instances")"
  [[ "${code}" == "401" ]] \
    || fail "expected 401 for bad token, got ${code}"

  # Valid token → 200.
  local token; token="$(cat "${DATA_DIR}/api_token")"
  code="$(curl -s -o /dev/null -w '%{http_code}' \
    -H "Authorization: Bearer ${token}" \
    "http://127.0.0.1:${PORT}/api/instances")"
  [[ "${code}" == "200" ]] \
    || fail "expected 200 for valid token, got ${code}"
}

# ----------------------------------------------------------------------------
# Scenario 15 — weighted reselect on dead backend
# ----------------------------------------------------------------------------
test_weighted_reselect() {
  # Spawn two weighted instances. Kill one. Issue weighted requests; all
  # of them should still succeed (proxy reselects past the dead one).
  ten spawn weighted:wr_a
  ten spawn weighted:wr_b
  wait_for_in_ps "weighted:wr_a"
  wait_for_in_ps "weighted:wr_b"
  wait_for "wr_a serving" 15 \
    curl -fsS -H "Host: wr_a.weighted.${DOMAIN}" "http://127.0.0.1:${PORT}/"
  wait_for "wr_b serving" 15 \
    curl -fsS -H "Host: wr_b.weighted.${DOMAIN}" "http://127.0.0.1:${PORT}/"

  # Pick the PID of wr_a and kill it.
  local resp; resp="$(proxy_curl "wr_a.weighted.${DOMAIN}" /)"
  local pid_a; pid_a="$(grep -oE 'pid=[0-9]+' <<<"${resp}" | cut -d= -f2)"
  log "killing weighted:wr_a pid ${pid_a}"
  kill -9 "${pid_a}" 2>/dev/null || true

  # Immediately fire weighted requests — every one should succeed thanks
  # to the proxy's reselect path. (Tenement will eventually respawn wr_a,
  # but we care about the request-time behavior here.)
  local fails=0
  for _ in $(seq 1 15); do
    if ! curl -fsS --max-time 5 \
        -H "Host: weighted.${DOMAIN}" "http://127.0.0.1:${PORT}/" \
        >/dev/null 2>&1; then
      fails=$((fails+1))
    fi
  done
  if (( fails > 0 )); then
    fail "${fails}/15 weighted requests failed after killing one backend"
  fi

  ten stop weighted:wr_a 2>/dev/null || true
  ten stop weighted:wr_b 2>/dev/null || true
  wait_for_not_in_ps "weighted:wr_a"
  wait_for_not_in_ps "weighted:wr_b"
}

# ----------------------------------------------------------------------------
# Run scenarios
# ----------------------------------------------------------------------------
scenario "basic"                test_basic
scenario "wake-on-request"      test_wake_on_request
scenario "weighted-multi"       test_weighted_multi_instance
scenario "weighted-reselect"    test_weighted_reselect
scenario "restart-on-crash"     test_restart_on_crash
scenario "idle-timeout"         test_idle_timeout
scenario "logs"                 test_logs
scenario "log-streaming"        test_log_streaming
scenario "cli-surface"          test_cli_surface
scenario "deploy-route"         test_deploy_route
scenario "auth-edges"           test_auth_edges
scenario "tenant-tokens"        test_tenant_tokens
scenario "unhealthy-detection"  test_unhealthy_detection
scenario "storage-persistence"  test_storage_persistence
scenario "fastapi-example"      test_fastapi_example

if (( ${#FAILED[@]} > 0 )); then
  exit 1
fi
log "ALL PASS"
