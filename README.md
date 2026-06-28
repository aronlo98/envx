# envx

A modern, safe replacement for `dotenv`. It processes `.envx` files — a superset of the classic `KEY="value"` format — and adds dynamic variables, pipe-based text manipulation, conditional logic, inter-variable dependencies, and file imports.

```env
APP_ENV  = "prod"
NAME     = "  Alice Smith  "
USERNAME = "${{ $NAME | trim | lower | replace(' ', '_') }}"
EMAIL    = "${{ $USERNAME }}@company.com"
PORT     = "${{ ENV('PORT') | default('3000') }}"
DB_HOST  = "${{ if eq($APP_ENV, 'prod') then 'api.db.com' else 'localhost' }}"
```

```sh
$ envx export app.envx
export APP_ENV="prod"
export NAME="  Alice Smith  "
export USERNAME="alice_smith"
export EMAIL="alice_smith@company.com"
export PORT="3000"
export DB_HOST="api.db.com"

$ envx run app.envx -- node server.js
```

---

## Why envx?

Plain `.env` files are static. Once your config grows past a handful of keys you end up copy-pasting values, maintaining per-environment duplicates, or reaching for templating systems that bring in a full scripting runtime.

`envx` fills that gap:

- **No duplication** — derive `EMAIL` from `USERNAME`, `DB_URL` from `DB_HOST` + `DB_PORT`.
- **No shell eval** — transformations run through a fixed, auditable function whitelist. There is no `eval`, no arbitrary shell execution, no subprocesses.
- **No runtime** — a single, self-contained binary with no interpreter dependency.
- **Composable files** — split config into `base.envx`, `prod.envx`, `secrets.envx` and import them.

---

## Installation

### From source (requires Rust ≥ 1.80)

```sh
git clone https://github.com/aronlo98/envx
cd envx
cargo install --path .
```

### Verify

```sh
envx --version
```

---

## Syntax

### Basic assignment

```env
KEY = "value"
```

- One assignment per line.
- Keys must match `[A-Za-z_][A-Za-z0-9_]*`.
- Values are always double-quoted strings.
- Everything outside `${{ }}` is literal text.
- Comments start with `#`.

### Template blocks

Embed a dynamic expression anywhere inside the double-quoted value using `${{ }}`:

```env
GREETING = "Hello, ${{ $FIRST_NAME }}!"
```

Multiple blocks per value are allowed:

```env
JDBC_URL = "jdbc:postgresql://${{ $DB_HOST }}:${{ $DB_PORT }}/${{ $DB_NAME }}"
```

### Variable references

Reference another variable in the same file (or any imported file) with `$NAME`:

```env
FIRST_NAME = "Ada"
LAST_NAME  = "Lovelace"
FULL_NAME  = "${{ $FIRST_NAME }} ${{ $LAST_NAME }}"
```

Variables can be declared in any order — `envx` resolves dependencies automatically via topological sort.

### Pipe chains

Pass a value through a series of transformation functions with `|`:

```env
SLUG = "${{ $FULL_NAME | trim | lower | replace(' ', '-') }}"
# → "ada-lovelace"
```

### Conditional expressions

```env
APP_ENV = "prod"
DB_HOST = "${{ if eq($APP_ENV, 'prod') then 'db.prod.internal' else 'localhost' }}"
```

### OS environment lookup

Read a variable from the process environment at runtime:

```env
PORT = "${{ ENV('PORT') | default('8080') }}"
```

`ENV` returns an empty string if the variable is not set, so chaining `| default(...)` is the idiomatic fallback pattern.

### Imports

Split your configuration into multiple files:

```env
# app.envx
@import "./base.envx"
@import "./secrets.envx"

APP_ENV = "prod"
DB_URL  = "postgres://${{ $DB_USER }}:${{ $DB_PASS }}@${{ $DB_HOST }}/mydb"
```

Import rules:
- Paths are **relative to the file containing the directive**.
- All imported variables enter a **flat, shared namespace** — no prefixes.
- **Redefinition is an error**: if two files define the same key, `envx` aborts immediately with both file names.
- **Diamond imports** are handled correctly: if A imports B and C, and both import D, D is loaded exactly once.
- **Circular imports** (A → B → A) are detected and reported.

---

## Built-in functions

All transformations go through a fixed whitelist. There are no user-defined functions and no arbitrary code execution.

| Function | Signature | Description |
|----------|-----------|-------------|
| `trim` | `Str → Str` | Remove leading and trailing whitespace |
| `lower` | `Str → Str` | Convert to lowercase |
| `upper` | `Str → Str` | Convert to uppercase |
| `len` | `Str → Int` | Length in bytes |
| `replace` | `Str, from:Str, to:Str → Str` | Replace all occurrences of `from` with `to` |
| `concat` | `Str... → Str` | Concatenate two or more strings |
| `default` | `Str, fallback:Str → Str` | Return `fallback` if the input is an empty string |
| `eq` | `Any, Any → Bool` | True if both arguments have the same string representation |

Functions that take a pipe receiver (`trim`, `lower`, `upper`, `len`, `replace`, `default`) must be used after `|`:

```env
CLEAN = "${{ $RAW | trim | lower }}"
```

Functions that do not take a pipe receiver (`concat`, `eq`) are called directly:

```env
BOTH   = "${{ concat($FIRST, ' ', $LAST) }}"
IS_PROD = "${{ eq($APP_ENV, 'prod') }}"
```

---

## CLI

### `envx export <file.envx>`

Evaluate the file and print each variable as a shell `export` statement. Suitable for use with `eval`:

```sh
eval $(envx export app.envx)
echo $DB_HOST
```

### `envx run <file.envx> -- <command> [args...]`

Evaluate the file and run a command with the variables injected into its environment. The exit code of the child process is propagated exactly.

```sh
envx run app.envx -- python manage.py migrate
envx run app.envx -- npm start
envx run staging.envx -- ./deploy.sh
```

### `envx eval '<expression>'`

Evaluate a single expression. Variable references (`$NAME`) resolve against the current OS environment, making this useful for quick debugging.

```sh
$ envx eval '$HOME | lower'
/users/alice

$ envx eval "concat('hello', '-', 'world') | upper"
HELLO-WORLD

$ envx eval "if eq('prod', 'prod') then 'yes' else 'no'"
yes
```

> **Note:** Use shell single quotes around expressions that contain `$` to prevent the shell from expanding them before `envx` sees them.

---

## Error messages

`envx` uses [miette](https://github.com/zkat/miette) for human-readable, source-annotated error messages.

**Undefined variable:**
```
envx::eval::undefined_var

  × undefined variable `$TYPO`
   ╭─[app.envx:4:14]
 4 │ EMAIL = "${{ $TYPO }}@example.com"
   ·              ──┬──
   ·                ╰── referenced here but never defined
   ╰────
```

**Circular dependency:**
```
envx::dag::circular_dep

  × circular dependency detected: A → B → A
  help: break the dependency cycle by removing one of the variable references
```

**Duplicate variable:**
```
envx::load::duplicate_var

  × duplicate variable `DB_HOST`: first defined in `base.envx`,
    redefined in `overrides.envx`
  help: each variable may only be defined once across all imported files
```

**Unknown function:**
```
envx::eval::unknown_fn

  × unknown function `frobulate`
  help: allowed functions: trim, lower, upper, replace, concat, default, eq, len
```

---

## Example: multi-file configuration

```
config/
├── base.envx       # shared across all environments
├── prod.envx       # production overrides
└── dev.envx        # development overrides
```

**`config/base.envx`**
```env
APP_NAME = "myapp"
LOG_LEVEL = "info"
DB_PORT   = "5432"
DB_NAME   = "myapp_db"
```

**`config/prod.envx`**
```env
@import "./base.envx"

APP_ENV  = "prod"
DB_HOST  = "db.prod.internal"
DB_USER  = "${{ ENV('DB_USER') }}"
DB_PASS  = "${{ ENV('DB_PASS') }}"
DB_URL   = "postgres://${{ $DB_USER }}:${{ $DB_PASS }}@${{ $DB_HOST }}:${{ $DB_PORT }}/${{ $DB_NAME }}"
LOG_LEVEL = "warn"
```

```sh
envx run config/prod.envx -- ./start.sh
```

---

## How it works

`envx` processes files through a four-phase pipeline:

```
.envx file(s)
    │
    ▼
Lexer       — tokenises the file into KEY, =, string contents, @import, newlines
    │
    ▼
Parser      — builds an AST; splits "${{ expr }}" segments; parses expressions
    │
    ▼
Loader      — recursively resolves @import directives (DFS); detects cycles and
              duplicate keys; merges everything into a flat ResolvedEnv
    │
    ▼
DAG         — builds a dependency graph with petgraph; topologically sorts
              variables; detects circular references
    │
    ▼
Evaluator   — walks the sorted order; evaluates each template; dispatches pipe
              and call expressions to the built-in function whitelist;
              memoises results
    │
    ▼
String map  — exported to the child process environment or printed to stdout
```

---

## Building from source

```sh
# Debug build (faster compile, larger binary)
cargo build

# Release build (optimised, ~2× smaller binary via LTO + strip)
cargo build --release

# Run all tests
cargo test
```

---

## License

MIT
