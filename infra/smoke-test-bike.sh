#!/usr/bin/env bash
# Smoke tests for moto-bike container (minimal base image)
# Verifies the container builds correctly and contains expected minimal contents.
#
# Usage:
#   ./infra/smoke-test-bike.sh

set -euo pipefail

IMAGE_NAME="moto-bike:latest"

echo "=== Moto Bike Container Smoke Tests ==="
echo ""

# Check if image exists
if ! docker image inspect "$IMAGE_NAME" &>/dev/null; then
    echo "ERROR: Image $IMAGE_NAME not found."
    echo "Build it first with: make build-bike"
    exit 1
fi

FAILED=0

run_test() {
    local name="$1"
    local check="$2"
    printf "  %-40s" "$name..."
    if eval "$check" &>/dev/null; then
        echo "OK"
        return 0
    else
        echo "FAIL"
        return 1
    fi
}

echo ""
echo "=== Image Size ==="

# Check image size is under 20MB (per spec)
SIZE_BYTES=$(docker image inspect "$IMAGE_NAME" --format '{{.Size}}')
SIZE_MB=$((SIZE_BYTES / 1024 / 1024))
printf "  %-40s" "Size under 20MB..."
if [ "$SIZE_MB" -lt 20 ]; then
    echo "OK (${SIZE_MB}MB)"
else
    echo "FAIL (${SIZE_MB}MB > 20MB)"
    ((FAILED++))
fi

echo ""
echo "=== User Configuration ==="

# Check default user is non-root (1000:1000)
USER_CONFIG=$(docker image inspect "$IMAGE_NAME" --format '{{.Config.User}}')
printf "  %-40s" "Non-root user (1000:1000)..."
if [ "$USER_CONFIG" = "1000:1000" ]; then
    echo "OK"
else
    echo "FAIL (got: $USER_CONFIG)"
    ((FAILED++))
fi

echo ""
echo "=== Environment Variables ==="

# Check SSL_CERT_FILE is set
printf "  %-40s" "SSL_CERT_FILE set..."
if docker image inspect "$IMAGE_NAME" --format '{{.Config.Env}}' | grep -q "SSL_CERT_FILE="; then
    echo "OK"
else
    echo "FAIL"
    ((FAILED++))
fi

# Check TZDIR is set
printf "  %-40s" "TZDIR set..."
if docker image inspect "$IMAGE_NAME" --format '{{.Config.Env}}' | grep -q "TZDIR="; then
    echo "OK"
else
    echo "FAIL"
    ((FAILED++))
fi

echo ""
echo "=== Minimal Contents ==="

# Check that no shell exists (security: minimal attack surface)
printf "  %-40s" "No shell present..."
if ! docker run --rm --entrypoint="" "$IMAGE_NAME" /bin/sh -c "echo test" &>/dev/null && \
   ! docker run --rm --entrypoint="" "$IMAGE_NAME" /bin/bash -c "echo test" &>/dev/null; then
    echo "OK"
else
    echo "FAIL (shell found)"
    ((FAILED++))
fi

echo ""
echo "=== Results ==="
if [ "$FAILED" -eq 0 ]; then
    echo "All smoke tests passed!"
    exit 0
else
    echo "$FAILED test(s) failed"
    exit 1
fi
