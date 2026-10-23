mod scaffold;
mod stack;

use std::fs;
use std::path::PathBuf;

use anyhow::{Result, bail};
use clap::{Args, Parser, Subcommand, ValueEnum};

use crate::scaffold::Vars;
use crate::stack::Stack;

/// Scaffold a new project in one stack, set up the same way every time.
#[derive(Parser)]
#[command(version)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Create a project from the base layer and one stack
    New(NewArgs),
    /// List the stacks `new` can build on
    Stacks,
}

#[derive(Args)]
struct NewArgs {
    /// Lowercase letters, digits and dashes; also the directory name and, later, the repo name
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
    /// Only write the files: no git init, no mise install, no stack setup
    #[arg(long)]
    no_setup: bool,
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

    let files = scaffold::render(
        args.stack,
        &Vars {
            name: args.name.clone(),
            description: args.description,
        },
    )?;
    let commands = if args.no_setup {
        Vec::new()
    } else {
        scaffold::setup_commands(args.stack)
    };

    if args.dry_run {
        println!("Would write {} files to {}:", files.len(), root.display());
        for path in files.keys() {
            println!("  {}", path.display());
        }
        if !commands.is_empty() {
            println!("Then run there:");
            for command in &commands {
                println!("  {}", command.join(" "));
            }
        }
        return Ok(());
    }

    scaffold::write(&root, &files)?;
    scaffold::run(&root, &commands)?;
    println!(
        "Created {} from the {} stack in {}",
        args.name,
        args.stack.name(),
        root.display()
    );
    Ok(())
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
