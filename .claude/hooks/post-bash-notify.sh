#!/usr/bin/env bash
# Post-bash notification hook

# Read JSON input from stdin
STDIN_DATA=$(cat)

# Parse JSON using jq
if ! command -v jq &>/dev/null; then
    echo '{"hookSpecificOutput":{"hookEventName":"PostToolUse","additionalContext":"⚠️ Hook skipped: jq not installed (needed to parse hook input)"}}'
    exit 0
fi

COMMAND=$(echo "$STDIN_DATA" | jq -r '.tool_input.command // empty')
STDERR=$(echo "$STDIN_DATA" | jq -r '.tool_response.stderr // empty')
INTERRUPTED=$(echo "$STDIN_DATA" | jq -r '.tool_response.interrupted // false')

# Determine success (no stderr and not interrupted)
if [[ -z "$STDERR" ]] && [[ "$INTERRUPTED" != "true" ]]; then
    EXIT_CODE=0
else
    EXIT_CODE=1
fi

# Notify for cargo build/test commands
if [[ "$COMMAND" =~ ^cargo\ (build|test|clippy) ]]; then
    if [[ "$EXIT_CODE" == "0" ]]; then
        echo "✓ Command completed successfully"
    else
        echo "✗ Command failed"
    fi
fi

# Remind to monitor CI after git push (non-blocking)
if [[ "$COMMAND" =~ git\ push ]] && [[ "$EXIT_CODE" == "0" ]]; then
    cat << 'EOF'
{
  "hookSpecificOutput": {
    "hookEventName": "PostToolUse",
    "additionalContext": "🚀 Push successful! Monitor CI status with: gh run watch"
  }
}
EOF
    exit 0
fi
