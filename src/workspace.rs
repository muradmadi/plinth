//! Workspaces of several TypeScript packages: finding the packages an existing root lists, and
//! finding them for a root that doesn't exist yet.

use std::fs;
use std::path::Path;

use anyhow::{Result, bail};
use serde_json::Value;

/// The package globs a workspace root lists: package.json's `workspaces` for npm and bun, or
/// pnpm-workspace.yaml's `packages` for pnpm. None for a single package.
pub fn globs(root: &Path, manifest: &Value) -> Option<Vec<String>> {
    let listed = match manifest.get("workspaces") {
        // Yarn's object form, which npm and bun also accept.
        Some(Value::Object(w)) => w.get("packages"),
        other => other,
    };
    if let Some(Value::Array(globs)) = listed {
        return Some(
            globs
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect(),
        );
    }
    let yaml = fs::read_to_string(root.join("pnpm-workspace.yaml")).ok()?;
    Some(pnpm_packages(&yaml))
}

/// The `packages:` list of a pnpm-workspace.yaml. A line-based read, enough for the plain list pnpm
/// documents; the file's other keys are skipped.
fn pnpm_packages(yaml: &str) -> Vec<String> {
    let mut in_packages = false;
    let mut globs = Vec::new();
    for line in yaml.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        if !line.starts_with([' ', '-']) {
            in_packages = trimmed == "packages:";
            continue;
        }
        if let Some(item) = trimmed.strip_prefix('-').filter(|_| in_packages) {
            globs.push(item.trim().trim_matches(['"', '\'']).to_owned());
        }
    }
    globs
}

/// The settings in members' pnpm-workspace.yaml files, merged for the root's: pnpm reads only the
/// root's inside a workspace, so a member's `allowBuilds` would go unseen there. Each top-level key
/// keeps its first line and gains the indented lines under it that it lacks; `packages` is the
/// root's own. A line-based merge, enough for the lists and flat maps the creators write.
pub fn pnpm_settings(files: &[String]) -> String {
    let mut keys: Vec<(String, Vec<String>)> = Vec::new();
    let mut current = None;
    for line in files.iter().flat_map(|file| file.lines()) {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        if !line.starts_with([' ', '-']) {
            let key = line.split(':').next().unwrap_or(line);
            current = if key == "packages" {
                None
            } else if let Some(i) = keys
                .iter()
                .position(|(k, _)| k.split(':').next() == Some(key))
            {
                Some(i)
            } else {
                keys.push((line.to_owned(), Vec::new()));
                Some(keys.len() - 1)
            };
        } else if let Some(i) = current {
            let children = &mut keys[i].1;
            if !children.iter().any(|c| c == line) {
                children.push(line.to_owned());
            }
        }
    }
    keys.into_iter()
        .flat_map(|(key, children)| [key].into_iter().chain(children))
        .map(|line| line + "\n")
        .collect()
}

/// The package directories `globs` match under `root`, relative to it and sorted. A glob is either
/// a directory or `dir/*`, which every workspace sadr has met uses; anything else is refused.
pub fn expand(root: &Path, globs: &[String]) -> Result<Vec<String>> {
    let mut members = Vec::new();
    for glob in globs {
        let glob = glob.trim_end_matches('/');
        if let Some(parent) = glob.strip_suffix("/*") {
            refuse_wildcards(parent, glob)?;
            members.extend(
                subdirs(&root.join(parent))
                    .into_iter()
                    .map(|dir| format!("{parent}/{dir}"))
                    .filter(|path| root.join(path).join("package.json").exists()),
            );
        } else {
            refuse_wildcards(glob, glob)?;
            if !root.join(glob).join("package.json").exists() {
                bail!("the workspace lists {glob}, which has no package.json");
            }
            members.push(glob.to_owned());
        }
    }
    members.sort();
    members.dedup();
    if members.is_empty() {
        bail!(
            "the workspace's globs ({}) match no package with a package.json",
            globs.join(", ")
        );
    }
    Ok(members)
}

fn refuse_wildcards(path: &str, glob: &str) -> Result<()> {
    if path.contains(['*', '?', '[', '{', '!']) {
        bail!(
            "the workspace glob {glob} is more than a directory or `dir/*`, which is all sadr \
             reads so far"
        );
    }
    Ok(())
}

/// The packages of a monorepo whose root has no package.json yet: each directory one or two levels
/// down with a package.json, as `bun create vite apps/web` leaves them. A directory with a
/// package.json is a package, so sadr doesn't look inside it.
pub fn find_members(root: &Path) -> Vec<String> {
    let mut members = Vec::new();
    for dir in subdirs(root) {
        if root.join(&dir).join("package.json").exists() {
            members.push(dir);
            continue;
        }
        for sub in subdirs(&root.join(&dir)) {
            if root.join(&dir).join(&sub).join("package.json").exists() {
                members.push(format!("{dir}/{sub}"));
            }
        }
    }
    members
}

/// The globs that list `members` in a new root: `apps/*` for packages under `apps/`, and the path
/// itself for a package at the top.
pub fn root_globs(members: &[String]) -> Vec<String> {
    let mut globs: Vec<String> = members
        .iter()
        .map(|path| match path.split_once('/') {
            Some((parent, _)) => format!("{parent}/*"),
            None => path.clone(),
        })
        .collect();
    globs.dedup();
    globs
}

/// The directories in `dir`, sorted, without hidden ones and node_modules.
fn subdirs(dir: &Path) -> Vec<String> {
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut dirs: Vec<String> = entries
        .filter_map(Result::ok)
        .filter(|e| e.path().is_dir())
        .filter_map(|e| e.file_name().into_string().ok())
        .filter(|name| !name.starts_with('.') && name != "node_modules")
        .collect();
    dirs.sort();
    dirs
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use tempfile::TempDir;

    /// A directory holding a package.json at each of `packages`, and an empty `docs/`.
    fn tree(packages: &[&str]) -> TempDir {
        let tmp = TempDir::new().unwrap();
        fs::create_dir(tmp.path().join("docs")).unwrap();
        for path in packages {
            fs::create_dir_all(tmp.path().join(path)).unwrap();
            fs::write(tmp.path().join(path).join("package.json"), "{}").unwrap();
        }
        tmp
    }

    #[test]
    fn reads_the_globs_npm_bun_and_pnpm_list() {
        let tmp = TempDir::new().unwrap();
        let array = json!({"workspaces": ["apps/*", "e2e"]});
        assert_eq!(globs(tmp.path(), &array).unwrap(), ["apps/*", "e2e"]);
        let object = json!({"workspaces": {"packages": ["packages/*"]}});
        assert_eq!(globs(tmp.path(), &object).unwrap(), ["packages/*"]);
        assert_eq!(globs(tmp.path(), &json!({"name": "one"})), None);

        let yaml = "packages:\n  - 'apps/*'\n  - \"packages/*\"\n\ncatalog:\n  - not-a-glob\n";
        fs::write(tmp.path().join("pnpm-workspace.yaml"), yaml).unwrap();
        assert_eq!(
            globs(tmp.path(), &json!({})).unwrap(),
            ["apps/*", "packages/*"]
        );
    }

    #[test]
    fn merges_the_settings_of_members_pnpm_workspace_files() {
        let api = "packages:\n  - .\nonlyBuiltDependencies:\n  - esbuild\nallowBuilds:\n  esbuild: true\n";
        let web = "# builds\nonlyBuiltDependencies:\n  - esbuild\n  - sharp\n";
        assert_eq!(
            pnpm_settings(&[api.to_owned(), web.to_owned()]),
            "onlyBuiltDependencies:\n  - esbuild\n  - sharp\nallowBuilds:\n  esbuild: true\n"
        );
        assert_eq!(pnpm_settings(&[]), "");
    }

    #[test]
    fn expands_directory_globs_to_the_packages_in_them() {
        let tmp = tree(&["apps/web", "apps/api", "e2e", "packages/core"]);
        fs::create_dir_all(tmp.path().join("apps/notes")).unwrap();
        let globs = ["apps/*".to_owned(), "e2e".to_owned()];
        assert_eq!(
            expand(tmp.path(), &globs).unwrap(),
            ["apps/api", "apps/web", "e2e"]
        );
        let deep = expand(tmp.path(), &["packages/**".to_owned()]).unwrap_err();
        assert!(deep.to_string().contains("more than a directory"));
        let missing = expand(tmp.path(), &["docs".to_owned()]).unwrap_err();
        assert!(missing.to_string().contains("no package.json"));
    }

    #[test]
    fn finds_the_packages_of_a_monorepo_without_a_root() {
        let tmp = tree(&["apps/web", "apps/api", "packages/core", "tools"]);
        fs::create_dir_all(tmp.path().join("apps/web/node_modules/vite")).unwrap();
        fs::write(
            tmp.path().join("apps/web/node_modules/vite/package.json"),
            "{}",
        )
        .unwrap();
        let members = find_members(tmp.path());
        assert_eq!(members, ["apps/api", "apps/web", "packages/core", "tools"]);
        assert_eq!(root_globs(&members), ["apps/*", "packages/*", "tools"]);
    }
}
