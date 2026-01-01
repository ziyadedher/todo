#!/usr/bin/env bash
# Format Rust files after editing

# Read JSON input from stdin
STDIN_DATA=$(cat)

# Parse file path from JSON using jq
if ! command -v jq &>/dev/null; then
    echo '{"hookSpecificOutput":{"hookEventName":"PostToolUse","additionalContext":"⚠️ Hook skipped: jq not installed (needed to parse hook input)"}}'
    exit 0
fi

FILE_PATH=$(echo "$STDIN_DATA" | jq -r '.tool_input.file_path // empty')

if [[ "$FILE_PATH" == *.rs ]]; then
    if command -v cargo &> /dev/null; then
        cargo fmt --quiet -- "$FILE_PATH" 2>/dev/null || true
    fi
fi
