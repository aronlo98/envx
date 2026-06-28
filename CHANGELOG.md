# Changelog

All notable changes to `envx` will be documented in this file.

The format is based on Keep a Changelog, and the project follows Semantic Versioning.

## [Unreleased]

## [0.1.1] - 2026-06-28

### Changed

- Updated `.gitignore` to exclude generated release artifacts such as `dist/`, `*.tar.gz`, and `*.sha256`.

### Fixed

- Fixed release packaging so generated archives always include the `envx` binary and the `completions/` directory.
- Avoided `tar` path resolution issues in GitHub Actions by staging release assets into a dedicated directory before archiving.

## [0.1.0] - 2026-06-28

### Added

- Initial public release of `envx`.
- `.envx` parsing with template expressions, pipe chains, conditionals, imports, and dependency-aware evaluation.
- CLI commands for `run`, `export`, `eval`, `print`, and `completions`.
- Generated shell completions for Bash, Zsh, Fish, and PowerShell.
