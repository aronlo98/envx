# envx — Agent Context

This file provides complete context for AI agents working on this codebase.
Read it fully before making any changes.

---

## What is envx

`envx` is a modern, safe CLI tool that replaces `dotenv`. It processes `.envx`
files — a superset of the traditional `KEY="value"` format that adds:

- **Dynamic variables** via `${{ expression }}` template blocks
- **Pipe chains** for text manipulation: `$NAME | trim | lower | replace(' ', '_')`
- **Inter-variable dependencies**: `EMAIL = "${{ $USERNAME }}@company.com"`
- **Conditional logic**: `if eq($APP_ENV, 'prod') then 'api.db.com' else 'localhost'`
- **OS environment lookup**: `ENV('PORT') | default(8080)`
- **File imports**: `@import "./base.envx"` (flat namespace, error on redefinition)
- **Typed internals**: the evaluator uses `String`, `Int(i64)`, `Bool(bool)` but
  always exports plain strings to the environment.

### Non-goals
- No `eval`, no shell execution of arbitrary code
- No external scripting engines
- No unsafe dependencies
- The function whitelist is fixed and intentional — do not add arbitrary functions

---

## .envx Syntax

```env
# Comment lines start with #
NAME = "John Doe"
USERNAME = "${{ $NAME | trim | replace(' ', '_') | lower }}"
EMAIL    = "${{ $USERNAME }}@company.com"
PORT     = "${{ ENV('PORT') | default(8080) }}"
APP_ENV  = "prod"
DB_HOST  = "${{ if eq($APP_ENV, 'prod') then 'api.db.com' else 'localhost' }}"
```

### Rules
- **Section headers**: `[SectionName]` to group variables. `SectionName` must match `[A-Za-z_][A-Za-z0-9_]*`.
- One assignment per line: `KEY = "value"`
- `KEY` must match `[A-Za-z_][A-Za-z0-9_]*`
- Everything outside `${{ }}` is literal text
- Inside `${{ }}`: variable refs use `$`, functions chain with `|`, string
  literals use single quotes `'...'`, integers are bare digits
- `@import "./path.envx"` directives are placed at the top (by convention)
- Paths in `@import` are **relative to the file containing the directive**

### Import rules (critical)
- **Flat namespace**: all imported variables enter the global scope directly
- **Error on redefinition**: if two files define the same `KEY`, abort immediately
- **Diamond imports**: if A imports B and C, and both import D, D is loaded once
- **Circular file imports** are detected and aborted (A → B → A)
- **Circular variable dependencies** are also detected (A refs B, B refs A)

---

## Architecture — 4 Phases + Loader

```
.envx file
    │
    ▼
[Phase A] lexer.rs      — FileToken stream (KEY, =, StringContent, @import, \n)
    │
    ▼
[Phase B] parser.rs     — AST: EnvxFile { statements: Vec<Statement> }
    │                     Template scanning: splits "${{ expr }}" into Segments
    │                     ExprToken → Expr AST (recursive descent)
    ▼
[loader.rs]             — Recursive DFS file loading
    │                     Detects circular imports (HashSet<PathBuf> visit stack)
    │                     Merges entries into ResolvedEnv (errors on duplicate KEY)
    ▼
[Phase C] dag.rs        — Builds DiGraph<key, ()> with petgraph
    │                     Calls petgraph::algo::toposort() → error on cycle
    │                     Returns ordered Vec<&str> of variable names
    ▼
[Phase D] evaluator.rs  — Walks sorted order, calls eval_expr() per variable
    │                     HashMap<String, Value> memoization cache
    │                     Dispatches to builtins.rs via function name whitelist
    ▼
HashMap<String, String>  — exported to process environment or stdout
```

---

## File Structure

```
envx/
├── Cargo.toml           — dependencies (see § Crates below)
├── AGENTS.md            — this file
├── src/
│   ├── main.rs          — CLI entry point (clap), orchestrates all phases
│   ├── error.rs         — EnvxError enum (miette + thiserror), Result<T> alias
│   ├── value.rs         — enum Value { Str(String), Int(i64), Bool(bool) }
│   ├── ast.rs           — all AST types (Span, Expr, Template, Statement, ...)
│   ├── lexer.rs         — FileToken, ExprToken, lex_file(), lex_expr()
│   ├── parser.rs        — TODO: Phase B
│   ├── loader.rs        — TODO: file loading, @import resolution, merging
│   ├── dag.rs           — TODO: Phase C (petgraph DAG + toposort)
│   ├── evaluator.rs     — TODO: Phase D (AST walker + memoization)
│   └── builtins.rs      — TODO: function whitelist implementation
```

---

## Crates

| Crate | Version | Purpose |
|-------|---------|---------|
| `clap` | 4 | CLI subcommands with derive macros |
| `logos` | 0.14 | Lexer via DFA-based macros (O(n)) |
| `petgraph` | 0.6 | DAG, `toposort()`, `is_cyclic_directed()` |
| `miette` | 7 | Human-friendly error reports with source spans |
| `thiserror` | 2 | `derive(Error)` for `EnvxError` |
| `indexmap` | 2 | `IndexMap` — `HashMap` that preserves insertion order |

---

## Key Types (already implemented)

### `src/value.rs`

```rust
pub enum Value {
    Str(String),
    Int(i64),
    Bool(bool),
}
// Notable methods:
// .into_string()    → String  (for export)
// .is_truthy()      → bool    (for if/then/else)
// .as_str_repr()    → String  (non-consuming display)
// .type_name()      → &str    (for error messages)
// From<String/&str/i64/bool> impls for ergonomic construction
```

### `src/ast.rs`

```rust
pub struct Span { pub start: usize, pub end: usize }

pub enum Expr {
    Literal(Value, Span),
    VarRef    { name: String, span: Span },
    EnvLookup { key: String,  span: Span },
    Pipe      { expr: Box<Expr>, func: FnCall, span: Span },
    Call(FnCall),
    IfExpr    { cond: Box<Expr>, then_val: Box<Expr>, else_val: Box<Expr>, span: Span },
}

pub struct FnCall { pub name: String, pub args: Vec<Expr>, pub span: Span }

pub enum Segment { Literal(String), Expr(Expr) }
pub struct Template { pub segments: Vec<Segment>, pub span: Span }

pub enum Statement {
    Section { name: String, span: Span },
    Import { raw_path: String, resolved: PathBuf, span: Span },
    Entry  { key: String, template: Template, source: PathBuf, span: Span },
}

pub struct EnvxFile {
    pub path: PathBuf,
    pub source: String,         // raw source text for miette
    pub statements: Vec<Statement>,
}

pub struct ResolvedEnv {
    // IndexMap preserves declaration order (important for export output)
    pub entries: IndexMap<String, (Template, PathBuf)>,
}
```

### `src/error.rs`

```rust
pub type Result<T> = std::result::Result<T, EnvxError>;

pub enum EnvxError {
    UnexpectedChar       { src, span }
    UnterminatedString   { src, span }
    UnclosedExpression   { src, span }
    UnexpectedToken      { expected, found, src, span }
    EmptyExpression      { src, span }
    CircularImport       { cycle: String }
    DuplicateVariable    { key, first_file, second_file }
    BadImportPath        { raw_path, from_file }
    CircularDependency   { cycle: String }
    UndefinedVariable    { name, src, span }
    UnknownFunction      { name, src, span }
    ArityError           { func, expected, got }
    TypeError            { func, expected, got }
    Io                   { path, source: io::Error }
}
```

### `src/lexer.rs`

```rust
pub type Spanned<T> = (T, Span);

// Outer file structure
pub enum FileToken {
    Import,
    Ident,
    Eq,
    StringContent(String),  // raw content between "..." (quotes stripped)
    Comment,
    Newline,
}

// Inner expression content
pub enum ExprToken {
    If, Then, Else, True, False,          // keywords
    VarRef(String),                        // $NAME → "NAME"
    Ident(String),                         // function names
    StrLit(String),                        // 'prod' → "prod"
    IntLit(i64),                           // 8080
    Pipe, LParen, RParen, Comma,
}

// Public API:
pub fn lex_file(source: &str, filename: &str) -> Result<Vec<Spanned<FileToken>>>
pub fn lex_expr(source: &str, base_offset: usize, filename: &str) -> Result<Vec<Spanned<ExprToken>>>
```

---

## Lexer Design Notes

- `FileToken::StringContent` uses a custom logos callback (`scan_string`) that
  manually scans for the closing `"`, advancing the logos cursor past it. This
  allows the raw template string (including `${{ }}`) to be captured as a single
  token for the parser to handle later.
- `ExprToken` keywords (`if`, `then`, `else`, `true`, `false`) are `#[token]`
  rules which take priority over the `#[regex]` `Ident` rule. Logos's longest-
  match rule ensures `iface` is still `Ident("iface")`, not `If + Ident("ace")`.
- `base_offset` in `lex_expr` shifts all spans so that expression-level errors
  point into the correct position within the parent file.

---

## Parser Design (Phase B — TODO)

The parser (`src/parser.rs`) receives `Vec<Spanned<FileToken>>` and:

1. **Line grouping**: Collects tokens between `Newline`s into logical lines,
   skipping `Comment` tokens.
2. **Statement dispatch**:
   - Line starts with `Import` → parse `StringContent` as import path, resolve
     path relative to `EnvxFile::path` parent directory.
   - Line starts with `Ident` → expect `Eq` then `StringContent` → call
     `scan_template()` on the string content.
3. **`scan_template(raw, base_offset, filename)`**: Scans `raw` for `${{`...`}}`
   delimiters (tracking `'...'` single-quoted strings to avoid false `}}` matches),
   producing `Vec<Segment>`. For each expression block, calls `lex_expr()` then
   `parse_expr()`.
4. **`parse_expr(tokens)`**: Recursive descent parser over `Vec<Spanned<ExprToken>>`:
   - Entry point: `parse_pipe_chain()` — handles `|` chains
   - `parse_primary()` — handles `VarRef`, `StrLit`, `IntLit`, `True`, `False`,
     `Ident(...)` (function call), `If ... then ... else ...`, `ENV(...)`

### Expression grammar (informal)

```
expr        := pipe_chain
pipe_chain  := primary ('|' fn_call)*
primary     := VarRef
             | StrLit | IntLit | 'true' | 'false'
             | Ident '(' arg_list ')'       -- function call (no pipe)
             | 'if' expr 'then' expr 'else' expr
arg_list    := (expr (',' expr)*)?
```

`ENV('KEY')` is parsed as a regular `Ident("ENV")` call; the evaluator converts
it to `EnvLookup` at call-dispatch time.

---

## Loader Design (TODO)

`src/loader.rs` exposes:

```rust
pub fn load(root: &Path) -> Result<ResolvedEnv>
```

Internally uses `load_recursive(path, visit_stack, loaded_set, env)`:

```
fn load_recursive(path, visit_stack, loaded_set, env):
    if visit_stack.contains(path):
        error CircularImport { cycle: format!("{}", visit_stack.join(" → ")) }
    if loaded_set.contains(path):
        return  // already processed (diamond import — not an error)

    source = fs::read_to_string(path)?
    file   = parse(source, path)?

    visit_stack.push(path)
    for stmt in file.statements:
        Import { resolved, .. } =>
            load_recursive(resolved, visit_stack, loaded_set, env)
        Entry { key, template, source: src_path, .. } =>
            if env.entries.contains_key(&key):
                error DuplicateVariable { key, first_file, second_file: src_path }
            else:
                env.entries.insert(key, (template, src_path))
    visit_stack.pop()
    loaded_set.insert(path)
```

---

## DAG Design (Phase C — TODO)

`src/dag.rs` exposes:

```rust
pub fn build_and_sort(env: &ResolvedEnv) -> Result<Vec<String>>
```

1. Create a `petgraph::graph::DiGraph<String, ()>` with one node per variable.
2. For each variable, call `template.collect_var_refs()` to get dependencies.
3. Add a directed edge from dependency → dependant (meaning "must evaluate first").
4. Call `petgraph::algo::toposort(&graph, None)`:
   - `Ok(order)` → return variable names in topological order
   - `Err(Cycle { node_id })` → build a human-readable cycle string and return
     `Err(EnvxError::CircularDependency { cycle })`

---

## Evaluator Design (Phase D — TODO)

`src/evaluator.rs` exposes:

```rust
pub fn evaluate(env: &ResolvedEnv, order: &[String]) -> Result<HashMap<String, Value>>
```

- `memo: HashMap<String, Value>` — starts empty, filled in `order` sequence.
- For each key in `order`:
  1. Evaluate `template` by joining all segments.
  2. `Segment::Literal(s)` → append `s` as-is.
  3. `Segment::Expr(expr)` → call `eval_expr(expr, &memo)` → `Value` → coerce to string → append.
  4. Store result in `memo`.

`eval_expr(expr, memo)` pattern-matches on `Expr`:
- `Literal(v, _)` → `v.clone()`
- `VarRef { name, span }` → `memo.get(name).cloned()` or `UndefinedVariable` error
- `EnvLookup { key, _ }` → `std::env::var(key)` → `Value::Str` (missing → empty string or error)
- `Pipe { expr, func, _ }` → `eval_expr(expr)` → call `builtins::dispatch(func.name, vec![result] + eval_args(func.args))`
- `Call(fn_call)` → `builtins::dispatch(fn_call.name, eval_args(fn_call.args))`
- `IfExpr { cond, then_val, else_val, _ }` → if `eval_expr(cond).is_truthy()` then eval `then_val` else eval `else_val`

---

## Builtins Whitelist

All functions are pure (no side effects, no I/O). `src/builtins.rs` dispatches by name:

| Function | Signature | Description |
|----------|-----------|-------------|
| `abs` | `(Int) → Int` | Absolute value |
| `capitalize` | `(Str) → Str` | Capitalize first letter |
| `concat` | `(Str, Str, ...) → Str` | Concatenate strings |
| `date_add` | `(Str, Int, Str) → Str` | Add offset to date (e.g. days) |
| `date_diff` | `(Str, Str, Str) → Int` | Difference between dates |
| `date_format` | `(Str, Str) → Str` | Format date string |
| `day` | `(Str) → Int` | Extract day of month |
| `default` | `(Any, Any) → Any` | Return first arg if truthy, else second |
| `emoji` | `(Str) → Str` | Lookup emoji by name |
| `eq` | `(Any, Any) → Bool` | Structural equality |
| `int` | `(Str) → Int` | Parse integer |
| `len` | `(Str) → Int` | String length |
| `lower` | `(Str) → Str` | ASCII lowercase |
| `month` | `(Str) → Int` | Extract month |
| `now` | `([Str]) → Str` | Current timestamp (optional format) |
| `replace` | `(Str, Str, Str) → Str` | Replace first arg in value with second |
| `round` | `(Str, [Int]) → Str` | Round number to decimals |
| `secret` | `([Int], [Str]) → Str` | Generate random string |
| `timestamp` | `() → Int` | Current UNIX timestamp |
| `title` | `(Str) → Str` | Title case |
| `trim` | `(Str) → Str` | Strip leading/trailing whitespace |
| `truncate` | `(Str, Int) → Str` | Truncate string to length |
| `upper` | `(Str) → Str` | ASCII uppercase |
| `uuid` | `([Int]) → Str` | Generate UUID (v4 or v7) |
| `weekday` | `(Str) → Str` | Extract day of week name |
| `year` | `(Str) → Int` | Extract year |

Any other function name → `EnvxError::UnknownFunction`.

---

## CLI Commands (`src/main.rs`)

```
envx run   <file.envx> [--] <cmd> [args...]
    Evaluate file, inject all variables into the child process's environment,
    exec the command. Uses std::process::Command::envs().

envx export <file.envx>
    Print all resolved variables as shell export statements:
    export KEY="value"
    Suitable for: eval $(envx export app.envx)

envx eval  '<expression>'
    Evaluate a single expression for debugging. Reads OS environment for $VARs.
    Example: envx eval '$HOME | lower'

envx print <file.envx> [--tags]
    Parse and print the resolved variables without executing a command.
    With --tags, groups variables by their [SectionName] tag.

envx fmt   <file.envx> [--check]
    Format the file, aligning '=' signs and normalizing whitespace in template blocks.

envx completions <shell>
    Generate shell completion scripts (bash, zsh, fish, powershell).
```

---

## Implementation Status

| Module | Status | Notes |
|--------|--------|-------|
| `Cargo.toml` | ✅ Done | All deps locked |
| `src/value.rs` | ✅ Done | `Value` enum + impls |
| `src/ast.rs` | ✅ Done | Full AST type definitions |
| `src/error.rs` | ✅ Done | All error variants with miette |
| `src/lexer.rs` | ✅ Done | `FileToken`, `ExprToken`, `lex_file`, `lex_expr` + tests |
| `src/parser.rs` | ✅ Done | `parse()`, `FileParser`, `scan_template()`, `ExprParser` + 24 tests |
| `src/loader.rs` | ✅ Done | `load()`, DFS con `visit_stack`+`loaded`, merge con error en redefinición + 17 tests |
| `src/dag.rs` | ✅ Done | Kahn's BFS sort (stable order), `kosaraju_scc` for cycle path + 7 tests |
| `src/evaluator.rs` | ✅ Done | `evaluate()`, memoization, full `Expr` dispatch + 16 tests |
| `src/builtins.rs` | ✅ Done | 26 whitelisted functions (`trim/lower/upper/len/replace/concat/default/eq/date_add/etc...`) |
| `src/main.rs` | ✅ Done | clap CLI: `run`, `export`, `eval`, `print`, `completions` + miette error reporting |

---

## Release Process

- The project uses GitHub Actions for an automated release process.
- The release workflow is triggered automatically upon pushing a Git tag that starts with `v*` (e.g., `v0.1.2`).
- Binary artifacts are built for multiple platforms, packaged into `.tar.gz` with their shell completions, and attached to a GitHub Release.


---

## Coding Conventions

- No `unwrap()` in non-test code — propagate errors via `?` and `EnvxError`
- No external crates beyond those in `Cargo.toml` without discussion
- No `unsafe` blocks
- All public functions must have a `#[cfg(test)]` module with at least one test
- Spans are in **byte offsets** (not char offsets) — consistent with logos output
- `miette::NamedSource::new(filename, source.clone())` for every error with source context
- `SourceSpan::from((start, length))` — miette uses `(offset, length)`, not `(start, end)`
- `Result<T>` always means `std::result::Result<T, EnvxError>` (re-exported in `error.rs`)
