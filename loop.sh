#!/bin/bash

cd "$(dirname "$0")"

# 1. Discovery (Sonnet) - populate tracks.md with implementation items
echo "=== Running discovery ==="
DISCOVERY_OUTPUT="/tmp/moto-discovery-$(date +%Y%m%d-%H%M%S).txt"
cat discovery-prompt.md | claude -p \
    --dangerously-skip-permissions \
    --output-format=stream-json \
    --verbose \
    | tee "$DISCOVERY_OUTPUT" \
    | npx repomirror visualize

# Match agent text output, not tool results containing file content
if grep -q '"text":"[^"]*DISCOVERY: no new items' "$DISCOVERY_OUTPUT"; then
    echo "=== No new items to implement ==="
fi

# 2. Implementation loop (Opus)
TASK_NUM=0
while true; do
    TASK_NUM=$((TASK_NUM + 1))
    TIMESTAMP=$(date +%Y%m%d-%H%M%S)
    OUTPUT_FILE="/tmp/moto-loop-${TIMESTAMP}-${TASK_NUM}.txt"

    echo "=== Starting task ${TASK_NUM} ==="

    cat prompt.md | claude -p \
        --dangerously-skip-permissions \
        --output-format=stream-json \
        --verbose \
        | tee "$OUTPUT_FILE" \
        | npx repomirror visualize

    # Match agent text output, not tool results containing file content
    if grep -q '"text":"[^"]*LOOP_COMPLETE: true' "$OUTPUT_FILE"; then
        echo "=== All tasks complete ==="
        break
    fi

    if grep -q '"text":"[^"]*You'"'"'ve hit your limit' "$OUTPUT_FILE"; then
        echo "=== API usage limit reached ==="
        break
    fi
    echo "=== Task ${TASK_NUM} complete, sleeping 2s ==="
    sleep 2
done

# 3. Bookkeeping (Haiku) - archive completed items to tracks-history.md
echo "=== Running bookkeeping ==="
BOOKKEEPING_OUTPUT="/tmp/moto-bookkeeping-$(date +%Y%m%d-%H%M%S).txt"
cat bookkeeping-prompt.md | claude -p \
    --model haiku \
    --dangerously-skip-permissions \
    --output-format=stream-json \
    --verbose \
    | tee "$BOOKKEEPING_OUTPUT" \
    | npx repomirror visualize

echo "=== Loop finished ==="
