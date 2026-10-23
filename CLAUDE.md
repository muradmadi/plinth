# plinth

plinth is a Rust CLI that scaffolds a new project and sets up its GitHub repo in the house style
`~/Projects/tide` established: mise + just, `just ci` as the gate, squash-only PRs behind a `main`
ruleset, a Conventional Commit title check, release-please, monthly grouped Dependabot, PR and issue
templates, and a pre-commit guard. This repo is set up exactly the way plinth sets up others.

The build plan is `~/.claude/plans/warm-baking-aho.md`. Rust is the only stack until the stack
discussion settles TypeScript.

## Principles

- **Never prompts.** Every choice is a flag, so Claude or any other harness can drive it.
- **Practise what it preaches.** The shared files plinth generates match this repo's own copies; a
  test enforces it. Change one, change both. A Dependabot PR that bumps an action in `.github/` fails
  that test until the same bump is made under `templates/`; push it onto Dependabot's branch.
- **Asks before touching GitHub.** `--dry-run` shows the `gh` calls; new repos are private unless
  `--public` is given.

## Layout

```
src/main.rs        CLI: `new` and `stacks` (clap), name and directory checks
src/scaffold.rs    renders the layers, writes the files, runs the setup commands
src/stack.rs       the stacks and their setup commands
templates/base/    files every project gets; `.j2` files are rendered with minijinja
templates/rust/    the Rust stack; a file here replaces a base file at the same path
tests/new.rs       runs the binary, including the check that it reproduces this repo
skills/plinth/     the Claude Code skill (planned)
```

## Git and GitHub

The global PR workflow applies. `main` is protected by a ruleset: changes reach it only through a
squash-merged PR whose `check` passes and whose branch is up to date. Labels: `feature`, `bug`,
`docs`, `ci`, `dependencies`. Wait for Murad before merging.

## Toolchain

Rust and just are pinned in `mise.toml`; run `mise install`, then `just setup` once. `just check` is
the gate: `cargo fmt --check`, clippy with warnings as errors, `cargo test`. Murad is new to Rust
(background in C# and C++), so explain Rust-specific choices briefly in PRs where they aren't obvious.
