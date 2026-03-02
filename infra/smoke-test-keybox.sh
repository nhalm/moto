#!/usr/bin/env bash
# Smoke tests for moto-keybox deployed in k3d cluster.
# Verifies auth matrix enforcement and DEK rotation against a live keybox instance.
#
# Requires: k3d cluster running with keybox deployed.
# Service token read from .dev/k8s-secrets/service-token.
#
# Usage:
#   KEYBOX_URL=http://localhost:18090 ./infra/smoke-test-keybox.sh

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

KEYBOX_URL="${KEYBOX_URL:-http://localhost:18090}"
SERVICE_TOKEN_FILE="${SERVICE_TOKEN_FILE:-$REPO_ROOT/.dev/k8s-secrets/service-token}"

if [ ! -f "$SERVICE_TOKEN_FILE" ]; then
    echo "ERROR: Service token not found at $SERVICE_TOKEN_FILE"
    echo "Run 'make deploy-secrets' first."
    exit 1
fi

SERVICE_TOKEN=$(cat "$SERVICE_TOKEN_FILE")

# Unique test secret name to avoid collisions
TEST_SECRET="smoke-test-$$-$(date +%s)"

FAILED=0
PASSED=0

# Track secrets for cleanup
CREATED_SECRETS=()

cleanup() {
    echo ""
    echo "=== Cleanup ==="
    for secret in "${CREATED_SECRETS[@]}"; do
        echo "  Deleting: $secret"
        curl -s -o /dev/null -X DELETE "$KEYBOX_URL/secrets/global/$secret" \
            -H "Authorization: Bearer $SERVICE_TOKEN" 2>/dev/null || true
    done
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

    printf "  %-60s" "$name..."
    local status
    status=$(curl -s -o /dev/null -w "%{http_code}" -X "$method" "$url" "$@" 2>/dev/null)

    if [ "$status" = "$expected" ]; then
        echo "OK ($status)"
        PASSED=$((PASSED + 1))
    else
        echo "FAIL (expected $expected, got $status)"
        FAILED=$((FAILED + 1))
    fi
}

echo "=== Moto Keybox Smoke Tests ==="
echo "URL: $KEYBOX_URL"
echo ""

# --- Obtain SVID token for testing forbidden paths ---
echo "Obtaining SVID token..."
SVID_RESPONSE=$(curl -s -X POST "$KEYBOX_URL/auth/token" \
    -H "Content-Type: application/json" \
    -d '{"principal_type":"garage","principal_id":"smoke-test-garage"}')
SVID_TOKEN=$(echo "$SVID_RESPONSE" | jq -r '.token')

if [ -z "$SVID_TOKEN" ] || [ "$SVID_TOKEN" = "null" ]; then
    echo "ERROR: Failed to obtain SVID token"
    echo "Response: $SVID_RESPONSE"
    exit 1
fi
echo "SVID token obtained."
echo ""

# --- Auth Matrix Enforcement ---
echo "=== Auth Matrix Enforcement ==="

# POST /secrets/ with service token succeeds (200)
ENCODED_VALUE=$(echo -n 'smoke-test-value' | base64)
assert_status "POST /secrets/ with service token (200)" "200" \
    POST "$KEYBOX_URL/secrets/global/$TEST_SECRET" \
    -H "Authorization: Bearer $SERVICE_TOKEN" \
    -H "Content-Type: application/json" \
    -d "{\"value\":\"$ENCODED_VALUE\"}"
CREATED_SECRETS+=("$TEST_SECRET")

# POST /secrets/ with SVID token returns 403 FORBIDDEN
assert_status "POST /secrets/ with SVID token (403)" "403" \
    POST "$KEYBOX_URL/secrets/global/${TEST_SECRET}-svid" \
    -H "Authorization: Bearer $SVID_TOKEN" \
    -H "Content-Type: application/json" \
    -d "{\"value\":\"$ENCODED_VALUE\"}"

# DELETE /secrets/ with SVID token returns 403 FORBIDDEN
assert_status "DELETE /secrets/ with SVID token (403)" "403" \
    DELETE "$KEYBOX_URL/secrets/global/$TEST_SECRET" \
    -H "Authorization: Bearer $SVID_TOKEN"

# GET /secrets/ with service token succeeds (200)
assert_status "GET /secrets/ with service token (200)" "200" \
    GET "$KEYBOX_URL/secrets/global/$TEST_SECRET" \
    -H "Authorization: Bearer $SERVICE_TOKEN"

# GET /audit/logs with service token succeeds (200)
assert_status "GET /audit/logs with service token (200)" "200" \
    GET "$KEYBOX_URL/audit/logs" \
    -H "Authorization: Bearer $SERVICE_TOKEN"

# GET /audit/logs with SVID token returns 403 FORBIDDEN
assert_status "GET /audit/logs with SVID token (403)" "403" \
    GET "$KEYBOX_URL/audit/logs" \
    -H "Authorization: Bearer $SVID_TOKEN"

echo ""

# --- DEK Rotation ---
echo "=== DEK Rotation ==="

# POST /admin/rotate-dek/ with service token succeeds (200, version increments)
printf "  %-60s" "POST /admin/rotate-dek/ with service token (200, v>=2)..."
ROTATE_RESPONSE=$(curl -s -w "\n%{http_code}" -X POST \
    "$KEYBOX_URL/admin/rotate-dek/$TEST_SECRET?scope=global" \
    -H "Authorization: Bearer $SERVICE_TOKEN" 2>/dev/null)
ROTATE_STATUS=$(echo "$ROTATE_RESPONSE" | tail -1)
ROTATE_BODY=$(echo "$ROTATE_RESPONSE" | sed '$d')
ROTATE_VERSION=$(echo "$ROTATE_BODY" | jq -r '.version' 2>/dev/null)

if [ "$ROTATE_STATUS" = "200" ] && [ "$ROTATE_VERSION" -ge 2 ] 2>/dev/null; then
    echo "OK ($ROTATE_STATUS, version=$ROTATE_VERSION)"
    PASSED=$((PASSED + 1))
else
    echo "FAIL (status=$ROTATE_STATUS, version=$ROTATE_VERSION)"
    FAILED=$((FAILED + 1))
fi

# POST /admin/rotate-dek/ with SVID token returns 403 FORBIDDEN
assert_status "POST /admin/rotate-dek/ with SVID token (403)" "403" \
    POST "$KEYBOX_URL/admin/rotate-dek/$TEST_SECRET?scope=global" \
    -H "Authorization: Bearer $SVID_TOKEN"

# POST /admin/rotate-dek/ for non-existent secret returns 404 SECRET_NOT_FOUND
assert_status "POST /admin/rotate-dek/ non-existent (404)" "404" \
    POST "$KEYBOX_URL/admin/rotate-dek/does-not-exist-$$?scope=global" \
    -H "Authorization: Bearer $SERVICE_TOKEN"

# Secret value unchanged after rotation
printf "  %-60s" "Secret value unchanged after rotation..."
SECRET_VALUE=$(curl -s -X GET "$KEYBOX_URL/secrets/global/$TEST_SECRET" \
    -H "Authorization: Bearer $SERVICE_TOKEN" 2>/dev/null | jq -r '.value')

if [ "$SECRET_VALUE" = "$ENCODED_VALUE" ]; then
    echo "OK"
    PASSED=$((PASSED + 1))
else
    echo "FAIL (expected $ENCODED_VALUE, got $SECRET_VALUE)"
    FAILED=$((FAILED + 1))
fi

echo ""

# --- Results ---
echo "=== Results ==="
echo "Passed: $PASSED"
echo "Failed: $FAILED"

if [ "$FAILED" -eq 0 ]; then
    echo "All keybox smoke tests passed!"
    exit 0
else
    echo "$FAILED test(s) failed"
    exit 1
fi
