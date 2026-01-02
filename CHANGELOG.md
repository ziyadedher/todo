# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [1.8.0](https://github.com/ziyadedher/todo/compare/v1.7.0...v1.8.0) - 2026-01-02

### Added

- add `todo add` command to create new tasks with natural language dates

## [1.7.0](https://github.com/ziyadedher/todo/compare/v1.6.0...v1.7.0) - 2026-01-01

### Added

- add `todo complete` command to mark tasks as done

### Fixed

- update hooks to read JSON from stdin

### Other

- add unit tests for task grouping and date serde
- add CI monitoring guidance and post-push hook reminder

## [1.6.0](https://github.com/ziyadedher/todo/compare/v1.5.0...v1.6.0) - 2026-01-01

### Added

- use draft releases for immutable release support
- add ARM64 Linux build target for Termux/Android

### Fixed

- use correct ARM64 runner label (ubuntu-24.04-arm)
- skip focus project lookup when using cache mode

### Other

- update README with crates.io installation instructions
- fix README typos and add doc review reminder
- add CLAUDE.md and Claude Code hooks

## [1.5.0](https://github.com/ziyadedher/todo/compare/v1.4.1...v1.5.0) - 2026-01-01

### Added

- dynamic workspace and focus project selection

## [1.4.1](https://github.com/ziyadedher/todo/compare/v1.4.0...v1.4.1) - 2026-01-01

### Fixed

- conditionally import std::fs for macOS only
- use Some() instead of Ok() for Option return type

### Other

- modularize codebase into separate files

## [1.4.0](https://github.com/ziyadedher/todo/compare/v1.3.0...v1.4.0) - 2026-01-01

### Added

- add --color flag for status command

### Other

- share build config between CI and release-plz

## [1.3.0](https://github.com/ziyadedher/todo/compare/v1.2.0...v1.3.0) - 2026-01-01

### Added

- add colors to status output and rename format to 'short'
- auto-refresh cache after non-cached commands

### Fixed

- add explicit pkg-fmt to binstall metadata

## [1.2.0](https://github.com/ziyadedher/todo/compare/v1.1.2...v1.2.0) - 2026-01-01

### Added

- add zsh and improved tmux integration guides
- add zsh and improved tmux integration guides

### Other

- add cargo-husky for pre-commit hooks
- fix formatting

## [1.1.2](https://github.com/ziyadedher/todo/compare/v1.1.1...v1.1.2) - 2026-01-01

### Other

- update Cargo.lock dependencies

## [1.1.1](https://github.com/ziyadedher/todo/compare/v1.1.0...v1.1.1) - 2026-01-01

### Fixed

- use gh cli to upload release assets

## [1.1.0](https://github.com/ziyadedher/todo/compare/v1.0.1...v1.1.0) - 2026-01-01

### Added

- add cargo-binstall support

### Other

- re-enable changelog updates
- integrate binary builds into release-plz workflow
