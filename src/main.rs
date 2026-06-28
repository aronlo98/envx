mod ast;
mod builtins;
mod cli;
mod dag;
mod error;
mod evaluator;
mod lexer;
mod loader;
mod parser;
mod value;

use std::{io, path::PathBuf, process};

use clap::{CommandFactory, Parser};
use clap_complete::generate;
use indexmap::IndexMap;
use miette::Report;

use ast::{ResolvedEnv, Segment, Span, Statement, Template};
use cli::{Cli, Commands, ShellChoice};
use error::{EnvxError, Result};
use value::Value;

// ─── Entry point ──────────────────────────────────────────────────────────────

fn main() {
    let cli = Cli::parse();
    let result = match cli.command {
        Commands::Run { file, cmd } => cmd_run(file, cmd),
        Commands::Export { file } => cmd_export(file),
        Commands::Eval { expr } => cmd_eval(expr),
        Commands::Print { file } => cmd_print(file),
        Commands::Completions { shell } => {
            cmd_completions(shell);
            return;
        }
    };

    if let Err(e) = result {
        eprintln!("{:?}", Report::new(e));
        process::exit(1);
    }
}

// ─── Subcommand: run ──────────────────────────────────────────────────────────

fn cmd_run(file: PathBuf, cmd: Vec<String>) -> Result<()> {
    let vars = load_and_eval(&file)?;

    // cmd is guaranteed non-empty by clap's `required = true`.
    let binary = &cmd[0];
    let args = &cmd[1..];

    let status = process::Command::new(binary)
        .args(args)
        .envs(vars.into_iter().map(|(k, v)| (k, v.into_string())))
        .status()
        .map_err(|e| EnvxError::Io {
            path: binary.clone(),
            source: e,
        })?;

    process::exit(status.code().unwrap_or(1));
}

// ─── Subcommand: export ───────────────────────────────────────────────────────

fn cmd_export(file: PathBuf) -> Result<()> {
    let vars = load_and_eval(&file)?;
    for (key, val) in vars {
        // Escape backslashes first so the substitution below doesn't double-escape.
        let escaped = val.into_string().replace('\\', "\\\\").replace('"', "\\\"");
        println!("export {key}=\"{escaped}\"");
    }
    Ok(())
}

// ─── Subcommand: print ───────────────────────────────────────────────────────

fn cmd_print(file: PathBuf) -> Result<()> {
    let rows: Vec<(String, String)> = load_and_eval(&file)?
        .into_iter()
        .map(|(k, v)| (k, v.into_string()))
        .collect();

    let key_w = rows.iter().map(|(k, _)| k.len()).max().unwrap_or(0).max("KEY".len());
    let val_w = rows.iter().map(|(_, v)| v.len()).max().unwrap_or(0).max("VALUE".len());

    println!("{:<key_w$} | {:<val_w$}", "KEY", "VALUE");
    println!("{:-<key_w$}-+-{:-<val_w$}", "", "");
    for (key, val) in rows {
        println!("{key:<key_w$} | {val}");
    }
    Ok(())
}

// ─── Subcommand: eval ─────────────────────────────────────────────────────────

fn cmd_eval(expr: String) -> Result<()> {
    let eval_path = PathBuf::from("<eval>");

    // Wrap the user-supplied expression in a synthetic .envx entry.
    let src = format!("__RESULT__ = \"${{{{ {expr} }}}}\"\n");

    // Pre-populate the env with all OS environment variables so that
    // `$HOME`, `$PATH`, etc. resolve inside the expression.
    let mut env = ResolvedEnv::default();
    for (key, val) in std::env::vars() {
        if is_valid_envx_ident(&key) {
            env.entries
                .insert(key, (literal_template(val), eval_path.clone()));
        }
    }
    env.sources.insert(eval_path.clone(), src.clone());

    // Parse the synthetic entry; it may reference OS vars loaded above.
    let file = parser::parse(&src, "<eval>", eval_path)?;
    for stmt in file.statements {
        if let Statement::Entry { key, template, source, .. } = stmt {
            // Insert last so __RESULT__ always wins even if an OS var shares the name.
            env.entries.insert(key, (template, source));
        }
    }

    let order = dag::build_and_sort(&env)?;
    let values = evaluator::evaluate(&env, &order)?;

    if let Some(v) = values.get("__RESULT__") {
        println!("{}", v.as_str_repr());
    }

    Ok(())
}

// ─── Subcommand: completions ──────────────────────────────────────────────────

fn cmd_completions(shell: ShellChoice) {
    let mut cmd = Cli::command();
    let shell = match shell {
        ShellChoice::Bash => clap_complete::Shell::Bash,
        ShellChoice::Zsh => clap_complete::Shell::Zsh,
        ShellChoice::Fish => clap_complete::Shell::Fish,
        ShellChoice::PowerShell => clap_complete::Shell::PowerShell,
    };
    generate(shell, &mut cmd, "envx", &mut io::stdout());
}



// ─── Shared helpers ───────────────────────────────────────────────────────────

fn load_and_eval(file: &PathBuf) -> Result<IndexMap<String, Value>> {
    let env = loader::load(file)?;
    let order = dag::build_and_sort(&env)?;
    evaluator::evaluate(&env, &order)
}

/// `true` if `s` is a valid envx identifier: `[A-Za-z_][A-Za-z0-9_]*`.
/// Used to filter OS env vars whose names can't be referenced in an expression.
fn is_valid_envx_ident(s: &str) -> bool {
    let mut chars = s.chars();
    match chars.next() {
        Some(c) if c.is_ascii_alphabetic() || c == '_' => {
            chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
        }
        _ => false,
    }
}

/// Build a single-literal `Template` without going through the parser.
fn literal_template(val: String) -> Template {
    Template {
        segments: vec![Segment::Literal(val)],
        span: Span::default(),
    }
}
