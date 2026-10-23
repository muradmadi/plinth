# plinth

Scaffolds a new project in one stack, then creates and configures its GitHub repo the same way
every time: squash-only PRs behind a `main` ruleset, a Conventional Commit title check,
release-please and monthly Dependabot. A Claude Code plugin in this repo drives the same CLI.

Not usable yet; the first release adds `plinth new`.

## Development

Requires [mise](https://mise.jdx.dev).

```sh
mise install    # rust and just, pinned in mise.toml
just setup      # commit guard
just check      # format, lint, tests
```
