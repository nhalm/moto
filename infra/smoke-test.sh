#!/usr/bin/env bash
# Smoke tests for moto-garage container
# Verifies the container builds correctly and contains expected tooling.
#
# Usage:
#   ./infra/smoke-test.sh         # Run tests
#   ./infra/smoke-test.sh --keep  # Keep container for debugging

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
IMAGE_NAME="moto-garage:latest"
CONTAINER_NAME="moto-garage-smoke-test-$$"
KEEP_CONTAINER=false

# Parse arguments
for arg in "$@"; do
    case $arg in
        --keep)
            KEEP_CONTAINER=true
            shift
            ;;
    esac
done

cleanup() {
    if [ "$KEEP_CONTAINER" = false ]; then
        echo "Cleaning up container..."
        docker rm -f "$CONTAINER_NAME" 2>/dev/null || true
    else
        echo "Container kept for debugging: $CONTAINER_NAME"
        echo "To enter: docker exec -it $CONTAINER_NAME bash"
        echo "To remove: docker rm -f $CONTAINER_NAME"
    fi
}

trap cleanup EXIT

echo "=== Moto Garage Container Smoke Tests ==="
echo ""

# Check if image exists
if ! docker image inspect "$IMAGE_NAME" &>/dev/null; then
    echo "ERROR: Image $IMAGE_NAME not found."
    echo "Build it first with: make docker-build-moto-garage"
    exit 1
fi

echo "Starting container..."
docker run -d --name "$CONTAINER_NAME" "$IMAGE_NAME" sleep infinity

run_test() {
    local name="$1"
    local cmd="$2"
    printf "  %-40s" "$name..."
    if docker exec "$CONTAINER_NAME" bash -c "$cmd" &>/dev/null; then
        echo "OK"
        return 0
    else
        echo "FAIL"
        return 1
    fi
}

run_test_output() {
    local name="$1"
    local cmd="$2"
    local expected="$3"
    printf "  %-40s" "$name..."
    local output
    if output=$(docker exec "$CONTAINER_NAME" bash -c "$cmd" 2>/dev/null); then
        if [[ "$output" == *"$expected"* ]]; then
            echo "OK"
            return 0
        else
            echo "FAIL (expected '$expected', got '$output')"
            return 1
        fi
    else
        echo "FAIL (command failed)"
        return 1
    fi
}

FAILED=0

echo ""
echo "=== Core Tools ==="

run_test "rustc present" "which rustc" || ((FAILED++))
run_test "cargo present" "which cargo" || ((FAILED++))
run_test "git present" "which git" || ((FAILED++))
run_test "jj present" "which jj" || ((FAILED++))
run_test "kubectl present" "which kubectl" || ((FAILED++))

echo ""
echo "=== Environment Variables ==="

run_test "RUST_BACKTRACE set" '[ -n "$RUST_BACKTRACE" ]' || ((FAILED++))
run_test "CARGO_HOME set" '[ -n "$CARGO_HOME" ]' || ((FAILED++))
run_test "WORKSPACE set" '[ -n "$WORKSPACE" ]' || ((FAILED++))

echo ""
echo "=== Rust Compilation ==="

RUST_TEST='
cd /tmp
cat > test.rs << EOF
fn main() {
    println!("Hello from moto-garage!");
}
EOF
rustc test.rs -o test && ./test
'
run_test_output "compile and run" "$RUST_TEST" "Hello from moto-garage!" || ((FAILED++))

echo ""
echo "=== Results ==="
if [ "$FAILED" -eq 0 ]; then
    echo "All smoke tests passed!"
    exit 0
else
    echo "$FAILED test(s) failed"
    exit 1
fi
