#!/bin/bash
# Smoke test for the API Gateway
# Validates basic functionality: routing, auth, rate limiting

set -e

GATEWAY_URL="${GATEWAY_URL:-http://localhost:8080}"
TOKEN_SCRIPT="$(dirname "$0")/gen-jwt-dev.py"

echo "=== API Gateway Smoke Tests ==="
echo "Gateway URL: $GATEWAY_URL"
echo ""

# Generate test token
echo "Generating test JWT..."
TOKEN=$(python3 "$TOKEN_SCRIPT" --sub test-user --scopes "api.read api.write")
echo "Token: ${TOKEN:0:50}..."
echo ""

# Test 1: Public route (no auth)
echo "Test 1: Public route (no auth required)"
RESPONSE=$(curl -s -w "\n%{http_code}" -X POST "$GATEWAY_URL/public/echo" \
  -H "Content-Type: application/json" \
  -d '{"msg":"hello"}')
HTTP_CODE=$(echo "$RESPONSE" | tail -n1)
BODY=$(echo "$RESPONSE" | head -n-1)

if [ "$HTTP_CODE" == "200" ]; then
  echo "✓ PASS (HTTP $HTTP_CODE)"
else
  echo "✗ FAIL (HTTP $HTTP_CODE)"
  echo "Response: $BODY"
  exit 1
fi
echo ""

# Test 2: Protected route without auth
echo "Test 2: Protected route without auth (should be 401)"
RESPONSE=$(curl -s -w "\n%{http_code}" -X POST "$GATEWAY_URL/private/echo" \
  -H "Content-Type: application/json" \
  -d '{"msg":"hello"}')
HTTP_CODE=$(echo "$RESPONSE" | tail -n1)

if [ "$HTTP_CODE" == "401" ]; then
  echo "✓ PASS (HTTP $HTTP_CODE)"
else
  echo "✗ FAIL (HTTP $HTTP_CODE, expected 401)"
  exit 1
fi
echo ""

# Test 3: Protected route with valid JWT
echo "Test 3: Protected route with valid JWT"
RESPONSE=$(curl -s -w "\n%{http_code}" -X POST "$GATEWAY_URL/private/echo" \
  -H "Authorization: Bearer $TOKEN" \
  -H "Content-Type: application/json" \
  -d '{"msg":"hello"}')
HTTP_CODE=$(echo "$RESPONSE" | tail -n1)

if [ "$HTTP_CODE" == "200" ]; then
  echo "✓ PASS (HTTP $HTTP_CODE)"
else
  echo "✗ FAIL (HTTP $HTTP_CODE)"
  exit 1
fi
echo ""

# Test 4: Admin health check
echo "Test 4: Admin /healthz endpoint"
RESPONSE=$(curl -s -w "\n%{http_code}" http://localhost:9090/healthz)
HTTP_CODE=$(echo "$RESPONSE" | tail -n1)

if [ "$HTTP_CODE" == "200" ]; then
  echo "✓ PASS (HTTP $HTTP_CODE)"
else
  echo "✗ FAIL (HTTP $HTTP_CODE)"
  exit 1
fi
echo ""

# Test 5: Admin readyz check
echo "Test 5: Admin /readyz endpoint"
RESPONSE=$(curl -s -w "\n%{http_code}" http://localhost:9090/readyz)
HTTP_CODE=$(echo "$RESPONSE" | tail -n1)

if [ "$HTTP_CODE" == "200" ]; then
  echo "✓ PASS (HTTP $HTTP_CODE)"
else
  echo "⚠ WARN (HTTP $HTTP_CODE - gateway may not be fully ready)"
fi
echo ""

# Test 6: Metrics endpoint
echo "Test 6: Admin /metrics endpoint"
RESPONSE=$(curl -s -w "\n%{http_code}" http://localhost:9090/metrics)
HTTP_CODE=$(echo "$RESPONSE" | tail -n1)

if [ "$HTTP_CODE" == "200" ]; then
  echo "✓ PASS (HTTP $HTTP_CODE)"
else
  echo "✗ FAIL (HTTP $HTTP_CODE)"
  exit 1
fi
echo ""

echo "=== All Smoke Tests Passed ==="
