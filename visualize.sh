#!/bin/bash
# Simple stream-json visualizer that doesn't truncate

while IFS= read -r line; do
    TYPE=$(echo "$line" | jq -r '.type // empty' 2>/dev/null)

    case "$TYPE" in
        system)
            echo -e "\n\033[36m━━━ Session Started ━━━\033[0m\n"
            ;;
        assistant)
            # Show thinking
            THINKING=$(echo "$line" | jq -r '.message.content[]? | select(.type=="thinking") | .thinking // empty' 2>/dev/null)
            if [ -n "$THINKING" ]; then
                echo -e "\033[35m💭 $THINKING\033[0m"
            fi

            # Show text
            TEXT=$(echo "$line" | jq -r '.message.content[]? | select(.type=="text") | .text // empty' 2>/dev/null)
            if [ -n "$TEXT" ]; then
                echo -e "\033[1;37m$TEXT\033[0m"
            fi

            # Show tool calls
            echo "$line" | jq -r '.message.content[]? | select(.type=="tool_use") | "\n\033[1;33m⚡ \(.name)\033[0m\n\(.input | to_entries | map("   \(.key): \(.value | tostring | .[0:500])") | join("\n"))"' 2>/dev/null
            ;;
        user)
            # Show tool results (file reads, command output, etc.)
            TOOL_RESULT=$(echo "$line" | jq -r '.message.content[]? | select(.type=="tool_result") | .content // empty' 2>/dev/null)
            if [ -n "$TOOL_RESULT" ]; then
                # Truncate very long results but show more than repomirror
                LINES=$(echo "$TOOL_RESULT" | wc -l)
                if [ "$LINES" -gt 50 ]; then
                    echo -e "\033[36m$(echo "$TOOL_RESULT" | head -30)\033[0m"
                    echo -e "\033[33m   ... ($LINES total lines, showing first 30) ...\033[0m"
                else
                    echo -e "\033[36m$TOOL_RESULT\033[0m"
                fi
            fi
            ;;
        result)
            echo -e "\n\033[1;32m━━━ Complete ━━━\033[0m"
            echo "$line" | jq -r '.result // empty' 2>/dev/null
            COST=$(echo "$line" | jq -r '.total_cost_usd // empty' 2>/dev/null)
            if [ -n "$COST" ]; then
                echo -e "\033[32mCost: \$$COST\033[0m"
            fi
            ;;
    esac
done
