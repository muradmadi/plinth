mod github;
mod scaffold;
mod stack;
mod step;

use std::fs;
use std::path::PathBuf;

use anyhow::{Result, bail};
use clap::{Args, Parser, Subcommand, ValueEnum};

use crate::github::Repo;
use crate::scaffold::Vars;
use crate::stack::Stack;
use crate::step::Step;

/// Scaffold a new project in one stack, set up the same way every time.
#[derive(Parser)]
#[command(version)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Create a project from the base layer and one stack, and its GitHub repo
    New(NewArgs),
    /// List the stacks `new` can build on
    Stacks,
}

#[derive(Args)]
struct NewArgs {
    /// Lowercase letters, digits and dashes; also the directory and repo name
    name: String,
    /// The stack to build on
    #[arg(long, value_enum)]
    stack: Stack,
    /// Where to create it [default: ./<NAME>]
    #[arg(long)]
    dir: Option<PathBuf>,
    /// One line on what the project does
    #[arg(long, default_value = "")]
    description: String,
    /// Print the files and commands, then stop without touching anything
    #[arg(long)]
    dry_run: bool,
    /// Only write the files: no git init, no mise install, no stack setup (needs --no-github)
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
        Command::New(args) => new(args),
        Command::Stacks => {
            for stack in Stack::value_variants() {
                println!("{:<8}{}", stack.name(), stack.summary());
            }
            Ok(())
        }
    }
}

fn new(args: NewArgs) -> Result<()> {
    check_name(&args.name)?;
    if args.description.contains(['\n', '\r']) {
        bail!("--description must be one line");
    }
    let root = args.dir.unwrap_or_else(|| PathBuf::from(&args.name));
    if root.exists() && fs::read_dir(&root)?.next().is_some() {
        bail!("{} already exists and isn't empty", root.display());
    }

    let repo = if args.no_github {
        None
    } else {
        let owner = match args.owner {
            Some(owner) => owner,
            None => github::logged_in_user()?,
        };
        Some(Repo {
            owner,
            name: args.name.clone(),
            description: args.description.clone(),
            public: args.public,
        })
    };

    let files = scaffold::render(
        args.stack,
        &Vars {
            name: args.name.clone(),
            description: args.description,
        },
    )?;
    let setup = if args.no_setup {
        Vec::new()
    } else {
        scaffold::setup_steps(args.stack)
    };
    let github_steps = repo.as_ref().map(github::steps).unwrap_or_default();

    if args.dry_run {
        println!("Would write {} files to {}:", files.len(), root.display());
        for path in files.keys() {
            println!("  {}", path.display());
        }
        print_steps("Then run there:", &setup);
        print_steps("Then set up GitHub:", &github_steps);
        return Ok(());
    }

    // Checked before writing anything, so a taken name leaves no half-made project behind.
    if let Some(repo) = &repo {
        github::ensure_absent(repo)?;
    }
    scaffold::write(&root, &files)?;
    step::run_all(&root, &setup)?;
    step::run_all(&root, &github_steps)?;

    println!(
        "Created {} from the {} stack in {}",
        args.name,
        args.stack.name(),
        root.display()
    );
    if let Some(repo) = repo {
        println!("GitHub: https://github.com/{}", repo.slug());
    }
    Ok(())
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

/// Names end up as a directory, a crate or package name and a GitHub repo, so they're held to
/// what all three accept.
fn check_name(name: &str) -> Result<()> {
    let valid = name.starts_with(|c: char| c.is_ascii_lowercase())
        && !name.ends_with('-')
        && name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-');
    if !valid {
        bail!(
            "`{name}` isn't a valid project name: use lowercase letters, digits and dashes, starting with a letter"
        );
    }
    Ok(())
}
