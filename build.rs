use std::{fs, path::Path};

use clap::CommandFactory;
use clap_complete::{generate_to, Shell};

include!("src/cli.rs");

fn main() {
    let comp_dir = Path::new("completions");
    fs::create_dir_all(comp_dir).unwrap();

    let mut cmd = Cli::command();

    for shell in [Shell::Bash, Shell::Zsh, Shell::Fish, Shell::PowerShell] {
        generate_to(shell, &mut cmd, "envx", comp_dir).unwrap();
    }

    // Re-run only when the CLI definition changes.
    println!("cargo:rerun-if-changed=src/cli.rs");
}
