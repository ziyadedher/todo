# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Quick Start Commands

```bash
# Install locally
cargo install --path .

# Run the CLI
cargo run -- <command>

# Build release binary
cargo build --release

# Run all tests
cargo test

# Run a single test
cargo test <test_name>

# Lint and format
cargo clippy -- -D warnings
cargo fmt --check
cargo fmt  # to auto-fix
```

## Architecture Overview

This is a Rust CLI tool for Asana task management with OAuth2 authentication.

**Key modules:**
- `src/main.rs` - CLI entry point with clap-based argument parsing
- `src/asana.rs` - Asana API client with OAuth2/PAT authentication
- `src/commands/` - Subcommands (summary, list, complete, add, focus, update, status, install)
- `src/cache.rs` - JSON-based local cache for offline mode
- `src/config.rs` - TOML configuration management
- `src/focus.rs` - Daily focus tracking logic

**Data flow:**
1. Auth credentials stored in cache (`~/.cache/todo/cache.json`)
2. Config stored in `~/.config/todo/config.toml`
3. `--use-cache` flag enables offline mode using cached Asana data

## Development Notes

**Commit messages:** This project uses [release-plz](https://release-plz.dev/) for automated releases. **ALWAYS use semantic commit messages:**
- `feat:` - new features (bumps minor version)
- `fix:` - bug fixes (bumps patch version)
- `feat!:` or `fix!:` - breaking changes (bumps major version)
- `chore:`, `docs:`, `refactor:`, `test:` - no version bump

**Pre-commit hooks:** cargo-husky runs `cargo test`, `cargo clippy -- -D warnings`, and `cargo fmt --check` on every commit. Ensure these pass before committing.

**Before committing:** Always review `README.md` and `CLAUDE.md` to ensure they stay accurate and up-to-date with any changes made.

**Testing commands:** The CLI requires Asana OAuth setup. For quick iteration:
```bash
RUST_LOG=debug cargo run -- --use-cache summary
```

**Cross-platform builds:** CI builds for x86_64-linux, aarch64-linux (Termux/Android), x86_64-darwin, and aarch64-darwin. The `cargo-binstall` metadata in Cargo.toml enables binary installation.

**After pushing:** Always monitor GitHub Actions to ensure CI passes. Check the run status with:
```bash
gh run list --limit 5
gh run watch  # live monitor the latest run
```
