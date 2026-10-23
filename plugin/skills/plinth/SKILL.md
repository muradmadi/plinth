---
name: plinth
description: Start a new project with the plinth CLI, which writes it from a stack's templates and creates and configures its GitHub repo. Use when the user wants to start, scaffold, create, bootstrap or set up a new project, app, tool or repo, or asks for plinth by name. Not for adding files to a project that already exists.
---

# plinth

plinth writes a new project in one stack, installs its pinned tools, and creates its GitHub repo
with squash-only merges, the label set and a `main` ruleset. It never prompts; every choice is a
flag. Your job is to collect the choices, show the plan, get a yes, and run it.

## 1. Check plinth is there

Run `plinth --version`. If it isn't found, stop and tell the user it installs with
`mise use -g github:muradmadi/plinth`. Don't install it yourself.

## 2. Collect the choices

Run `plinth stacks` for the stacks this version has. Then settle, asking only for what the user
hasn't already said:

- **Name**: lowercase letters, digits and dashes, starting with a letter. It becomes the
  directory, the package name and the repo name. Suggest one if they described the project but
  didn't name it.
- **Stack**: one from `plinth stacks`. If what they want isn't listed, say so and stop. Don't
  substitute a stack they didn't ask for.
- **Description**: one line on what it does. Draft it from what they said and let them change it.
- **Where**: `~/Projects/<name>` unless they say otherwise. It must not exist or must be empty.
- **GitHub**: private unless they ask for public. `--no-github` if they don't want a repo.

## 3. Show the plan

Run it with `--dry-run` first, for example:

```sh
plinth new demo --stack rust --dir ~/Projects/demo --description "What it does" --dry-run
```

Show the user the output: the files, the setup commands, and the `gh` calls. Creating a GitHub
repo is visible outside this machine, so get an explicit yes to this exact plan, including
private or public, before going on.

## 4. Run it

Run the same command without `--dry-run`. It takes a minute or so: it installs the stack's tools
through mise before creating the repo.

If it fails, report the error as plinth printed it. A taken repo name or a non-empty directory is
caught before anything is written, so ask for another name or place. Never delete a directory to
make room.

## 5. Report

Say where the project is and link the repo. Then the two things to know:

- Changes reach `main` only through a PR whose `check` passes.
- After the first release-worthy merge, release-please opens a release PR. GitHub holds its
  workflows until someone clicks "Approve workflows to run" on it; after that it can merge.
