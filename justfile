# plinth task runner. `just` on its own lists everything.
#
# The toolchain (rust, just and the audit tools) is pinned in mise.toml — run `mise install` once.

# List available recipes
default:
    @just --list --unsorted

# One-time setup: commit guard
setup:
    git config core.hooksPath .githooks

# The gate: formatting, lint, tests, then the repo's own hygiene
check: format-check lint test audit

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

# Spelling, workflow mistakes and security problems, the commit hook, and secrets in any commit.
# zizmor also runs its online audits when GH_TOKEN is set, as it is in CI.
audit:
    typos --hidden --exclude .git
    actionlint
    zizmor --no-progress .github/workflows
    shellcheck .githooks/pre-commit
    gitleaks git --redact --no-banner
