#!/usr/bin/env bash
# Validate semantic commit message format for git commit commands

# Read JSON input from stdin
STDIN_DATA=$(cat)

# Parse command from JSON using jq
if ! command -v jq &>/dev/null; then
    echo '{"hookSpecificOutput":{"hookEventName":"PreToolUse","additionalContext":"⚠️ Hook skipped: jq not installed (needed to parse hook input)"}}'
    exit 0
fi

BASH_CMD=$(echo "$STDIN_DATA" | jq -r '.tool_input.command // empty')

# Only check git commit commands with -m flag
if [[ ! "$BASH_CMD" =~ git\ (add\ .*\&\&\ )?commit ]]; then
    exit 0
fi

# Remind to check docs
echo "📝 Reminder: Ensure README.md and CLAUDE.md are up-to-date with your changes."

# Extract commit message from -m "..." or -m '...'
COMMIT_MSG=$(echo "$BASH_CMD" | grep -oP '(-m\s+["\x27])?\K[^"\x27]+(?=["\x27](\s|$))' | head -1)

if [[ -z "$COMMIT_MSG" ]]; then
    exit 0
fi

# Check for semantic commit prefix
if ! echo "$COMMIT_MSG" | grep -qE "^(feat|fix|docs|style|refactor|perf|test|build|ci|chore|revert)(!)?(\(.+\))?: .+"; then
    echo "⚠️  Commit message should follow semantic format: type(scope)?: description"
    echo "   Examples: feat: add new feature, fix!: breaking change, docs(readme): update"
fi
