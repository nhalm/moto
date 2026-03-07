#!/usr/bin/env bash
# Smoke tests for moto-ai-proxy deployed in k3d cluster.
# Verifies passthrough auth/allowlist, unified endpoint routing,
# health endpoints, and missing provider handling.
#
# Requires: k3d cluster running with ai-proxy, keybox, and moto-club deployed.
# At least one AI provider key seeded in keybox (ai-proxy/anthropic).
#
# Usage:
#   AI_PROXY_URL=http://localhost:18091 \
#   AI_PROXY_HEALTH_URL=http://localhost:18092 \
#   KEYBOX_URL=http://localhost:18090 \
#   CLUB_URL=http://localhost:18093 \
#   ./infra/smoke-test-ai-proxy.sh

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

AI_PROXY_URL="${AI_PROXY_URL:-http://localhost:18091}"
AI_PROXY_HEALTH_URL="${AI_PROXY_HEALTH_URL:-http://localhost:18092}"
KEYBOX_URL="${KEYBOX_URL:-http://localhost:18090}"
CLUB_URL="${CLUB_URL:-http://localhost:18093}"
SERVICE_TOKEN_FILE="${SERVICE_TOKEN_FILE:-$REPO_ROOT/.dev/k8s-secrets/service-token}"

if [ ! -f "$SERVICE_TOKEN_FILE" ]; then
    echo "ERROR: Service token not found at $SERVICE_TOKEN_FILE"
    echo "Run 'make deploy-secrets' first."
    exit 1
fi

SERVICE_TOKEN=$(cat "$SERVICE_TOKEN_FILE")

GARAGE_NAME="smoke-ai-$$-$(date +%s)"
FAILED=0
PASSED=0
SKIPPED=0
SVID_TOKEN=""
GARAGE_CREATED=false

cleanup() {
    echo ""
    echo "=== Cleanup ==="
    if [ "$GARAGE_CREATED" = true ]; then
        echo "  Deleting garage: $GARAGE_NAME"
        curl -s -o /dev/null -X DELETE "$CLUB_URL/api/v1/garages/$GARAGE_NAME" \
            -H "Authorization: Bearer smoke-test" 2>/dev/null || true
    fi
    echo "  Done."
}

trap cleanup EXIT

# Assert an HTTP request returns the expected status code
assert_status() {
    local name="$1"
    local expected="$2"
    local method="$3"
    local url="$4"
    shift 4

    printf "  %-65s" "$name..."
    local status
    status=$(curl -s -o /dev/null -w "%{http_code}" --max-time 30 -X "$method" "$url" "$@" 2>/dev/null)

    if [ "$status" = "$expected" ]; then
        echo "OK ($status)"
        PASSED=$((PASSED + 1))
    else
        echo "FAIL (expected $expected, got $status)"
        FAILED=$((FAILED + 1))
    fi
}

# Assert request passes auth (response is not 401/403 from the proxy).
# Used for tests where we care that auth succeeded but the upstream
# response code may vary (e.g., 200 from provider, 400 on bad body).
assert_auth_passes() {
    local name="$1"
    local method="$2"
    local url="$3"
    shift 3

    printf "  %-65s" "$name..."
    local status
    status=$(curl -s -o /dev/null -w "%{http_code}" --max-time 30 -X "$method" "$url" "$@" 2>/dev/null)

    if [ "$status" != "401" ] && [ "$status" != "403" ] && [ "$status" != "000" ]; then
        echo "OK ($status)"
        PASSED=$((PASSED + 1))
    else
        echo "FAIL (got $status)"
        FAILED=$((FAILED + 1))
    fi
}

skip_test() {
    local name="$1"
    printf "  %-65s" "$name..."
    echo "SKIP (no SVID)"
    SKIPPED=$((SKIPPED + 1))
}

echo "=== Moto AI Proxy Smoke Tests ==="
echo "Proxy URL: $AI_PROXY_URL"
echo "Health URL: $AI_PROXY_HEALTH_URL"
echo ""

# --- Health Endpoints ---
echo "=== Health Endpoints ==="

assert_status "GET /health/live (200)" "200" \
    GET "$AI_PROXY_HEALTH_URL/health/live"

assert_status "GET /health/ready (200)" "200" \
    GET "$AI_PROXY_HEALTH_URL/health/ready"

echo ""

# --- Passthrough: No-auth tests (run without SVID) ---
echo "=== Passthrough (no-auth) ==="

# Without auth → 401
assert_status "POST /passthrough/anthropic/v1/messages without auth (401)" "401" \
    POST "$AI_PROXY_URL/passthrough/anthropic/v1/messages" \
    -H "Content-Type: application/json" \
    -d '{"model":"claude-sonnet-4-20250514","max_tokens":1,"messages":[{"role":"user","content":"hi"}]}'

# Path allowlist enforcement — checked before auth
assert_status "POST /passthrough/anthropic/admin/billing (403)" "403" \
    POST "$AI_PROXY_URL/passthrough/anthropic/admin/billing" \
    -H "Content-Type: application/json"

echo ""

# --- Unified Endpoint: No-auth test ---
echo "=== Unified Endpoint (no-auth) ==="

assert_status "POST /v1/chat/completions without auth (401)" "401" \
    POST "$AI_PROXY_URL/v1/chat/completions" \
    -H "Content-Type: application/json" \
    -d '{"model":"claude-sonnet-4-20250514","max_tokens":1,"messages":[{"role":"user","content":"hi"}]}'

echo ""

# --- Setup: Create test garage and obtain SVID ---
echo "=== Setup: Garage + SVID ==="

echo "  Creating test garage: $GARAGE_NAME"
CREATE_RESPONSE=$(curl -s -w "\n%{http_code}" -X POST "$CLUB_URL/api/v1/garages" \
    -H "Authorization: Bearer smoke-test" \
    -H "Content-Type: application/json" \
    -d "{\"name\":\"$GARAGE_NAME\",\"ttl_seconds\":300}" 2>/dev/null)
CREATE_STATUS=$(echo "$CREATE_RESPONSE" | tail -1)

if [ "$CREATE_STATUS" = "201" ] || [ "$CREATE_STATUS" = "200" ]; then
    GARAGE_CREATED=true
    echo "  Garage created (status $CREATE_STATUS)"

    # Wait for garage to become ready
    echo "  Waiting for garage to become ready..."
    READY=false
    for i in $(seq 1 90); do
        GARAGE_STATUS=$(curl -s "$CLUB_URL/api/v1/garages/$GARAGE_NAME" \
            -H "Authorization: Bearer smoke-test" 2>/dev/null | jq -r '.status' 2>/dev/null || echo "unknown")
        if [ "$GARAGE_STATUS" = "ready" ]; then
            READY=true
            echo "  Garage ready after ${i}s"
            break
        fi
        sleep 1
    done

    if [ "$READY" = true ]; then
        echo "  Obtaining SVID token..."
        SVID_RESPONSE=$(curl -s -X POST "$KEYBOX_URL/auth/token" \
            -H "Content-Type: application/json" \
            -d "{\"principal_type\":\"garage\",\"principal_id\":\"$GARAGE_NAME\"}")
        SVID_TOKEN=$(echo "$SVID_RESPONSE" | jq -r '.token')

        if [ -z "$SVID_TOKEN" ] || [ "$SVID_TOKEN" = "null" ]; then
            echo "  WARNING: Failed to obtain SVID token"
            echo "  Response: $SVID_RESPONSE"
            SVID_TOKEN=""
        else
            echo "  SVID token obtained."
        fi
    else
        echo "  WARNING: Garage did not become ready within 90s (status: $GARAGE_STATUS)"
    fi
else
    echo "  WARNING: Failed to create garage (status $CREATE_STATUS)"
    echo "  Response: $(echo "$CREATE_RESPONSE" | sed '$d')"
fi

echo ""

# --- Passthrough: Auth tests (require SVID) ---
echo "=== Passthrough (with auth) ==="

if [ -n "$SVID_TOKEN" ]; then
    # With valid SVID → 200 (or upstream provider response)
    assert_auth_passes "POST /passthrough/anthropic/v1/messages with SVID (200)" \
        POST "$AI_PROXY_URL/passthrough/anthropic/v1/messages" \
        -H "x-api-key: $SVID_TOKEN" \
        -H "Content-Type: application/json" \
        -H "anthropic-version: 2023-06-01" \
        -d '{"model":"claude-sonnet-4-20250514","max_tokens":1,"messages":[{"role":"user","content":"hi"}]}'
else
    skip_test "POST /passthrough/anthropic/v1/messages with SVID (200)"
fi

echo ""

# --- Unified Endpoint: Auth tests ---
echo "=== Unified Endpoint (with auth) ==="

if [ -n "$SVID_TOKEN" ]; then
    # With valid model → routes to Anthropic, returns 200
    assert_auth_passes "POST /v1/chat/completions claude-sonnet (200)" \
        POST "$AI_PROXY_URL/v1/chat/completions" \
        -H "Authorization: Bearer $SVID_TOKEN" \
        -H "Content-Type: application/json" \
        -d '{"model":"claude-sonnet-4-20250514","max_tokens":1,"messages":[{"role":"user","content":"hi"}]}'

    # Unknown model prefix → 400
    assert_status "POST /v1/chat/completions unknown model (400)" "400" \
        POST "$AI_PROXY_URL/v1/chat/completions" \
        -H "Authorization: Bearer $SVID_TOKEN" \
        -H "Content-Type: application/json" \
        -d '{"model":"unknown-model-xyz","max_tokens":1,"messages":[{"role":"user","content":"hi"}]}'
else
    skip_test "POST /v1/chat/completions claude-sonnet (200)"
    skip_test "POST /v1/chat/completions unknown model (400)"
fi

echo ""

# --- Missing Provider ---
echo "=== Missing Provider ==="

if [ -n "$SVID_TOKEN" ]; then
    # OpenAI key not seeded → 503
    assert_status "POST /v1/chat/completions gpt-4o (503)" "503" \
        POST "$AI_PROXY_URL/v1/chat/completions" \
        -H "Authorization: Bearer $SVID_TOKEN" \
        -H "Content-Type: application/json" \
        -d '{"model":"gpt-4o","max_tokens":1,"messages":[{"role":"user","content":"hi"}]}'
else
    skip_test "POST /v1/chat/completions gpt-4o (503)"
fi

echo ""

# --- Results ---
echo "=== Results ==="
echo "Passed: $PASSED"
echo "Failed: $FAILED"
echo "Skipped: $SKIPPED"

if [ "$FAILED" -eq 0 ] && [ "$SKIPPED" -eq 0 ]; then
    echo "All ai-proxy smoke tests passed!"
    exit 0
elif [ "$FAILED" -eq 0 ]; then
    echo "Tests passed but $SKIPPED test(s) skipped (garage setup failed)"
    exit 1
else
    echo "$FAILED test(s) failed"
    exit 1
fi
