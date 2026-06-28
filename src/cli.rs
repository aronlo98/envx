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
    ///           envx print app.envx > .env
    Print {
        /// Path to the .envx file
        file: PathBuf,
    },

    /// Print the shell completion script for the given shell
    ///
    /// Example:  envx completions zsh > ~/.zfunc/_envx
    Completions {
        shell: ShellChoice,
    },


}
