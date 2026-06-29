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

use ast::{LayoutItem, ResolvedEnv, Segment, Span, Statement, Template};
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
        Commands::Print { file, tags } => cmd_print(file, tags),
        Commands::Fmt { file, check } => cmd_fmt(file, check),
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

fn cmd_print(file: PathBuf, tags: bool) -> Result<()> {
    let env = loader::load(&file)?;
    let order = dag::build_and_sort(&env)?;
    let values = evaluator::evaluate(&env, &order)?;

    if tags {
        cmd_print_tags(&env, &values);
    } else {
        cmd_print_table(&env, &values);
    }
    Ok(())
}

fn cmd_print_table(env: &ast::ResolvedEnv, values: &IndexMap<String, Value>) {
    let key_w = env.entries.keys().map(|k| k.len()).max().unwrap_or(0).max("KEY".len());
    let val_w = values.values()
        .map(|v| v.as_str_repr().len())
        .max()
        .unwrap_or(0)
        .max("VALUE".len());

    println!("{:<key_w$} | {:<val_w$}", "KEY", "VALUE");
    println!("{:-<key_w$}-+-{:-<val_w$}", "", "");

    for item in &env.layout {
        if let LayoutItem::Entry(key) = item {
            if let Some(val) = values.get(key) {
                println!("{key:<key_w$} | {}", val.as_str_repr());
            }
        }
    }
}

fn cmd_print_tags(env: &ast::ResolvedEnv, values: &IndexMap<String, Value>) {
    // Build (tag, key, value) triples by walking the layout in order.
    let mut rows: Vec<(String, String, String)> = Vec::new();
    let mut current_tag = String::new();
    for item in &env.layout {
        match item {
            LayoutItem::Section(name) => current_tag = name.clone(),
            LayoutItem::Entry(key) => {
                if let Some(val) = values.get(key) {
                    rows.push((current_tag.clone(), key.clone(), val.as_str_repr()));
                }
            }
        }
    }

    // Sort by tag ascending (stable sort keeps declaration order within each tag).
    rows.sort_by(|a, b| a.0.cmp(&b.0));

    let tag_w = rows.iter().map(|(t, _, _)| t.len()).max().unwrap_or(0).max("TAG".len());
    let key_w = rows.iter().map(|(_, k, _)| k.len()).max().unwrap_or(0).max("KEY".len());
    let val_w = rows.iter().map(|(_, _, v)| v.len()).max().unwrap_or(0).max("VALUE".len());

    println!("{:<tag_w$} | {:<key_w$} | {:<val_w$}", "TAG", "KEY", "VALUE");
    println!("{:-<tag_w$}-+-{:-<key_w$}-+-{:-<val_w$}", "", "", "");

    for (tag, key, val) in &rows {
        println!("{tag:<tag_w$} | {key:<key_w$} | {val}");
    }
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



// ─── Subcommand: fmt ─────────────────────────────────────────────────────────

fn cmd_fmt(file: PathBuf, check: bool) -> Result<()> {
    let source = std::fs::read_to_string(&file)
        .map_err(|e| EnvxError::Io { path: file.display().to_string(), source: e })?;

    let formatted = format_source(&source);

    if check {
        if formatted != source {
            eprintln!("error: `{}` is not formatted — run `envx fmt` to fix", file.display());
            process::exit(1);
        }
        eprintln!("{}: ok", file.display());
    } else {
        std::fs::write(&file, &formatted)
            .map_err(|e| EnvxError::Io { path: file.display().to_string(), source: e })?;
        eprintln!("formatted: {}", file.display());
    }
    Ok(())
}

/// Reformat `source`: align `=` on all assignment lines to the longest key width,
/// and normalise whitespace inside every `${{ }}` expression block.
/// Comments, blank lines, and `@import` lines are left untouched.
fn format_source(source: &str) -> String {
    let lines: Vec<&str> = source.lines().collect();

    let max_key = lines
        .iter()
        .filter_map(|l| split_assignment(l).map(|(k, _)| k.len()))
        .max()
        .unwrap_or(0);

    let mut out: Vec<String> = Vec::with_capacity(lines.len());
    for line in &lines {
        if let Some((key, value)) = split_assignment(line) {
            out.push(format!("{key:<max_key$} = {}", normalize_exprs(value)));
        } else if let Some(sec) = normalize_section(line) {
            out.push(sec);
        } else {
            out.push(line.to_string());
        }
    }

    let mut result = out.join("\n");
    if source.ends_with('\n') {
        result.push('\n');
    }
    result
}

/// Normalise whitespace inside every `${{ … }}` block in a value string.
/// The expression content is trimmed: `${{  secret()   }}` → `${{ secret() }}`.
/// Single-quoted strings inside expressions are treated as opaque so a `}}`
/// inside `'}'` does not close the block.
fn normalize_exprs(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let bytes = s.as_bytes();
    let len = bytes.len();
    let mut pos = 0;

    while pos < len {
        if pos + 3 <= len && &s[pos..pos + 3] == "${{" {
            let inner_start = pos + 3;
            let mut inner = inner_start;
            let mut in_str = false;

            while inner < len {
                if in_str {
                    if bytes[inner] == b'\'' { in_str = false; }
                    inner += 1;
                } else if bytes[inner] == b'\'' {
                    in_str = true;
                    inner += 1;
                } else if inner + 2 <= len && &s[inner..inner + 2] == "}}" {
                    break;
                } else {
                    inner += 1;
                }
            }

            let expr = s[inner_start..inner].trim();
            out.push_str("${{ ");
            out.push_str(expr);
            out.push_str(" }}");
            pos = inner + 2; // skip past `}}`
        } else {
            // Copy one char (handles multi-byte UTF-8 safely).
            let ch = s[pos..].chars().next().unwrap();
            out.push(ch);
            pos += ch.len_utf8();
        }
    }

    out
}

/// Normalise a section header line: `  [ hola  ]  ` → `[hola]`.
/// Returns `None` if the line is not a section header or the name is not a
/// valid identifier (`[A-Za-z_][A-Za-z0-9_]*`).
fn normalize_section(line: &str) -> Option<String> {
    let trimmed = line.trim();
    if !trimmed.starts_with('[') || !trimmed.ends_with(']') {
        return None;
    }
    let name = trimmed[1..trimmed.len() - 1].trim();
    let mut chars = name.chars();
    let first = chars.next()?;
    if !first.is_ascii_alphabetic() && first != '_' {
        return None;
    }
    if !chars.all(|c| c.is_ascii_alphanumeric() || c == '_') {
        return None;
    }
    Some(format!("[{name}]"))
}

/// Return `(key, value)` if `line` is an assignment, otherwise `None`.
/// Strips surrounding whitespace and normalises spacing around `=`.
fn split_assignment(line: &str) -> Option<(&str, &str)> {
    let trimmed = line.trim();
    if trimmed.is_empty() || trimmed.starts_with('#') || trimmed.starts_with('@') {
        return None;
    }

    // Key: [A-Za-z_][A-Za-z0-9_]*
    let first = trimmed.chars().next()?;
    if !first.is_ascii_alphabetic() && first != '_' {
        return None;
    }
    let key_end = trimmed
        .find(|c: char| !c.is_ascii_alphanumeric() && c != '_')
        .unwrap_or(trimmed.len());
    let key = &trimmed[..key_end];

    let rest = trimmed[key_end..].trim_start();
    if !rest.starts_with('=') {
        return None;
    }

    let value = rest[1..].trim_start();
    Some((key, value))
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
