#!/bin/bash
# Multi-tenant demo: 5 tenants, each with their own notes database.
#
# Prerequisites:
#   1. Build tenement: cargo build --release
#   2. Add to PATH: export PATH="$PWD/target/release:$PATH"
#   3. cd examples/multi-tenant
#   4. ten init  (or use the provided tenement.toml)
#   5. ten token-gen
#   6. ten serve --port 8080 --domain localhost &
#   7. ./demo.sh
#
# What this demonstrates:
#   - Each tenant gets their own process and SQLite database
#   - Subdomain routing: alice.notes.localhost:8080
#   - Data isolation: alice's notes are not visible to bob
#   - Scale-to-zero: idle tenants are stopped automatically
#   - Wake-on-request: sleeping tenants wake on first request

set -e

SERVER="http://localhost:8080"
TOKEN=$(cat /var/lib/tenement/api_token 2>/dev/null || echo "")

if [ -z "$TOKEN" ]; then
    echo "No API token found. Run 'ten token-gen' first."
    exit 1
fi

AUTH="Authorization: Bearer $TOKEN"
TENANTS=("alice" "bob" "charlie" "diana" "eve")

echo "=== Multi-Tenant Demo ==="
echo ""

# Spawn tenants
echo "1. Spawning ${#TENANTS[@]} tenants..."
for tenant in "${TENANTS[@]}"; do
    curl -s -X POST "$SERVER/api/instances/spawn" \
        -H "$AUTH" -H "Content-Type: application/json" \
        -d "{\"process\":\"notes\",\"id\":\"$tenant\"}" > /dev/null
    echo "   Spawned notes:$tenant"
done

sleep 1

# List running instances
echo ""
echo "2. Running instances:"
curl -s "$SERVER/api/instances" -H "$AUTH" | python3 -m json.tool 2>/dev/null || \
    curl -s "$SERVER/api/instances" -H "$AUTH"
echo ""

# Add notes for each tenant
echo "3. Adding notes (each tenant has isolated data)..."
for tenant in "${TENANTS[@]}"; do
    curl -s -X POST "http://$tenant.notes.localhost:8080/notes" \
        -H "Content-Type: application/json" \
        -d "{\"text\":\"Hello from $tenant!\"}" > /dev/null
    echo "   $tenant: added a note"
done

# Show data isolation
echo ""
echo "4. Data isolation (alice's notes):"
curl -s "http://alice.notes.localhost:8080/notes" | python3 -m json.tool 2>/dev/null || \
    curl -s "http://alice.notes.localhost:8080/notes"
echo ""

echo "   bob's notes:"
curl -s "http://bob.notes.localhost:8080/notes" | python3 -m json.tool 2>/dev/null || \
    curl -s "http://bob.notes.localhost:8080/notes"
echo ""

# Show metrics
echo "5. Prometheus metrics:"
curl -s "$SERVER/metrics" | grep -E "^tenement_(requests_total|instances_up)" | head -10
echo ""

echo "=== Demo Complete ==="
echo ""
echo "Try it yourself:"
echo "  curl http://alice.notes.localhost:8080/notes"
echo "  curl -X POST http://alice.notes.localhost:8080/notes -H 'Content-Type: application/json' -d '{\"text\":\"my note\"}'"
echo "  ten ps"
echo "  ten logs notes:alice"
