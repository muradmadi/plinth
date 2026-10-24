use std::fmt;
use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

use anyhow::{Context, Result, bail};

/// One command sadr runs in the new project, kept as data so `--dry-run` can print exactly what
/// a real run would do.
pub struct Step {
    pub args: Vec<String>,
    /// Fed to the command's standard input, for `gh api --input -` with a JSON body.
    pub stdin: Option<String>,
}

impl Step {
    pub fn new<I: IntoIterator<Item = S>, S: Into<String>>(args: I) -> Self {
        Step {
            args: args.into_iter().map(Into::into).collect(),
            stdin: None,
        }
    }

    pub fn with_stdin(mut self, input: String) -> Self {
        self.stdin = Some(input);
        self
    }

    fn run(&self, dir: &Path) -> Result<()> {
        let mut command = Command::new(&self.args[0]);
        command.args(&self.args[1..]).current_dir(dir);
        if self.stdin.is_some() {
            command.stdin(Stdio::piped());
        }
        let mut child = command
            .spawn()
            .with_context(|| format!("couldn't start `{self}`"))?;
        if let (Some(input), Some(mut pipe)) = (&self.stdin, child.stdin.take()) {
            pipe.write_all(input.as_bytes())?;
        }
        let status = child.wait()?;
        if !status.success() {
            bail!("`{self}` failed ({status})");
        }
        Ok(())
    }
}

/// The command as it would be typed, quoting any argument with a space in it.
impl fmt::Display for Step {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let words: Vec<String> = self
            .args
            .iter()
            .map(|a| {
                if a.contains(' ') {
                    format!("'{a}'")
                } else {
                    a.clone()
                }
            })
            .collect();
        write!(f, "{}", words.join(" "))?;
        if self.stdin.is_some() {
            write!(f, " <<< (JSON)")?;
        }
        Ok(())
    }
}

/// Runs each step in `dir`, stopping at the first that fails.
pub fn run_all(dir: &Path, steps: &[Step]) -> Result<()> {
    for step in steps {
        step.run(dir)?;
    }
    Ok(())
}
