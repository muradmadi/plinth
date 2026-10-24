use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

use anyhow::{Context, Result, bail};
use serde_json::Value;

/// A language plinth knows how to set up, found from the project's own manifest. Each one has a
/// template layer under templates/ with the same name, written on top of the base layer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Lang {
    Rust,
    Astro,
}

/// A JavaScript project's package manager, which installs its dependencies and runs its tools.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Pm {
    Npm,
    Pnpm,
    Bun,
}

/// What plinth needs to know about the project it is setting up.
#[derive(Debug)]
pub struct Project {
    pub lang: Lang,
    /// The package name, which release-please uses.
    pub package: String,
    /// The current version, where release-please starts counting from.
    pub version: String,
    /// The package manager, for a JavaScript project.
    pub pm: Option<Pm>,
    /// Every dependency the manifest already names, so setup adds only the ones it lacks.
    deps: BTreeSet<String>,
}

/// The dev dependencies an Astro project's gate runs, as (name, what to install). TypeScript is
/// held at 6: `@astrojs/check` and typescript-eslint don't support 7 yet.
const ASTRO_DEV_DEPS: [(&str, &str); 8] = [
    ("prettier", "prettier"),
    ("prettier-plugin-astro", "prettier-plugin-astro"),
    ("eslint", "eslint"),
    ("@eslint/js", "@eslint/js"),
    ("typescript-eslint", "typescript-eslint"),
    ("eslint-plugin-astro", "eslint-plugin-astro"),
    ("@astrojs/check", "@astrojs/check"),
    ("typescript", "typescript@~6.0"),
];

/// Each package manager's lockfile. bun wrote the binary `bun.lockb` before 1.2.
const LOCKFILES: [(&str, Pm); 4] = [
    ("package-lock.json", Pm::Npm),
    ("pnpm-lock.yaml", Pm::Pnpm),
    ("bun.lock", Pm::Bun),
    ("bun.lockb", Pm::Bun),
];

impl Lang {
    pub fn name(self) -> &'static str {
        match self {
            Lang::Rust => "rust",
            Lang::Astro => "astro",
        }
    }
}

impl Pm {
    pub fn name(self) -> &'static str {
        match self {
            Pm::Npm => "npm",
            Pm::Pnpm => "pnpm",
            Pm::Bun => "bun",
        }
    }

    /// Runs a tool the project has installed, without fetching one it hasn't.
    pub fn exec(self) -> &'static str {
        match self {
            // npx takes flags after the tool name as its own without the `--`.
            Pm::Npm => "npx --no --",
            Pm::Pnpm => "pnpm exec",
            Pm::Bun => "bun x --no-install",
        }
    }

    /// Installs exactly what the lockfile says, failing if it is out of date with package.json.
    pub fn install_frozen(self) -> &'static str {
        match self {
            Pm::Npm => "npm ci",
            Pm::Pnpm => "pnpm install --frozen-lockfile",
            Pm::Bun => "bun install --frozen-lockfile",
        }
    }

    fn add_dev(self) -> &'static [&'static str] {
        match self {
            Pm::Npm => &["npm", "install", "--save-dev"],
            Pm::Pnpm => &["pnpm", "add", "--save-dev"],
            Pm::Bun => &["bun", "add", "--dev"],
        }
    }
}

impl Project {
    /// Commands that finish the project once its tools are installed, leaving out what it already
    /// has. Each runs through `mise exec`, so it gets the versions pinned in the project's
    /// mise.toml.
    pub fn setup(&self, root: &Path) -> Vec<Vec<String>> {
        match self.lang {
            // CI runs cargo with --locked, which fails without a lockfile.
            Lang::Rust if !root.join("Cargo.lock").exists() => {
                vec![vec!["cargo".into(), "generate-lockfile".into()]]
            }
            Lang::Rust => Vec::new(),
            Lang::Astro => {
                let missing: Vec<&str> = ASTRO_DEV_DEPS
                    .iter()
                    .filter(|(name, _)| !self.deps.contains(*name))
                    .map(|&(_, spec)| spec)
                    .collect();
                let Some(pm) = self.pm.filter(|_| !missing.is_empty()) else {
                    return Vec::new();
                };
                vec![
                    pm.add_dev()
                        .iter()
                        .chain(&missing)
                        .map(|s| s.to_string())
                        .collect(),
                ]
            }
        }
    }
}

/// Works out the language from the manifest in `root`.
pub fn detect(root: &Path) -> Result<Project> {
    let cargo = root.join("Cargo.toml");
    let node = root.join("package.json");
    match (cargo.exists(), node.exists()) {
        (true, false) => rust(&cargo),
        (false, true) => astro(root, &node),
        (true, true) => bail!(
            "{} has both a Cargo.toml and a package.json; plinth sets up one language per project",
            root.display()
        ),
        (false, false) => bail!(
            "{} has no Cargo.toml or package.json; create the project with its own tool first, \
             e.g. `mise exec rust -- cargo new` or `npm create astro@latest`",
            root.display()
        ),
    }
}

fn rust(manifest: &Path) -> Result<Project> {
    let text = fs::read_to_string(manifest)?;
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
        pm: None,
        deps: BTreeSet::new(),
    })
}

fn astro(root: &Path, manifest: &Path) -> Result<Project> {
    let json: Value = serde_json::from_str(&fs::read_to_string(manifest)?)
        .with_context(|| format!("couldn't read {}", manifest.display()))?;
    let deps: BTreeSet<String> = ["dependencies", "devDependencies"]
        .iter()
        .filter_map(|key| json.get(key)?.as_object())
        .flat_map(|deps| deps.keys().cloned())
        .collect();
    if !deps.contains("astro") {
        bail!(
            "{} doesn't depend on astro; of JavaScript projects, plinth sets up only Astro ones so \
             far",
            manifest.display()
        );
    }
    let field = |key: &str| {
        json.get(key)
            .and_then(Value::as_str)
            .map(str::to_owned)
            .with_context(|| format!("{} has no \"{key}\"", manifest.display()))
    };
    Ok(Project {
        lang: Lang::Astro,
        package: field("name")?,
        version: field("version")?,
        pm: Some(package_manager(root, &json)?),
        deps,
    })
}

/// The package manager named by package.json's `packageManager` field, or else by the lockfile.
fn package_manager(root: &Path, manifest: &Value) -> Result<Pm> {
    let supported = "plinth supports npm, pnpm and bun";
    if let Some(field) = manifest.get("packageManager").and_then(Value::as_str) {
        let name = field.split('@').next().unwrap_or_default();
        return match name {
            "npm" => Ok(Pm::Npm),
            "pnpm" => Ok(Pm::Pnpm),
            "bun" => Ok(Pm::Bun),
            _ => bail!("package.json's packageManager is {field}; {supported}"),
        };
    }
    if root.join("yarn.lock").exists() {
        bail!("{} uses yarn (yarn.lock); {supported}", root.display());
    }
    let found: Vec<&str> = LOCKFILES
        .iter()
        .filter(|(file, _)| root.join(file).exists())
        .map(|&(file, _)| file)
        .collect();
    let pms: BTreeSet<Pm> = LOCKFILES
        .iter()
        .filter(|(file, _)| found.contains(file))
        .map(|&(_, pm)| pm)
        .collect();
    match pms.into_iter().collect::<Vec<_>>()[..] {
        [pm] => Ok(pm),
        [] => bail!(
            "{} has no lockfile to tell its package manager by; install its dependencies first, \
             e.g. with `npm install`",
            root.display()
        ),
        _ => bail!(
            "{} has lockfiles for more than one package manager ({}); keep the one it uses",
            root.display(),
            found.join(", ")
        ),
    }
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

    /// A directory with `files`, each given as (path, contents).
    fn dir(files: &[(&str, &str)]) -> TempDir {
        let tmp = TempDir::new().unwrap();
        for (path, text) in files {
            fs::write(tmp.path().join(path), text).unwrap();
        }
        tmp
    }

    const ASTRO: &str =
        r#"{"name": "site", "version": "0.0.1", "dependencies": {"astro": "^7.4.2"}}"#;

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
        let err = detect(dir(&[]).path()).unwrap_err().to_string();
        assert!(err.contains("cargo new") && err.contains("create astro"));
    }

    #[test]
    fn finds_an_astro_project_and_its_package_manager() {
        for (lockfile, pm) in [
            ("package-lock.json", Pm::Npm),
            ("pnpm-lock.yaml", Pm::Pnpm),
            ("bun.lock", Pm::Bun),
        ] {
            let tmp = dir(&[("package.json", ASTRO), (lockfile, "")]);
            let p = detect(tmp.path()).unwrap();
            assert_eq!((p.lang, p.pm), (Lang::Astro, Some(pm)));
            assert_eq!((p.package.as_str(), p.version.as_str()), ("site", "0.0.1"));
        }
    }

    #[test]
    fn the_package_manager_field_comes_before_the_lockfile() {
        let manifest = r#"{"name": "site", "version": "0.0.1", "dependencies": {"astro": "^7"},
            "packageManager": "pnpm@12.15.0"}"#;
        let tmp = dir(&[("package.json", manifest), ("package-lock.json", "")]);
        assert_eq!(detect(tmp.path()).unwrap().pm, Some(Pm::Pnpm));
    }

    #[test]
    fn refuses_what_it_cant_set_up() {
        let refusal = |files: &[(&str, &str)]| detect(dir(files).path()).unwrap_err().to_string();
        let react = r#"{"name": "app", "version": "1.0.0", "dependencies": {"react": "^19"}}"#;
        assert!(refusal(&[("package.json", react)]).contains("only Astro"));
        assert!(refusal(&[("package.json", ASTRO), ("yarn.lock", "")]).contains("yarn"));
        assert!(refusal(&[("package.json", ASTRO)]).contains("no lockfile"));
        let two = [
            ("package.json", ASTRO),
            ("package-lock.json", ""),
            ("bun.lock", ""),
        ];
        assert!(refusal(&two).contains("more than one"));
        let both = [("package.json", ASTRO), ("Cargo.toml", "")];
        assert!(refusal(&both).contains("both"));
    }

    #[test]
    fn setup_adds_only_the_dev_dependencies_the_project_lacks() {
        let tmp = dir(&[("package.json", ASTRO), ("pnpm-lock.yaml", "")]);
        let fresh = detect(tmp.path()).unwrap().setup(tmp.path());
        assert_eq!(fresh.len(), 1);
        assert_eq!(fresh[0][..3], ["pnpm", "add", "--save-dev"]);
        assert!(fresh[0].contains(&"typescript@~6.0".to_owned()));

        let all: Vec<String> = ASTRO_DEV_DEPS
            .iter()
            .map(|(name, _)| format!("\"{name}\": \"*\""))
            .collect();
        let manifest = ASTRO.replace(
            r#""dependencies""#,
            &format!(
                r#""devDependencies": {{{}}}, "dependencies""#,
                all.join(", ")
            ),
        );
        fs::write(tmp.path().join("package.json"), manifest).unwrap();
        assert!(detect(tmp.path()).unwrap().setup(tmp.path()).is_empty());
    }
}
