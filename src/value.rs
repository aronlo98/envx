/// Runtime value produced by the evaluator.
///
/// The evaluator works with typed values internally so that functions like
/// `default(8080)` can carry a real integer and `eq($APP_ENV, 'prod')` can
/// return a real boolean for use in `if/then/else`. The final export step
/// always calls `into_string`, so the outside world only ever sees strings.
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Str(String),
    Int(i64),
    Bool(bool),
}

impl Value {
    /// Coerce any variant to its string representation for export.
    pub fn into_string(self) -> String {
        match self {
            Value::Str(s) => s,
            Value::Int(n) => n.to_string(),
            Value::Bool(b) => b.to_string(),
        }
    }

    /// Reference-based version for cases where we need the string without consuming.
    pub fn as_str_repr(&self) -> String {
        match self {
            Value::Str(s) => s.clone(),
            Value::Int(n) => n.to_string(),
            Value::Bool(b) => b.to_string(),
        }
    }

    /// Used by `if/then/else` to decide which branch to take.
    pub fn is_truthy(&self) -> bool {
        match self {
            Value::Bool(b) => *b,
            Value::Str(s) => !s.is_empty(),
            Value::Int(n) => *n != 0,
        }
    }

    pub fn type_name(&self) -> &'static str {
        match self {
            Value::Str(_) => "string",
            Value::Int(_) => "integer",
            Value::Bool(_) => "boolean",
        }
    }
}

impl std::fmt::Display for Value {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Value::Str(s) => write!(f, "{}", s),
            Value::Int(n) => write!(f, "{}", n),
            Value::Bool(b) => write!(f, "{}", b),
        }
    }
}

impl From<String> for Value {
    fn from(s: String) -> Self {
        Value::Str(s)
    }
}

impl From<&str> for Value {
    fn from(s: &str) -> Self {
        Value::Str(s.to_owned())
    }
}

impl From<i64> for Value {
    fn from(n: i64) -> Self {
        Value::Int(n)
    }
}

impl From<bool> for Value {
    fn from(b: bool) -> Self {
        Value::Bool(b)
    }
}
