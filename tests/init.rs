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
    assert!(String::from_utf8_lossy(&out.stderr).contains("no Cargo.toml"));
    assert_eq!(tree(tmp.path()), [PathBuf::from("keep.txt")]);
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

    let check = Command::new("mise")
        .args(["exec", "--", "just", "check"])
        .current_dir(&root)
        .output()
        .unwrap();
    assert!(
        check.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&check.stdout),
        String::from_utf8_lossy(&check.stderr)
    );
}
