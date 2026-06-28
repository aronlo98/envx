use std::collections::HashMap;

use indexmap::IndexMap;
use miette::NamedSource;

use crate::{
    ast::{Expr, ResolvedEnv, Segment, Template},
    builtins,
    error::{EnvxError, Result},
    value::Value,
};

// ─── Public entry point ───────────────────────────────────────────────────────

/// Evaluate all variables in `env` in the order given by `order`
/// (which must be a topological sort produced by `dag::build_and_sort`).
///
/// Returns an `IndexMap` in evaluation order so the caller can iterate in a
/// predictable sequence (useful for `export` output).
pub fn evaluate(env: &ResolvedEnv, order: &[String]) -> Result<IndexMap<String, Value>> {
    let mut ctx = EvalContext {
        env,
        memo: HashMap::new(),
    };
    let mut out: IndexMap<String, Value> = IndexMap::new();

    for name in order {
        let value = ctx.eval_var(name)?;
        out.insert(name.clone(), value);
    }

    Ok(out)
}

// ─── Evaluation context ───────────────────────────────────────────────────────

struct EvalContext<'e> {
    env: &'e ResolvedEnv,
    /// Memoization table: once a variable is evaluated, its value is cached so
    /// that diamond-dependency patterns don't recompute sub-expressions.
    memo: HashMap<String, Value>,
}

impl<'e> EvalContext<'e> {
    fn eval_var(&mut self, name: &str) -> Result<Value> {
        if let Some(v) = self.memo.get(name) {
            return Ok(v.clone());
        }
        let (template, source_path) = self.env.entries.get(name).expect(
            "eval_var called for a name not in env — dag order is inconsistent",
        );
        let template = template.clone();
        let source_path = source_path.clone();
        let value = self.eval_template(&template, &source_path)?;
        self.memo.insert(name.to_string(), value.clone());
        Ok(value)
    }

    fn eval_template(&mut self, template: &Template, source_path: &std::path::Path) -> Result<Value> {
        // Templates always produce Str — Int/Bool exist only inside expression trees.
        let mut buf = String::new();
        for seg in &template.segments {
            match seg {
                Segment::Literal(s) => buf.push_str(s),
                Segment::Expr(expr) => {
                    let v = self.eval_expr(expr, source_path)?;
                    buf.push_str(&v.into_string());
                }
            }
        }
        Ok(Value::Str(buf))
    }

    fn eval_expr(&mut self, expr: &Expr, source_path: &std::path::Path) -> Result<Value> {
        match expr {
            Expr::Literal(v, _) => Ok(v.clone()),

            Expr::VarRef { name, span } => {
                if self.env.entries.contains_key(name.as_str()) {
                    self.eval_var(name)
                } else {
                    // Build the NamedSource from our stored source map.
                    let (src_name, src_text) = self
                        .env
                        .sources
                        .get(source_path)
                        .map(|text| {
                            (
                                source_path
                                    .file_name()
                                    .and_then(|n| n.to_str())
                                    .unwrap_or("<unknown>")
                                    .to_string(),
                                text.clone(),
                            )
                        })
                        .unwrap_or_else(|| ("<unknown>".to_string(), String::new()));

                    Err(EnvxError::UndefinedVariable {
                        name: name.clone(),
                        src: NamedSource::new(src_name, src_text),
                        span: (*span).into(),
                    })
                }
            }

            Expr::EnvLookup { key, .. } => {
                let val = std::env::var(key).unwrap_or_default();
                Ok(Value::Str(val))
            }

            Expr::IfExpr { cond, then_val, else_val, .. } => {
                let cond_val = self.eval_expr(cond, source_path)?;
                if cond_val.is_truthy() {
                    self.eval_expr(then_val, source_path)
                } else {
                    self.eval_expr(else_val, source_path)
                }
            }

            Expr::Call(fn_call) => {
                let mut arg_vals = Vec::with_capacity(fn_call.args.len());
                for arg in &fn_call.args {
                    arg_vals.push(self.eval_expr(arg, source_path)?);
                }
                builtins::dispatch(&fn_call.name, None, arg_vals)
            }

            Expr::Pipe { expr: lhs, func, .. } => {
                let recv = self.eval_expr(lhs, source_path)?;
                let mut arg_vals = Vec::with_capacity(func.args.len());
                for arg in &func.args {
                    arg_vals.push(self.eval_expr(arg, source_path)?);
                }
                builtins::dispatch(&func.name, Some(recv), arg_vals)
            }
        }
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    use crate::{ast::Statement, dag, parser};

    fn eval_src(src: &str) -> Result<IndexMap<String, Value>> {
        let path = PathBuf::from("/test/test.envx");
        let file = parser::parse(src, "test.envx", path.clone()).unwrap();
        let mut env = ResolvedEnv::default();
        for stmt in file.statements {
            if let Statement::Entry { key, template, source, .. } = stmt {
                env.entries.insert(key, (template, source));
            }
        }
        env.sources.insert(path, src.to_string());

        let order = dag::build_and_sort(&env)?;
        evaluate(&env, &order)
    }

    fn eval(src: &str) -> IndexMap<String, Value> {
        eval_src(src).expect("unexpected evaluation error")
    }

    fn s(v: &str) -> Value { Value::Str(v.to_string()) }

    // ── Basic literal values ──────────────────────────────────────────────────

    #[test]
    fn literal_string() {
        let out = eval("NAME = \"Alice\"\n");
        assert_eq!(out["NAME"], s("Alice"));
    }

    #[test]
    fn literal_integer_becomes_string_via_template() {
        // A bare integer inside ${{ }} is evaluated as Int, then coerced to Str
        // because the template has surrounding context.
        let out = eval("PORT = \"${{ 8080 }}\"\n");
        assert_eq!(out["PORT"], s("8080"));
    }

    // ── Variable references ───────────────────────────────────────────────────

    #[test]
    fn var_ref_forward_declared() {
        // EMAIL references USERNAME which is declared after it — DAG sorts them.
        let out = eval("EMAIL = \"${{ $USERNAME }}@example.com\"\nUSERNAME = \"alice\"\n");
        assert_eq!(out["USERNAME"], s("alice"));
        assert_eq!(out["EMAIL"], s("alice@example.com"));
    }

    #[test]
    fn diamond_dependency_evaluates_once() {
        // B and C both use A; D uses B and C.
        let out = eval(
            "A = \"x\"\nB = \"${{ $A }}1\"\nC = \"${{ $A }}2\"\nD = \"${{ $B }}-${{ $C }}\"\n",
        );
        assert_eq!(out["D"], s("x1-x2"));
    }

    // ── Pipe builtins ─────────────────────────────────────────────────────────

    #[test]
    fn pipe_trim() {
        let out = eval("V = \"${{  '  hi  ' | trim }}\"\n");
        assert_eq!(out["V"], s("hi"));
    }

    #[test]
    fn pipe_lower() {
        let out = eval("V = \"${{ 'HELLO' | lower }}\"\n");
        assert_eq!(out["V"], s("hello"));
    }

    #[test]
    fn pipe_upper() {
        let out = eval("V = \"${{ 'hello' | upper }}\"\n");
        assert_eq!(out["V"], s("HELLO"));
    }

    #[test]
    fn pipe_chain_lower_then_trim() {
        let out = eval("V = \"${{ '  HELLO  ' | trim | lower }}\"\n");
        assert_eq!(out["V"], s("hello"));
    }

    #[test]
    fn pipe_replace() {
        let out = eval("V = \"${{ 'a b c' | replace(' ', '_') }}\"\n");
        assert_eq!(out["V"], s("a_b_c"));
    }

    #[test]
    fn pipe_var_then_upper() {
        let out = eval("A = \"hello\"\nB = \"${{ $A | upper }}\"\n");
        assert_eq!(out["B"], s("HELLO"));
    }

    // ── Direct function calls ─────────────────────────────────────────────────

    #[test]
    fn direct_call_concat() {
        let out = eval("A = \"foo\"\nB = \"bar\"\nC = \"${{ concat($A, '-', $B) }}\"\n");
        assert_eq!(out["C"], s("foo-bar"));
    }

    #[test]
    fn direct_call_eq_true() {
        let out = eval("ENV = \"prod\"\nIS_PROD = \"${{ eq($ENV, 'prod') }}\"\n");
        assert_eq!(out["IS_PROD"], s("true"));
    }

    // ── Conditional expressions ───────────────────────────────────────────────

    #[test]
    fn if_then_else_true_branch() {
        let out = eval("APP_ENV = \"prod\"\nDB = \"${{ if eq($APP_ENV, 'prod') then 'api.db' else 'localhost' }}\"\n");
        assert_eq!(out["DB"], s("api.db"));
    }

    #[test]
    fn if_then_else_false_branch() {
        let out = eval("APP_ENV = \"dev\"\nDB = \"${{ if eq($APP_ENV, 'prod') then 'api.db' else 'localhost' }}\"\n");
        assert_eq!(out["DB"], s("localhost"));
    }

    // ── ENV lookup ────────────────────────────────────────────────────────────

    #[test]
    fn env_lookup_existing_var() {
        unsafe { std::env::set_var("__ENVX_TEST_PORT", "9000"); }
        let out = eval("PORT = \"${{ ENV('__ENVX_TEST_PORT') }}\"\n");
        assert_eq!(out["PORT"], s("9000"));
        unsafe { std::env::remove_var("__ENVX_TEST_PORT"); }
    }

    #[test]
    fn env_lookup_missing_var_returns_empty() {
        unsafe { std::env::remove_var("__ENVX_NONEXISTENT_XYZ"); }
        let out = eval("PORT = \"${{ ENV('__ENVX_NONEXISTENT_XYZ') }}\"\n");
        assert_eq!(out["PORT"], s(""));
    }

    // ── Default builtin ───────────────────────────────────────────────────────

    #[test]
    fn default_with_env_fallback() {
        unsafe { std::env::remove_var("__ENVX_MISSING"); }
        let out = eval("PORT = \"${{ ENV('__ENVX_MISSING') | default('3000') }}\"\n");
        assert_eq!(out["PORT"], s("3000"));
    }

    // ── Error cases ───────────────────────────────────────────────────────────

    #[test]
    fn undefined_variable_is_error() {
        let err = eval_src("X = \"${{ $UNDEFINED }}\"\n").unwrap_err();
        assert!(matches!(err, EnvxError::UndefinedVariable { .. }));
    }
}
