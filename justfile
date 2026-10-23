# plinth task runner. `just` on its own lists everything.
#
# The toolchain (rust, just) is pinned in mise.toml — run `mise install` once.

# List available recipes
default:
    @just --list --unsorted

# One-time setup: commit guard
setup:
    git config core.hooksPath .githooks

# The gate: formatting, lint, tests
check: format-check lint test

# What CI runs: the gate with the lockfile enforced
ci: check

# Fail on any file rustfmt would change
format-check:
    cargo fmt --check

# Rewrite files the way rustfmt and clippy's fixers want them
format:
    cargo fmt
    cargo clippy --fix --allow-dirty --allow-staged --all-targets

# Lint everything, tests included; warnings fail too
lint:
    cargo clippy --locked --all-targets -- -D warnings

# Run the test suite, including the slow tests `cargo test` skips
test:
    cargo test --locked -- --include-ignored
