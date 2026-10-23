use std::collections::BTreeMap;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail};
use include_dir::{Dir, File, include_dir};
use minijinja::syntax::SyntaxConfig;
use minijinja::{Environment, UndefinedBehavior, Value, context};

use crate::stack::Stack;

/// Every template, compiled into the binary so a release needs nothing beside it.
static TEMPLATES: Dir = include_dir!("$CARGO_MANIFEST_DIR/templates");

/// What the `.j2` templates can refer to.
pub struct Vars {
    pub name: String,
    pub description: String,
}

/// Every file of the new project, keyed by its path inside the project. The base layer goes in
/// first, then the stack's, so a stack file replaces a base file at the same path. A file ending in
/// `.j2` is rendered and loses the suffix; anything else is copied as it is.
pub fn render(stack: Stack, vars: &Vars) -> Result<BTreeMap<PathBuf, String>> {
    let mut env = Environment::new();
    // A typo in a template variable fails loudly instead of rendering as an empty string.
    env.set_undefined_behavior(UndefinedBehavior::Strict);
    env.set_syntax(
        SyntaxConfig::builder()
            .keep_trailing_newline(true)
            .build()?,
    );
    env.add_filter("toml_string", toml_string);
    let ctx = context! { name => vars.name.as_str(), description => vars.description.as_str() };

    let mut files = BTreeMap::new();
    for layer in ["base", stack.name()] {
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

/// A TOML basic string. The CLI already refuses line breaks, so quotes and backslashes are all
/// that need escaping.
fn toml_string(value: String) -> Value {
    Value::from(format!(
        "\"{}\"",
        value.replace('\\', "\\\\").replace('"', "\\\"")
    ))
}

/// Writes the files under `root`, creating directories as needed. Git hooks are made executable,
/// or git would skip them.
pub fn write(root: &Path, files: &BTreeMap<PathBuf, String>) -> Result<()> {
    for (path, text) in files {
        let dest = root.join(path);
        if let Some(parent) = dest.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&dest, text).with_context(|| format!("couldn't write {}", dest.display()))?;
        if path.starts_with(".githooks") {
            fs::set_permissions(&dest, fs::Permissions::from_mode(0o755))?;
        }
    }
    Ok(())
}

/// The commands that turn the written files into a working project, in the order they run.
pub fn setup_commands(stack: Stack) -> Vec<Vec<&'static str>> {
    let mut commands = vec![
        vec!["git", "init", "--quiet", "--initial-branch=main"],
        vec!["mise", "trust"],
        vec!["mise", "install"],
    ];
    for command in stack.setup() {
        let mut through_mise = vec!["mise", "exec", "--"];
        through_mise.extend_from_slice(command);
        commands.push(through_mise);
    }
    commands.push(vec!["mise", "exec", "--", "just", "setup"]);
    commands
}

/// Runs each command in `root`, stopping at the first that fails.
pub fn run(root: &Path, commands: &[Vec<&str>]) -> Result<()> {
    for command in commands {
        let line = command.join(" ");
        let status = Command::new(command[0])
            .args(&command[1..])
            .current_dir(root)
            .status()
            .with_context(|| format!("couldn't start `{line}`"))?;
        if !status.success() {
            bail!("`{line}` failed ({status})");
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vars(description: &str) -> Vars {
        Vars {
            name: "demo".into(),
            description: description.into(),
        }
    }

    #[test]
    fn stack_files_replace_base_files() {
        let files = render(Stack::Rust, &vars("")).unwrap();
        // Both layers have a .gitignore; the Rust one also ignores target/.
        assert!(files[Path::new(".gitignore")].starts_with("/target\n"));
    }

    #[test]
    fn templates_render_and_lose_the_suffix() {
        let files = render(Stack::Rust, &vars("")).unwrap();
        assert!(
            files
                .keys()
                .all(|p| p.extension().is_none_or(|e| e != "j2"))
        );
        assert!(files[Path::new("Cargo.toml")].contains("name = \"demo\"\n"));
    }

    #[test]
    fn description_is_escaped_for_toml() {
        let files = render(Stack::Rust, &vars(r#"say "hi" \o/"#)).unwrap();
        assert!(files[Path::new("Cargo.toml")].contains(r#"description = "say \"hi\" \\o/""#));
    }

    #[test]
    fn empty_description_leaves_the_key_out() {
        let files = render(Stack::Rust, &vars("")).unwrap();
        assert!(!files[Path::new("Cargo.toml")].contains("description"));
    }
}
