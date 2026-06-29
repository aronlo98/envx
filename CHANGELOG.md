# Changelog

All notable changes to `envx` will be documented in this file.

The format is based on Keep a Changelog, and the project follows Semantic Versioning.

## [Unreleased]

## [0.1.4] - 2026-06-29

### Added

- `envx print --tag TAG` / `-t TAG` — filter printed variables to those belonging to the given section tag. Repeatable for multiple tags: `-t database -t app`. Works with or without `-T`.
- `envx run --tag TAG` / `-t TAG` — only inject variables from the specified section(s) into the child process environment. All variables are still evaluated (to resolve cross-section dependencies), but only those in the requested tag(s) are passed to the process.

### Changed

- `envx print --tags` short flag renamed from `-t` to `-T`, freeing `-t` for the new `--tag` filter flag.

---

## [0.1.3] - 2026-06-29

### Added

#### Syntax

- **Section headers** — `[section_name]` groups variables visually (INI-style). Sections have no effect on evaluation; variables remain in a flat shared namespace.

```env
[database]
DB_HOST = "localhost"
DB_PORT = "5432"

[app]
APP_ENV = "prod"
PORT    = "3000"
```

#### CLI

- `envx fmt <file>` — format a `.envx` file in-place:
  - Aligns `=` across all assignments to the width of the longest key.
  - Normalises whitespace inside every `${{ }}` expression block (`${{  secret()   }}` → `${{ secret() }}`).
  - Normalises section headers: strips leading/trailing whitespace and trims spaces inside brackets (`  [ hola  ]` → `[hola]`).
  - `--check` flag exits with a non-zero code if the file is not already formatted (useful in CI / pre-commit hooks).
- `envx print --tags` / `envx print -t` — print variables in a three-column `TAG | KEY | VALUE` table sorted by tag name ascending. Variables without a section appear with an empty `TAG` and sort first.

### Changed

- `envx print` default output does not show section headers — the flat `KEY | VALUE` table is unchanged regardless of whether the file uses sections. Use `--tags` to see section grouping.
- `envx export` documentation now shows `source <(envx export app.envx)` as an alternative to `eval $(envx export app.envx)`.

---

## [0.1.2] - 2026-06-28

### Added

#### Built-in functions

- `secret()` — generates a random string (32 alphanumeric chars by default). Accepts optional `length` and an alphabet preset: `'hex'`, `'base64'`, `'base64url'`, `'alpha'`, `'numeric'`, or a custom character set.
- `capitalize` — first letter uppercase, rest lowercase (`"hello world" → "Hello world"`).
- `title` — each word capitalised (`"hello world" → "Hello World"`).
- `truncate(n)` — keep only the first `n` characters.
- `abs` — absolute value of an `Int` pipe receiver.
- `round` / `round(n)` — round a numeric string to `n` decimal places (default 0).
- `int` — parse a string as an integer, truncating decimals (`"3.9" → 3`).
- `uuid()` / `uuid(4)` / `uuid(7)` — generate a UUID. Version 4 is random; version 7 is time-ordered and suitable as a database primary key.
- `emoji('name')` — return a named emoji character (53 entries across animals, faces, dev/tech, nature, food, and symbols).
- `timestamp()` — current Unix timestamp in seconds.
- `date_add(n, unit)` — add (or subtract with negative `n`) time to a date. Units: `seconds`, `minutes`, `hours`, `days`, `weeks`, `months`, `years` (singular and plural both accepted).
- `date_diff(date2, unit)` — difference between two dates (`date2 − receiver`) in the given unit. Negative when `date2` is earlier.
- `date_format(fmt)` — reformat a date string using moment.js-style tokens (same as `now()`).
- `year()`, `month()`, `day()` — extract the year, month (1–12), or day (1–31) from a date string.
- `weekday()` — full weekday name in English (`"Monday"` … `"Sunday"`).
- `now(fmt)` — extended to accept moment.js-style format tokens (`YYYY`, `MM`, `DD`, `HH`, `mm`, `ss`, `MMMM`, `MMM`, `DDDD`, `DDD`, `hh`, `A`).

All date pipe functions accept `"YYYY-MM-DD"` or `"YYYY-MM-DDTHH:MM:SS"` as input and can be chained:

```env
EXPIRES = "${{ now() | date_add(90, 'days') | date_format('YYYY-MM-DD') }}"
```

#### Syntax

- **Bare (unquoted) values** — keys can now be assigned without double quotes for plain literals: `PORT = 8080`, `APP_ENV = development`.
- **Inline comments** — `#` comments are now valid after a value on the same line: `KEY = "value" # note`.

#### CLI

- `envx print <file>` — print variables in an aligned SQL-style table (`KEY | VALUE`).
- `envx completions <shell>` — print the completion script for the given shell to stdout.
- `envx --version` — print the current version.

#### Developer experience

- `Makefile` with targets: `build`, `release`, `install`, `uninstall`, `test`, `lint`, `fmt`, `check`, `clean`.

### Changed

- `envx print` output uses a psql-style aligned table format with a `KEY | VALUE` header and separator line.
- Updated `petgraph` from 0.6 to 0.8.
- Updated `rand` from 0.8 to 0.9 (migrated `thread_rng()` → `rng()` and `gen_range` → `random_range`).

### Fixed

- Suppressed spurious `dead_code` warning on `find_var_ref_at` methods in `ast.rs` (the methods are part of the public API consumed by an external tool).

---

## [0.1.1] - 2026-06-28

### Changed

- Updated `.gitignore` to exclude generated release artifacts such as `dist/`, `*.tar.gz`, and `*.sha256`.

### Fixed

- Fixed release packaging so generated archives always include the `envx` binary and the `completions/` directory.
- Avoided `tar` path resolution issues in GitHub Actions by staging release assets into a dedicated directory before archiving.

---

## [0.1.0] - 2026-06-28

### Added

- Initial public release of `envx`.
- `.envx` parsing with template expressions, pipe chains, conditionals, imports, and dependency-aware evaluation.
- CLI commands for `run`, `export`, `eval`, `print`, and `completions`.
- Generated shell completions for Bash, Zsh, Fish, and PowerShell.
