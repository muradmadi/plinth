---
name: plinth
description: Set up a project's tooling and GitHub repo with the plinth CLI, after creating the project with its own tool (cargo new for Rust; create-astro, create-vite, create-hono or bun init for TypeScript). Use when the user wants to start, scaffold, create, bootstrap or set up a new project, app, tool or repo, wants the house tooling (mise, just, CI, release-please, the main ruleset) added to a project that has none yet, or asks for plinth by name.
---

# plinth

plinth adds the tooling around a project: mise and just with a `just check` gate, CI,
release-please, Renovate, PR and issue templates, a pre-commit guard, AGENTS.md and Claude Code
settings. Then it creates the GitHub repo with squash-only merges, the label set and a `main`
ruleset. It never touches the project's code and never prompts; every choice is a flag. The project
comes first, from its own tool. Your job is to collect the choices, create the project, show
plinth's plan, get a yes, and run it.

## 1. Check plinth is there

Run `plinth --version`. If it isn't found, stop and tell the user it installs with
`mise use -g github:muradmadi/plinth`. Don't install it yourself.

## 2. Collect the choices

plinth sets up Rust projects and TypeScript ones so far: an Astro site, a React app built with
Vite, a Hono server, or a plain package such as a library or CLI, alone or several together in a
monorepo. It finds them from `Cargo.toml` or `package.json`. If the user wants anything else, such
as a Next.js app, a Cargo workspace or a Python tool, say plinth can't set it up yet and stop. Don't
substitute something it supports.

Settle, asking only for what the user hasn't already said:

- **Name**: lowercase letters, digits and dashes, starting with a letter. It becomes the
  directory and the repo name. Suggest one if they described the project but didn't name it.
- **Kind**: for Rust, a binary (`cargo new`) or a library (`cargo new --lib`). For TypeScript,
  which of the four, and the package manager: bun unless they prefer npm or pnpm. For a monorepo,
  which kind each package is and where it goes: apps in `apps/<name>`, shared code in
  `packages/<name>`, one package manager for all of them.
- **Description**: one line on what it does. Draft it from what they said and let them change it.
- **Where**: `~/Projects/<name>` unless they say otherwise.
- **GitHub**: private unless they ask for public. `--no-github` if they don't want a repo.

For a project that already exists, skip the kind and use its directory.

## 3. Create the project

Going through mise works on a machine with no Rust or Node set up outside a project; plinth pins
the project's own versions after.

- **Rust:** `mise exec rust -- cargo new <dir>`, with `--lib` for a library.
- **Astro:** the minimal starter, installed, without a git repo of its own (plinth makes one):
  - npm: `mise exec node@24 -- npm create astro@latest <dir> -- --template minimal --install --no-git --yes`
  - pnpm: `mise exec node@24 pnpm -- pnpm create astro@latest <dir> --template minimal --install --no-git --yes`
  - bun: `mise exec node@24 bun -- bun create astro@latest <dir> --template minimal --install --no-git --yes`

  Use another template only if the user asks for one.
- **React app:** `mise exec node@24 bun -- bun create vite@latest <dir> --template react-ts
  --no-interactive`, then `bun install` in it. With npm: `npm create vite@latest <dir> --
  --template react-ts --no-interactive`, then `npm install`.
- **Hono server:** `mise exec node@24 bun -- bun create hono@latest <dir> --template bun --install
  --pm bun`.
- **Plain package:** `mise exec bun -- bun init -y` in a new `<dir>`. It asks for TypeScript 7,
  which plinth refuses until its lint supports 7: change `"typescript": "^7"` in package.json to
  `"~6.0"` and run `bun install` before plinth.

- **Monorepo:** make the directory, then each package in it with the command above, using
  `apps/<name>` or `packages/<name>` as `<dir>` and running from the monorepo's directory. With
  pnpm, `pnpm create hono@latest <dir> --template nodejs --install --pm pnpm` makes the server; it
  asks for TypeScript 7 too, so change it as for `bun init`. plinth then assembles the root and
  installs once there. create-hono's Node starter isn't Prettier-formatted, so run `just format`
  once after plinth if `just check` says so.

The directory must not already exist; if it does, ask whether to set up that project instead.
Never delete a directory to make room.

## 4. Show the plan

Run plinth with `--dry-run` first, for example:

```sh
plinth init --dir ~/Projects/demo --description "What it does" --dry-run
```

Show the user the output: the files it adds, the files it leaves alone, the setup commands (for
TypeScript, the dev dependencies it installs), and the `gh` calls. For a new monorepo it also lists
the package names it adds and the lockfiles and `node_modules` it removes. Creating a GitHub repo is
visible outside this machine, so get an explicit yes to this exact plan, including private or
public, before going on.

## 5. Run it

Run the same command without `--dry-run`. It takes a minute or so: it installs the pinned tools
through mise before creating the repo.

If it fails, report the error as plinth printed it. A taken repo name, an existing git remote, or
uncommitted changes in a project with commits are caught before anything is written; ask the user
how to go on. Don't commit or stash their changes yourself.

If the project already had a `CLAUDE.md`, plinth says so: Claude Code then reads that file instead
of the new AGENTS.md. Offer to add `@AGENTS.md` as its first line, and add it only on a yes.

## 6. Report

Say where the project is and link the repo. Then the two things to know:

- Changes reach `main` only through a PR whose `check` passes.
- After the first release-worthy merge, release-please opens a release PR. GitHub holds its
  workflows until someone clicks "Approve workflows to run" on it; after that it can merge.

## Later: a new package

When a package is added to a monorepo plinth set up, run `plinth init --no-github --dry-run` in its
root, then without `--dry-run` on a yes. It rewrites `plinth.just` and `plinth.eslint.mjs` for the
new package and installs any dev dependency it now needs. If it prints a note about
`.prettierrc.json`, add what it says to that file, then run `just format` and `just check`.
