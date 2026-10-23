# plinth

Scaffolds a new project in one stack, then creates and configures its GitHub repo the same way
every time: squash-only PRs behind a `main` ruleset, a Conventional Commit title check,
release-please and monthly Dependabot. A Claude Code plugin in this repo drives the same CLI.

## Use

```sh
plinth stacks                                    # what `new` can build on
plinth new demo --stack rust --description "…"   # writes ./demo, then git init, mise install, setup
plinth new demo --stack rust --dry-run           # lists the files and commands, touches nothing
```

`--no-setup` writes the files only. The GitHub repo step isn't built yet.

## Development

Requires [mise](https://mise.jdx.dev).

```sh
mise install    # rust and just, pinned in mise.toml
just setup      # commit guard
just check      # format, lint, tests
```
