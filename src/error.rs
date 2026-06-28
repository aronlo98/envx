use miette::{Diagnostic, NamedSource, SourceSpan};
use thiserror::Error;

pub type Result<T> = std::result::Result<T, EnvxError>;

#[derive(Debug, Error, Diagnostic)]
pub enum EnvxError {
    // ── Lexer ─────────────────────────────────────────────────────────────────

    #[error("unexpected character in source")]
    #[diagnostic(
        code(envx::lex::unexpected_char),
        help("only ASCII identifiers, quoted strings, `=`, `@import`, and `#` comments are valid")
    )]
    UnexpectedChar {
        #[source_code]
        src: NamedSource<String>,
        #[label("this character is not allowed here")]
        span: SourceSpan,
    },

    #[error("unterminated string literal")]
    #[diagnostic(
        code(envx::lex::unterminated_string),
        help("close the string with a matching `\"`")
    )]
    UnterminatedString {
        #[source_code]
        src: NamedSource<String>,
        #[label("string opened here, never closed")]
        span: SourceSpan,
    },

    // ── Parser ────────────────────────────────────────────────────────────────

    /// `${{` without matching `}}`
    #[error("unclosed expression block: `${{{{` was never closed with `}}}}`")]
    #[diagnostic(
        code(envx::parse::unclosed_expr),
        help("add `}}` after the expression to close the block")
    )]
    UnclosedExpression {
        #[source_code]
        src: NamedSource<String>,
        #[label("expression block opened here")]
        span: SourceSpan,
    },

    #[error("expected {expected}, found `{found}`")]
    #[diagnostic(code(envx::parse::unexpected_token))]
    UnexpectedToken {
        expected: String,
        found: String,
        #[source_code]
        src: NamedSource<String>,
        #[label("unexpected token")]
        span: SourceSpan,
    },

    #[error("expression block is empty — `${{{{  }}}}` must contain an expression")]
    #[diagnostic(code(envx::parse::empty_expr))]
    EmptyExpression {
        #[source_code]
        src: NamedSource<String>,
        #[label("empty block here")]
        span: SourceSpan,
    },

    // ── Loader ────────────────────────────────────────────────────────────────

    #[error("circular import detected: {cycle}")]
    #[diagnostic(
        code(envx::load::circular_import),
        help("remove one of the `@import` edges to break the cycle")
    )]
    CircularImport { cycle: String },

    #[error("duplicate variable `{key}`: first defined in `{first_file}`, redefined in `{second_file}`")]
    #[diagnostic(
        code(envx::load::duplicate_var),
        help("each variable may only be defined once across all imported files")
    )]
    DuplicateVariable {
        key: String,
        first_file: String,
        second_file: String,
    },

    #[error("cannot resolve import path `{raw_path}` from `{from_file}`")]
    #[diagnostic(code(envx::load::bad_import_path))]
    BadImportPath { raw_path: String, from_file: String },

    // ── DAG ───────────────────────────────────────────────────────────────────

    #[error("circular dependency detected: {cycle}")]
    #[diagnostic(
        code(envx::dag::circular_dep),
        help("break the dependency cycle by removing one of the variable references")
    )]
    CircularDependency { cycle: String },

    // ── Evaluator ─────────────────────────────────────────────────────────────

    #[error("undefined variable `${name}`")]
    #[diagnostic(code(envx::eval::undefined_var))]
    UndefinedVariable {
        name: String,
        #[source_code]
        src: NamedSource<String>,
        #[label("referenced here but never defined")]
        span: SourceSpan,
    },

    #[error("unknown function `{name}`")]
    #[diagnostic(
        code(envx::eval::unknown_fn),
        help("allowed functions: abs, capitalize, concat, default, emoji, eq, int, len, lower, now, replace, round, secret, title, trim, truncate, upper, uuid")
    )]
    UnknownFunction { name: String },

    #[error("wrong number of arguments for `{func}`: expected {expected}, got {got}")]
    #[diagnostic(code(envx::eval::arity_error))]
    ArityError {
        func: String,
        expected: String,
        got: usize,
    },

    #[error("type error in `{func}`: expected {expected}, got {got}")]
    #[diagnostic(code(envx::eval::type_error))]
    TypeError {
        func: String,
        expected: String,
        got: String,
    },

    #[error("invalid argument for `{func}`: {message}")]
    #[diagnostic(code(envx::eval::invalid_arg))]
    InvalidArgument { func: String, message: String },

    // ── I/O ───────────────────────────────────────────────────────────────────

    #[error("cannot read file `{path}`")]
    #[diagnostic(code(envx::io::read_error))]
    Io {
        path: String,
        #[source]
        source: std::io::Error,
    },
}
