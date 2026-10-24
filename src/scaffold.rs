use std::collections::BTreeMap;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use include_dir::{Dir, File, include_dir};
use minijinja::syntax::SyntaxConfig;
use minijinja::{Environment, UndefinedBehavior, context};

use crate::lang::{Kind, Lang, Member, Pm, Project};
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
    /// The package manager, for a TypeScript project; its commands go in the justfile.
    pub pm: Option<Pm>,
    /// The packages of a TypeScript project, each adding its own checks.
    pub members: Vec<Member>,
}

/// Every file plinth adds to a project, keyed by its path inside the project. The base layer goes
/// in first, then the language's, so a language file replaces a base file at the same path. Last
/// comes the package manager's layer for that language, such as `typescript-pnpm`, where there is
/// one.
/// A file ending in `.j2` is rendered and loses the suffix; anything else is copied as it is.
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
        pm => vars.pm.map(Pm::name),
        exec => vars.pm.map(Pm::exec),
        install_frozen => vars.pm.map(Pm::install_frozen),
        members => vars.members.iter().map(|m| context! {
            path => m.path.as_str(),
            prefix => m.prefix(),
            kind => m.kind.name(),
            tsconfig => m.tsconfig.name(),
        }).collect::<Vec<_>>(),
        any_astro => has(vars, Kind::Astro),
        any_web => has(vars, Kind::Web),
        any_build => has(vars, Kind::Astro) || has(vars, Kind::Web),
        indent => if root_kind(vars) == Some(Kind::Astro) { "tab" } else { "space" },
        prettierrc => prettierrc(vars),
    };

    // Plain files rather than `.j2`, so Renovate can read and bump each package manager's mise.toml.
    let pm_layer = vars
        .pm
        .map(|pm| format!("{}-{}", lang.name(), pm.name()))
        .filter(|layer| TEMPLATES.get_dir(layer).is_some());
    let layers = ["base", lang.name()]
        .map(String::from)
        .into_iter()
        .chain(pm_layer);

    let mut files = BTreeMap::new();
    for layer in layers {
        let layer = layer.as_str();
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

fn has(vars: &Vars, kind: Kind) -> bool {
    vars.members.iter().any(|m| m.kind == kind)
}

fn root_kind(vars: &Vars) -> Option<Kind> {
    vars.members.iter().find(|m| m.path == ".").map(|m| m.kind)
}

/// The Prettier config, in the style the project's own tool wrote its files in, so its untouched
/// code passes: create-vite and create-hono leave out semicolons, and they and create-astro use
/// single quotes. A package plinth can't place, such as one from `bun init`, gets Prettier's
/// defaults.
fn prettierrc(vars: &Vars) -> String {
    let mut entries = Vec::new();
    if has(vars, Kind::Astro) {
        entries.push(r#""plugins": ["prettier-plugin-astro"]"#);
    }
    let root = root_kind(vars);
    if matches!(root, Some(Kind::Web | Kind::Api)) {
        entries.push(r#""semi": false"#);
    }
    if matches!(root, Some(Kind::Astro | Kind::Web | Kind::Api)) {
        entries.push(r#""singleQuote": true"#);
        // renovate.json5 keeps JSON's double quotes, like Renovate's own examples.
        entries
            .push(r#""overrides": [{ "files": "*.json5", "options": { "singleQuote": false } }]"#);
    }
    if entries.is_empty() {
        return "{}".into();
    }
    format!("{{\n  {}\n}}", entries.join(",\n  "))
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
    /// Text to append to an existing file: the `.gitignore` or `.gitattributes` lines it lacks, or
    /// the house rules an AGENTS.md of the project's own lacks.
    pub append: BTreeMap<PathBuf, String>,
    /// Files the project already has in its own form, left as they are.
    pub keep: Vec<PathBuf>,
}

/// A TypeScript project's ESLint config, which plinth formats once the project's Prettier is in.
const ESLINT_CONFIG: &str = "eslint.config.mjs";

/// Files of one rule per line, which plinth adds its missing lines to instead of leaving alone.
const APPENDED: [&str; 2] = [".gitignore", ".gitattributes"];

/// The heading an AGENTS.md has once it carries plinth's house rules.
const HOUSE_RULES: &str = "## Git and GitHub";

/// Sorts the rendered files against what is already under `root`.
pub fn plan(root: &Path, files: BTreeMap<PathBuf, String>) -> Result<Plan> {
    let mut plan = Plan::default();
    for (path, text) in files {
        let dest = root.join(&path);
        if let Some(own) = config_of_its_own(root, &path) {
            plan.keep.push(own);
            continue;
        }
        if !dest.exists() {
            plan.add.insert(path, text);
            continue;
        }
        let existing = fs::read_to_string(&dest)
            .with_context(|| format!("couldn't read {}", dest.display()))?;
        if existing == text {
            continue;
        }
        if APPENDED.iter().any(|a| path == Path::new(a)) {
            if let Some(extra) = missing_lines(&existing, &text) {
                plan.append.insert(path, extra);
            }
        } else if path == Path::new("AGENTS.md") && !has_house_rules(&existing) {
            plan.append.insert(path, house_rules(&existing, &text));
        } else {
            plan.keep.push(path);
        }
    }
    Ok(plan)
}

/// Tools that read their config from any of several file names, keyed by the one plinth writes.
/// The tool reads only one of them, so a project with another keeps it and gets none of plinth's.
const CONFIG_NAMES: [(&str, &[&str]); 2] = [
    (
        ESLINT_CONFIG,
        &[
            "eslint.config.js",
            "eslint.config.cjs",
            "eslint.config.ts",
            "eslint.config.mts",
            "eslint.config.cts",
            ".eslintrc.js",
            ".eslintrc.cjs",
            ".eslintrc.json",
            ".eslintrc.yaml",
            ".eslintrc.yml",
        ],
    ),
    (
        ".prettierrc.json",
        &[
            ".prettierrc",
            ".prettierrc.json5",
            ".prettierrc.yaml",
            ".prettierrc.yml",
            ".prettierrc.toml",
            ".prettierrc.js",
            ".prettierrc.mjs",
            ".prettierrc.cjs",
            "prettier.config.js",
            "prettier.config.mjs",
            "prettier.config.cjs",
            "prettier.config.ts",
        ],
    ),
];

/// The project's own config under another name for the tool `path` configures, if it has one.
fn config_of_its_own(root: &Path, path: &Path) -> Option<PathBuf> {
    let (_, others) = CONFIG_NAMES
        .iter()
        .find(|(ours, _)| path == Path::new(ours))?;
    others
        .iter()
        .map(PathBuf::from)
        .find(|other| root.join(other).exists())
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
    extra.push_str("\n# Added by plinth\n");
    for line in missing {
        extra.push_str(line);
        extra.push('\n');
    }
    Some(extra)
}

fn has_house_rules(agents_md: &str) -> bool {
    agents_md.lines().any(|l| l.trim_end() == HOUSE_RULES)
}

/// plinth's AGENTS.md from its first section on, to follow the project's own text. The title and
/// description stay out, as the project's file has its own.
fn house_rules(existing: &str, ours: &str) -> String {
    let sections = ours.find("\n## ").map_or(ours, |i| &ours[i + 1..]);
    let mut extra = String::new();
    if !existing.is_empty() && !existing.ends_with('\n') {
        extra.push('\n');
    }
    extra.push('\n');
    extra.push_str(sections);
    extra
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
pub fn setup_steps(root: &Path, project: &Project, plan: &Plan) -> Vec<Step> {
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
    for command in project.setup(root) {
        steps.push(Step::new(
            ["mise", "exec", "--"]
                .map(String::from)
                .into_iter()
                .chain(command),
        ));
    }
    // plinth writes its ESLint config in Prettier's default style; the project's own Prettier
    // config then puts it in the project's style, so `just format-check` passes on it.
    if let Some(pm) = project
        .pm
        .filter(|_| plan.add.contains_key(Path::new(ESLINT_CONFIG)))
    {
        steps.push(Step::new(
            ["mise", "exec", "--"]
                .into_iter()
                .chain(pm.exec().split(' '))
                .chain(["prettier", "--write", ESLINT_CONFIG]),
        ));
    }
    steps.push(Step::new(["mise", "exec", "--", "just", "setup"]));
    steps
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lang::Tsconfig;
    use tempfile::TempDir;

    fn vars(description: &str) -> Vars {
        Vars {
            name: "demo".into(),
            description: description.into(),
            package: "demo-cli".into(),
            version: "0.1.0".into(),
            pm: None,
            members: Vec::new(),
        }
    }

    fn member(kind: Kind, tsconfig: Tsconfig) -> Member {
        Member {
            path: ".".into(),
            kind,
            tsconfig,
        }
    }

    fn typescript(pm: Pm, members: Vec<Member>) -> BTreeMap<PathBuf, String> {
        let vars = Vars {
            pm: Some(pm),
            members,
            ..vars("")
        };
        render(Lang::TypeScript, &vars).unwrap()
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
        assert_eq!(extra, "\n\n# Added by plinth\n*.db\n");
        assert_eq!(
            missing_lines("/target\n*.db\n", "/target\n# x\n*.db\n"),
            None
        );
    }

    #[test]
    fn appends_to_an_existing_gitattributes_too() {
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join(".gitattributes"), "* text=auto eol=lf\n").unwrap();
        let plan = plan(tmp.path(), rendered()).unwrap();
        let extra = &plan.append[Path::new(".gitattributes")];
        assert!(extra.contains("Cargo.lock linguist-generated\n"));
        assert!(!extra.contains("text=auto"));
        assert!(!plan.keep.contains(&PathBuf::from(".gitattributes")));
    }

    #[test]
    fn appends_the_house_rules_to_the_projects_own_agents_md_once() {
        let tmp = TempDir::new().unwrap();
        let agents_md = tmp.path().join("AGENTS.md");
        fs::write(&agents_md, "## Development\n\nRun `astro dev`.").unwrap();

        let plan = plan(tmp.path(), rendered()).unwrap();
        let extra = &plan.append[Path::new("AGENTS.md")];
        assert!(extra.starts_with("\n\n## Working here\n"));
        assert!(extra.contains(HOUSE_RULES));
        assert!(!extra.contains("# demo"));

        write(tmp.path(), &plan).unwrap();
        let again = super::plan(tmp.path(), rendered()).unwrap();
        assert!(!again.append.contains_key(Path::new("AGENTS.md")));
        assert!(again.keep.contains(&PathBuf::from("AGENTS.md")));
    }

    #[test]
    fn each_kind_of_package_gets_its_own_checks() {
        let justfile = |kind, tsconfig| {
            typescript(Pm::Npm, vec![member(kind, tsconfig)])[Path::new("justfile")].clone()
        };
        let astro = justfile(Kind::Astro, Tsconfig::Plain);
        assert!(astro.contains("npx --no -- astro check --root .\n"));
        assert!(astro.contains("npx --no -- astro build --root .\n"));
        let web = justfile(Kind::Web, Tsconfig::Solution);
        assert!(web.contains("npx --no -- tsc -b .\n"));
        assert!(web.contains("npx --no -- vite build .\n"));
        let api = justfile(Kind::Api, Tsconfig::Plain);
        assert!(api.contains("npx --no -- tsc --noEmit -p .\n"));
        assert!(api.contains("# Build what the project ships; nothing yet\nbuild:\n\n"));
        assert!(!justfile(Kind::Lib, Tsconfig::None).contains("tsc"));
    }

    #[test]
    fn the_style_follows_the_tool_that_made_the_project() {
        let files = |kind| typescript(Pm::Bun, vec![member(kind, Tsconfig::Plain)]);
        let web = files(Kind::Web);
        assert!(web[Path::new(".prettierrc.json")].contains("\"semi\": false"));
        assert!(web[Path::new("eslint.config.mjs")].contains("reactHooks.configs"));
        assert!(!web[Path::new("eslint.config.mjs")].contains("eslint-plugin-astro"));
        assert!(web[Path::new(".editorconfig")].contains("indent_style = space\nindent_size = 2"));
        let astro = files(Kind::Astro);
        assert!(astro[Path::new(".prettierrc.json")].contains("prettier-plugin-astro"));
        assert!(astro[Path::new(".editorconfig")].contains("indent_style = tab"));
        assert!(astro[Path::new("eslint.config.mjs")].contains("astro.configs.recommended"));
        let api = files(Kind::Api);
        assert!(api[Path::new(".prettierignore")].contains("\ntsconfig.json\n"));
        assert_eq!(files(Kind::Lib)[Path::new(".prettierrc.json")], "{}\n");
    }

    #[test]
    fn keeps_a_config_the_project_has_under_another_name() {
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join("eslint.config.js"), "export default [];\n").unwrap();
        let files = typescript(Pm::Npm, vec![member(Kind::Astro, Tsconfig::Plain)]);
        let plan = plan(tmp.path(), files).unwrap();
        assert!(!plan.add.contains_key(Path::new(ESLINT_CONFIG)));
        assert!(plan.keep.contains(&PathBuf::from("eslint.config.js")));
        assert!(plan.add.contains_key(Path::new(".prettierrc.json")));
    }

    #[test]
    fn the_typescript_layer_runs_the_tools_through_the_package_manager() {
        for (pm, exec, install) in [
            (Pm::Npm, "npx --no -- prettier --check", "npm ci"),
            (
                Pm::Pnpm,
                "pnpm exec prettier",
                "pnpm install --frozen-lockfile",
            ),
            (
                Pm::Bun,
                "bun x --no-install prettier",
                "bun install --frozen-lockfile",
            ),
        ] {
            let files = typescript(pm, vec![member(Kind::Astro, Tsconfig::Plain)]);
            let justfile = &files[Path::new("justfile")];
            assert!(justfile.contains(exec) && justfile.contains(install));
            assert_eq!(
                files[Path::new("mise.toml")].contains("pnpm ="),
                pm == Pm::Pnpm
            );
            assert_eq!(
                files[Path::new("mise.toml")].contains("bun ="),
                pm == Pm::Bun
            );
        }
    }

    #[test]
    fn setup_skips_what_the_project_already_did() {
        let tmp = TempDir::new().unwrap();
        fs::write(
            tmp.path().join("Cargo.toml"),
            "[package]\nname = \"demo\"\nversion = \"0.1.0\"\n",
        )
        .unwrap();
        let lines = |root: &Path| -> Vec<String> {
            setup_steps(root, &crate::lang::detect(root).unwrap(), &Plan::default())
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
