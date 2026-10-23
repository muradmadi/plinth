# plinth

Scaffolds a new project in one stack, then creates and configures its GitHub repo the same way
every time: squash-only PRs behind a `main` ruleset, a Conventional Commit title check,
release-please and monthly Dependabot. A Claude Code plugin in this repo drives the same CLI.

## Use

```sh
plinth stacks                                    # what `new` can build on
plinth new demo --stack rust --description "…"   # writes ./demo, sets it up, creates muradmadi/demo
plinth new demo --stack rust --dry-run           # lists the files and commands, touches nothing
```

The repo is private unless `--public` is given, and goes under the account gh is logged in as unless
`--owner` names another. It gets squash-only merges, the label set, and a `main` ruleset that
requires a PR with a passing `check`. `--no-github` stops before GitHub; `--no-setup --no-github`
writes the files only.

## Development

Requires [mise](https://mise.jdx.dev).

```sh
mise install    # rust and just, pinned in mise.toml
just setup      # commit guard
just check      # format, lint, tests
```
