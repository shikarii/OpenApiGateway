#!/usr/bin/env bash
# End-to-end integration tests for OpenApiGateway.
#
# Spins up the full Docker stack (Redis, echo-backend, fake-jwks,
# gateway-manager, Envoy), runs HTTP tests, then tears everything down.
#
# Usage:
#   bash tests/e2e/run.sh
#
# Environment:
#   ENVOY_URL   -- override Envoy address   (default: http://localhost:10080)
#   ADMIN_URL   -- override admin address    (default: http://localhost:19090)
#   SKIP_BUILD  -- set to 1 to skip docker build step
#   KEEP_STACK  -- set to 1 to leave containers running after tests

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
COMPOSE_FILE="$SCRIPT_DIR/docker-compose.yml"
COMPOSE_PROJECT="e2e-gateway"

ENVOY_URL="${ENVOY_URL:-http://localhost:10080}"
ADMIN_URL="${ADMIN_URL:-http://localhost:19090}"

PASSED=0
FAILED=0
SKIPPED=0
TOTAL=0

# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------

cleanup() {
    if [ "${KEEP_STACK:-0}" = "1" ]; then
        log "KEEP_STACK=1, leaving containers running"
        return
    fi
    log "Tearing down stack..."
    docker compose -p "$COMPOSE_PROJECT" -f "$COMPOSE_FILE" down -v --remove-orphans 2>/dev/null || true
}
trap cleanup EXIT

log() { echo "--- $*"; }

dump_stack_diagnostics() {
    log "Docker compose status:"
    docker compose -p "$COMPOSE_PROJECT" -f "$COMPOSE_FILE" ps || true
    log "Relevant container logs:"
    docker compose -p "$COMPOSE_PROJECT" -f "$COMPOSE_FILE" logs fake-jwks gateway-manager envoy || true
}

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

# Poll a URL until it returns HTTP 200, or timeout.
# Usage: wait_for_url <url> <max_seconds>
wait_for_url() {
    local url="$1"
    local max_wait="${2:-60}"
    local elapsed=0
    while [ "$elapsed" -lt "$max_wait" ]; do
        if curl -sf -o /dev/null "$url" 2>/dev/null; then
            return 0
        fi
        sleep 2
        elapsed=$((elapsed + 2))
    done
    echo "Timed out waiting for $url after ${max_wait}s"
    return 1
}

# Extract a JSON field value (simple grep-based, no jq dependency).
# Usage: json_field <json_string> <field_name>
# Returns the value (unquoted for strings, raw for booleans/numbers).
json_field() {
    local json="$1"
    local field="$2"
    echo "$json" | grep -o "\"$field\":[^,}]*" | head -1 | sed "s/\"$field\"://" | tr -d '" '
}

# ---------------------------------------------------------------------------
# Stack lifecycle
# ---------------------------------------------------------------------------

start_stack() {
    log "Building and starting Docker stack..."
    cd "$SCRIPT_DIR"

    if [ "${SKIP_BUILD:-0}" != "1" ]; then
        docker compose -p "$COMPOSE_PROJECT" -f "$COMPOSE_FILE" build --quiet 2>&1
    fi

    if ! docker compose -p "$COMPOSE_PROJECT" -f "$COMPOSE_FILE" up -d; then
        dump_stack_diagnostics
        exit 1
    fi

    log "Waiting for admin API to be healthy..."
    if ! wait_for_url "$ADMIN_URL/healthz" 90; then
        log "Admin API failed to start. Container logs:"
        dump_stack_diagnostics
        exit 1
    fi

    log "Waiting for Envoy to accept traffic..."
    if ! wait_for_url "$ENVOY_URL/public/" 45; then
        log "Envoy failed to start. Container logs:"
        dump_stack_diagnostics
        exit 1
    fi

    log "All services healthy"
}

# ---------------------------------------------------------------------------
# Test cases -- Envoy Routing
# ---------------------------------------------------------------------------

test_public_route_returns_200() {
    local response http_code
    response=$(curl -s -w "\n%{http_code}" -X POST "$ENVOY_URL/public/echo" \
        -H "Content-Type: application/json" -d '{"msg":"hello"}')
    http_code=$(echo "$response" | tail -n1)

    if [ "$http_code" = "200" ]; then
        pass "Public route returns 200"
    else
        fail "Public route returns 200" "got HTTP $http_code"
    fi
}

test_unknown_route_returns_404() {
    local http_code
    http_code=$(curl -s -o /dev/null -w "%{http_code}" "$ENVOY_URL/nonexistent/path")

    if [ "$http_code" = "404" ]; then
        pass "Unknown route returns 404"
    else
        fail "Unknown route returns 404" "got HTTP $http_code"
    fi
}

# ---------------------------------------------------------------------------
# Test cases -- Admin API
# ---------------------------------------------------------------------------

test_healthz_returns_200() {
    local response http_code body
    response=$(curl -s -w "\n%{http_code}" "$ADMIN_URL/healthz")
    http_code=$(echo "$response" | tail -n1)
    body=$(echo "$response" | head -n1)

    if [ "$http_code" != "200" ]; then
        fail "/healthz returns 200" "got HTTP $http_code"
        return
    fi

    local ok_val
    ok_val=$(json_field "$body" "ok")
    if [ "$ok_val" = "true" ]; then
        pass "/healthz returns 200 with ok=true"
    else
        fail "/healthz returns 200 with ok=true" "ok=$ok_val"
    fi
}

test_readyz_returns_healthy() {
    local body
    body=$(curl -s "$ADMIN_URL/readyz")

    local config_loaded redis_ok
    config_loaded=$(json_field "$body" "config_loaded")
    redis_ok=$(json_field "$body" "redis_ok")

    if [ "$config_loaded" = "true" ] && [ "$redis_ok" = "true" ]; then
        pass "/readyz reports config_loaded=true, redis_ok=true"
    else
        fail "/readyz reports healthy" "config_loaded=$config_loaded redis_ok=$redis_ok"
    fi
}

test_config_status_valid_json() {
    local body
    body=$(curl -s "$ADMIN_URL/config/status")

    local version sha256 reload_result
    version=$(json_field "$body" "active_config_version")
    sha256=$(json_field "$body" "active_config_sha256")
    reload_result=$(json_field "$body" "last_reload_result")

    if [ -z "$version" ] || [ -z "$sha256" ]; then
        fail "/config/status returns valid JSON" "version=$version sha256=$sha256"
        return
    fi

    # SHA256 should be 64 hex characters.
    if echo "$sha256" | grep -qE '^[0-9a-f]{64}$'; then
        pass "/config/status returns version=$version, valid SHA256, result=$reload_result"
    else
        fail "/config/status SHA256 format" "sha256=$sha256"
    fi
}

test_config_reload_succeeds() {
    local response http_code body
    response=$(curl -s -w "\n%{http_code}" -X POST "$ADMIN_URL/config/reload")
    http_code=$(echo "$response" | tail -n1)
    body=$(echo "$response" | head -n1)

    if [ "$http_code" != "200" ]; then
        fail "/config/reload returns 200" "got HTTP $http_code body=$body"
        return
    fi

    local ok_val
    ok_val=$(json_field "$body" "ok")
    if [ "$ok_val" = "true" ]; then
        pass "/config/reload succeeds with ok=true"
    else
        fail "/config/reload succeeds with ok=true" "ok=$ok_val body=$body"
    fi
}

test_metrics_returns_prometheus() {
    local response http_code body
    response=$(curl -s -w "\n%{http_code}" "$ADMIN_URL/metrics")
    http_code=$(echo "$response" | tail -n1)
    body=$(echo "$response" | sed '$d')

    if [ "$http_code" != "200" ]; then
        fail "/metrics returns 200" "got HTTP $http_code"
        return
    fi

    # Prometheus text format should contain at least one HELP or TYPE line.
    if echo "$body" | grep -q "# HELP\|# TYPE"; then
        pass "/metrics returns Prometheus text format"
    else
        fail "/metrics returns Prometheus text format" "no HELP/TYPE lines found"
    fi
}

# ---------------------------------------------------------------------------
# Test cases -- Readiness degradation
# ---------------------------------------------------------------------------

test_readiness_degrades_when_redis_stops() {
    log "Stopping Redis container..."
    docker stop e2e-redis >/dev/null 2>&1

    # Give the gateway-manager time to detect the dead connection.
    sleep 3

    local body redis_ok
    body=$(curl -s "$ADMIN_URL/readyz")
    redis_ok=$(json_field "$body" "redis_ok")

    if [ "$redis_ok" = "false" ]; then
        pass "/readyz shows redis_ok=false after Redis stopped"
    else
        fail "/readyz shows redis_ok=false after Redis stopped" "redis_ok=$redis_ok"
    fi
}

test_readiness_recovers_when_redis_restarts() {
    log "Restarting Redis container..."
    docker start e2e-redis >/dev/null 2>&1

    # Poll for recovery (up to 15 seconds).
    local elapsed=0
    local redis_ok="false"
    while [ "$elapsed" -lt 15 ]; do
        sleep 2
        elapsed=$((elapsed + 2))
        local body
        body=$(curl -s "$ADMIN_URL/readyz")
        redis_ok=$(json_field "$body" "redis_ok")
        if [ "$redis_ok" = "true" ]; then
            break
        fi
    done

    if [ "$redis_ok" = "true" ]; then
        pass "/readyz shows redis_ok=true after Redis restarted"
    else
        fail "/readyz shows redis_ok=true after Redis restarted" "redis_ok=$redis_ok after ${elapsed}s"
    fi
}

# ---------------------------------------------------------------------------
# Placeholder tests -- blocked on ext_authz / ratelimit filter wiring
# ---------------------------------------------------------------------------

test_protected_route_rejects_without_jwt() {
    skip "Protected route without JWT -- needs ext_authz filter in Envoy"
}

test_protected_route_accepts_with_valid_jwt() {
    skip "Protected route with valid JWT -- needs ext_authz filter in Envoy"
}

test_rate_limiting_returns_429() {
    skip "Rate limiting 429 -- needs ratelimit filter in Envoy"
}

# ---------------------------------------------------------------------------
# Main
# ---------------------------------------------------------------------------

main() {
    log "OpenApiGateway E2E Integration Tests"
    echo ""

    start_stack
    echo ""

    log "=== Envoy Routing ==="
    test_public_route_returns_200
    test_unknown_route_returns_404
    echo ""

    log "=== Admin API ==="
    test_healthz_returns_200
    test_readyz_returns_healthy
    test_config_status_valid_json
    test_config_reload_succeeds
    test_metrics_returns_prometheus
    echo ""

    log "=== Readiness Degradation ==="
    test_readiness_degrades_when_redis_stops
    test_readiness_recovers_when_redis_restarts
    echo ""

    log "=== Placeholder Tests ==="
    test_protected_route_rejects_without_jwt
    test_protected_route_accepts_with_valid_jwt
    test_rate_limiting_returns_429
    echo ""

    log "=== Results ==="
    echo "  Total: $TOTAL  Passed: $PASSED  Failed: $FAILED  Skipped: $SKIPPED"
    echo ""

    if [ "$FAILED" -gt 0 ]; then
        log "FAILED -- $FAILED test(s) failed"
        exit 1
    fi

    log "ALL TESTS PASSED ($PASSED passed, $SKIPPED skipped)"
}

main "$@"
