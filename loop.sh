#!/bin/bash

cd "$(dirname "$0")"

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

    if grep -q '"text":"[^"]*LOOP_COMPLETE: true' "$OUTPUT_FILE"; then
        echo "=== All tasks complete ==="
        exit 0
    fi

    if grep -q '"text":"[^"]*You'"'"'ve hit your limit' "$OUTPUT_FILE"; then
        echo "=== API usage limit reached ==="
        exit 0
    fi
    echo "=== Task ${TASK_NUM} complete, sleeping 2s ==="
    sleep 2
done
