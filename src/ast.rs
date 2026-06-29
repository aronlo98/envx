use std::{collections::HashMap, path::PathBuf};

use crate::value::Value;

// ─── Span ─────────────────────────────────────────────────────────────────────

/// Byte-offset range within a source file.
/// Stored on every AST node so miette can draw source-code arrows on errors.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Span {
    pub start: usize,
    pub end: usize,
}

impl Span {
    pub fn new(start: usize, end: usize) -> Self {
        Span { start, end }
    }

    /// Merge two spans into one covering both (useful when building parent nodes).
    pub fn merge(self, other: Span) -> Span {
        Span {
            start: self.start.min(other.start),
            end: self.end.max(other.end),
        }
    }
}

impl From<std::ops::Range<usize>> for Span {
    fn from(r: std::ops::Range<usize>) -> Self {
        Span::new(r.start, r.end)
    }
}

/// Allows writing `span: my_span.into()` wherever a `miette::SourceSpan` is expected.
/// miette uses `(offset, length)` — not `(start, end)`.
impl From<Span> for miette::SourceSpan {
    fn from(s: Span) -> Self {
        (s.start, s.end.saturating_sub(s.start)).into()
    }
}

// ─── Expression AST ───────────────────────────────────────────────────────────

/// A single function call with zero or more arguments.
/// Used both as a standalone call (`ENV("PORT")`) and as the right-hand side
/// of a pipe (`$NAME | replace(' ', '_')`).
#[derive(Debug, Clone, PartialEq)]
pub struct FnCall {
    pub name: String,
    pub args: Vec<Expr>,
    pub span: Span,
}

/// A node in the expression AST produced by the inner (expression) parser.
///
/// The evaluator pattern-matches on this enum recursively. Every variant carries
/// a `Span` so that runtime errors can point back to the exact source location.
#[derive(Debug, Clone, PartialEq)]
pub enum Expr {
    /// A literal value written directly in the source: `'prod'`, `8080`, `true`.
    Literal(Value, Span),

    /// Reference to a variable defined earlier: `$NAME`.
    VarRef {
        name: String,
        span: Span,
    },

    /// Look up an OS environment variable: `ENV("PORT")`.
    /// Distinct from VarRef so the evaluator can access `std::env::var` cleanly.
    EnvLookup {
        key: String,
        span: Span,
    },

    /// A pipeline step: `<expr> | <function>(<args...>)`.
    ///
    /// The left-hand `expr` is evaluated first; its result becomes the implicit
    /// first argument passed to `func`. The function's explicit `args` follow.
    ///
    /// Example: `$NAME | replace(' ', '_') | lower`
    ///   is parsed as:
    ///     Pipe { expr: Pipe { expr: VarRef("NAME"), func: replace(' ', '_') },
    ///            func: lower() }
    Pipe {
        expr: Box<Expr>,
        func: FnCall,
        span: Span,
    },

    /// A direct function call (not via pipe): `default(8080)`, `concat($A, $B)`.
    Call(FnCall),

    /// Conditional expression: `if eq($APP_ENV, 'prod') then 'api.db.com' else 'localhost'`.
    IfExpr {
        /// Must evaluate to a Bool (or a truthy value via `Value::is_truthy`).
        cond: Box<Expr>,
        then_val: Box<Expr>,
        else_val: Box<Expr>,
        span: Span,
    },
}

impl Expr {
    /// Extract the span from any variant.
    pub fn span(&self) -> Span {
        match self {
            Expr::Literal(_, s) => *s,
            Expr::VarRef { span, .. } => *span,
            Expr::EnvLookup { span, .. } => *span,
            Expr::Pipe { span, .. } => *span,
            Expr::Call(f) => f.span,
            Expr::IfExpr { span, .. } => *span,
        }
    }

    /// Collect all `$VAR` names referenced (directly or transitively) by this
    /// expression. Used by the DAG builder to draw dependency edges.
    pub fn collect_var_refs(&self, out: &mut Vec<String>) {
        match self {
            Expr::VarRef { name, .. } => out.push(name.clone()),
            Expr::Literal(_, _) | Expr::EnvLookup { .. } => {}
            Expr::Pipe { expr, func, .. } => {
                expr.collect_var_refs(out);
                for arg in &func.args {
                    arg.collect_var_refs(out);
                }
            }
            Expr::Call(f) => {
                for arg in &f.args {
                    arg.collect_var_refs(out);
                }
            }
            Expr::IfExpr { cond, then_val, else_val, .. } => {
                cond.collect_var_refs(out);
                then_val.collect_var_refs(out);
                else_val.collect_var_refs(out);
            }
        }
    }

    /// Finds if the given byte offset falls inside a `VarRef` expression.
    /// Used by the LSP to support "Go to Definition" for variables.
    #[allow(dead_code)]
    pub fn find_var_ref_at(&self, offset: usize) -> Option<String> {
        let s = self.span();
        if offset < s.start || offset > s.end {
            return None;
        }

        match self {
            Expr::VarRef { name, span } => {
                if offset >= span.start && offset <= span.end {
                    Some(name.clone())
                } else {
                    None
                }
            }
            Expr::Pipe { expr, func, .. } => {
                expr.find_var_ref_at(offset).or_else(|| {
                    func.args.iter().find_map(|arg| arg.find_var_ref_at(offset))
                })
            }
            Expr::Call(f) => f.args.iter().find_map(|arg| arg.find_var_ref_at(offset)),
            Expr::IfExpr { cond, then_val, else_val, .. } => cond
                .find_var_ref_at(offset)
                .or_else(|| then_val.find_var_ref_at(offset))
                .or_else(|| else_val.find_var_ref_at(offset)),
            _ => None,
        }
    }
}

// ─── Template (value of one assignment) ──────────────────────────────────────

/// One piece of a template value: either raw text or an expression block.
#[derive(Debug, Clone, PartialEq)]
pub enum Segment {
    /// Literal text outside `${{ }}`: the `@company.com` part of
    /// `"${{ $USERNAME }}@company.com"`.
    Literal(String),

    /// An expression block `${{ <expr> }}`.
    Expr(Expr),
}

/// The full right-hand side of one assignment.
///
/// A template is a sequence of segments. Evaluating it means evaluating each
/// `Expr` segment and concatenating every segment's string representation.
///
/// `NAME = "John Doe"` → `Template { segments: [Literal("John Doe")] }`
/// `EMAIL = "${{ $USERNAME }}@company.com"` →
///   `Template { segments: [Expr(VarRef("USERNAME")), Literal("@company.com")] }`
#[derive(Debug, Clone, PartialEq)]
pub struct Template {
    pub segments: Vec<Segment>,
    pub span: Span,
}

impl Template {
    /// Collect all `$VAR` references across all expression segments.
    /// Called by the DAG builder to determine this variable's dependencies.
    pub fn collect_var_refs(&self) -> Vec<String> {
        let mut refs = Vec::new();
        for seg in &self.segments {
            if let Segment::Expr(expr) = seg {
                expr.collect_var_refs(&mut refs);
            }
        }
        refs
    }

    /// Finds a variable reference at the given byte offset within this template.
    #[allow(dead_code)]
    pub fn find_var_ref_at(&self, offset: usize) -> Option<String> {
        if offset < self.span.start || offset > self.span.end {
            return None;
        }
        self.segments.iter().find_map(|seg| {
            if let Segment::Expr(expr) = seg {
                expr.find_var_ref_at(offset)
            } else {
                None
            }
        })
    }
}

// ─── File-level AST ───────────────────────────────────────────────────────────

/// A top-level statement in an `.envx` file.
#[derive(Debug, Clone, PartialEq)]
pub enum Statement {
    /// `@import "./base.envx"`
    Import {
        /// The raw string as written in the source (for error messages).
        raw_path: String,
        /// Absolute path resolved at parse time relative to this file's directory.
        resolved: PathBuf,
        span: Span,
    },

    /// `KEY = "value"` or `KEY = "${{ expr }}"`
    Entry {
        key: String,
        template: Template,
        /// Source file this entry came from. Filled in by the loader when merging
        /// entries from multiple files, so duplicate-key errors can cite both origins.
        source: PathBuf,
        span: Span,
    },
}

/// The parsed representation of a single `.envx` file.
/// The loader processes this into a `ResolvedEnv` by following imports recursively.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct EnvxFile {
    /// Absolute path to this file (used to resolve relative imports inside it).
    pub path: PathBuf,
    /// Raw source text (kept for miette source display).
    pub source: String,
    /// Statements in declaration order.
    pub statements: Vec<Statement>,
}

impl EnvxFile {
    /// Finds a variable reference at the given byte offset across all statements.
    #[allow(dead_code)]
    pub fn find_var_ref_at(&self, offset: usize) -> Option<String> {
        self.statements.iter().find_map(|stmt| {
            if let Statement::Entry { template, .. } = stmt {
                template.find_var_ref_at(offset)
            } else {
                None
            }
        })
    }
}

// ─── Resolved environment ─────────────────────────────────────────────────────

/// The flat, merged set of variable definitions after all imports have been
/// resolved and all files have been loaded. This is what the DAG and evaluator
/// operate on — they are completely unaware of file boundaries.
///
/// `IndexMap` is used instead of `HashMap` to preserve the declaration order,
/// which is useful both for the DAG (stable node ordering) and for `export`
/// output (variables printed in the order they were declared).
#[derive(Debug, Default)]
pub struct ResolvedEnv {
    /// Maps variable name → (template, originating file path).
    /// The `PathBuf` is stored alongside so that redefinition errors can name
    /// both the first and second definition sites.
    pub entries: indexmap::IndexMap<String, (Template, PathBuf)>,

    /// Raw source text for each loaded file, keyed by canonical path.
    /// Populated by the loader and used by the evaluator to provide
    /// source-context spans in `UndefinedVariable` diagnostics.
    pub sources: HashMap<PathBuf, String>,
}
