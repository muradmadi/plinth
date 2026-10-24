use std::collections::BTreeMap;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use include_dir::{Dir, File, include_dir};
use minijinja::syntax::SyntaxConfig;
use minijinja::{Environment, UndefinedBehavior, context};

use crate::lang::Lang;
use crate::step::Step;

/// Every template, compiled into the binary so a release needs nothing beside it.
static TEMPLATES: Dir = include_dir!("$CARGO_MANIFEST_DIR/templates");

/// What the `.j2` templates can refer to.
pub struct Vars {
    pub name: String,
    pub description: String,
    /// The package name from the project's manifest, which can differ from `name`.
    pub package: String,
    pub version: String,
}

/// Every file plinth adds to a project, keyed by its path inside the project. The base layer goes
/// in first, then the language's, so a language file replaces a base file at the same path. A file
/// ending in `.j2` is rendered and loses the suffix; anything else is copied as it is.
pub fn render(lang: Lang, vars: &Vars) -> Result<BTreeMap<PathBuf, String>> {
    let mut env = Environment::new();
    // A typo in a template variable fails loudly instead of rendering as an empty string.
    env.set_undefined_behavior(UndefinedBehavior::Strict);
    env.set_syntax(
        SyntaxConfig::builder()
            .keep_trailing_newline(true)
            .build()?,
    );
    let ctx = context! {
        name => vars.name.as_str(),
        description => vars.description.as_str(),
        package => vars.package.as_str(),
        version => vars.version.as_str(),
    };

    let mut files = BTreeMap::new();
    for layer in ["base", lang.name()] {
        let dir = TEMPLATES
            .get_dir(layer)
            .with_context(|| format!("no templates for the {layer} layer"))?;
        let mut layer_files = Vec::new();
        files_in(dir, &mut layer_files);
        for file in layer_files {
            let path = file.path().strip_prefix(layer)?;
            let text = file
                .contents_utf8()
                .with_context(|| format!("{} isn't UTF-8", file.path().display()))?;
            let (path, text) = match path.to_str().and_then(|p| p.strip_suffix(".j2")) {
                Some(rendered_path) => {
                    let text = env
                        .render_str(text, ctx.clone())
                        .with_context(|| format!("couldn't render {}", file.path().display()))?;
                    (PathBuf::from(rendered_path), text)
                }
                None => (path.to_path_buf(), text.to_owned()),
            };
            files.insert(path, text);
        }
    }
    Ok(files)
}

fn files_in(dir: &'static Dir<'static>, out: &mut Vec<&'static File<'static>>) {
    out.extend(dir.files());
    for sub in dir.dirs() {
        files_in(sub, out);
    }
}

/// What plinth does with each rendered file, given what the project already has. It never
/// overwrites a file.
#[derive(Debug, Default)]
pub struct Plan {
    /// Files the project doesn't have yet.
    pub add: BTreeMap<PathBuf, String>,
    /// Text to append to an existing file: the `.gitignore` lines it lacks.
    pub append: BTreeMap<PathBuf, String>,
    /// Files the project already has in its own form, left as they are.
    pub keep: Vec<PathBuf>,
}

/// Sorts the rendered files against what is already under `root`.
pub fn plan(root: &Path, files: BTreeMap<PathBuf, String>) -> Result<Plan> {
    let mut plan = Plan::default();
    for (path, text) in files {
        let dest = root.join(&path);
        if !dest.exists() {
            plan.add.insert(path, text);
            continue;
        }
        let existing = fs::read_to_string(&dest)
            .with_context(|| format!("couldn't read {}", dest.display()))?;
        if existing == text {
            continue;
        }
        if path == Path::new(".gitignore") {
            if let Some(extra) = missing_lines(&existing, &text) {
                plan.append.insert(path, extra);
            }
        } else {
            plan.keep.push(path);
        }
    }
    Ok(plan)
}

/// The patterns in `ours` that `existing` lacks, ready to append to it, or None if it has them
/// all. Comments and blank lines in `ours` aren't carried over.
fn missing_lines(existing: &str, ours: &str) -> Option<String> {
    let have: Vec<&str> = existing.lines().map(str::trim).collect();
    let missing: Vec<&str> = ours
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#') && !have.contains(l))
        .collect();
    if missing.is_empty() {
        return None;
    }
    let mut extra = String::new();
    if !existing.is_empty() && !existing.ends_with('\n') {
        extra.push('\n');
    }
    extra.push_str("\n# Added by plinth; .githooks/pre-commit refuses secrets and data too.\n");
    for line in missing {
        extra.push_str(line);
        extra.push('\n');
    }
    Some(extra)
}

/// Writes the plan under `root`, creating directories as needed. Git hooks are made executable,
/// or git would skip them.
pub fn write(root: &Path, plan: &Plan) -> Result<()> {
    for (path, text) in &plan.add {
        let dest = root.join(path);
        if let Some(parent) = dest.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&dest, text).with_context(|| format!("couldn't write {}", dest.display()))?;
        if path.starts_with(".githooks") {
            fs::set_permissions(&dest, fs::Permissions::from_mode(0o755))?;
        }
    }
    for (path, text) in &plan.append {
        let dest = root.join(path);
        OpenOptions::new()
            .append(true)
            .open(&dest)
            .and_then(|mut f| f.write_all(text.as_bytes()))
            .with_context(|| format!("couldn't append to {}", dest.display()))?;
    }
    Ok(())
}

/// The commands that turn the project and the written files into a working setup, in the order
/// they run. Steps the project has already done, such as `git init` after `cargo new`, are left
/// out.
pub fn setup_steps(root: &Path, lang: Lang) -> Vec<Step> {
    let mut steps = Vec::new();
    if !root.join(".git").exists() {
        steps.push(Step::new([
            "git",
            "init",
            "--quiet",
            "--initial-branch=main",
        ]));
    }
    steps.push(Step::new(["mise", "trust"]));
    steps.push(Step::new(["mise", "install"]));
    for (done_when, command) in lang.setup() {
        if !root.join(done_when).exists() {
            steps.push(Step::new(
                ["mise", "exec", "--"].iter().chain(command.iter()).copied(),
            ));
        }
    }
    steps.push(Step::new(["mise", "exec", "--", "just", "setup"]));
    steps
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn vars(description: &str) -> Vars {
        Vars {
            name: "demo".into(),
            description: description.into(),
            package: "demo-cli".into(),
            version: "0.1.0".into(),
        }
    }

    fn rendered() -> BTreeMap<PathBuf, String> {
        render(Lang::Rust, &vars("")).unwrap()
    }

    #[test]
    fn language_files_replace_base_files() {
        // Both layers have a .gitignore; the Rust one also ignores target/.
        assert!(rendered()[Path::new(".gitignore")].starts_with("/target\n"));
    }

    #[test]
    fn templates_render_and_lose_the_suffix() {
        let files = rendered();
        assert!(
            files
                .keys()
                .all(|p| p.extension().is_none_or(|e| e != "j2"))
        );
        assert!(files[Path::new(".release-please-manifest.json")].contains("\".\": \"0.1.0\""));
        assert!(
            files[Path::new("release-please-config.json")]
                .contains("\"package-name\": \"demo-cli\"")
        );
    }

    #[test]
    fn never_touches_the_projects_code() {
        let files = rendered();
        assert!(!files.contains_key(Path::new("Cargo.toml")));
        assert!(!files.keys().any(|p| p.starts_with("src")));
    }

    #[test]
    fn adds_what_is_missing_and_keeps_what_differs() {
        let tmp = TempDir::new().unwrap();
        let files = rendered();
        fs::write(tmp.path().join("README.md"), "# mine\n").unwrap();
        fs::write(tmp.path().join("justfile"), &files[Path::new("justfile")]).unwrap();

        let plan = plan(tmp.path(), files).unwrap();
        assert_eq!(plan.keep, [PathBuf::from("README.md")]);
        assert!(!plan.add.contains_key(Path::new("README.md")));
        // Identical already, so nothing to do.
        assert!(!plan.add.contains_key(Path::new("justfile")));
        assert!(plan.add.contains_key(Path::new(".github/workflows/ci.yml")));
    }

    #[test]
    fn appends_only_the_gitignore_lines_it_lacks() {
        let extra = missing_lines("/target\n.env", "/target\n\n# Secrets\n.env\n*.db\n").unwrap();
        assert_eq!(
            extra,
            "\n\n# Added by plinth; .githooks/pre-commit refuses secrets and data too.\n*.db\n"
        );
        assert_eq!(
            missing_lines("/target\n*.db\n", "/target\n# x\n*.db\n"),
            None
        );
    }

    #[test]
    fn setup_skips_what_the_project_already_did() {
        let tmp = TempDir::new().unwrap();
        let lines = |root: &Path| -> Vec<String> {
            setup_steps(root, Lang::Rust)
                .iter()
                .map(ToString::to_string)
                .collect()
        };
        let fresh = lines(tmp.path());
        assert!(fresh.iter().any(|l| l.starts_with("git init")));
        assert!(fresh.iter().any(|l| l.ends_with("cargo generate-lockfile")));

        fs::create_dir(tmp.path().join(".git")).unwrap();
        fs::write(tmp.path().join("Cargo.lock"), "").unwrap();
        let done = lines(tmp.path());
        assert!(!done.iter().any(|l| l.starts_with("git init")));
        assert!(!done.iter().any(|l| l.ends_with("cargo generate-lockfile")));
    }
}
