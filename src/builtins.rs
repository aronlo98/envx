use crate::{
    error::{EnvxError, Result},
    value::Value,
};

// ─── Function whitelist ───────────────────────────────────────────────────────

/// Dispatch a builtin function call.
///
/// `name`  – function name (already canonicalized to the identifier as written)
/// `recv`  – the receiver value from the left-hand side of a pipe (`None` for
///           direct calls like `concat($A, $B)`; `Some(v)` for `$VAR | trim`)
/// `args`  – explicit argument list (not including the implicit pipe receiver)
///
/// The 8 whitelisted functions and their signatures:
///
/// | name    | pipe-recv | args      | returns |
/// |---------|-----------|-----------|---------|
/// | trim    | Str       | –         | Str     |
/// | lower   | Str       | –         | Str     |
/// | upper   | Str       | –         | Str     |
/// | len     | Str       | –         | Int     |
/// | replace | Str       | Str, Str  | Str     |
/// | concat  | –         | Str...    | Str     |
/// | default | Str       | Str       | Str     |
/// | eq      | –         | Any, Any  | Bool    |
pub fn dispatch(name: &str, recv: Option<Value>, args: Vec<Value>) -> Result<Value> {
    match name {
        "trim" => {
            let s = require_recv_str(name, recv, &args, 0)?;
            Ok(Value::Str(s.trim().to_string()))
        }
        "lower" => {
            let s = require_recv_str(name, recv, &args, 0)?;
            Ok(Value::Str(s.to_lowercase()))
        }
        "upper" => {
            let s = require_recv_str(name, recv, &args, 0)?;
            Ok(Value::Str(s.to_uppercase()))
        }
        "len" => {
            let s = require_recv_str(name, recv, &args, 0)?;
            Ok(Value::Int(s.len() as i64))
        }
        "replace" => {
            let s = require_recv_str(name, recv, &args, 2)?;
            let from = arg_str(name, &args, 0)?;
            let to = arg_str(name, &args, 1)?;
            Ok(Value::Str(s.replace(&from, &to)))
        }
        "concat" => {
            // Direct call only; no pipe receiver.
            if recv.is_some() {
                return Err(EnvxError::ArityError {
                    func: name.to_string(),
                    expected: "no pipe receiver (call as concat($A, $B))".to_string(),
                    got: args.len() + 1,
                });
            }
            if args.is_empty() {
                return Err(EnvxError::ArityError {
                    func: name.to_string(),
                    expected: "at least 1".to_string(),
                    got: 0,
                });
            }
            let mut out = String::new();
            for (i, v) in args.into_iter().enumerate() {
                match v {
                    Value::Str(s) => out.push_str(&s),
                    other => {
                        return Err(EnvxError::TypeError {
                            func: name.to_string(),
                            expected: format!("Str for argument {}", i + 1),
                            got: other.type_name().to_string(),
                        })
                    }
                }
            }
            Ok(Value::Str(out))
        }
        "default" => {
            // $VAR | default('fallback')
            // Returns the fallback only when the receiver is an empty string.
            let s = require_recv_str(name, recv, &args, 1)?;
            let fallback = arg_str(name, &args, 0)?;
            if s.is_empty() {
                Ok(Value::Str(fallback))
            } else {
                Ok(Value::Str(s))
            }
        }
        "eq" => {
            // Direct call only: eq($A, $B)
            if recv.is_some() {
                return Err(EnvxError::ArityError {
                    func: name.to_string(),
                    expected: "no pipe receiver (call as eq($A, $B))".to_string(),
                    got: args.len() + 1,
                });
            }
            if args.len() != 2 {
                return Err(EnvxError::ArityError {
                    func: name.to_string(),
                    expected: "2".to_string(),
                    got: args.len(),
                });
            }
            let lhs = args[0].as_str_repr();
            let rhs = args[1].as_str_repr();
            Ok(Value::Bool(lhs == rhs))
        }
        other => Err(EnvxError::UnknownFunction {
            name: other.to_string(),
        }),
    }
}

// ─── Helpers ──────────────────────────────────────────────────────────────────

/// Resolve the receiver value (from a pipe) as a String.
/// Checks that `args.len() == expected_args` while we're here.
fn require_recv_str(
    func: &str,
    recv: Option<Value>,
    args: &[Value],
    expected_args: usize,
) -> Result<String> {
    let r = match recv {
        Some(v) => v,
        None => {
            return Err(EnvxError::ArityError {
                func: func.to_string(),
                expected: format!("pipe receiver + {expected_args} arg(s)"),
                got: 0,
            })
        }
    };
    if args.len() != expected_args {
        return Err(EnvxError::ArityError {
            func: func.to_string(),
            expected: expected_args.to_string(),
            got: args.len(),
        });
    }
    match r {
        Value::Str(s) => Ok(s),
        other => Err(EnvxError::TypeError {
            func: func.to_string(),
            expected: "Str".to_string(),
            got: other.type_name().to_string(),
        }),
    }
}

/// Extract argument at position `idx` as an owned `String`.
fn arg_str(func: &str, args: &[Value], idx: usize) -> Result<String> {
    match &args[idx] {
        Value::Str(s) => Ok(s.clone()),
        other => Err(EnvxError::TypeError {
            func: func.to_string(),
            expected: format!("Str for argument {}", idx + 1),
            got: other.type_name().to_string(),
        }),
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn s(v: &str) -> Value { Value::Str(v.to_string()) }

    #[test]
    fn trim_whitespace() {
        assert_eq!(dispatch("trim", Some(s("  hi  ")), vec![]).unwrap(), s("hi"));
    }

    #[test]
    fn lower_and_upper() {
        assert_eq!(dispatch("lower", Some(s("Hello")), vec![]).unwrap(), s("hello"));
        assert_eq!(dispatch("upper", Some(s("Hello")), vec![]).unwrap(), s("HELLO"));
    }

    #[test]
    fn len_returns_int() {
        assert_eq!(dispatch("len", Some(s("abc")), vec![]).unwrap(), Value::Int(3));
    }

    #[test]
    fn replace_substitutes() {
        let result = dispatch("replace", Some(s("a b c")), vec![s(" "), s("_")]).unwrap();
        assert_eq!(result, s("a_b_c"));
    }

    #[test]
    fn replace_all_occurrences() {
        let result = dispatch("replace", Some(s("aaa")), vec![s("a"), s("b")]).unwrap();
        assert_eq!(result, s("bbb"));
    }

    #[test]
    fn concat_joins_strings() {
        let result = dispatch("concat", None, vec![s("foo"), s("-"), s("bar")]).unwrap();
        assert_eq!(result, s("foo-bar"));
    }

    #[test]
    fn concat_requires_args() {
        assert!(matches!(
            dispatch("concat", None, vec![]),
            Err(EnvxError::ArityError { .. })
        ));
    }

    #[test]
    fn default_returns_fallback_when_empty() {
        assert_eq!(
            dispatch("default", Some(s("")), vec![s("fallback")]).unwrap(),
            s("fallback")
        );
    }

    #[test]
    fn default_keeps_value_when_non_empty() {
        assert_eq!(
            dispatch("default", Some(s("real")), vec![s("fallback")]).unwrap(),
            s("real")
        );
    }

    #[test]
    fn eq_equal_strings() {
        assert_eq!(
            dispatch("eq", None, vec![s("prod"), s("prod")]).unwrap(),
            Value::Bool(true)
        );
    }

    #[test]
    fn eq_unequal_strings() {
        assert_eq!(
            dispatch("eq", None, vec![s("prod"), s("dev")]).unwrap(),
            Value::Bool(false)
        );
    }

    #[test]
    fn unknown_function_is_error() {
        assert!(matches!(
            dispatch("frobulate", None, vec![]),
            Err(EnvxError::UnknownFunction { .. })
        ));
    }

    #[test]
    fn missing_pipe_receiver_is_error() {
        assert!(matches!(
            dispatch("trim", None, vec![]),
            Err(EnvxError::ArityError { .. })
        ));
    }

    #[test]
    fn wrong_arg_count_for_replace() {
        assert!(matches!(
            dispatch("replace", Some(s("x")), vec![s("y")]),
            Err(EnvxError::ArityError { .. })
        ));
    }
}
