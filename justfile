# sadr task runner. `just` on its own lists everything.
#
# The toolchain (rust, just and the audit tools) is pinned in mise.toml — run `mise install` once.
# The gate's recipes are in sadr.just, which `sadr init` rewrites. The project's own recipes go
# here, and one named like a recipe there replaces it.

set allow-duplicate-recipes

import 'sadr.just'

# List available recipes
default:
    @just --list --unsorted

# One-time setup: commit guard
setup:
    git config core.hooksPath .githooks
