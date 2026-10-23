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

fn new_without_setup(name: &str, dir: &Path, description: &str) -> Output {
    plinth(&[
        "new",
        name,
        "--stack",
        "rust",
        "--no-setup",
        "--no-github",
        "--dir",
        dir.to_str().unwrap(),
        "--description",
        description,
    ])
}

#[test]
fn writes_the_base_and_rust_layers() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path().join("demo");
    let out = new_without_setup("demo", &root, "A demo");
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );

    let expected = [
        ".githooks/pre-commit",
        ".github/ISSUE_TEMPLATE/bug.yml",
        ".github/ISSUE_TEMPLATE/config.yml",
        ".github/ISSUE_TEMPLATE/feature.yml",
        ".github/dependabot.yml",
        ".github/pull_request_template.md",
        ".github/workflows/ci.yml",
        ".github/workflows/release.yml",
        ".github/workflows/title.yml",
        ".gitignore",
        ".release-please-manifest.json",
        "CLAUDE.md",
        "Cargo.toml",
        "README.md",
        "justfile",
        "mise.toml",
        "release-please-config.json",
        "src/main.rs",
    ];
    assert_eq!(tree(&root), expected.map(PathBuf::from));

    use std::os::unix::fs::PermissionsExt;
    let mode = fs::metadata(root.join(".githooks/pre-commit"))
        .unwrap()
        .permissions()
        .mode();
    assert_eq!(mode & 0o111, 0o111, "the hook must be executable");
}

/// plinth's own repo is a Rust project set up the way plinth sets one up. Generating "plinth"
/// must reproduce this repo's files byte for byte, apart from the ones that describe plinth
/// itself or record its releases, so the templates can't drift from what the repo actually runs.
#[test]
fn generating_plinth_reproduces_this_repo() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path().join("plinth");
    let out = new_without_setup("plinth", &root, env!("CARGO_PKG_DESCRIPTION"));
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );

    let not_compared = [
        "Cargo.toml",
        "CLAUDE.md",
        "README.md",
        "src/main.rs",
        // release-please moves it on with every release.
        ".release-please-manifest.json",
    ];
    let repo = Path::new(env!("CARGO_MANIFEST_DIR"));
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

#[test]
fn refuses_a_directory_that_isnt_empty() {
    let tmp = TempDir::new().unwrap();
    fs::write(tmp.path().join("keep.txt"), "mine").unwrap();
    let out = new_without_setup("demo", tmp.path(), "");
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("isn't empty"));
    assert_eq!(tree(tmp.path()), [PathBuf::from("keep.txt")]);
}

#[test]
fn refuses_a_name_github_or_cargo_would_reject() {
    let tmp = TempDir::new().unwrap();
    for name in ["Demo", "1demo", "demo-", "my_demo", "de mo"] {
        let out = new_without_setup(name, &tmp.path().join("x"), "");
        assert!(!out.status.success(), "{name} should be refused");
    }
}

#[test]
fn dry_run_lists_files_and_commands_but_writes_nothing() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path().join("demo");
    let out = plinth(&[
        "new",
        "demo",
        "--stack",
        "rust",
        "--dry-run",
        "--owner",
        "someone",
        "--dir",
        root.to_str().unwrap(),
    ]);
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("  Cargo.toml"));
    assert!(stdout.contains("  mise exec -- cargo generate-lockfile"));
    assert!(
        stdout
            .contains("  gh repo create someone/demo --private --source . --remote origin --push")
    );
    assert!(stdout.contains(
        "  gh api --silent --method POST repos/someone/demo/rulesets --input - <<< (JSON)"
    ));
    assert!(!root.exists());
}

#[test]
fn github_needs_the_setup_that_makes_a_git_repo() {
    let tmp = TempDir::new().unwrap();
    let out = plinth(&[
        "new",
        "demo",
        "--stack",
        "rust",
        "--no-setup",
        "--dir",
        tmp.path().join("demo").to_str().unwrap(),
    ]);
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("--no-github"));
}

/// The ruleset plinth applies requires a status check named `check`, so CI must have that job.
#[test]
fn ci_has_the_job_the_ruleset_requires() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path().join("demo");
    assert!(new_without_setup("demo", &root, "").status.success());
    let ci = fs::read_to_string(root.join(".github/workflows/ci.yml")).unwrap();
    assert!(ci.contains("\njobs:\n  check:\n"));
}

/// The whole path: write, install the pinned tools, finish the stack's setup, then the new
/// project's own gate. Slow and needs mise and the network the first time, so `cargo test` skips
/// it; `just test` and CI run it.
#[test]
#[ignore]
fn a_new_rust_project_passes_its_own_gate() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path().join("demo");
    let out = plinth(&[
        "new",
        "demo",
        "--stack",
        "rust",
        "--no-github",
        "--dir",
        root.to_str().unwrap(),
    ]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
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
