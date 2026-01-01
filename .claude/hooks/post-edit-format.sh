#!/usr/bin/env bash
# Format Rust files after editing
FILE_PATH="$1"

if [[ "$FILE_PATH" == *.rs ]]; then
    if command -v cargo &> /dev/null; then
        cargo fmt --quiet -- "$FILE_PATH" 2>/dev/null || true
    fi
fi
