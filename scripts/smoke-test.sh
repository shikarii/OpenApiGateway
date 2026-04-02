#!/bin/bash
# Smoke test for the API Gateway single-node example.
# Validates routing through Envoy and the admin API.
#
# Usage:
#   bash scripts/smoke-test.sh
#
# Environment:
#   ENVOY_URL  -- override Envoy address  (default: http://localhost)
#   ADMIN_URL  -- override admin address   (default: http://localhost:9090)

set -e

ENVOY_URL="${ENVOY_URL:-http://localhost}"
ADMIN_URL="${ADMIN_URL:-http://localhost:9090}"
TOKEN_SCRIPT="$(dirname "$0")/gen-jwt-dev.py"

PASSED=0
FAILED=0
SKIPPED=0
TOTAL=0

pass() {
    PASSED=$((PASSED + 1))
    TOTAL=$((TOTAL + 1))
    echo "  PASS: $1"
}

fail() {
    FAILED=$((FAILED + 1))
    TOTAL=$((TOTAL + 1))
    echo "  FAIL: $1"
    if [ -n "${2:-}" ]; then echo "        $2"; fi
}

skip() {
    SKIPPED=$((SKIPPED + 1))
    TOTAL=$((TOTAL + 1))
    echo "  SKIP: $1"
}

echo "=== API Gateway Smoke Tests ==="
echo "Envoy URL: $ENVOY_URL"
echo "Admin URL: $ADMIN_URL"
echo ""

# ---- Envoy Routing ----
echo "--- Envoy Routing ---"

# Test 1: Public route (no auth)
RESPONSE=$(curl -s -w "\n%{http_code}" -X POST "$ENVOY_URL/public/echo" \
  -H "Content-Type: application/json" \
  -d '{"msg":"hello"}')
HTTP_CODE=$(echo "$RESPONSE" | tail -n1)

if [ "$HTTP_CODE" = "200" ]; then
    pass "Public route returns 200"
else
    fail "Public route returns 200" "got HTTP $HTTP_CODE"
fi

# Test 2: Unknown route returns 404
HTTP_CODE=$(curl -s -o /dev/null -w "%{http_code}" "$ENVOY_URL/nonexistent/path")
if [ "$HTTP_CODE" = "404" ]; then
    pass "Unknown route returns 404"
else
    fail "Unknown route returns 404" "got HTTP $HTTP_CODE"
fi
echo ""

# ---- Auth Tests (skipped -- ext_authz filter not yet wired in Envoy) ----
echo "--- Auth Tests ---"
skip "Protected route without JWT (needs ext_authz filter in Envoy)"
skip "Protected route with valid JWT (needs ext_authz filter in Envoy)"
echo ""

# ---- Admin API ----
echo "--- Admin API ---"

# Test 3: /healthz
RESPONSE=$(curl -s -w "\n%{http_code}" "$ADMIN_URL/healthz")
HTTP_CODE=$(echo "$RESPONSE" | tail -n1)

if [ "$HTTP_CODE" = "200" ]; then
    pass "/healthz returns 200"
else
    fail "/healthz returns 200" "got HTTP $HTTP_CODE"
fi

# Test 4: /readyz
BODY=$(curl -s "$ADMIN_URL/readyz")

config_loaded=$(echo "$BODY" | grep -o '"config_loaded":[^,}]*' | head -1 | sed 's/"config_loaded"://' | tr -d '" ')
redis_ok=$(echo "$BODY" | grep -o '"redis_ok":[^,}]*' | head -1 | sed 's/"redis_ok"://' | tr -d '" ')

if [ "$config_loaded" = "true" ] && [ "$redis_ok" = "true" ]; then
    pass "/readyz reports config_loaded=true, redis_ok=true"
else
    fail "/readyz reports healthy" "config_loaded=$config_loaded redis_ok=$redis_ok"
fi

# Test 5: /metrics
RESPONSE=$(curl -s -w "\n%{http_code}" "$ADMIN_URL/metrics")
HTTP_CODE=$(echo "$RESPONSE" | tail -n1)
BODY=$(echo "$RESPONSE" | sed '$d')

if [ "$HTTP_CODE" != "200" ]; then
    fail "/metrics returns 200" "got HTTP $HTTP_CODE"
elif echo "$BODY" | grep -q "# HELP\|# TYPE"; then
    pass "/metrics returns Prometheus text format"
else
    fail "/metrics returns Prometheus text format" "no HELP/TYPE lines found"
fi

# Test 6: /config/status
BODY=$(curl -s "$ADMIN_URL/config/status")

sha256=$(echo "$BODY" | grep -o '"active_config_sha256":"[^"]*"' | head -1 | sed 's/"active_config_sha256":"//' | tr -d '"')

if echo "$sha256" | grep -qE '^[0-9a-f]{64}$'; then
    pass "/config/status returns valid SHA256"
else
    fail "/config/status returns valid SHA256" "sha256=$sha256"
fi
echo ""

# ---- JWT Generation ----
echo "--- JWT Generation ---"

if command -v python3 &>/dev/null && python3 -c "import jwt" 2>/dev/null; then
    TOKEN=$(python3 "$TOKEN_SCRIPT" --sub test-user --scopes "api.read")
    if [ -n "$TOKEN" ] && [ ${#TOKEN} -gt 50 ]; then
        pass "gen-jwt-dev.py generates valid token (${#TOKEN} chars)"
    else
        fail "gen-jwt-dev.py generates valid token" "empty or short output"
    fi
else
    skip "gen-jwt-dev.py (python3 or PyJWT not available)"
fi
echo ""

# ---- Results ----
echo "=== Results ==="
echo "  Total: $TOTAL  Passed: $PASSED  Failed: $FAILED  Skipped: $SKIPPED"
echo ""

if [ "$FAILED" -gt 0 ]; then
    echo "FAILED -- $FAILED test(s) failed"
    exit 1
fi

echo "ALL TESTS PASSED ($PASSED passed, $SKIPPED skipped)"
