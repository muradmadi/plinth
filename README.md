# plinth

Sets up the tooling around a project, then creates and configures its GitHub repo the same way
every time: mise and just with a `just check` gate that also audits the workflows (actionlint,
zizmor) and scans for secrets (gitleaks, in the gate and the commit hook), CI, squash-only PRs
behind a `main` ruleset, a Conventional Commit title check, release-please and monthly Renovate
updates, and an AGENTS.md with those rules for coding agents. It never touches the project's code:
create the project with its own tool first, then run `plinth init` in it. It sets up Rust projects
and Astro sites so far. A Claude Code plugin in this repo drives the same CLI.

## Install

Needs [mise](https://mise.jdx.dev), git, and the [GitHub CLI](https://cli.github.com) logged in
(`gh auth login`); the repo is created under the account gh is logged in as.

```sh
mise use -g github:muradmadi/plinth
```

Each release carries builds for Linux x86_64 and macOS on Apple silicon, each with a build
attestation: `gh attestation verify plinth-<target>.tar.gz --repo muradmadi/plinth` checks that a
download was built by this repo's `publish.yml`.

### Claude Code

```
/plugin install plinth --marketplace muradmadi/plinth
```

The `plinth` skill turns "start a new Rust CLI called wordcount" into `cargo new wordcount` and a
`plinth init` call, or "start an Astro site" into create-astro and the same call: it shows the `--dry-run` first and waits for a yes before creating the GitHub
repo. It needs the CLI installed as above.

### Renovate

Every project plinth sets up, plinth included, has `.github/renovate.json5`: monthly grouped
updates of its dependencies, its GitHub Actions (pinned by commit SHA) and the tools in `mise.toml`.
Patch and minor updates merge themselves once `check` passes. It needs the
[Renovate app](https://github.com/apps/renovate) installed once on the GitHub account, for all
repositories. An install for all repositories starts in Silent mode, which opens no PRs; switch the
account to Interactive on the [Mend developer portal](https://developer.mend.io).

### Release PRs

release-please opens its release PR with the workflow's own token, and GitHub holds the workflows
on such a PR until someone clicks "Approve workflows to run" on it. Click it once per release PR;
its `check` then runs and the PR can merge.

## Use

```sh
mise exec rust -- cargo new demo   # works without a global Rust; plinth pins the project's own
cd demo
plinth init --dry-run              # lists the files and commands, touches nothing
plinth init --description "…"      # adds the tooling, sets it up, creates <you>/demo
```

An Astro site works the same way, with npm, pnpm or bun:

```sh
mise exec node@24 -- npm create astro@latest site -- --template minimal --install --no-git --yes
cd site
plinth init --dry-run              # also lists the dev dependencies it will install
```

plinth works out the language from the project's manifest: `Cargo.toml` for Rust, a `package.json`
that depends on `astro` for Astro. It refuses any other project and says why. It adds the files the
project lacks, appends missing lines to `.gitignore` and `.gitattributes`, adds its house rules to
an AGENTS.md the project already has, and lists the files it left alone because the project has its
own. Running it again adds nothing.

What `just check` runs:

- **Rust:** rustfmt, clippy with warnings as errors, and the tests.
- **Astro:** Prettier, ESLint and `astro check`. `just ci` also builds the site.

Both also run the audit: typos, actionlint, zizmor, shellcheck and gitleaks.

For Astro, plinth installs Prettier, ESLint, `@astrojs/check` and TypeScript as dev dependencies
with the package manager the lockfile names. It also pins that package manager and Node in
`mise.toml`.

The repo is named after the directory unless `--name` gives another, is private unless `--public`
is given, and goes under the account gh is logged in as unless `--owner` names another. It gets
squash-only merges, the label set, and a `main` ruleset that requires a PR with a passing `check`.
plinth refuses a project that already has a git remote, or that has commits and uncommitted
changes. `--no-github` stops before GitHub; `--no-setup --no-github` writes the files only.

## Development

Requires [mise](https://mise.jdx.dev).

```sh
mise install    # rust and just, pinned in mise.toml
just setup      # commit guard
just check      # format, lint, tests, spelling, workflow and secret checks
```
