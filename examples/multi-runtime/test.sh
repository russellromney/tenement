#!/usr/bin/env bash
set -euo pipefail

# Multi-runtime tenement integration test
# Spawns 2 instances each of Python, Node, and Go servers,
# then tests auth, data isolation, and cross-tenant rejection.

TEN="${TEN:-ten}"
SERVER="http://localhost:9090"
DOMAIN="localhost:9090"
PASS=0
FAIL=0
TOTAL=0

green() { printf "\033[32m%s\033[0m\n" "$1"; }
red()   { printf "\033[31m%s\033[0m\n" "$1"; }
bold()  { printf "\033[1m%s\033[0m\n" "$1"; }

assert_eq() {
    TOTAL=$((TOTAL + 1))
    local desc="$1" expected="$2" actual="$3"
    if [ "$expected" = "$actual" ]; then
        green "  PASS: $desc"
        PASS=$((PASS + 1))
    else
        red "  FAIL: $desc"
        red "    expected: $expected"
        red "    actual:   $actual"
        FAIL=$((FAIL + 1))
    fi
}

assert_contains() {
    TOTAL=$((TOTAL + 1))
    local desc="$1" needle="$2" haystack="$3"
    if echo "$haystack" | grep -q "$needle"; then
        green "  PASS: $desc"
        PASS=$((PASS + 1))
    else
        red "  FAIL: $desc"
        red "    expected to contain: $needle"
        red "    actual: $haystack"
        FAIL=$((FAIL + 1))
    fi
}

http_code() { curl -s -o /dev/null -w "%{http_code}" "$@"; }
http_json()  { curl -s "$@"; }
json_field() { python3 -c "import sys,json; print(json.load(sys.stdin).get('$1',''))" <<< "$2"; }

# Token cache using flat variables (macOS bash 3 has no associative arrays)
get_token_for() {
    local svc="$1" tenant="$2"
    curl -s "http://$tenant.$svc.$DOMAIN/token" | python3 -c "import sys,json; print(json.load(sys.stdin)['token'])"
}

cleanup() {
    bold "Cleaning up..."
    pkill -f "ten serve.*9090" 2>/dev/null || true
    pkill -f "py_app.py" 2>/dev/null || true
    pkill -f "node_app.js" 2>/dev/null || true
    pkill -f "go_app.go" 2>/dev/null || true
    sleep 1
}
trap cleanup EXIT

# ------------------------------------------------------------------
bold "=== Multi-Runtime Tenement Test ==="
echo ""

# Clean previous state
rm -rf ./data

# Start tenement
bold "Starting tenement server..."
$TEN serve --port 9090 --domain localhost &>/dev/null &
sleep 2

# Generate admin token
bold "Generating admin token..."
$TEN token-gen --server "$SERVER" > /dev/null 2>&1

# ------------------------------------------------------------------
bold "Spawning instances..."

for svc in pyapi nodeapi goapi; do
    for tenant in alice bob; do
        echo "  $svc:$tenant"
        $TEN spawn "$svc:$tenant" --server "$SERVER" 2>&1 | head -1
    done
done

# Go takes longer to compile
bold "Waiting for all instances to start..."
sleep 10

# ------------------------------------------------------------------
bold "Listing instances..."
$TEN ps --server "$SERVER"
echo ""

# ------------------------------------------------------------------
bold "1. Health checks (no auth required)"
for svc in pyapi nodeapi goapi; do
    for tenant in alice bob; do
        resp=$(http_json "http://$tenant.$svc.$DOMAIN/health")
        status=$(json_field "status" "$resp")
        tenant_got=$(json_field "tenant" "$resp")
        assert_eq "$svc:$tenant health status" "ok" "$status"
        assert_eq "$svc:$tenant reports correct tenant" "$tenant" "$tenant_got"
    done
done
echo ""

# ------------------------------------------------------------------
bold "2. Each instance has its own token"

# Fetch all tokens
TOKEN_PYAPI_ALICE=$(get_token_for pyapi alice)
TOKEN_PYAPI_BOB=$(get_token_for pyapi bob)
TOKEN_NODEAPI_ALICE=$(get_token_for nodeapi alice)
TOKEN_NODEAPI_BOB=$(get_token_for nodeapi bob)
TOKEN_GOAPI_ALICE=$(get_token_for goapi alice)
TOKEN_GOAPI_BOB=$(get_token_for goapi bob)

for svc in pyapi nodeapi goapi; do
    for tenant in alice bob; do
        varname="TOKEN_$(echo "${svc}_${tenant}" | tr '[:lower:]' '[:upper:]')"
        token=$(eval echo "\$$varname")
        assert_contains "$svc:$tenant has a token" "[a-f0-9]" "$token"
    done
done

# Verify tokens differ across tenants of same service
TOTAL=$((TOTAL + 1))
if [ "$TOKEN_PYAPI_ALICE" != "$TOKEN_PYAPI_BOB" ]; then
    green "  PASS: pyapi alice != bob tokens"; PASS=$((PASS + 1))
else
    red "  FAIL: pyapi alice and bob have same token"; FAIL=$((FAIL + 1))
fi
TOTAL=$((TOTAL + 1))
if [ "$TOKEN_NODEAPI_ALICE" != "$TOKEN_NODEAPI_BOB" ]; then
    green "  PASS: nodeapi alice != bob tokens"; PASS=$((PASS + 1))
else
    red "  FAIL: nodeapi alice and bob have same token"; FAIL=$((FAIL + 1))
fi
TOTAL=$((TOTAL + 1))
if [ "$TOKEN_GOAPI_ALICE" != "$TOKEN_GOAPI_BOB" ]; then
    green "  PASS: goapi alice != bob tokens"; PASS=$((PASS + 1))
else
    red "  FAIL: goapi alice and bob have same token"; FAIL=$((FAIL + 1))
fi
echo ""

# ------------------------------------------------------------------
bold "3. Auth enforcement"
for svc in pyapi nodeapi goapi; do
    varname_alice="TOKEN_$(echo "${svc}_alice" | tr '[:lower:]' '[:upper:]')"
    token_alice=$(eval echo "\$$varname_alice")

    # No auth -> 401
    code=$(http_code "http://alice.$svc.$DOMAIN/notes")
    assert_eq "$svc no-auth -> 401" "401" "$code"

    # Wrong token -> 403
    code=$(http_code -H "Authorization: Bearer wrong" "http://alice.$svc.$DOMAIN/notes")
    assert_eq "$svc wrong-token -> 403" "403" "$code"

    # Correct token -> 200
    code=$(http_code -H "Authorization: Bearer $token_alice" "http://alice.$svc.$DOMAIN/notes")
    assert_eq "$svc correct-token -> 200" "200" "$code"

    # Alice's token on Bob -> 403
    code=$(http_code -H "Authorization: Bearer $token_alice" "http://bob.$svc.$DOMAIN/notes")
    assert_eq "$svc alice-token-on-bob -> 403" "403" "$code"
done
echo ""

# ------------------------------------------------------------------
bold "4. Create notes and verify data isolation"
for svc in pyapi nodeapi goapi; do
    for tenant in alice bob; do
        varname="TOKEN_$(echo "${svc}_${tenant}" | tr '[:lower:]' '[:upper:]')"
        token=$(eval echo "\$$varname")
        resp=$(http_json -X POST \
            -H "Authorization: Bearer $token" \
            -H "Content-Type: application/json" \
            -d "{\"text\":\"Hello from $tenant via $svc\"}" \
            "http://$tenant.$svc.$DOMAIN/notes")
        assert_contains "$svc:$tenant create note" "$tenant" "$resp"
    done
done

# Verify each tenant only sees their own note
for svc in pyapi nodeapi goapi; do
    for tenant in alice bob; do
        varname="TOKEN_$(echo "${svc}_${tenant}" | tr '[:lower:]' '[:upper:]')"
        token=$(eval echo "\$$varname")
        resp=$(http_json -H "Authorization: Bearer $token" "http://$tenant.$svc.$DOMAIN/notes")
        notes_str=$(python3 -c "import sys,json; notes=json.load(sys.stdin)['notes']; print(len(notes), notes[0]['text'] if notes else '')" <<< "$resp")
        count=$(echo "$notes_str" | cut -d' ' -f1)
        assert_eq "$svc:$tenant has exactly 1 note" "1" "$count"
        assert_contains "$svc:$tenant note is their own" "$tenant" "$notes_str"
    done
done
echo ""

# ------------------------------------------------------------------
bold "5. Cross-service isolation (pyapi alice can't hit nodeapi alice)"
code=$(http_code -H "Authorization: Bearer $TOKEN_PYAPI_ALICE" "http://alice.nodeapi.$DOMAIN/notes")
assert_eq "pyapi token rejected by nodeapi" "403" "$code"

code=$(http_code -H "Authorization: Bearer $TOKEN_NODEAPI_ALICE" "http://alice.goapi.$DOMAIN/notes")
assert_eq "nodeapi token rejected by goapi" "403" "$code"
echo ""

# ------------------------------------------------------------------
bold "6. Runtime identification"
for svc in pyapi nodeapi goapi; do
    resp=$(http_json "http://alice.$svc.$DOMAIN/health")
    runtime=$(json_field "runtime" "$resp")
    case "$svc" in
        pyapi)   expected="python" ;;
        nodeapi) expected="node" ;;
        goapi)   expected="go" ;;
    esac
    assert_eq "$svc reports runtime=$expected" "$expected" "$runtime"
done
echo ""

# ------------------------------------------------------------------
bold "=== Results ==="
echo ""
if [ $FAIL -eq 0 ]; then
    green "ALL $TOTAL TESTS PASSED"
else
    red "$FAIL/$TOTAL FAILED"
    green "$PASS/$TOTAL passed"
fi
echo ""

exit $FAIL
