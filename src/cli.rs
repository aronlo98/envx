use std::path::PathBuf;

use clap::{Parser, Subcommand, ValueEnum};

#[derive(Clone, ValueEnum)]
pub enum ShellChoice {
    Bash,
    Zsh,
    Fish,
    PowerShell,
}

#[derive(Parser)]
#[command(
    name = "envx",
    version,
    about = "Modern .envx processor — dynamic variables, pipe functions, imports"
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand)]
pub enum Commands {
    /// Evaluate a .envx file and run a command with those variables injected
    Run {
        /// Path to the .envx file
        file: PathBuf,

        /// Command and arguments (use `--` to separate from envx options)
        #[arg(trailing_var_arg = true, required = true, value_name = "CMD")]
        cmd: Vec<String>,
    },

    /// Print all variables as `export KEY="VALUE"` statements
    ///
    /// Typical use:  eval $(envx export app.envx)
    Export {
        /// Path to the .envx file
        file: PathBuf,
    },

    /// Evaluate a single expression using OS environment for variable references
    ///
    /// Example:  envx eval '$HOME | lower'
    Eval {
        /// The expression to evaluate
        #[arg(value_name = "EXPR")]
        expr: String,
    },

    /// Print all resolved variables as KEY=VALUE pairs
    ///
    /// Example:  envx print app.envx
    ///           envx print --tags app.envx
    Print {
        /// Path to the .envx file
        file: PathBuf,

        /// Show a TAG column and sort rows by tag name ascending
        #[arg(long, short = 't')]
        tags: bool,
    },

    /// Print the shell completion script for the given shell
    ///
    /// Example:  envx completions zsh > ~/.zfunc/_envx
    Completions {
        shell: ShellChoice,
    },

    /// Format a .envx file — aligns `=` across all assignments
    ///
    /// Example:  envx fmt app.envx
    ///           envx fmt --check app.envx
    Fmt {
        /// Path to the .envx file
        file: PathBuf,

        /// Exit with a non-zero code if the file is not already formatted (useful in CI)
        #[arg(long)]
        check: bool,
    },
}
