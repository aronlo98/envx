use std::{
    collections::HashSet,
    path::{Path, PathBuf},
};

use crate::{
    ast::{LayoutItem, ResolvedEnv, Statement},
    error::{EnvxError, Result},
    parser::parse,
};

// ─── Public entry point ───────────────────────────────────────────────────────

/// Load a `.envx` file and all its transitive `@import`s, returning a flat
/// `ResolvedEnv` with every variable definition merged into a single map.
///
/// Two levels of cycle detection run during loading:
///
/// **Level 1 — file cycles** (`visit_stack`): a `Vec<PathBuf>` that tracks the
/// current DFS path. If the file we are about to load is already in the stack,
/// we have a circular import (`A → B → A`). We abort with `CircularImport`
/// and build a human-readable cycle string for the user.
///
/// **Level 2 — diamond imports** (`loaded`): a `HashSet<PathBuf>` of canonical
/// paths that have been fully processed. If a file appears more than once via
/// different import chains (diamond pattern), we skip it on the second visit
/// rather than re-processing it — this prevents false duplicate-variable errors.
///
/// **Redefinition policy**: two files defining the same key is always an error,
/// regardless of import depth. The error names both origin files.
pub fn load(root: &Path) -> Result<ResolvedEnv> {
    let root = fs_canonicalize(root, None)?;
    let mut env = ResolvedEnv::default();
    let mut visit_stack: Vec<PathBuf> = Vec::new();
    let mut loaded: HashSet<PathBuf> = HashSet::new();
    load_recursive(&root, &mut visit_stack, &mut loaded, &mut env)?;
    Ok(env)
}

// ─── Recursive loader ─────────────────────────────────────────────────────────

fn load_recursive(
    path: &Path,
    visit_stack: &mut Vec<PathBuf>,
    loaded: &mut HashSet<PathBuf>,
    env: &mut ResolvedEnv,
) -> Result<()> {
    // ── Diamond guard: already fully processed → skip ─────────────────────────
    if loaded.contains(path) {
        return Ok(());
    }

    // ── Cycle detection: already in the current DFS path → abort ─────────────
    if visit_stack.iter().any(|p| p == path) {
        return Err(EnvxError::CircularImport {
            cycle: cycle_string(visit_stack, path),
        });
    }

    // ── Read source ───────────────────────────────────────────────────────────
    let source = std::fs::read_to_string(path).map_err(|e| EnvxError::Io {
        path: path.display().to_string(),
        source: e,
    })?;

    let filename = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("<unknown>");

    let file = parse(&source, filename, path.to_path_buf())?;

    // Store raw source so the evaluator can build source-context diagnostics.
    env.sources.insert(path.to_path_buf(), source);

    // ── DFS: push → process statements → pop ─────────────────────────────────
    visit_stack.push(path.to_path_buf());

    for stmt in file.statements {
        match stmt {
            Statement::Import { resolved, raw_path, .. } => {
                // Canonicalize the resolved path; error if the target file
                // does not exist so the user gets a clear message.
                let canonical = fs_canonicalize(&resolved, Some(&raw_path))?;
                load_recursive(&canonical, visit_stack, loaded, env)?;
            }
            Statement::Section { name, .. } => {
                env.layout.push(LayoutItem::Section(name));
            }
            Statement::Entry { key, template, source: src_file, .. } => {
                if let Some((_, first)) = env.entries.get(&key) {
                    return Err(EnvxError::DuplicateVariable {
                        key,
                        first_file: first.display().to_string(),
                        second_file: src_file.display().to_string(),
                    });
                }
                env.layout.push(LayoutItem::Entry(key.clone()));
                env.entries.insert(key, (template, src_file));
            }
        }
    }

    visit_stack.pop();
    loaded.insert(path.to_path_buf());
    Ok(())
}

// ─── Helpers ──────────────────────────────────────────────────────────────────

/// Canonicalize `path` to an absolute, symlink-resolved form.
///
/// When `raw_import` is `Some`, the file not found case is reported as a
/// `BadImportPath` (shows the `@import` literal the user wrote).
/// When it is `None` (root file), use a plain `Io` error.
fn fs_canonicalize(path: &Path, raw_import: Option<&str>) -> Result<PathBuf> {
    std::fs::canonicalize(path).map_err(|e| match raw_import {
        Some(raw) => EnvxError::BadImportPath {
            raw_path: raw.to_string(),
            from_file: path.display().to_string(),
        },
        None => EnvxError::Io { path: path.display().to_string(), source: e },
    })
}

/// Build a human-readable cycle string for `CircularImport` errors.
///
/// Finds where the repeated path first appears in the stack and prints only
/// that portion, appending the repeated file name at the end:
/// `"app.envx → base.envx → shared.envx → app.envx"`
fn cycle_string(visit_stack: &[PathBuf], repeated: &Path) -> String {
    let start = visit_stack.iter().position(|p| p == repeated).unwrap_or(0);
    visit_stack[start..]
        .iter()
        .map(|p| short_name(p))
        .chain(std::iter::once(short_name(repeated)))
        .collect::<Vec<_>>()
        .join(" → ")
}

/// Best-effort short name for a path: the file name if available, else full path.
fn short_name(p: &Path) -> String {
    p.file_name()
        .and_then(|n| n.to_str())
        .unwrap_or_else(|| p.to_str().unwrap_or("<unknown>"))
        .to_string()
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        fs,
        sync::atomic::{AtomicU32, Ordering},
    };

    // ── Test-only temp directory ──────────────────────────────────────────────

    /// A temporary directory that is created on construction and deleted on drop.
    /// Uses an atomic counter for uniqueness across parallel test threads.
    struct TempDir {
        path: PathBuf,
    }

    static COUNTER: AtomicU32 = AtomicU32::new(0);

    impl TempDir {
        fn new() -> Self {
            let n = COUNTER.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir().join(format!("envx_loader_test_{}", n));
            fs::create_dir_all(&path).unwrap();
            TempDir { path }
        }

        /// Write `content` to `<tmpdir>/<name>` and return the absolute path.
        fn write(&self, name: &str, content: &str) -> PathBuf {
            let p = self.path.join(name);
            fs::write(&p, content).unwrap();
            p
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.path);
        }
    }

    // ── Basic loading ─────────────────────────────────────────────────────────

    #[test]
    fn single_file_loads_correctly() {
        let tmp = TempDir::new();
        let f = tmp.write("app.envx", "NAME = \"Alice\"\nAGE = \"30\"\n");
        let env = load(&f).unwrap();
        assert_eq!(env.entries.len(), 2);
        assert!(env.entries.contains_key("NAME"));
        assert!(env.entries.contains_key("AGE"));
    }

    #[test]
    fn insertion_order_preserved() {
        let tmp = TempDir::new();
        let f = tmp.write("app.envx", "C = \"3\"\nA = \"1\"\nB = \"2\"\n");
        let env = load(&f).unwrap();
        let keys: Vec<&str> = env.entries.keys().map(|s| s.as_str()).collect();
        assert_eq!(keys, &["C", "A", "B"]);
    }

    // ── Imports ───────────────────────────────────────────────────────────────

    #[test]
    fn simple_import() {
        let tmp = TempDir::new();
        tmp.write("base.envx", "DB_HOST = \"localhost\"\nDB_PORT = \"5432\"\n");
        let app = tmp.write("app.envx", "@import \"./base.envx\"\nAPP_NAME = \"myapp\"\n");

        let env = load(&app).unwrap();
        assert_eq!(env.entries.len(), 3);
        assert!(env.entries.contains_key("DB_HOST"));
        assert!(env.entries.contains_key("DB_PORT"));
        assert!(env.entries.contains_key("APP_NAME"));
    }

    #[test]
    fn imported_vars_come_before_local_vars() {
        // Imports are DFS-first, so base.envx entries appear before app.envx entries
        // in the IndexMap.
        let tmp = TempDir::new();
        tmp.write("base.envx", "FIRST = \"1\"\n");
        let app = tmp.write("app.envx", "@import \"./base.envx\"\nSECOND = \"2\"\n");

        let env = load(&app).unwrap();
        let keys: Vec<&str> = env.entries.keys().map(|s| s.as_str()).collect();
        assert_eq!(keys, &["FIRST", "SECOND"]);
    }

    #[test]
    fn chained_imports() {
        // app → base → shared
        let tmp = TempDir::new();
        tmp.write("shared.envx", "SHARED = \"s\"\n");
        tmp.write("base.envx", "@import \"./shared.envx\"\nBASE = \"b\"\n");
        let app = tmp.write("app.envx", "@import \"./base.envx\"\nAPP = \"a\"\n");

        let env = load(&app).unwrap();
        let keys: Vec<&str> = env.entries.keys().map(|s| s.as_str()).collect();
        assert_eq!(keys, &["SHARED", "BASE", "APP"]);
    }

    #[test]
    fn diamond_import_loads_shared_file_once() {
        // app imports both left and right; both import shared.
        // shared.envx must be loaded only once (no duplicate-variable error).
        let tmp = TempDir::new();
        tmp.write("shared.envx", "SHARED = \"yes\"\n");
        tmp.write("left.envx", "@import \"./shared.envx\"\nLEFT = \"l\"\n");
        tmp.write("right.envx", "@import \"./shared.envx\"\nRIGHT = \"r\"\n");
        let app = tmp.write(
            "app.envx",
            "@import \"./left.envx\"\n@import \"./right.envx\"\nAPP = \"a\"\n",
        );

        let env = load(&app).unwrap();
        // SHARED appears exactly once
        assert_eq!(env.entries.len(), 4);
        let keys: Vec<&str> = env.entries.keys().map(|s| s.as_str()).collect();
        assert_eq!(keys, &["SHARED", "LEFT", "RIGHT", "APP"]);
    }

    #[test]
    fn multiple_imports_in_one_file() {
        let tmp = TempDir::new();
        tmp.write("db.envx", "DB_HOST = \"localhost\"\n");
        tmp.write("cache.envx", "CACHE_URL = \"redis://localhost\"\n");
        let app = tmp.write(
            "app.envx",
            "@import \"./db.envx\"\n@import \"./cache.envx\"\nAPP = \"x\"\n",
        );

        let env = load(&app).unwrap();
        assert_eq!(env.entries.len(), 3);
    }

    // ── Error: circular imports ───────────────────────────────────────────────

    #[test]
    fn direct_self_import_is_circular() {
        // app.envx imports itself
        let tmp = TempDir::new();
        // We must write a placeholder first so canonicalize works on the import target,
        // then overwrite with the self-referential content.
        let app = tmp.write("app.envx", "");
        fs::write(&app, "@import \"./app.envx\"\nX = \"1\"\n").unwrap();

        let err = load(&app).unwrap_err();
        assert!(
            matches!(err, EnvxError::CircularImport { ref cycle } if cycle.contains("app.envx")),
            "unexpected error: {:?}", err
        );
    }

    #[test]
    fn two_file_cycle() {
        // a.envx → b.envx → a.envx
        let tmp = TempDir::new();
        let a = tmp.write("a.envx", "");
        let b = tmp.write("b.envx", "");
        fs::write(&a, "@import \"./b.envx\"\nA = \"1\"\n").unwrap();
        fs::write(&b, "@import \"./a.envx\"\nB = \"2\"\n").unwrap();

        let err = load(&a).unwrap_err();
        assert!(matches!(err, EnvxError::CircularImport { .. }));
        if let EnvxError::CircularImport { cycle } = &err {
            assert!(cycle.contains("a.envx"), "cycle: {}", cycle);
            assert!(cycle.contains("b.envx"), "cycle: {}", cycle);
        }
    }

    #[test]
    fn three_file_cycle() {
        // a → b → c → a
        let tmp = TempDir::new();
        let a = tmp.write("a.envx", "");
        let b = tmp.write("b.envx", "");
        let c = tmp.write("c.envx", "");
        fs::write(&a, "@import \"./b.envx\"\n").unwrap();
        fs::write(&b, "@import \"./c.envx\"\n").unwrap();
        fs::write(&c, "@import \"./a.envx\"\n").unwrap();

        assert!(matches!(load(&a), Err(EnvxError::CircularImport { .. })));
    }

    // ── Error: duplicate variable ─────────────────────────────────────────────

    #[test]
    fn duplicate_key_across_files_is_error() {
        let tmp = TempDir::new();
        tmp.write("base.envx", "DB_HOST = \"localhost\"\n");
        let app = tmp.write("app.envx", "@import \"./base.envx\"\nDB_HOST = \"prod.db.com\"\n");

        let err = load(&app).unwrap_err();
        assert!(
            matches!(err, EnvxError::DuplicateVariable { ref key, .. } if key == "DB_HOST"),
            "unexpected error: {:?}", err
        );
    }

    #[test]
    fn duplicate_key_names_both_files() {
        let tmp = TempDir::new();
        tmp.write("base.envx", "X = \"1\"\n");
        let app = tmp.write("app.envx", "@import \"./base.envx\"\nX = \"2\"\n");

        if let Err(EnvxError::DuplicateVariable { key, first_file, second_file }) =
            load(&app)
        {
            assert_eq!(key, "X");
            assert!(first_file.contains("base.envx"), "first_file: {}", first_file);
            assert!(second_file.contains("app.envx"), "second_file: {}", second_file);
        } else {
            panic!("expected DuplicateVariable");
        }
    }

    #[test]
    fn duplicate_in_two_imported_files_is_error() {
        // Neither file is the root — the conflict is between two imports.
        let tmp = TempDir::new();
        tmp.write("db.envx", "PORT = \"5432\"\n");
        tmp.write("app_ports.envx", "PORT = \"8080\"\n");
        let root =
            tmp.write("root.envx", "@import \"./db.envx\"\n@import \"./app_ports.envx\"\n");

        assert!(matches!(load(&root), Err(EnvxError::DuplicateVariable { .. })));
    }

    // ── Error: missing file ───────────────────────────────────────────────────

    #[test]
    fn missing_root_file_is_io_error() {
        let p = PathBuf::from("/nonexistent/path/that/does/not/exist.envx");
        assert!(matches!(load(&p), Err(EnvxError::Io { .. })));
    }

    #[test]
    fn missing_import_target_is_bad_import_path() {
        let tmp = TempDir::new();
        let app = tmp.write("app.envx", "@import \"./does_not_exist.envx\"\n");
        assert!(matches!(load(&app), Err(EnvxError::BadImportPath { .. })));
    }

    // ── Cycle string formatting ───────────────────────────────────────────────

    #[test]
    fn cycle_string_format() {
        let stack = vec![
            PathBuf::from("/project/app.envx"),
            PathBuf::from("/project/base.envx"),
            PathBuf::from("/project/shared.envx"),
        ];
        let repeated = Path::new("/project/app.envx");
        let s = cycle_string(&stack, repeated);
        assert_eq!(s, "app.envx → base.envx → shared.envx → app.envx");
    }

    #[test]
    fn cycle_string_partial_stack() {
        // The cycle starts mid-stack (e.g. a non-root file causes the cycle)
        let stack = vec![
            PathBuf::from("/project/root.envx"),
            PathBuf::from("/project/a.envx"),
            PathBuf::from("/project/b.envx"),
        ];
        let repeated = Path::new("/project/a.envx");
        let s = cycle_string(&stack, repeated);
        // Cycle starts at a.envx, not root.envx
        assert_eq!(s, "a.envx → b.envx → a.envx");
    }
}
