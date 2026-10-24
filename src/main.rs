mod github;
mod lang;
mod scaffold;
mod step;

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use clap::{Args, Parser, Subcommand};

use crate::github::Repo;
use crate::scaffold::Vars;
use crate::step::Step;

/// Set up the tooling around a project the same way every time, and create its GitHub repo.
#[derive(Parser)]
#[command(version)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Add the house tooling to a project and create its GitHub repo. The project's own files
    /// are never changed.
    Init(InitArgs),
}

#[derive(Args)]
struct InitArgs {
    /// The project to set up; it must already have its manifest, such as Cargo.toml
    #[arg(long, default_value = ".")]
    dir: PathBuf,
    /// Lowercase letters, digits and dashes; the repo name [default: the directory's name]
    #[arg(long)]
    name: Option<String>,
    /// One line on what the project does
    #[arg(long, default_value = "")]
    description: String,
    /// Print the files and commands, then stop without touching anything
    #[arg(long)]
    dry_run: bool,
    /// Only write the files: no git init, no mise install, no setup commands (needs --no-github)
    #[arg(long, requires = "no_github")]
    no_setup: bool,
    /// Don't create the GitHub repo
    #[arg(long)]
    no_github: bool,
    /// Make the GitHub repo public; it is private otherwise
    #[arg(long, conflicts_with = "no_github")]
    public: bool,
    /// The GitHub account or organization to create the repo under [default: the one gh is
    /// logged in as]
    #[arg(long, conflicts_with = "no_github")]
    owner: Option<String>,
}

fn main() -> Result<()> {
    match Cli::parse().command {
        Command::Init(args) => init(args),
    }
}

fn init(args: InitArgs) -> Result<()> {
    let root = fs::canonicalize(&args.dir)
        .with_context(|| format!("{} doesn't exist", args.dir.display()))?;
    let name = match args.name {
        Some(name) => name,
        None => root
            .file_name()
            .and_then(|n| n.to_str())
            .context("couldn't name the project after its directory; give --name")?
            .to_owned(),
    };
    check_name(&name)?;
    if args.description.contains(['\n', '\r']) {
        bail!("--description must be one line");
    }
    let project = lang::detect(&root)?;

    let repo = if args.no_github {
        None
    } else {
        // Checked first: it needs no network and catches a repo plinth can't push.
        github::ensure_ready(&root)?;
        let owner = match args.owner {
            Some(owner) => owner,
            None => github::logged_in_user()?,
        };
        Some(Repo {
            owner,
            name: name.clone(),
            description: args.description.clone(),
            public: args.public,
        })
    };

    let files = scaffold::render(
        project.lang,
        &Vars {
            name: name.clone(),
            description: args.description,
            package: project.package.clone(),
            version: project.version.clone(),
            pm: project.pm,
            members: project.members.clone(),
        },
    )?;
    let plan = scaffold::plan(&root, files)?;
    let setup = if args.no_setup {
        Vec::new()
    } else {
        scaffold::setup_steps(&root, &project, &plan)
    };
    let github_steps = repo.as_ref().map(github::steps).unwrap_or_default();
    let claude_note = claude_md_note(&root);

    if args.dry_run {
        if plan.add.is_empty() && plan.append.is_empty() {
            println!("Nothing to add to {}", root.display());
        }
        print_paths(
            &format!("Would add {} files to {}:", plan.add.len(), root.display()),
            plan.add.keys(),
        );
        print_paths("Would append to:", plan.append.keys());
        print_paths("Would leave alone, as the project has its own:", &plan.keep);
        print_steps("Then run there:", &setup);
        print_steps("Then set up GitHub:", &github_steps);
        if let Some(note) = &claude_note {
            println!("{note}");
        }
        return Ok(());
    }

    // Checked before writing anything, so a taken name leaves the project as it was.
    if let Some(repo) = &repo {
        github::ensure_absent(repo)?;
    }
    scaffold::write(&root, &plan)?;
    step::run_all(&root, &setup)?;
    step::run_all(&root, &github_steps)?;

    println!(
        "Set up {name} in {} ({})",
        root.display(),
        project.lang.name()
    );
    if !plan.keep.is_empty() {
        println!("Left alone, as the project has its own:");
        for path in &plan.keep {
            println!("  {}", path.display());
        }
    }
    if let Some(repo) = repo {
        println!("GitHub: https://github.com/{}", repo.slug());
    }
    if let Some(note) = claude_note {
        println!("{note}");
    }
    Ok(())
}

/// Claude Code reads AGENTS.md only when there's no CLAUDE.md, so a project that already has one
/// needs it to import AGENTS.md. plinth doesn't edit the file; it says what to add.
fn claude_md_note(root: &Path) -> Option<String> {
    let text = fs::read_to_string(root.join("CLAUDE.md")).ok()?;
    if text.contains("@AGENTS.md") {
        return None;
    }
    Some(
        "Note: Claude Code reads the project's CLAUDE.md instead of AGENTS.md; add `@AGENTS.md` \
         as its first line to include the house rules."
            .to_owned(),
    )
}

fn print_paths<'a>(heading: &str, paths: impl IntoIterator<Item = &'a PathBuf>) {
    let mut paths = paths.into_iter().peekable();
    if paths.peek().is_none() {
        return;
    }
    println!("{heading}");
    for path in paths {
        println!("  {}", path.display());
    }
}

fn print_steps(heading: &str, steps: &[Step]) {
    if steps.is_empty() {
        return;
    }
    println!("{heading}");
    for step in steps {
        println!("  {step}");
    }
}

/// The name becomes the GitHub repo's, held to lowercase letters, digits and dashes so every repo
/// is named alike.
fn check_name(name: &str) -> Result<()> {
    let valid = name.starts_with(|c: char| c.is_ascii_lowercase())
        && !name.ends_with('-')
        && name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-');
    if !valid {
        bail!(
            "`{name}` isn't a valid project name: use lowercase letters, digits and dashes, starting with a letter, or give one with --name"
        );
    }
    Ok(())
}
