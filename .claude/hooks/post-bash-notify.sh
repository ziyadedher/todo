#!/usr/bin/env bash
# Notify when cargo commands complete (for long builds)
COMMAND="$1"
EXIT_CODE="$2"

# Only notify for cargo build/test commands
if [[ "$COMMAND" =~ ^cargo\ (build|test|clippy) ]]; then
    if [[ "$EXIT_CODE" == "0" ]]; then
        echo "✓ $COMMAND completed successfully"
    else
        echo "✗ $COMMAND failed (exit code: $EXIT_CODE)"
    fi
fi

# Remind to monitor CI after git push
if [[ "$COMMAND" =~ git\ push ]] && [[ "$EXIT_CODE" == "0" ]]; then
    echo ""
    echo "🚀 Push successful! Monitor CI status with: gh run watch"
fi
