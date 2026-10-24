# plinth

Sets up the tooling around a project, then creates and configures its GitHub repo the same way
every time: mise and just with a `just check` gate that also audits the workflows (actionlint,
zizmor) and scans for secrets (gitleaks, in the gate and the commit hook), CI, squash-only PRs
behind a `main` ruleset, a Conventional Commit title check, release-please and monthly Renovate
updates, and an AGENTS.md with those rules for coding agents. It never touches the project's code:
create the project with its own tool first, then run `plinth init` in it. It sets up Rust projects
and TypeScript ones so far: Astro sites, React apps built with Vite, Hono servers and plain
packages, alone or together in a monorepo. A Claude Code plugin in this repo drives the same CLI.

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
`plinth init` call, or "start a React app" into create-vite and the same call. It shows the
`--dry-run` first and waits for a yes before creating the GitHub repo. It needs the CLI installed as
above.

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

A TypeScript project works the same way, with npm, pnpm or bun:

```sh
mise exec node@24 bun -- bun create vite@latest web --template react-ts --no-interactive
cd web && mise exec bun -- bun install
plinth init --dry-run              # also lists the dev dependencies it will install
```

The other starting points: `npm create astro@latest site -- --template minimal --install --no-git`
for an Astro site, `bun create hono@latest api --template bun --install --pm bun` for a Hono server,
and `bun init` for a plain package. plinth then works out what the project is from the manifest:

- **`Cargo.toml`:** Rust.
- **`package.json`:** TypeScript, with the package manager its lockfile names. Its dependencies say
  what kind: `astro` for an Astro site, `react` with `vite` for a React app, `hono` for a server;
  anything else is a plain package.

A monorepo gets one gate at its root that covers each package by its kind. An existing one lists
its packages in package.json's `workspaces` or in `pnpm-workspace.yaml`. To start a new one, make
each package with its own tool in its own directory, such as `apps/web` and `apps/api`, then run
plinth in the directory above them. It assembles the root: a `package.json` listing `apps/*`, a
name for each package that lacks one, and one install at the root in place of each package's own
lockfile and `node_modules`. The dry run lists each change first.

It refuses what it can't set up yet and says why: Cargo workspaces, Next.js, Vue, Svelte,
Expo and other frameworks, React without Vite, yarn, and a project that asks for TypeScript 7
(`bun init` does; change it to `~6.0`).

It adds the files the project lacks, appends missing lines to `.gitignore` and `.gitattributes`,
adds its house rules to an AGENTS.md the project already has, and lists the files it left alone
because the project has its own, an ESLint or Prettier config under any name included.

The gate itself lives in two files plinth owns: `plinth.just` with the `just check` recipes, and for
TypeScript `plinth.eslint.mjs` with the lint rules. The project's `justfile` and `eslint.config.mjs`
import them; they are written once and left to the project, and a recipe or rule there replaces
plinth's. Running plinth again rewrites the owned files to fit the project as it is then, such as a
monorepo with a new package, and adds nothing else. It says what to add to `.prettierrc.json` for
a new package's style. Delete an owned file's first line to make it the project's own.

What `just check` runs:

- **Rust:** rustfmt, clippy with warnings as errors, and the tests.
- **TypeScript:**
  - Prettier, in the style the project's tool wrote its files in;
  - ESLint with typescript-eslint's strict, type-aware rules, plus the React hooks rules or Astro's;
  - `tsc`, or `astro check` for an Astro site;
  - with bun, `bun test`.

  `just ci` also builds a site or app.

Both also run the audit: typos, actionlint, zizmor, shellcheck and gitleaks.

For TypeScript, plinth installs Prettier, ESLint, TypeScript 6 and what the kind of project needs
as dev dependencies, with the project's own package manager. It also pins that package manager and
Node in `mise.toml`.

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
