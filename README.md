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
make install
```

### Verify

```sh
envx --version
```

### Shell completions

Completion scripts for bash, zsh, fish, and PowerShell are included in every release inside the `completions/` folder.

**zsh**
```sh
cp completions/_envx ~/.zfunc/_envx
# Make sure ~/.zfunc is in your fpath (add to ~/.zshrc if needed):
# fpath=(~/.zfunc $fpath)
```

**bash**
```sh
cp completions/envx.bash ~/.bash_completion.d/envx
```

**fish**
```sh
cp completions/envx.fish ~/.config/fish/completions/envx.fish
```

**PowerShell**
```powershell
cp completions/_envx.ps1 $PROFILE\..\completions\_envx.ps1
```

---

## Syntax

### Basic assignment

```env
KEY     = "value"
PORT    = 8080
APP_ENV = development
```

- One assignment per line.
- Keys must match `[A-Za-z_][A-Za-z0-9_]*`.
- Values may be **double-quoted strings** or **bare (unquoted) literals**.
- Everything outside `${{ }}` is literal text.
- Comments start with `#`.

> **Note:** Bare values are always plain strings — they cannot contain `${{ }}` template blocks. Use double quotes when you need dynamic expressions.

### String literals inside expressions

String arguments inside `${{ }}` blocks use **single quotes**:

```env
ENV_LABEL = "${{ if eq($APP_ENV, 'prod') then 'Production' else 'Development' }}"
TIMESTAMP = "${{ now('YYYYMMDD') }}"
```

Double quotes delimit the outer value, so single quotes are used inside to avoid ambiguity.

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

### Sections

Group variables visually with `[section_name]` headers (INI-style). Sections are purely organisational — they have no effect on evaluation and variables remain in a flat shared namespace.

```env
[database]
DB_HOST = "localhost"
DB_PORT = "5432"
DB_NAME = "myapp"

[app]
APP_ENV = "prod"
PORT    = "3000"
```

Section names must match `[A-Za-z_][A-Za-z0-9_]*`. Use `envx fmt` to normalise spacing around brackets.

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
| `now` | `() → Str` or `(fmt:Str) → Str` | Current local date/time. No argument returns ISO 8601. Accepts moment.js-style format tokens (see below) |
| `secret` | `() → Str`, `(len:Int) → Str`, or `(len:Int, preset:Str) → Str` | Random string. Default: 32 alphanumeric chars. Second argument accepts a named preset (`'hex'`, `'base64'`, `'base64url'`, `'alpha'`, `'numeric'`) or a custom character set |
| `capitalize` | `Str → Str` | First letter uppercase, rest lowercase: `"hello world" → "Hello world"` |
| `title` | `Str → Str` | Each word capitalized: `"hello world" → "Hello World"` |
| `truncate` | `Str, n:Int → Str` | Keep only the first `n` characters |
| `abs` | `Int → Int` | Absolute value. Use after `\| int` if the input is a string |
| `round` | `Str → Str` or `Str, decimals:Int → Str` | Round a numeric string. Default: 0 decimal places |
| `int` | `Str → Int` | Parse a string as an integer. Truncates decimals (`"3.9" → 3`) |
| `uuid` | `() → Str` or `(version:Int) → Str` | Generate a UUID. Supported versions: `4` (random, default), `7` (time-ordered, sortable) |
| `timestamp` | `() → Int` | Current Unix timestamp in seconds (since epoch) |
| `date_add` | `Str, n:Int, unit:Str → Str` | Add time to a date. Negative `n` subtracts. Units: `seconds`, `minutes`, `hours`, `days`, `weeks`, `months`, `years` (singular and plural both accepted) |
| `date_diff` | `Str, date2:Str, unit:Str → Int` | Difference between two dates (`date2 − receiver`) in the given unit. Negative when `date2` is earlier |
| `date_format` | `Str, fmt:Str → Str` | Reformat a date string using moment.js-style tokens (same as `now()`) |
| `year` | `Str → Int` | Year component of a date |
| `month` | `Str → Int` | Month component (1–12) |
| `day` | `Str → Int` | Day of month (1–31) |
| `weekday` | `Str → Str` | Full weekday name in English (`"Monday"` … `"Sunday"`) |

All date pipe functions accept `"YYYY-MM-DD"` or `"YYYY-MM-DDTHH:MM:SS"` as input. `date_add` always outputs ISO datetime format; pipe through `date_format` to reformat.

```env
EXPIRES   = "${{ now() | date_add(90, 'days') | date_format('YYYY-MM-DD') }}"
BUILD_TAG = "${{ now() | date_format('YYYYMMDD') }}"
DAYS_LEFT = "${{ now() | date_diff('2027-01-01', 'days') }}"
BUILT_TS  = "${{ timestamp() }}"
YEAR      = "${{ now() | year() }}"
WEEKDAY   = "${{ now() | weekday() }}"
```

Functions that take a pipe receiver (`trim`, `lower`, `upper`, `len`, `replace`, `default`) must be used after `|`:

```env
CLEAN = "${{ $RAW | trim | lower }}"
```

Functions that do not take a pipe receiver (`concat`, `eq`, `now`) are called directly:

```env
BOTH      = "${{ concat($FIRST, ' ', $LAST) }}"
IS_PROD   = "${{ eq($APP_ENV, 'prod') }}"
BUILT_AT  = "${{ now() }}"
BUILD_TAG = "${{ now('YYYYMMDD') }}"
API_KEY      = "${{ secret() }}"
TOKEN        = "${{ secret(64) }}"
HEX_TOKEN    = "${{ secret(32, 'hex') }}"
B64_TOKEN    = "${{ secret(32, 'base64url') }}"
PIN          = "${{ secret(6, 'numeric') }}"
CUSTOM_TOKEN = "${{ secret(16, 'abcdef0123456789') }}"
```

### `now()` format tokens

| Token  | Meaning            | Example   |
|--------|--------------------|-----------|
| `YYYY` | 4-digit year       | `2026`    |
| `YY`   | 2-digit year       | `26`      |
| `MMMM` | Full month name    | `January` |
| `MMM`  | Short month name   | `Jan`     |
| `MM`   | 2-digit month      | `06`      |
| `DDDD` | Full weekday name  | `Monday`  |
| `DDD`  | Short weekday name | `Mon`     |
| `DD`   | 2-digit day        | `28`      |
| `HH`   | 24-hour hour       | `15`      |
| `hh`   | 12-hour hour       | `03`      |
| `mm`   | Minutes            | `30`      |
| `ss`   | Seconds            | `00`      |
| `A`    | AM/PM marker       | `PM`      |

Tokens not listed above are passed through unchanged, so separators like `-`, `:`, and `T` work as-is.

---

## CLI

### `envx export <file.envx>`

Evaluate the file and print each variable as a shell `export` statement. Suitable for use with `eval` or `source`:

```sh
# Load into the current shell session
eval $(envx export app.envx)
echo $DB_HOST

# Alternative — same effect, no eval
source <(envx export app.envx)
```

### `envx run <file.envx> -- <command> [args...]`

Evaluate the file and run a command with the variables injected into its environment. The exit code of the child process is propagated exactly.

```sh
envx run app.envx -- python manage.py migrate
envx run app.envx -- npm start
envx run staging.envx -- ./deploy.sh
```

Use `--tag` / `-t` to inject only variables from specific sections. All variables are still evaluated (to resolve cross-section dependencies), but only those in the requested tag(s) are passed to the process:

```sh
envx run -t database app.envx -- ./migrate.sh
envx run -t database -t app app.envx -- ./start.sh
```

### `envx print <file.envx>`

Evaluate the file and print all variables in an aligned `KEY | VALUE` table.

```sh
$ envx print app.envx
KEY      | VALUE
---------+---------
APP_ENV  | prod
USERNAME | alice_smith
PORT     | 3000
```

**`-T` / `--tags`** — add a `TAG` column and sort by tag name ascending:

```sh
$ envx print -T app.envx
TAG      | KEY     | VALUE
---------+---------+-------
app      | APP_ENV | prod
database | DB_HOST | localhost
```

**`-t TAG` / `--tag TAG`** — filter to variables from a specific section (repeatable):

```sh
$ envx print -t database app.envx
$ envx print -T -t database -t app app.envx
```

### `envx fmt <file.envx>`

Format a `.envx` file in-place — aligns `=` across all assignments to the width of the longest key.

```sh
$ envx fmt app.envx
formatted: app.envx
```

Before:
```env
APP_ENV="prod"
BUILT_AT="${{ now('YYYY') }}"
TOKEN="${{ secret(64) }}"
HEX_TOKEN =    "${{ secret(32, 'hex') }}"
```

After:
```env
APP_ENV   = "prod"
BUILT_AT  = "${{ now('YYYY') }}"
TOKEN     = "${{ secret(64) }}"
HEX_TOKEN = "${{ secret(32, 'hex') }}"
```

Use `--check` to verify formatting without modifying the file (useful in CI):

```sh
envx fmt --check app.envx
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

$ envx eval "now('YYYY-MM-DD')"
2026-06-28
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
  help: allowed functions: abs, capitalize, concat, date_add, date_diff, date_format, day, default, emoji, eq, int, len, lower, month, now, replace, round, secret, title, trim, truncate, upper, uuid, weekday, year
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

## Development

```sh
make build      # debug build (fast compile)
make release    # optimised build (LTO + strip)
make test       # run all tests
make lint       # clippy — warnings as errors
make fmt        # format with rustfmt
make check      # type-check without producing a binary
make clean      # remove build artefacts
make install    # install to ~/.cargo/bin
make uninstall  # remove from ~/.cargo/bin
```

---

## License

MIT
