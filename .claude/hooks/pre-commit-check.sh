#!/usr/bin/env bash
# Validate semantic commit message format for git commit commands
BASH_CMD="$1"

# Only check git commit commands with -m flag
if [[ ! "$BASH_CMD" =~ git\ (add\ .*\&\&\ )?commit ]]; then
    exit 0
fi

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
