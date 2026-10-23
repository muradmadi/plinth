use clap::ValueEnum;

/// A language layer, written on top of the base layer every project gets. Each one is a directory
/// under templates/ with the same name.
#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
pub enum Stack {
    Rust,
}

impl Stack {
    pub fn name(self) -> &'static str {
        match self {
            Stack::Rust => "rust",
        }
    }

    pub fn summary(self) -> &'static str {
        match self {
            Stack::Rust => "a Rust binary crate; the gate is rustfmt, clippy and cargo test",
        }
    }

    /// Commands that finish the project once its tools are installed. Each runs through
    /// `mise exec`, so it gets the versions pinned in the project's mise.toml.
    pub fn setup(self) -> &'static [&'static [&'static str]] {
        match self {
            // CI runs cargo with --locked, which fails without a lockfile.
            Stack::Rust => &[&["cargo", "generate-lockfile"]],
        }
    }
}
