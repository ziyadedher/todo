#!/usr/bin/env bash
echo '{"async":true,"asyncTimeout":30000}'

# Check if cargo is available
if ! command -v cargo &> /dev/null; then
    echo "⚠️  cargo not found. Install Rust: https://rustup.rs"
    exit 0
fi

# Check if dependencies need updating (Cargo.lock newer than target)
if [ -f "Cargo.lock" ] && [ -d "target" ]; then
    if [ "Cargo.lock" -nt "target" ]; then
        echo "📦 Dependencies may have changed, consider running: cargo build"
    fi
fi
