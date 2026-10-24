//! Runs the built plinth binary the way someone would, against temporary directories.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use tempfile::TempDir;

fn plinth(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_plinth"))
        .args(args)
        .output()
        .expect("plinth should start")
}

/// Every file under `root`, as sorted paths relative to it.
fn tree(root: &Path) -> Vec<PathBuf> {
    fn walk(root: &Path, dir: &Path, out: &mut Vec<PathBuf>) {
        for entry in fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                walk(root, &path, out);
            } else {
                out.push(path.strip_prefix(root).unwrap().to_path_buf());
            }
        }
    }
    let mut out = Vec::new();
    walk(root, root, &mut out);
    out.sort();
    out
}

const MANIFEST: &str = "[package]\nname = \"demo\"\nversion = \"0.1.0\"\nedition = \"2024\"\n";
const MAIN: &str = "fn main() {}\n";

/// A directory named `name` holding what `cargo new` writes, minus the git repo.
fn cargo_project(tmp: &TempDir, name: &str) -> PathBuf {
    let root = tmp.path().join(name);
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(root.join("Cargo.toml"), MANIFEST).unwrap();
    fs::write(root.join("src/main.rs"), MAIN).unwrap();
    fs::write(root.join(".gitignore"), "/target\n").unwrap();
    root
}

const PACKAGE_JSON: &str = r#"{
  "name": "site",
  "type": "module",
  "version": "0.0.1",
  "dependencies": {
    "astro": "^7.4.2"
  }
}
"#;
const ASTRO_AGENTS_MD: &str = "## Development\n\nUse `astro dev --background`.\n";

/// A directory named `name` holding what create-astro writes that plinth looks at, with the
/// lockfile of the package manager that installed it.
fn astro_project(tmp: &TempDir, name: &str, lockfile: &str) -> PathBuf {
    let root = tmp.path().join(name);
    fs::create_dir_all(root.join("src/pages")).unwrap();
    fs::write(root.join("package.json"), PACKAGE_JSON).unwrap();
    fs::write(root.join(lockfile), "").unwrap();
    fs::write(root.join("AGENTS.md"), ASTRO_AGENTS_MD).unwrap();
    fs::write(root.join("src/pages/index.astro"), "<h1>Astro</h1>\n").unwrap();
    fs::write(root.join(".gitignore"), "dist/\n.astro/\nnode_modules/\n").unwrap();
    root
}

fn init_without_setup(root: &Path, extra: &[&str]) -> Output {
    let mut args = vec![
        "init",
        "--no-setup",
        "--no-github",
        "--dir",
        root.to_str().unwrap(),
    ];
    args.extend(extra);
    plinth(&args)
}

fn assert_ok(out: &Output) {
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn adds_the_tooling_and_leaves_the_code_alone() {
    let tmp = TempDir::new().unwrap();
    let root = cargo_project(&tmp, "demo");
    assert_ok(&init_without_setup(&root, &["--description", "A demo"]));

    let expected = [
        ".claude/settings.json",
        ".editorconfig",
        ".gitattributes",
        ".githooks/pre-commit",
        ".github/ISSUE_TEMPLATE/bug.yml",
        ".github/ISSUE_TEMPLATE/config.yml",
        ".github/ISSUE_TEMPLATE/feature.yml",
        ".github/pull_request_template.md",
        ".github/renovate.json5",
        ".github/workflows/ci.yml",
        ".github/workflows/release.yml",
        ".github/workflows/title.yml",
        ".gitignore",
        ".release-please-manifest.json",
        "AGENTS.md",
        "Cargo.toml",
        "README.md",
        "justfile",
        "mise.toml",
        "release-please-config.json",
        "src/main.rs",
    ];
    assert_eq!(tree(&root), expected.map(PathBuf::from));
    assert_eq!(
        fs::read_to_string(root.join("Cargo.toml")).unwrap(),
        MANIFEST
    );
    assert_eq!(fs::read_to_string(root.join("src/main.rs")).unwrap(), MAIN);
    assert!(
        fs::read_to_string(root.join(".release-please-manifest.json"))
            .unwrap()
            .contains("\"0.1.0\"")
    );

    use std::os::unix::fs::PermissionsExt;
    let mode = fs::metadata(root.join(".githooks/pre-commit"))
        .unwrap()
        .permissions()
        .mode();
    assert_eq!(mode & 0o111, 0o111, "the hook must be executable");
}

#[test]
fn keeps_existing_files_and_appends_missing_ignores() {
    let tmp = TempDir::new().unwrap();
    let root = cargo_project(&tmp, "demo");
    fs::write(root.join("README.md"), "# mine\n").unwrap();
    let out = init_without_setup(&root, &[]);
    assert_ok(&out);

    assert_eq!(
        fs::read_to_string(root.join("README.md")).unwrap(),
        "# mine\n"
    );
    let ignore = fs::read_to_string(root.join(".gitignore")).unwrap();
    assert!(ignore.starts_with("/target\n\n# Added by plinth"));
    assert_eq!(ignore.matches("/target").count(), 1);
    assert!(ignore.ends_with("\n*.db\n"));
    assert!(String::from_utf8_lossy(&out.stdout).contains("  README.md"));
}

#[test]
fn says_how_an_existing_claude_md_can_include_agents_md() {
    let tmp = TempDir::new().unwrap();
    let root = cargo_project(&tmp, "demo");
    fs::write(root.join("CLAUDE.md"), "# Notes for Claude\n").unwrap();
    let out = init_without_setup(&root, &["--dry-run"]);
    assert_ok(&out);
    assert!(String::from_utf8_lossy(&out.stdout).contains("add `@AGENTS.md`"));

    fs::write(root.join("CLAUDE.md"), "@AGENTS.md\n\n# Notes for Claude\n").unwrap();
    let out = init_without_setup(&root, &["--dry-run"]);
    assert!(!String::from_utf8_lossy(&out.stdout).contains("@AGENTS.md"));
}

#[test]
fn running_twice_adds_nothing() {
    let tmp = TempDir::new().unwrap();
    let root = cargo_project(&tmp, "demo");
    assert_ok(&init_without_setup(&root, &[]));
    let out = init_without_setup(&root, &["--dry-run"]);
    assert_ok(&out);
    assert!(String::from_utf8_lossy(&out.stdout).starts_with("Nothing to add to "));
}

#[test]
fn refuses_a_directory_without_a_manifest() {
    let tmp = TempDir::new().unwrap();
    fs::write(tmp.path().join("keep.txt"), "mine").unwrap();
    let out = init_without_setup(tmp.path(), &["--name", "demo"]);
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("no Cargo.toml or package.json"));
    assert_eq!(tree(tmp.path()), [PathBuf::from("keep.txt")]);
}

#[test]
fn adds_the_tooling_to_an_astro_project_and_leaves_its_code_alone() {
    let tmp = TempDir::new().unwrap();
    let root = astro_project(&tmp, "site", "package-lock.json");
    assert_ok(&init_without_setup(&root, &[]));

    let expected = [
        ".claude/settings.json",
        ".editorconfig",
        ".gitattributes",
        ".githooks/pre-commit",
        ".github/ISSUE_TEMPLATE/bug.yml",
        ".github/ISSUE_TEMPLATE/config.yml",
        ".github/ISSUE_TEMPLATE/feature.yml",
        ".github/pull_request_template.md",
        ".github/renovate.json5",
        ".github/workflows/ci.yml",
        ".github/workflows/release.yml",
        ".github/workflows/title.yml",
        ".gitignore",
        ".prettierignore",
        ".prettierrc.json",
        ".release-please-manifest.json",
        "AGENTS.md",
        "README.md",
        "eslint.config.mjs",
        "justfile",
        "mise.toml",
        "package-lock.json",
        "package.json",
        "release-please-config.json",
        "src/pages/index.astro",
    ];
    assert_eq!(tree(&root), expected.map(PathBuf::from));
    assert_eq!(
        fs::read_to_string(root.join("package.json")).unwrap(),
        PACKAGE_JSON
    );
    let agents_md = fs::read_to_string(root.join("AGENTS.md")).unwrap();
    assert!(agents_md.starts_with(ASTRO_AGENTS_MD));
    assert!(agents_md.contains("\n## Git and GitHub\n"));
    let ignore = fs::read_to_string(root.join(".gitignore")).unwrap();
    assert!(
        ignore.ends_with("# Added by plinth\n*.tsbuildinfo\n.env\n.env.*\n!.env.example\n*.db\n")
    );
    assert!(
        fs::read_to_string(root.join("release-please-config.json"))
            .unwrap()
            .contains("\"release-type\": \"node\"")
    );
    assert!(
        fs::read_to_string(root.join("justfile"))
            .unwrap()
            .contains("npx --no -- astro check")
    );

    let again = init_without_setup(&root, &["--dry-run"]);
    assert!(String::from_utf8_lossy(&again.stdout).starts_with("Nothing to add to "));
}

#[test]
fn installs_the_astro_gate_with_the_projects_package_manager() {
    let tmp = TempDir::new().unwrap();
    let root = astro_project(&tmp, "site", "pnpm-lock.yaml");
    let out = plinth(&[
        "init",
        "--dry-run",
        "--no-github",
        "--dir",
        root.to_str().unwrap(),
    ]);
    assert_ok(&out);
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("Would append to:\n  .gitignore\n  AGENTS.md\n"));
    assert!(stdout.contains("  mise exec -- pnpm add --save-dev prettier "));
    assert!(
        stdout.contains(
            " typescript@~6.0 prettier-plugin-astro eslint-plugin-astro @astrojs/check\n"
        )
    );
    assert!(stdout.contains(
        "  mise exec -- pnpm exec prettier --write eslint.config.mjs .prettierrc.json\n"
    ));
}

#[test]
fn refuses_a_framework_it_cant_set_up_yet() {
    let tmp = TempDir::new().unwrap();
    let root = astro_project(&tmp, "app", "package-lock.json");
    let next = PACKAGE_JSON.replace(
        "\"astro\": \"^7.4.2\"",
        "\"next\": \"^16\", \"react\": \"^19\"",
    );
    fs::write(root.join("package.json"), &next).unwrap();
    let before = tree(&root);
    let out = init_without_setup(&root, &[]);
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("uses Next.js"));
    assert_eq!(tree(&root), before);
}

#[test]
fn refuses_a_name_github_would_reject_or_the_house_style_wouldnt_use() {
    let tmp = TempDir::new().unwrap();
    let root = cargo_project(&tmp, "demo");
    for name in ["Demo", "1demo", "demo-", "my_demo", "de mo"] {
        let out = init_without_setup(&root, &["--name", name]);
        assert!(!out.status.success(), "{name} should be refused");
    }
    let odd = cargo_project(&tmp, "My_Project");
    let out = init_without_setup(&odd, &[]);
    assert!(String::from_utf8_lossy(&out.stderr).contains("--name"));
}

#[test]
fn dry_run_lists_files_and_commands_but_writes_nothing() {
    let tmp = TempDir::new().unwrap();
    let root = cargo_project(&tmp, "demo");
    let before = tree(&root);
    let out = plinth(&[
        "init",
        "--dry-run",
        "--owner",
        "someone",
        "--dir",
        root.to_str().unwrap(),
    ]);
    assert_ok(&out);
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("  justfile"));
    assert!(stdout.contains("Would append to:\n  .gitignore"));
    assert!(stdout.contains("  git init --quiet --initial-branch=main"));
    assert!(stdout.contains("  mise exec -- cargo generate-lockfile"));
    assert!(
        stdout
            .contains("  gh repo create someone/demo --private --source . --remote origin --push")
    );
    assert!(stdout.contains(
        "  gh api --silent --method POST repos/someone/demo/rulesets --input - <<< (JSON)"
    ));
    assert_eq!(tree(&root), before);
}

#[test]
fn github_needs_the_setup_that_makes_a_git_repo() {
    let tmp = TempDir::new().unwrap();
    let root = cargo_project(&tmp, "demo");
    let out = plinth(&["init", "--no-setup", "--dir", root.to_str().unwrap()]);
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("--no-github"));
}

fn git(root: &Path, args: &[&str]) {
    let status = Command::new("git")
        .args(args)
        .current_dir(root)
        .output()
        .unwrap()
        .status;
    assert!(status.success(), "git {args:?}");
}

#[test]
fn github_refuses_a_repo_it_cant_push_cleanly() {
    let tmp = TempDir::new().unwrap();
    let root = cargo_project(&tmp, "demo");
    let dry_run = || {
        plinth(&[
            "init",
            "--dry-run",
            "--owner",
            "someone",
            "--dir",
            root.to_str().unwrap(),
        ])
    };
    git(&root, &["init", "--quiet"]);
    // No commits yet, as `cargo new` leaves it: fine.
    assert_ok(&dry_run());

    git(&root, &["add", "--all"]);
    git(
        &root,
        &[
            "-c",
            "user.name=t",
            "-c",
            "user.email=t@example.com",
            "commit",
            "--quiet",
            "-m",
            "first",
        ],
    );
    fs::write(root.join("src/main.rs"), "fn main() { todo!() }\n").unwrap();
    let out = dry_run();
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("uncommitted changes"));

    git(&root, &["checkout", "--quiet", "--", "src/main.rs"]);
    git(
        &root,
        &["remote", "add", "origin", "https://example.com/x.git"],
    );
    let out = dry_run();
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("already has a git remote"));
}

/// The ruleset plinth applies requires a status check named `check`, so CI must have that job.
#[test]
fn ci_has_the_job_the_ruleset_requires() {
    let tmp = TempDir::new().unwrap();
    let root = cargo_project(&tmp, "demo");
    assert_ok(&init_without_setup(&root, &[]));
    let ci = fs::read_to_string(root.join(".github/workflows/ci.yml")).unwrap();
    assert!(ci.contains("\njobs:\n  check:\n"));
}

const REPO: &str = env!("CARGO_MANIFEST_DIR");

/// plinth's own repo is a Rust project set up the way plinth sets one up. Setting up plinth's bare
/// crate must reproduce this repo's tooling byte for byte, apart from the files that describe
/// plinth itself, so the templates can't drift from what the repo actually runs.
#[test]
fn setting_up_plinth_reproduces_this_repo() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path().join("plinth");
    fs::create_dir_all(root.join("src")).unwrap();
    let repo = Path::new(REPO);
    fs::copy(repo.join("Cargo.toml"), root.join("Cargo.toml")).unwrap();
    assert_ok(&init_without_setup(
        &root,
        &["--description", env!("CARGO_PKG_DESCRIPTION")],
    ));

    let not_compared = ["Cargo.toml", "AGENTS.md", "README.md"];
    for path in tree(&root) {
        if not_compared.iter().any(|own| path == Path::new(own)) {
            continue;
        }
        let generated = fs::read_to_string(root.join(&path)).unwrap();
        let ours = fs::read_to_string(repo.join(&path)).unwrap_or_else(|_| {
            panic!("plinth generates {} but this repo has none", path.display())
        });
        assert_eq!(
            generated,
            ours,
            "{} differs from this repo's copy",
            path.display()
        );
    }
}

/// And the other way round: plinth run on its own repo finds nothing to add.
#[test]
fn plinth_finds_nothing_to_add_to_this_repo() {
    let out = plinth(&[
        "init",
        "--no-setup",
        "--no-github",
        "--dry-run",
        "--dir",
        REPO,
        "--name",
        "plinth",
    ]);
    assert_ok(&out);
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.starts_with("Nothing to add to "), "{stdout}");
}

/// The whole path from `cargo new`: add the tooling, install the pinned tools, finish the setup,
/// then the project's own gate. Slow and needs mise and the network the first time, so
/// `cargo test` skips it; `just test` and CI run it.
#[test]
#[ignore]
fn a_new_cargo_project_passes_its_own_gate() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path().join("demo");
    let new = Command::new("cargo")
        .args(["new", "--quiet", root.to_str().unwrap()])
        .output()
        .unwrap();
    assert_ok(&new);
    let out = plinth(&["init", "--no-github", "--dir", root.to_str().unwrap()]);
    assert_ok(&out);
    assert!(root.join("Cargo.lock").exists());
    assert!(root.join(".git").exists());

    assert_ok_verbose(&just(&root, "check"));
}

fn just(root: &Path, recipe: &str) -> Output {
    Command::new("mise")
        .args(["exec", "--", "just", recipe])
        .current_dir(root)
        .output()
        .unwrap()
}

/// Like `assert_ok`, but shows stdout too, where `just` recipes print most of what they say.
fn assert_ok_verbose(out: &Output) {
    assert!(
        out.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
}

/// The tools a TypeScript layer's mise.toml pins, such as `node@24.22.1` and `bun@1.4.4`, so the
/// creators run on what the project will, and the tests follow Renovate's bumps of them.
fn pinned_tools(layer: &str) -> Vec<String> {
    let path = Path::new(REPO)
        .join("templates")
        .join(layer)
        .join("mise.toml");
    let pins: toml::Table = fs::read_to_string(path).unwrap().parse().unwrap();
    ["node", "pnpm", "bun"]
        .iter()
        .filter_map(|tool| Some(format!("{tool}@{}", pins["tools"].get(tool)?.as_str()?)))
        .collect()
}

/// Runs `command` in `dir` through mise with `layer`'s tools.
fn with_tools(layer: &str, dir: &Path, command: &[&str]) -> Output {
    let tools = pinned_tools(layer);
    let mut args: Vec<&str> = vec!["exec"];
    args.extend(tools.iter().map(String::as_str));
    args.push("--");
    args.extend(command);
    Command::new("mise")
        .args(&args)
        .current_dir(dir)
        .output()
        .unwrap()
}

/// The whole path from a project's own tool: each of `commands` runs in turn, the first in a fresh
/// directory and the rest in the `site` it creates; then plinth with its setup, then the project's
/// CI recipe, which installs from the lockfile, runs the gate and builds.
fn a_new_project_passes_its_own_ci(layer: &str, commands: &[&[&str]]) {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path().join("site");
    for (i, command) in commands.iter().enumerate() {
        let dir = if i == 0 { tmp.path() } else { root.as_path() };
        assert_ok_verbose(&with_tools(layer, dir, command));
    }
    let out = plinth(&["init", "--no-github", "--dir", root.to_str().unwrap()]);
    assert_ok_verbose(&out);
    assert_ok_verbose(&just(&root, "ci"));
}

const CREATE_ASTRO: [&str; 6] = [
    "--template",
    "minimal",
    "--install",
    "--no-git",
    "--skip-houston",
    "--yes",
];

#[test]
#[ignore]
fn a_new_astro_site_with_npm_passes_its_own_ci() {
    // npm needs the `--` to hand the flags to create-astro.
    let create = [
        &["npm", "create", "astro@latest", "--", "site"][..],
        &CREATE_ASTRO,
    ]
    .concat();
    a_new_project_passes_its_own_ci("typescript", &[&create]);
}

#[test]
#[ignore]
fn a_new_astro_site_with_pnpm_passes_its_own_ci() {
    let create = [
        &["pnpm", "create", "astro@latest", "site"][..],
        &CREATE_ASTRO,
    ]
    .concat();
    a_new_project_passes_its_own_ci("typescript-pnpm", &[&create]);
}

#[test]
#[ignore]
fn a_new_astro_site_with_bun_passes_its_own_ci() {
    let create = [
        &["bun", "create", "astro@latest", "site"][..],
        &CREATE_ASTRO,
    ]
    .concat();
    a_new_project_passes_its_own_ci("typescript-bun", &[&create]);
}

#[test]
#[ignore]
fn a_new_vite_react_app_with_bun_passes_its_own_ci() {
    a_new_project_passes_its_own_ci(
        "typescript-bun",
        &[
            &[
                "bun",
                "create",
                "vite@latest",
                "site",
                "--template",
                "react-ts",
                "--no-interactive",
            ],
            &["bun", "install"],
        ],
    );
}

#[test]
#[ignore]
fn a_new_vite_react_app_with_npm_passes_its_own_ci() {
    a_new_project_passes_its_own_ci(
        "typescript",
        &[
            &[
                "npm",
                "create",
                "vite@latest",
                "site",
                "--",
                "--template",
                "react-ts",
                "--no-interactive",
            ],
            &["npm", "install"],
        ],
    );
}

#[test]
#[ignore]
fn a_new_hono_server_with_bun_passes_its_own_ci() {
    a_new_project_passes_its_own_ci(
        "typescript-bun",
        &[&[
            "bun",
            "create",
            "hono@latest",
            "site",
            "--template",
            "bun",
            "--install",
            "--pm",
            "bun",
        ]],
    );
}

/// `bun init` asks for TypeScript 7, which the gate can't run on yet: plinth refuses and says what
/// to change, and after that change the project passes.
#[test]
#[ignore]
fn a_new_bun_package_passes_its_own_ci_once_it_asks_for_typescript_6() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path().join("site");
    fs::create_dir(&root).unwrap();
    assert_ok_verbose(&with_tools("typescript-bun", &root, &["bun", "init", "-y"]));
    let out = plinth(&["init", "--no-github", "--dir", root.to_str().unwrap()]);
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("Change it to \"~6.0\""));

    let manifest = fs::read_to_string(root.join("package.json")).unwrap();
    let manifest = manifest.replace("\"typescript\": \"^7\"", "\"typescript\": \"~6.0\"");
    fs::write(root.join("package.json"), manifest).unwrap();
    assert_ok_verbose(&with_tools("typescript-bun", &root, &["bun", "install"]));
    let out = plinth(&["init", "--no-github", "--dir", root.to_str().unwrap()]);
    assert_ok_verbose(&out);
    assert_ok_verbose(&just(&root, "ci"));
}

/// A monorepo made the way its packages' own tools make them, with no root: plinth assembles the
/// root, installs once there, and the whole workspace passes one gate.
/// Makes a monorepo's packages with `commands`, sets it up, runs each of `recipes` once as its owner
/// would, then `just ci`.
fn a_new_monorepo_passes_its_own_ci(
    layer: &str,
    commands: &[&[&str]],
    recipes: &[&str],
) -> PathBuf {
    let tmp = TempDir::new().unwrap().keep();
    let root = tmp.join("shop");
    fs::create_dir(&root).unwrap();
    for command in commands {
        assert_ok_verbose(&with_tools(layer, &root, command));
    }
    let out = plinth(&["init", "--no-github", "--dir", root.to_str().unwrap()]);
    assert_ok_verbose(&out);
    for recipe in recipes {
        assert_ok_verbose(&just(&root, recipe));
    }
    assert_ok_verbose(&just(&root, "ci"));
    root
}

#[test]
#[ignore]
fn a_new_bun_monorepo_passes_its_own_ci() {
    let root = a_new_monorepo_passes_its_own_ci(
        "typescript-bun",
        &[
            &[
                "bun",
                "create",
                "vite@latest",
                "apps/web",
                "--template",
                "react-ts",
                "--no-interactive",
            ],
            &[
                "bun",
                "create",
                "hono@latest",
                "apps/api",
                "--template",
                "bun",
                "--install",
                "--pm",
                "bun",
            ],
            &[
                "bun",
                "create",
                "astro@latest",
                "apps/site",
                "--template",
                "minimal",
                "--no-install",
                "--no-git",
                "--skip-houston",
                "--yes",
            ],
            // `bun init` asks for TypeScript 7, which plinth refuses; this is the change it asks for.
            &[
                "sh",
                "-c",
                "mkdir -p packages/core && cd packages/core && bun init -y >/dev/null && \
                sed -i 's/\"typescript\": \"^7\"/\"typescript\": \"~6.0\"/' package.json",
            ],
        ],
        &[],
    );
    let manifest = fs::read_to_string(root.join("package.json")).unwrap();
    assert!(manifest.contains("\"apps/*\""), "{manifest}");
    assert!(!root.join("apps/api/bun.lock").exists());
    assert!(root.join("bun.lock").exists());
    let justfile = fs::read_to_string(root.join("justfile")).unwrap();
    assert!(justfile.contains("cd apps/site && bun x --no-install astro check"));
    assert!(justfile.contains("tsc -b apps/web"));
    assert!(justfile.contains("tsc --noEmit --skipLibCheck -p packages/core"));
    fs::remove_dir_all(root.parent().unwrap()).unwrap();
}

#[test]
#[ignore]
fn a_new_pnpm_monorepo_passes_its_own_ci() {
    let root = a_new_monorepo_passes_its_own_ci(
        "typescript-pnpm",
        &[
            &[
                "pnpm",
                "create",
                "vite@latest",
                "apps/web",
                "--template",
                "react-ts",
                "--no-interactive",
            ],
            &[
                "pnpm",
                "create",
                "hono@latest",
                "apps/api",
                "--template",
                "nodejs",
                "--install",
                "--pm",
                "pnpm",
            ],
            // create-hono's Node template asks for TypeScript 7; this is the change plinth asks for.
            &[
                "sed",
                "-i",
                r#"s/"typescript": "^7[^"]*"/"typescript": "~6.0"/"#,
                "apps/api/package.json",
            ],
        ],
        // create-hono's Node starter isn't Prettier-formatted, and plinth leaves code alone.
        &["format"],
    );
    assert!(root.join("pnpm-workspace.yaml").exists());
    fs::remove_dir_all(root.parent().unwrap()).unwrap();
}

#[test]
#[ignore]
fn a_new_npm_monorepo_passes_its_own_ci() {
    let root = a_new_monorepo_passes_its_own_ci(
        "typescript",
        &[
            &[
                "npm",
                "create",
                "vite@latest",
                "apps/web",
                "--",
                "--template",
                "react-ts",
                "--no-interactive",
            ],
            &["npm", "install", "--prefix", "apps/web"],
            &[
                "npm",
                "create",
                "astro@latest",
                "apps/site",
                "--",
                "--template",
                "minimal",
                "--install",
                "--no-git",
                "--skip-houston",
                "--yes",
            ],
        ],
        &[],
    );
    let justfile = fs::read_to_string(root.join("justfile")).unwrap();
    assert!(justfile.contains("cd apps/site && npx --no -- astro build"));
    fs::remove_dir_all(root.parent().unwrap()).unwrap();
}
