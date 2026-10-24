use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use serde_json::{Map, Value};

use crate::workspace;

/// A language plinth knows how to set up, found from the project's own manifest. Each one has a
/// template layer under templates/ with the same name, written on top of the base layer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Lang {
    Rust,
    TypeScript,
}

/// A JavaScript project's package manager, which installs its dependencies and runs its tools.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Pm {
    Npm,
    Pnpm,
    Bun,
}

/// What a TypeScript package is, read from its dependencies. Each kind adds its own checks, lint
/// rules and build to the gate.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// An Astro site.
    Astro,
    /// A React app built with Vite.
    Web,
    /// A Hono server.
    Api,
    /// Anything else: a library, a CLI, a script.
    Lib,
}

/// How a package's TypeScript is set up, which decides how `just typecheck` runs tsc on it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tsconfig {
    None,
    /// One tsconfig.json covering the package.
    Plain,
    /// A tsconfig.json that only references others, as create-vite writes; `tsc -b` follows them.
    Solution,
}

/// One package of a TypeScript project, at `path` from the root: `.` for a single package.
#[derive(Clone, Debug)]
pub struct Member {
    pub path: String,
    pub kind: Kind,
    pub tsconfig: Tsconfig,
}

/// What plinth needs to know about the project it is setting up.
#[derive(Debug)]
pub struct Project {
    pub lang: Lang,
    /// The package name, which release-please uses.
    pub package: String,
    /// The current version, where release-please starts counting from.
    pub version: String,
    /// The package manager, for a TypeScript project.
    pub pm: Option<Pm>,
    /// The packages of a TypeScript project.
    pub members: Vec<Member>,
    /// Whether the root is a workspace of several packages rather than a package itself.
    pub workspace: bool,
    /// What plinth does to turn packages without a root into a workspace.
    pub assembly: Option<Assembly>,
    /// Every dependency the root manifest already names, so setup adds only the ones it lacks.
    deps: BTreeSet<String>,
}

/// The changes that make a workspace of packages that each came from their own tool: a root that
/// lists them, a name for any package without one, and one install at the root in place of each
/// package's own.
#[derive(Debug, Default)]
pub struct Assembly {
    /// New files at the root: package.json, and pnpm-workspace.yaml for pnpm.
    pub create: Vec<(PathBuf, String)>,
    /// Package manifests given a name, as (path, new text).
    pub rename: Vec<(PathBuf, String)>,
    /// Each package's lockfile and node_modules, which the root install replaces.
    pub remove: Vec<PathBuf>,
}

/// A dev dependency of the gate: its name, what to install, and the file plinth writes that runs
/// it. A project that keeps its own file there keeps its own tools for it too.
type DevDep = (&'static str, &'static str, &'static str);

const PRETTIER: &str = ".prettierrc.json";
const ESLINT: &str = "plinth.eslint.mjs";
const JUSTFILE: &str = "plinth.just";

/// The dev dependencies every TypeScript gate runs. TypeScript is held at 6: typescript-eslint and
/// `@astrojs/check` don't support 7 yet.
const TS_DEV_DEPS: [DevDep; 6] = [
    ("prettier", "prettier", PRETTIER),
    ("eslint", "eslint", ESLINT),
    ("@eslint/js", "@eslint/js", ESLINT),
    ("typescript-eslint", "typescript-eslint", ESLINT),
    ("eslint-config-prettier", "eslint-config-prettier", ESLINT),
    ("typescript", "typescript@~6.0", JUSTFILE),
];

/// What a React app adds to them.
const WEB_DEV_DEPS: [DevDep; 2] = [
    (
        "eslint-plugin-react-hooks",
        "eslint-plugin-react-hooks",
        ESLINT,
    ),
    ("globals", "globals", ESLINT),
];

/// What an Astro site adds to them.
const ASTRO_DEV_DEPS: [DevDep; 3] = [
    ("prettier-plugin-astro", "prettier-plugin-astro", PRETTIER),
    ("eslint-plugin-astro", "eslint-plugin-astro", ESLINT),
    ("@astrojs/check", "@astrojs/check", JUSTFILE),
];

/// Frameworks plinth recognises but can't set up yet, as (dependency, name).
const UNSUPPORTED: [(&str, &str); 11] = [
    ("next", "Next.js"),
    ("nuxt", "Nuxt"),
    ("vue", "Vue"),
    ("@sveltejs/kit", "SvelteKit"),
    ("svelte", "Svelte"),
    ("expo", "Expo"),
    ("react-native", "React Native"),
    ("@angular/core", "Angular"),
    ("solid-js", "Solid"),
    ("@remix-run/react", "Remix"),
    ("@builder.io/qwik", "Qwik"),
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
            Lang::TypeScript => "typescript",
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

impl Kind {
    pub fn name(self) -> &'static str {
        match self {
            Kind::Astro => "astro",
            Kind::Web => "web",
            Kind::Api => "api",
            Kind::Lib => "lib",
        }
    }
}

impl Tsconfig {
    pub fn name(self) -> &'static str {
        match self {
            Tsconfig::None => "none",
            Tsconfig::Plain => "plain",
            Tsconfig::Solution => "solution",
        }
    }
}

impl Member {
    /// What goes before a path inside the member in a glob: nothing for the root package.
    pub fn prefix(&self) -> String {
        match self.path.as_str() {
            "." => String::new(),
            path => format!("{path}/"),
        }
    }
}

impl Project {
    pub fn has(&self, kind: Kind) -> bool {
        self.members.iter().any(|m| m.kind == kind)
    }

    /// Commands that finish the project once its tools are installed, leaving out what it already
    /// has. `writes` says whether plinth is adding a file, which decides the dev dependencies that
    /// file's tool needs. Each runs through `mise exec`, so it gets the versions pinned in the
    /// project's mise.toml.
    pub fn setup(&self, root: &Path, writes: impl Fn(&str) -> bool) -> Vec<Vec<String>> {
        match self.lang {
            // CI runs cargo with --locked, which fails without a lockfile.
            Lang::Rust if !root.join("Cargo.lock").exists() => {
                vec![vec!["cargo".into(), "generate-lockfile".into()]]
            }
            Lang::Rust => Vec::new(),
            Lang::TypeScript => {
                let Some(pm) = self.pm else {
                    return Vec::new();
                };
                let mut commands = Vec::new();
                if self.assembly.is_some() {
                    commands.push(vec![pm.name().to_owned(), "install".to_owned()]);
                }
                let missing: Vec<&str> = self
                    .dev_deps()
                    .filter(|&&(name, _, file)| writes(file) && !self.deps.contains(name))
                    .map(|&(_, spec, _)| spec)
                    .collect();
                if !missing.is_empty() {
                    let mut add = pm.add_dev().to_vec();
                    // pnpm refuses to add to a workspace root unless told that's meant.
                    if self.workspace && pm == Pm::Pnpm {
                        add.push("--workspace-root");
                    }
                    commands.push(add.into_iter().chain(missing).map(String::from).collect());
                }
                commands
            }
        }
    }

    /// The dev dependencies the gate needs for this project's kinds of package.
    fn dev_deps(&self) -> impl Iterator<Item = &'static DevDep> {
        let web: &[_] = if self.has(Kind::Web) {
            &WEB_DEV_DEPS
        } else {
            &[]
        };
        let astro: &[_] = if self.has(Kind::Astro) {
            &ASTRO_DEV_DEPS
        } else {
            &[]
        };
        TS_DEV_DEPS.iter().chain(web).chain(astro)
    }
}

/// Works out the language from the manifest in `root`. `name` names the root package plinth writes
/// for a monorepo that has none.
pub fn detect(root: &Path, name: &str) -> Result<Project> {
    let cargo = root.join("Cargo.toml");
    let node = root.join("package.json");
    match (cargo.exists(), node.exists()) {
        (true, false) => rust(&cargo),
        (false, true) => typescript(root, &node),
        (true, true) => bail!(
            "{} has both a Cargo.toml and a package.json; plinth sets up one language per project",
            root.display()
        ),
        (false, false) if !workspace::find_members(root).is_empty() => assemble(root, name),
        (false, false) => bail!(
            "{} has no Cargo.toml or package.json; create the project with its own tool first, \
             e.g. `mise exec rust -- cargo new`, `npm create astro@latest` or `bun create vite`",
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
        members: Vec::new(),
        workspace: false,
        assembly: None,
        deps: BTreeSet::new(),
    })
}

fn read_manifest(manifest: &Path) -> Result<Value> {
    serde_json::from_str(&fs::read_to_string(manifest)?)
        .with_context(|| format!("couldn't read {}", manifest.display()))
}

/// A single package, or a workspace root that lists its packages.
fn typescript(root: &Path, manifest: &Path) -> Result<Project> {
    let json = read_manifest(manifest)?;
    refuse_typescript_7(&json, manifest)?;
    let deps = dependencies(&json, &["dependencies", "devDependencies"]);
    let (members, workspace) = match workspace::globs(root, &json) {
        Some(globs) => {
            let members = workspace::expand(root, &globs)?
                .into_iter()
                .map(|path| member(root, path))
                .collect::<Result<_>>()?;
            (members, true)
        }
        None => {
            let single = Member {
                path: ".".into(),
                kind: classify(&deps, manifest)?,
                tsconfig: tsconfig(root),
            };
            (vec![single], false)
        }
    };
    let field = |key: &str| json.get(key).and_then(Value::as_str).map(str::to_owned);
    Ok(Project {
        lang: Lang::TypeScript,
        // create-hono writes neither; the directory's name and an unreleased version stand in.
        package: field("name")
            .or_else(|| Some(root.file_name()?.to_str()?.to_owned()))
            .context("couldn't name the package after its directory")?,
        version: field("version").unwrap_or_else(|| "0.0.0".into()),
        pm: Some(package_manager(root, &json)?),
        members,
        workspace,
        assembly: None,
        deps,
    })
}

/// One package of a workspace, at `path` from the root.
fn member(root: &Path, path: String) -> Result<Member> {
    let dir = root.join(&path);
    let manifest = dir.join("package.json");
    let json = read_manifest(&manifest)?;
    refuse_typescript_7(&json, &manifest)?;
    let deps = dependencies(&json, &["dependencies", "devDependencies"]);
    Ok(Member {
        kind: classify(&deps, &manifest)?,
        tsconfig: tsconfig(&dir),
        path,
    })
}

/// A monorepo whose packages each came from their own tool, with no root package.json yet: the
/// workspace plinth makes of them.
fn assemble(root: &Path, name: &str) -> Result<Project> {
    let paths = workspace::find_members(root);
    let mut pms = BTreeSet::new();
    let mut assembly = Assembly::default();
    let mut members = Vec::new();
    let mut pnpm_files = Vec::new();
    for path in paths {
        let dir = root.join(&path);
        if let Ok(yaml) = fs::read_to_string(dir.join("pnpm-workspace.yaml")) {
            pnpm_files.push((PathBuf::from(&path).join("pnpm-workspace.yaml"), yaml));
        }
        let mut json = read_manifest(&dir.join("package.json"))?;
        pms.extend(declared_pm(&dir, &json)?);
        assembly.remove.extend(
            LOCKFILES
                .iter()
                .map(|(file, _)| PathBuf::from(&path).join(file))
                .chain([PathBuf::from(&path).join("node_modules")])
                .filter(|p| root.join(p).exists()),
        );
        if let Some(fields) = json.as_object_mut().filter(|o| !o.contains_key("name")) {
            let dir_name = path.rsplit('/').next().unwrap_or(&path);
            let mut named = Map::new();
            named.insert("name".into(), format!("@{name}/{dir_name}").into());
            named.extend(std::mem::take(fields));
            let text = serde_json::to_string_pretty(&named)? + "\n";
            assembly
                .rename
                .push((PathBuf::from(&path).join("package.json"), text));
        }
        members.push(member(root, path)?);
    }
    let pm = match pms.into_iter().collect::<Vec<_>>()[..] {
        [pm] => pm,
        [] => bail!(
            "{} has packages but no root package.json, and none of the packages has a lockfile to \
             tell the package manager by; install one of them first, e.g. with `bun install`",
            root.display()
        ),
        ref several => bail!(
            "{}'s packages use more than one package manager ({}); a workspace has one",
            root.display(),
            several
                .iter()
                .map(|pm| pm.name())
                .collect::<Vec<_>>()
                .join(", ")
        ),
    };

    let paths: Vec<String> = members.iter().map(|m| m.path.clone()).collect();
    let globs = workspace::root_globs(&paths);
    let mut manifest = Map::new();
    manifest.insert("name".into(), name.into());
    manifest.insert("version".into(), "0.0.0".into());
    manifest.insert("private".into(), true.into());
    manifest.insert("type".into(), "module".into());
    if pm == Pm::Pnpm {
        let yaml: String = globs.iter().map(|g| format!("  - \"{g}\"\n")).collect();
        let (paths, files): (Vec<_>, Vec<_>) = pnpm_files.into_iter().unzip();
        let settings = workspace::pnpm_settings(&files);
        assembly.remove.extend(paths);
        assembly.create.push((
            "pnpm-workspace.yaml".into(),
            format!("packages:\n{yaml}{settings}"),
        ));
    } else {
        manifest.insert("workspaces".into(), globs.into());
    }
    let text = serde_json::to_string_pretty(&manifest)? + "\n";
    assembly.create.push(("package.json".into(), text));

    Ok(Project {
        lang: Lang::TypeScript,
        package: name.to_owned(),
        version: "0.0.0".into(),
        pm: Some(pm),
        members,
        workspace: true,
        assembly: Some(assembly),
        deps: BTreeSet::new(),
    })
}

/// The names of every dependency listed under `keys` in a package.json.
fn dependencies(json: &Value, keys: &[&str]) -> BTreeSet<String> {
    keys.iter()
        .filter_map(|key| json.get(key)?.as_object())
        .flat_map(|deps| deps.keys().cloned())
        .collect()
}

fn classify(deps: &BTreeSet<String>, manifest: &Path) -> Result<Kind> {
    // An Astro site may carry React or Svelte islands; it is still an Astro site.
    if deps.contains("astro") {
        return Ok(Kind::Astro);
    }
    if let Some((_, name)) = UNSUPPORTED.iter().find(|(dep, _)| deps.contains(*dep)) {
        bail!(
            "{} uses {name}, which plinth can't set up yet; it sets up Astro sites, React apps \
             built with Vite, Hono servers and plain TypeScript packages",
            manifest.display()
        );
    }
    let has = |dep: &str| deps.contains(dep);
    match (has("react"), has("vite"), has("hono")) {
        (true, true, _) => Ok(Kind::Web),
        (true, false, _) => bail!(
            "{} uses React without Vite; plinth sets up React apps built with Vite so far (`npm \
             create vite`)",
            manifest.display()
        ),
        (false, _, true) => Ok(Kind::Api),
        _ => Ok(Kind::Lib),
    }
}

/// The gate's type-aware lint and `astro check` run on TypeScript 6; with a project asking for 7,
/// the package manager installs both and typescript-eslint breaks.
fn refuse_typescript_7(json: &Value, manifest: &Path) -> Result<()> {
    for key in ["dependencies", "devDependencies", "peerDependencies"] {
        let Some(range) = json.get(key).and_then(|d| d.get("typescript")?.as_str()) else {
            continue;
        };
        let bare = range.trim_start_matches(['^', '~', '>', '=', ' ']);
        if bare.starts_with('7') || range == "latest" {
            bail!(
                "{}'s {key} ask for TypeScript {range}{}; the gate's lint and type checks need \
                 TypeScript 6 until typescript-eslint supports 7. Change it to \"~6.0\", then run \
                 plinth again",
                manifest.display(),
                if key == "peerDependencies" {
                    ", as `bun init` writes it"
                } else {
                    ""
                }
            );
        }
    }
    Ok(())
}

fn tsconfig(dir: &Path) -> Tsconfig {
    match fs::read_to_string(dir.join("tsconfig.json")) {
        // A substring check, as tsconfig.json may hold comments a JSON parser would reject.
        Ok(text) if text.contains("\"references\"") => Tsconfig::Solution,
        Ok(_) => Tsconfig::Plain,
        Err(_) => Tsconfig::None,
    }
}

/// The package manager a project in `root` uses; refused if it can't be told.
fn package_manager(root: &Path, manifest: &Value) -> Result<Pm> {
    declared_pm(root, manifest)?.with_context(|| {
        format!(
            "{} has no lockfile to tell its package manager by; install its dependencies first, \
             e.g. with `npm install`",
            root.display()
        )
    })
}

/// The package manager named by package.json's `packageManager` field, or else by the lockfile in
/// `root`; None if there is neither.
fn declared_pm(root: &Path, manifest: &Value) -> Result<Option<Pm>> {
    let supported = "plinth supports npm, pnpm and bun";
    if let Some(field) = manifest.get("packageManager").and_then(Value::as_str) {
        let name = field.split('@').next().unwrap_or_default();
        return match name {
            "npm" => Ok(Some(Pm::Npm)),
            "pnpm" => Ok(Some(Pm::Pnpm)),
            "bun" => Ok(Some(Pm::Bun)),
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
        [pm] => Ok(Some(pm)),
        [] => Ok(None),
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
        detect(tmp.path(), "demo")
    }

    /// A directory with `files`, each given as (path, contents).
    fn dir(files: &[(&str, &str)]) -> TempDir {
        let tmp = TempDir::new().unwrap();
        for (path, text) in files {
            let path = tmp.path().join(path);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, text).unwrap();
        }
        tmp
    }

    /// A package.json with `deps` as its dependencies.
    fn manifest(deps: &str) -> String {
        format!(r#"{{"name": "site", "version": "0.0.1", "dependencies": {{{deps}}}}}"#)
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
    fn refuses_a_cargo_workspace_root() {
        let err = project("[workspace]\nmembers = [\"a\"]\n").unwrap_err();
        assert!(err.to_string().contains("workspaces"));
    }

    const WEB: &str = r#"{"name": "web", "dependencies": {"react": "^19", "vite": "^8"}}"#;
    const API: &str = r#"{"dependencies": {"hono": "^4"}}"#;

    #[test]
    fn reads_the_packages_of_a_workspace() {
        let root = r#"{"name": "shop", "version": "1.2.0", "workspaces": ["apps/*", "packages/*"],
            "devDependencies": {"prettier": "^3"}}"#;
        let tmp = dir(&[
            ("package.json", root),
            ("bun.lock", ""),
            ("apps/web/package.json", WEB),
            ("apps/api/package.json", API),
            ("packages/core/package.json", r#"{"name": "@shop/core"}"#),
        ]);
        let p = detect(tmp.path(), "demo").unwrap();
        assert!(p.workspace && p.assembly.is_none());
        assert_eq!((p.package.as_str(), p.version.as_str()), ("shop", "1.2.0"));
        let members: Vec<(&str, Kind)> = p
            .members
            .iter()
            .map(|m| (m.path.as_str(), m.kind))
            .collect();
        assert_eq!(
            members,
            [
                ("apps/api", Kind::Api),
                ("apps/web", Kind::Web),
                ("packages/core", Kind::Lib)
            ]
        );
        let add = &p.setup(tmp.path(), |_| true)[0];
        assert!(add.contains(&"eslint-plugin-react-hooks".to_owned()));
        assert!(!add.contains(&"prettier".to_owned()));
    }

    #[test]
    fn names_the_package_a_workspace_member_refuses_for() {
        let tmp = dir(&[
            (
                "package.json",
                r#"{"name": "app", "workspaces": ["apps/*"]}"#,
            ),
            ("pnpm-lock.yaml", ""),
            (
                "apps/mobile/package.json",
                r#"{"dependencies": {"expo": "^55"}}"#,
            ),
        ]);
        let err = detect(tmp.path(), "demo").unwrap_err().to_string();
        assert!(err.contains("apps/mobile/package.json uses Expo"), "{err}");
    }

    #[test]
    fn pnpm_adds_to_a_workspace_root_only_when_told() {
        let tmp = dir(&[
            ("package.json", r#"{"name": "shop"}"#),
            ("pnpm-workspace.yaml", "packages:\n  - apps/*\n"),
            ("pnpm-lock.yaml", ""),
            ("apps/web/package.json", WEB),
        ]);
        let add = &detect(tmp.path(), "demo")
            .unwrap()
            .setup(tmp.path(), |_| true)[0];
        assert_eq!(add[..4], ["pnpm", "add", "--save-dev", "--workspace-root"]);
    }

    #[test]
    fn assembles_a_root_for_packages_made_by_their_own_tools() {
        let tmp = dir(&[
            ("apps/web/package.json", WEB),
            ("apps/web/bun.lock", ""),
            ("apps/web/node_modules/vite/package.json", "{}"),
            ("apps/api/package.json", API),
            ("apps/api/bun.lock", ""),
        ]);
        let p = detect(tmp.path(), "shop").unwrap();
        assert_eq!((p.package.as_str(), p.version.as_str()), ("shop", "0.0.0"));
        assert_eq!(p.pm, Some(Pm::Bun));
        let a = p.assembly.as_ref().unwrap();
        let (path, root) = &a.create[0];
        assert_eq!(path, Path::new("package.json"));
        assert!(
            root.contains("\"workspaces\": [\n    \"apps/*\"\n  ]"),
            "{root}"
        );
        // Only the unnamed package is renamed, with its name first and the rest kept.
        assert_eq!(a.rename.len(), 1);
        let (path, api) = &a.rename[0];
        assert_eq!(path, Path::new("apps/api/package.json"));
        assert!(
            api.starts_with("{\n  \"name\": \"@shop/api\",\n  \"dependencies\""),
            "{api}"
        );
        assert_eq!(
            a.remove,
            [
                PathBuf::from("apps/api/bun.lock"),
                PathBuf::from("apps/web/bun.lock"),
                PathBuf::from("apps/web/node_modules")
            ]
        );
        let setup = p.setup(tmp.path(), |_| true);
        assert_eq!(setup[0], ["bun", "install"]);
    }

    #[test]
    fn a_pnpm_root_lists_its_packages_in_pnpm_workspace_yaml() {
        let tmp = dir(&[
            ("apps/web/package.json", WEB),
            ("apps/web/pnpm-lock.yaml", ""),
            ("apps/api/package.json", API),
            ("apps/api/pnpm-lock.yaml", ""),
            (
                "apps/api/pnpm-workspace.yaml",
                "allowBuilds:\n  esbuild: true\n",
            ),
        ]);
        let a = detect(tmp.path(), "shop").unwrap().assembly.unwrap();
        // pnpm reads only the root's settings in a workspace, so the member's move up.
        assert_eq!(
            a.create[0],
            (
                "pnpm-workspace.yaml".into(),
                "packages:\n  - \"apps/*\"\nallowBuilds:\n  esbuild: true\n".into()
            )
        );
        assert!(
            a.remove
                .contains(&PathBuf::from("apps/api/pnpm-workspace.yaml"))
        );
        assert!(!a.create[1].1.contains("workspaces"));
    }

    #[test]
    fn an_assembled_root_needs_one_package_manager() {
        let mixed = dir(&[
            ("apps/web/package.json", WEB),
            ("apps/web/bun.lock", ""),
            ("apps/api/package.json", API),
            ("apps/api/package-lock.json", ""),
        ]);
        let err = detect(mixed.path(), "shop").unwrap_err().to_string();
        assert!(
            err.contains("more than one package manager (npm, bun)"),
            "{err}"
        );
        let none = dir(&[("apps/web/package.json", WEB)]);
        assert!(
            detect(none.path(), "shop")
                .unwrap_err()
                .to_string()
                .contains("lockfile")
        );
    }

    #[test]
    fn refuses_a_directory_without_a_manifest() {
        let err = detect(dir(&[]).path(), "demo").unwrap_err().to_string();
        assert!(err.contains("cargo new") && err.contains("create astro"));
    }

    #[test]
    fn finds_a_typescript_project_and_its_package_manager() {
        for (lockfile, pm) in [
            ("package-lock.json", Pm::Npm),
            ("pnpm-lock.yaml", Pm::Pnpm),
            ("bun.lock", Pm::Bun),
        ] {
            let tmp = dir(&[("package.json", ASTRO), (lockfile, "")]);
            let p = detect(tmp.path(), "demo").unwrap();
            assert_eq!((p.lang, p.pm), (Lang::TypeScript, Some(pm)));
            assert_eq!((p.package.as_str(), p.version.as_str()), ("site", "0.0.1"));
        }
    }

    #[test]
    fn tells_the_kind_of_package_from_its_dependencies() {
        let kind = |deps: &str| {
            let tmp = dir(&[("package.json", &manifest(deps)), ("bun.lock", "")]);
            detect(tmp.path(), "demo").unwrap().members[0].kind
        };
        assert_eq!(kind(r#""astro": "^7", "react": "^19""#), Kind::Astro);
        assert_eq!(kind(r#""react": "^19", "vite": "^8""#), Kind::Web);
        assert_eq!(kind(r#""hono": "^4""#), Kind::Api);
        assert_eq!(kind(r#""zod": "^4""#), Kind::Lib);
    }

    #[test]
    fn tells_a_solution_tsconfig_from_a_plain_one() {
        let solution = r#"{ "files": [], "references": [{ "path": "./tsconfig.app.json" }] }"#;
        for (text, expected) in [
            (Some(solution), Tsconfig::Solution),
            (
                Some("{ /* strict */ \"compilerOptions\": {} }"),
                Tsconfig::Plain,
            ),
            (None, Tsconfig::None),
        ] {
            let tmp = dir(&[("package.json", ASTRO), ("bun.lock", "")]);
            if let Some(text) = text {
                fs::write(tmp.path().join("tsconfig.json"), text).unwrap();
            }
            assert_eq!(
                detect(tmp.path(), "demo").unwrap().members[0].tsconfig,
                expected
            );
        }
    }

    #[test]
    fn names_an_unnamed_package_after_its_directory() {
        let hono = r#"{"dependencies": {"hono": "^4"}}"#;
        let tmp = dir(&[("package.json", hono), ("bun.lock", "")]);
        let p = detect(tmp.path(), "demo").unwrap();
        let dir_name = tmp.path().file_name().unwrap().to_str().unwrap();
        assert_eq!(
            (p.package.as_str(), p.version.as_str()),
            (dir_name, "0.0.0")
        );
    }

    #[test]
    fn the_package_manager_field_comes_before_the_lockfile() {
        let manifest = r#"{"name": "site", "version": "0.0.1", "dependencies": {"astro": "^7"},
            "packageManager": "pnpm@12.15.0"}"#;
        let tmp = dir(&[("package.json", manifest), ("package-lock.json", "")]);
        assert_eq!(detect(tmp.path(), "demo").unwrap().pm, Some(Pm::Pnpm));
    }

    #[test]
    fn refuses_what_it_cant_set_up() {
        let refusal =
            |files: &[(&str, &str)]| detect(dir(files).path(), "demo").unwrap_err().to_string();
        let with = |deps: &str| refusal(&[("package.json", &manifest(deps)), ("bun.lock", "")]);
        assert!(with(r#""next": "^16", "react": "^19""#).contains("uses Next.js"));
        assert!(with(r#""vue": "^3", "vite": "^8""#).contains("uses Vue"));
        assert!(with(r#""react": "^19""#).contains("without Vite"));
        let ts7 = r#"{"name": "lib", "peerDependencies": {"typescript": "^7"}}"#;
        let err = refusal(&[("package.json", ts7), ("bun.lock", "")]);
        assert!(err.contains("TypeScript ^7") && err.contains("bun init"));
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
        let fresh = detect(tmp.path(), "demo")
            .unwrap()
            .setup(tmp.path(), |_| true);
        assert_eq!(fresh.len(), 1);
        assert_eq!(fresh[0][..3], ["pnpm", "add", "--save-dev"]);
        assert!(fresh[0].contains(&"typescript@~6.0".to_owned()));
        assert!(fresh[0].contains(&"@astrojs/check".to_owned()));
        assert!(!fresh[0].contains(&"eslint-plugin-react-hooks".to_owned()));

        let all: Vec<String> = TS_DEV_DEPS
            .iter()
            .chain(&ASTRO_DEV_DEPS)
            .map(|(name, _, _)| format!("\"{name}\": \"*\""))
            .collect();
        let manifest = ASTRO.replace(
            r#""dependencies""#,
            &format!(
                r#""devDependencies": {{{}}}, "dependencies""#,
                all.join(", ")
            ),
        );
        fs::write(tmp.path().join("package.json"), manifest).unwrap();
        assert!(
            detect(tmp.path(), "demo")
                .unwrap()
                .setup(tmp.path(), |_| true)
                .is_empty()
        );
    }

    #[test]
    fn a_project_keeping_its_own_lint_config_keeps_its_own_lint_tools() {
        let tmp = dir(&[("package.json", ASTRO), ("bun.lock", "")]);
        let p = detect(tmp.path(), "demo").unwrap();
        let setup = p.setup(tmp.path(), |file| file != ESLINT);
        assert!(setup[0].contains(&"prettier".to_owned()));
        assert!(!setup[0].iter().any(|dep| dep.contains("eslint")));
        assert!(p.setup(tmp.path(), |_| false).is_empty());
    }

    #[test]
    fn a_react_app_gets_the_hooks_rules() {
        let tmp = dir(&[
            ("package.json", &manifest(r#""react": "^19", "vite": "^8""#)),
            ("package-lock.json", ""),
        ]);
        let setup = detect(tmp.path(), "demo")
            .unwrap()
            .setup(tmp.path(), |_| true);
        assert!(setup[0].contains(&"eslint-plugin-react-hooks".to_owned()));
        assert!(!setup[0].contains(&"@astrojs/check".to_owned()));
    }
}
