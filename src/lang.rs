use std::fs;
use std::path::Path;

use anyhow::{Context, Result, bail};

/// A language plinth knows how to set up, found from the project's own manifest. Each one has a
/// template layer under templates/ with the same name, written on top of the base layer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Lang {
    Rust,
}

/// What plinth needs to know about the project it is setting up.
#[derive(Debug)]
pub struct Project {
    pub lang: Lang,
    /// The package name, which release-please uses.
    pub package: String,
    /// The current version, where release-please starts counting from.
    pub version: String,
}

impl Lang {
    pub fn name(self) -> &'static str {
        match self {
            Lang::Rust => "rust",
        }
    }

    /// Commands that finish the project once its tools are installed, each with a file whose
    /// presence means it has already run. Each runs through `mise exec`, so it gets the versions
    /// pinned in the project's mise.toml.
    pub fn setup(self) -> &'static [(&'static str, &'static [&'static str])] {
        match self {
            // CI runs cargo with --locked, which fails without a lockfile.
            Lang::Rust => &[("Cargo.lock", &["cargo", "generate-lockfile"])],
        }
    }
}

/// Works out the language from the manifest in `root`.
pub fn detect(root: &Path) -> Result<Project> {
    let manifest = root.join("Cargo.toml");
    if !manifest.exists() {
        bail!(
            "{} has no Cargo.toml; plinth sets up Rust projects so far, so create one first, e.g. \
             with `cargo new`",
            root.display()
        );
    }
    let text = fs::read_to_string(&manifest)?;
    let toml: toml::Table = text
        .parse()
        .with_context(|| format!("couldn't read {}", manifest.display()))?;
    let Some(package) = toml.get("package").and_then(|p| p.as_table()) else {
        bail!(
            "{} has no [package]; workspaces aren't supported yet",
            manifest.display()
        );
    };
    let field = |key: &str| {
        package
            .get(key)
            .and_then(|v| v.as_str())
            .map(str::to_owned)
            .with_context(|| format!("{} has no package {key}", manifest.display()))
    };
    Ok(Project {
        lang: Lang::Rust,
        package: field("name")?,
        version: field("version")?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn project(manifest: &str) -> Result<Project> {
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join("Cargo.toml"), manifest).unwrap();
        detect(tmp.path())
    }

    #[test]
    fn reads_the_package_name_and_version() {
        let p = project("[package]\nname = \"demo\"\nversion = \"0.1.0\"\n").unwrap();
        assert_eq!(p.lang, Lang::Rust);
        assert_eq!(p.package, "demo");
        assert_eq!(p.version, "0.1.0");
    }

    #[test]
    fn refuses_a_workspace_root() {
        let err = project("[workspace]\nmembers = [\"a\"]\n").unwrap_err();
        assert!(err.to_string().contains("workspaces"));
    }

    #[test]
    fn refuses_a_directory_without_a_manifest() {
        let tmp = TempDir::new().unwrap();
        assert!(
            detect(tmp.path())
                .unwrap_err()
                .to_string()
                .contains("cargo new")
        );
    }
}
