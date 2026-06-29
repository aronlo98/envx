use chrono::{Datelike, Local, Months, NaiveDate, NaiveDateTime, Weekday};
use rand::Rng;
use uuid::Uuid;

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
                        });
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
        "capitalize" => {
            let s = require_recv_str(name, recv, &args, 0)?;
            let mut chars = s.chars();
            let out = match chars.next() {
                None => String::new(),
                Some(c) => c.to_uppercase().collect::<String>() + &chars.as_str().to_lowercase(),
            };
            Ok(Value::Str(out))
        }
        "title" => {
            let s = require_recv_str(name, recv, &args, 0)?;
            let out = s
                .split_whitespace()
                .map(|w| {
                    let mut c = w.chars();
                    match c.next() {
                        None => String::new(),
                        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
                    }
                })
                .collect::<Vec<_>>()
                .join(" ");
            Ok(Value::Str(out))
        }
        "truncate" => {
            let s = require_recv_str(name, recv, &args, 1)?;
            let n = match &args[0] {
                Value::Int(n) if *n >= 0 => *n as usize,
                Value::Int(_) => {
                    return Err(EnvxError::InvalidArgument {
                        func: name.to_string(),
                        message: "length must be ≥ 0".to_string(),
                    });
                }
                other => {
                    return Err(EnvxError::TypeError {
                        func: name.to_string(),
                        expected: "Int".to_string(),
                        got: other.type_name().to_string(),
                    });
                }
            };
            Ok(Value::Str(s.chars().take(n).collect()))
        }
        "abs" => {
            if !args.is_empty() {
                return Err(EnvxError::ArityError {
                    func: name.to_string(),
                    expected: "0".to_string(),
                    got: args.len(),
                });
            }
            match recv {
                Some(Value::Int(n)) => Ok(Value::Int(n.saturating_abs())),
                Some(other) => Err(EnvxError::TypeError {
                    func: name.to_string(),
                    expected: "Int".to_string(),
                    got: other.type_name().to_string(),
                }),
                None => Err(EnvxError::ArityError {
                    func: name.to_string(),
                    expected: "pipe receiver (Int)".to_string(),
                    got: 0,
                }),
            }
        }
        "round" => {
            let s = match recv {
                Some(Value::Str(s)) => s,
                Some(other) => {
                    return Err(EnvxError::TypeError {
                        func: name.to_string(),
                        expected: "Str".to_string(),
                        got: other.type_name().to_string(),
                    });
                }
                None => {
                    return Err(EnvxError::ArityError {
                        func: name.to_string(),
                        expected: "pipe receiver".to_string(),
                        got: 0,
                    });
                }
            };
            let decimals: usize = match args.as_slice() {
                [] => 0,
                [Value::Int(n)] if *n >= 0 => *n as usize,
                [Value::Int(_)] => {
                    return Err(EnvxError::InvalidArgument {
                        func: name.to_string(),
                        message: "decimal places must be ≥ 0".to_string(),
                    });
                }
                _ => {
                    return Err(EnvxError::ArityError {
                        func: name.to_string(),
                        expected: "0 or 1".to_string(),
                        got: args.len(),
                    });
                }
            };
            let f: f64 = s.trim().parse().map_err(|_| EnvxError::InvalidArgument {
                func: name.to_string(),
                message: format!("'{}' is not a valid number", s),
            })?;
            let factor = 10f64.powi(decimals as i32);
            let rounded = (f * factor).round() / factor;
            Ok(Value::Str(format!("{:.prec$}", rounded, prec = decimals)))
        }
        "int" => {
            let s = require_recv_str(name, recv, &args, 0)?;
            let n = s
                .trim()
                .parse::<i64>()
                .or_else(|_| s.trim().parse::<f64>().map(|f| f as i64))
                .map_err(|_| EnvxError::InvalidArgument {
                    func: name.to_string(),
                    message: format!("'{}' cannot be parsed as an integer", s),
                })?;
            Ok(Value::Int(n))
        }
        "emoji" => {
            if recv.is_some() {
                return Err(EnvxError::ArityError {
                    func: "emoji".into(),
                    expected: "no pipe receiver (call as emoji('panda'))".into(),
                    got: args.len() + 1,
                });
            }
            let name = match args.as_slice() {
                [Value::Str(s)] => s.clone(),
                _ => {
                    return Err(EnvxError::ArityError {
                        func: "emoji".into(),
                        expected: "1".into(),
                        got: args.len(),
                    });
                }
            };
            let ch = emoji_lookup(&name).ok_or_else(|| EnvxError::InvalidArgument {
                func: "emoji".into(),
                message: format!("unknown emoji '{name}'"),
            })?;
            Ok(Value::Str(ch.to_string()))
        }
        "me" => {
            if recv.is_some() || !args.is_empty() {
                return Err(EnvxError::ArityError {
                    func: "me".into(),
                    expected: "no arguments".into(),
                    got: args.len() + recv.is_some() as usize,
                });
            }
            Ok(Value::Str("aronlo98 🐼".to_string()))
        }
        // ── Date functions ────────────────────────────────────────────────────
        //
        // All date pipe functions accept a date string as the pipe receiver
        // in one of two formats:
        //   • "YYYY-MM-DD"               (date only, time assumed 00:00:00)
        //   • "YYYY-MM-DDTHH:MM:SS"      (full ISO 8601 datetime)
        //
        // Usage pattern:
        //   now() | date_add(30, 'days') | date_format('YYYYMMDD')
        //   now() | year()    now() | month()    now() | weekday()
        "timestamp" => {
            if recv.is_some() || !args.is_empty() {
                return Err(EnvxError::ArityError {
                    func: "timestamp".into(),
                    expected: "no arguments".into(),
                    got: args.len() + recv.is_some() as usize,
                });
            }
            Ok(Value::Int(Local::now().timestamp()))
        }
        "date_add" => {
            // $DATE | date_add(n, unit)  →  new date string (ISO datetime)
            // Negative n subtracts. Units: seconds/minutes/hours/days/weeks/months/years
            let date_str = require_recv_str(name, recv, &args, 2)?;
            let n = match &args[0] {
                Value::Int(n) => *n,
                other => {
                    return Err(EnvxError::TypeError {
                        func: name.to_string(),
                        expected: "Int for argument 1 (amount)".to_string(),
                        got: other.type_name().to_string(),
                    });
                }
            };
            let unit = match &args[1] {
                Value::Str(s) => s.clone(),
                other => {
                    return Err(EnvxError::TypeError {
                        func: name.to_string(),
                        expected: "Str for argument 2 (unit)".to_string(),
                        got: other.type_name().to_string(),
                    });
                }
            };
            let dt = parse_date_str(&date_str, name)?;
            let result = apply_date_offset(dt, n, &unit, name)?;
            Ok(Value::Str(result.format("%Y-%m-%dT%H:%M:%S").to_string()))
        }
        "date_diff" => {
            // $DATE1 | date_diff(date2, unit)  →  Int (date2 − date1 in unit)
            // Result is negative when date2 is earlier than date1.
            let date1_str = require_recv_str(name, recv, &args, 2)?;
            let date2_str = match &args[0] {
                Value::Str(s) => s.clone(),
                other => {
                    return Err(EnvxError::TypeError {
                        func: name.to_string(),
                        expected: "Str for argument 1 (date2)".to_string(),
                        got: other.type_name().to_string(),
                    });
                }
            };
            let unit = match &args[1] {
                Value::Str(s) => s.clone(),
                other => {
                    return Err(EnvxError::TypeError {
                        func: name.to_string(),
                        expected: "Str for argument 2 (unit)".to_string(),
                        got: other.type_name().to_string(),
                    });
                }
            };
            let dt1 = parse_date_str(&date1_str, name)?;
            let dt2 = parse_date_str(&date2_str, name)?;
            Ok(Value::Int(calc_date_diff(dt1, dt2, &unit, name)?))
        }
        "date_format" => {
            // $DATE | date_format('YYYY/MM/DD')  →  formatted string
            // Accepts the same moment.js-style tokens as now().
            let date_str = require_recv_str(name, recv, &args, 1)?;
            let fmt = match &args[0] {
                Value::Str(s) => to_strftime(s),
                other => {
                    return Err(EnvxError::TypeError {
                        func: name.to_string(),
                        expected: "Str for argument 1 (format)".to_string(),
                        got: other.type_name().to_string(),
                    });
                }
            };
            let dt = parse_date_str(&date_str, name)?;
            Ok(Value::Str(dt.format(&fmt).to_string()))
        }
        "year" => {
            let dt = parse_date_str(&require_recv_str(name, recv, &args, 0)?, name)?;
            Ok(Value::Int(dt.year() as i64))
        }
        "month" => {
            let dt = parse_date_str(&require_recv_str(name, recv, &args, 0)?, name)?;
            Ok(Value::Int(dt.month() as i64))
        }
        "day" => {
            let dt = parse_date_str(&require_recv_str(name, recv, &args, 0)?, name)?;
            Ok(Value::Int(dt.day() as i64))
        }
        "weekday" => {
            let dt = parse_date_str(&require_recv_str(name, recv, &args, 0)?, name)?;
            let day_name = match dt.weekday() {
                Weekday::Mon => "Monday",
                Weekday::Tue => "Tuesday",
                Weekday::Wed => "Wednesday",
                Weekday::Thu => "Thursday",
                Weekday::Fri => "Friday",
                Weekday::Sat => "Saturday",
                Weekday::Sun => "Sunday",
            };
            Ok(Value::Str(day_name.to_string()))
        }
        "uuid" => {
            if recv.is_some() {
                return Err(EnvxError::ArityError {
                    func: "uuid".into(),
                    expected: "no pipe receiver (call as uuid() or uuid(4))".into(),
                    got: args.len() + 1,
                });
            }
            let id = match args.as_slice() {
                [] | [Value::Int(4)] => Uuid::new_v4(),
                [Value::Int(7)] => Uuid::now_v7(),
                [Value::Int(n)] => {
                    return Err(EnvxError::InvalidArgument {
                        func: "uuid".into(),
                        message: format!("unsupported UUID version {n}; supported: 4, 7"),
                    });
                }
                _ => {
                    return Err(EnvxError::ArityError {
                        func: "uuid".into(),
                        expected: "0 or 1".into(),
                        got: args.len(),
                    });
                }
            };
            Ok(Value::Str(id.to_string()))
        }
        "now" => {
            if recv.is_some() {
                return Err(EnvxError::ArityError {
                    func: "now".to_string(),
                    expected: "no pipe receiver (call as now() or now('YYYYMMDD'))".to_string(),
                    got: args.len() + 1,
                });
            }
            let fmt = match args.as_slice() {
                [] => "%Y-%m-%dT%H:%M:%S".to_string(),
                [Value::Str(user_fmt)] => to_strftime(user_fmt),
                _ => {
                    return Err(EnvxError::ArityError {
                        func: "now".to_string(),
                        expected: "0 or 1".to_string(),
                        got: args.len(),
                    });
                }
            };
            Ok(Value::Str(Local::now().format(&fmt).to_string()))
        }

        "secret" => {
            if recv.is_some() {
                return Err(EnvxError::ArityError {
                    func: "secret".into(),
                    expected: "no pipe receiver (call as secret() or secret(32))".into(),
                    got: args.len() + 1,
                });
            }

            const DEFAULT_LEN: usize = 32;
            const DEFAULT_ALPHA: &str =
                "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789";

            let (length, alphabet) = match args.as_slice() {
                [] => (DEFAULT_LEN, DEFAULT_ALPHA.to_string()),
                [Value::Int(n)] => {
                    if *n < 1 {
                        return Err(EnvxError::InvalidArgument {
                            func: "secret".into(),
                            message: "length must be ≥ 1".into(),
                        });
                    }
                    (*n as usize, DEFAULT_ALPHA.to_string())
                }
                [Value::Int(n), Value::Str(alpha)] => {
                    if *n < 1 {
                        return Err(EnvxError::InvalidArgument {
                            func: "secret".into(),
                            message: "length must be ≥ 1".into(),
                        });
                    }
                    let resolved = resolve_alphabet(alpha);
                    if resolved.is_empty() {
                        return Err(EnvxError::InvalidArgument {
                            func: "secret".into(),
                            message: "alphabet must not be empty".into(),
                        });
                    }
                    (*n as usize, resolved.to_string())
                }
                _ => {
                    return Err(EnvxError::ArityError {
                        func: "secret".into(),
                        expected: "0, 1, or 2".into(),
                        got: args.len(),
                    });
                }
            };

            let chars: Vec<char> = alphabet.chars().collect();
            let mut rng = rand::rng();
            let s: String = (0..length)
                .map(|_| chars[rng.random_range(0..chars.len())])
                .collect();
            Ok(Value::Str(s))
        }

        other => Err(EnvxError::UnknownFunction {
            name: other.to_string(),
        }),
    }
}

// ─── Format converter ─────────────────────────────────────────────────────────

/// Convert a user-friendly format string (moment.js style) to strftime format.
///
/// Supported tokens (processed longest-first to avoid partial matches):
///
/// | Token  | Meaning            | Example  |
/// |--------|--------------------|----------|
/// | `YYYY` | 4-digit year       | `2026`   |
/// | `YY`   | 2-digit year       | `26`     |
/// | `MMMM` | Full month name    | `January`|
/// | `MMM`  | Short month name   | `Jan`    |
/// | `MM`   | 2-digit month      | `06`     |
/// | `DDDD` | Full weekday name  | `Monday` |
/// | `DDD`  | Short weekday name | `Mon`    |
/// | `DD`   | 2-digit day        | `28`     |
/// | `HH`   | 24-hour hour       | `15`     |
/// | `hh`   | 12-hour hour       | `03`     |
/// | `mm`   | Minutes            | `30`     |
/// | `ss`   | Seconds            | `00`     |
/// | `A`    | AM/PM              | `PM`     |
///
/// Any character not part of a token is passed through unchanged.
fn to_strftime(fmt: &str) -> String {
    fmt.replace("YYYY", "%Y")
        .replace("YY", "%y")
        .replace("MMMM", "%B")
        .replace("MMM", "%b")
        .replace("MM", "%m")
        .replace("DDDD", "%A")
        .replace("DDD", "%a")
        .replace("DD", "%d")
        .replace("HH", "%H")
        .replace("hh", "%I")
        .replace("mm", "%M")
        .replace("ss", "%S")
        .replace('A', "%p")
}

// ─── Alphabet presets ─────────────────────────────────────────────────────────

/// Resolve a named alphabet preset or return the string as-is (custom alphabet).
///
/// | Name         | Characters                            |
/// |--------------|---------------------------------------|
/// | `hex`        | `0-9a-f`                              |
/// | `base64`     | `A-Za-z0-9+/`                         |
/// | `base64url`  | `A-Za-z0-9-_` (URL-safe, RFC 4648)    |
/// | `alpha`      | `A-Za-z`                              |
/// | `numeric`    | `0-9`                                 |
/// | anything else | treated as a literal character set  |
fn resolve_alphabet(s: &str) -> &str {
    match s {
        "hex" => "0123456789abcdef",
        "base64" => "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/",
        "base64url" => "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_",
        "alpha" => "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz",
        "numeric" => "0123456789",
        other => other,
    }
}

// ─── Date helpers ─────────────────────────────────────────────────────────────

/// Parse a date string in `YYYY-MM-DD` or `YYYY-MM-DDTHH:MM:SS` format.
fn parse_date_str(s: &str, func: &str) -> Result<NaiveDateTime> {
    NaiveDateTime::parse_from_str(s.trim(), "%Y-%m-%dT%H:%M:%S")
        .or_else(|_| {
            NaiveDate::parse_from_str(s.trim(), "%Y-%m-%d").map(|d| d.and_hms_opt(0, 0, 0).unwrap())
        })
        .map_err(|_| EnvxError::InvalidArgument {
            func: func.to_string(),
            message: format!("cannot parse '{s}' as a date; use YYYY-MM-DD or YYYY-MM-DDTHH:MM:SS"),
        })
}

/// Apply a signed offset to a `NaiveDateTime`.
/// `unit` is normalised by stripping a trailing `s` so both `"day"` and
/// `"days"` are accepted.
fn apply_date_offset(dt: NaiveDateTime, n: i64, unit: &str, func: &str) -> Result<NaiveDateTime> {
    use chrono::Duration;
    let result = match unit.trim_end_matches('s') {
        "second" => dt.checked_add_signed(Duration::seconds(n)),
        "minute" => dt.checked_add_signed(Duration::minutes(n)),
        "hour" => dt.checked_add_signed(Duration::hours(n)),
        "day" => dt.checked_add_signed(Duration::days(n)),
        "week" => dt.checked_add_signed(Duration::weeks(n)),
        "month" => {
            if n >= 0 {
                dt.checked_add_months(Months::new(n as u32))
            } else {
                dt.checked_sub_months(Months::new((-n) as u32))
            }
        }
        "year" => {
            if n >= 0 {
                dt.checked_add_months(Months::new(n as u32 * 12))
            } else {
                dt.checked_sub_months(Months::new((-n) as u32 * 12))
            }
        }
        _ => {
            return Err(EnvxError::InvalidArgument {
                func: func.to_string(),
                message: format!(
                    "unknown unit '{unit}'; use: seconds, minutes, hours, days, weeks, months, years"
                ),
            });
        }
    };
    result.ok_or_else(|| EnvxError::InvalidArgument {
        func: func.to_string(),
        message: "date arithmetic overflow".to_string(),
    })
}

/// Compute `dt2 − dt1` in the requested unit.
fn calc_date_diff(dt1: NaiveDateTime, dt2: NaiveDateTime, unit: &str, func: &str) -> Result<i64> {
    let dur = dt2.signed_duration_since(dt1);
    match unit.trim_end_matches('s') {
        "second" => Ok(dur.num_seconds()),
        "minute" => Ok(dur.num_minutes()),
        "hour" => Ok(dur.num_hours()),
        "day" => Ok(dur.num_days()),
        "week" => Ok(dur.num_weeks()),
        "month" => {
            Ok((dt2.year() - dt1.year()) as i64 * 12 + dt2.month() as i64 - dt1.month() as i64)
        }
        "year" => Ok((dt2.year() - dt1.year()) as i64),
        _ => Err(EnvxError::InvalidArgument {
            func: func.to_string(),
            message: format!(
                "unknown unit '{unit}'; use: seconds, minutes, hours, days, weeks, months, years"
            ),
        }),
    }
}

// ─── Emoji lookup ─────────────────────────────────────────────────────────────

fn emoji_lookup(name: &str) -> Option<&'static str> {
    match name {
        // Animals
        "panda" => Some("🐼"),
        "cat" => Some("🐱"),
        "dog" => Some("🐶"),
        "fox" => Some("🦊"),
        "bear" => Some("🐻"),
        "rabbit" => Some("🐰"),
        "penguin" => Some("🐧"),
        "lion" => Some("🦁"),
        "wolf" => Some("🐺"),
        "bird" => Some("🐦"),
        // Faces
        "smile" => Some("😊"),
        "laugh" => Some("😂"),
        "wink" => Some("😉"),
        "cool" => Some("😎"),
        "heart_eyes" => Some("😍"),
        "thinking" => Some("🤔"),
        "party" => Some("🥳"),
        "sad" => Some("😢"),
        // Dev / Tech
        "rocket" => Some("🚀"),
        "computer" => Some("💻"),
        "phone" => Some("📱"),
        "key" => Some("🔑"),
        "lock" => Some("🔒"),
        "gear" => Some("⚙️"),
        "bug" => Some("🐛"),
        "wrench" => Some("🔧"),
        "package" => Some("📦"),
        "chart" => Some("📊"),
        // Nature
        "sun" => Some("☀️"),
        "moon" => Some("🌙"),
        "fire" => Some("🔥"),
        "snow" => Some("❄️"),
        "star" => Some("⭐"),
        "zap" => Some("⚡"),
        "globe" => Some("🌍"),
        "tree" => Some("🌳"),
        "flower" => Some("🌸"),
        "rainbow" => Some("🌈"),
        // Food
        "pizza" => Some("🍕"),
        "coffee" => Some("☕"),
        "beer" => Some("🍺"),
        "cake" => Some("🎂"),
        "apple" => Some("🍎"),
        // Symbols
        "check" => Some("✅"),
        "cross" => Some("❌"),
        "warning" => Some("⚠️"),
        "heart" => Some("❤️"),
        "thumbsup" => Some("👍"),
        "thumbsdown" => Some("👎"),
        "clap" => Some("👏"),
        "wave" => Some("👋"),
        "trophy" => Some("🏆"),
        "flag" => Some("🚩"),
        _ => None,
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
            });
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

    fn s(v: &str) -> Value {
        Value::Str(v.to_string())
    }

    #[test]
    fn trim_whitespace() {
        assert_eq!(
            dispatch("trim", Some(s("  hi  ")), vec![]).unwrap(),
            s("hi")
        );
    }

    #[test]
    fn lower_and_upper() {
        assert_eq!(
            dispatch("lower", Some(s("Hello")), vec![]).unwrap(),
            s("hello")
        );
        assert_eq!(
            dispatch("upper", Some(s("Hello")), vec![]).unwrap(),
            s("HELLO")
        );
    }

    #[test]
    fn len_returns_int() {
        assert_eq!(
            dispatch("len", Some(s("abc")), vec![]).unwrap(),
            Value::Int(3)
        );
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

    // ── capitalize ────────────────────────────────────────────────────────────

    #[test]
    fn capitalize_lowercases_rest() {
        assert_eq!(
            dispatch("capitalize", Some(s("hello world")), vec![]).unwrap(),
            s("Hello world")
        );
    }

    #[test]
    fn capitalize_already_upper() {
        assert_eq!(
            dispatch("capitalize", Some(s("HELLO")), vec![]).unwrap(),
            s("Hello")
        );
    }

    #[test]
    fn capitalize_empty() {
        assert_eq!(dispatch("capitalize", Some(s("")), vec![]).unwrap(), s(""));
    }

    // ── title ─────────────────────────────────────────────────────────────────

    #[test]
    fn title_multiple_words() {
        assert_eq!(
            dispatch("title", Some(s("hello world")), vec![]).unwrap(),
            s("Hello World")
        );
    }

    #[test]
    fn title_single_word() {
        assert_eq!(
            dispatch("title", Some(s("hello")), vec![]).unwrap(),
            s("Hello")
        );
    }

    #[test]
    fn title_preserves_existing_case_of_rest() {
        assert_eq!(
            dispatch("title", Some(s("hELLO wORLD")), vec![]).unwrap(),
            s("HELLO WORLD")
        );
    }

    // ── truncate ──────────────────────────────────────────────────────────────

    #[test]
    fn truncate_cuts_to_length() {
        assert_eq!(
            dispatch("truncate", Some(s("hello world")), vec![Value::Int(5)]).unwrap(),
            s("hello")
        );
    }

    #[test]
    fn truncate_shorter_than_limit_unchanged() {
        assert_eq!(
            dispatch("truncate", Some(s("hi")), vec![Value::Int(10)]).unwrap(),
            s("hi")
        );
    }

    #[test]
    fn truncate_zero_gives_empty() {
        assert_eq!(
            dispatch("truncate", Some(s("hello")), vec![Value::Int(0)]).unwrap(),
            s("")
        );
    }

    #[test]
    fn truncate_rejects_negative() {
        assert!(matches!(
            dispatch("truncate", Some(s("hello")), vec![Value::Int(-1)]),
            Err(EnvxError::InvalidArgument { .. })
        ));
    }

    // ── abs ───────────────────────────────────────────────────────────────────

    #[test]
    fn abs_negative_int() {
        assert_eq!(
            dispatch("abs", Some(Value::Int(-42)), vec![]).unwrap(),
            Value::Int(42)
        );
    }

    #[test]
    fn abs_positive_int_unchanged() {
        assert_eq!(
            dispatch("abs", Some(Value::Int(7)), vec![]).unwrap(),
            Value::Int(7)
        );
    }

    #[test]
    fn abs_zero() {
        assert_eq!(
            dispatch("abs", Some(Value::Int(0)), vec![]).unwrap(),
            Value::Int(0)
        );
    }

    #[test]
    fn abs_rejects_str_receiver() {
        assert!(matches!(
            dispatch("abs", Some(s("hello")), vec![]),
            Err(EnvxError::TypeError { .. })
        ));
    }

    // ── round ─────────────────────────────────────────────────────────────────

    #[test]
    fn round_default_rounds_to_integer() {
        assert_eq!(dispatch("round", Some(s("3.7")), vec![]).unwrap(), s("4"));
    }

    #[test]
    fn round_rounds_down() {
        assert_eq!(dispatch("round", Some(s("3.2")), vec![]).unwrap(), s("3"));
    }

    #[test]
    fn round_two_decimals() {
        assert_eq!(
            dispatch("round", Some(s("3.14159")), vec![Value::Int(2)]).unwrap(),
            s("3.14")
        );
    }

    #[test]
    fn round_integer_string() {
        assert_eq!(dispatch("round", Some(s("5")), vec![]).unwrap(), s("5"));
    }

    #[test]
    fn round_rejects_non_numeric() {
        assert!(matches!(
            dispatch("round", Some(s("abc")), vec![]),
            Err(EnvxError::InvalidArgument { .. })
        ));
    }

    // ── int ───────────────────────────────────────────────────────────────────

    #[test]
    fn int_parses_integer_string() {
        assert_eq!(
            dispatch("int", Some(s("42")), vec![]).unwrap(),
            Value::Int(42)
        );
    }

    #[test]
    fn int_truncates_float() {
        assert_eq!(
            dispatch("int", Some(s("3.9")), vec![]).unwrap(),
            Value::Int(3)
        );
    }

    #[test]
    fn int_negative() {
        assert_eq!(
            dispatch("int", Some(s("-7")), vec![]).unwrap(),
            Value::Int(-7)
        );
    }

    #[test]
    fn int_rejects_non_numeric() {
        assert!(matches!(
            dispatch("int", Some(s("abc")), vec![]),
            Err(EnvxError::InvalidArgument { .. })
        ));
    }

    // ── date functions ────────────────────────────────────────────────────────

    #[test]
    fn timestamp_returns_positive_int() {
        let v = dispatch("timestamp", None, vec![]).unwrap();
        assert!(matches!(v, Value::Int(n) if n > 0));
    }

    #[test]
    fn date_add_days() {
        let v = dispatch(
            "date_add",
            Some(s("2026-06-28T00:00:00")),
            vec![Value::Int(5), s("days")],
        )
        .unwrap();
        assert_eq!(v, s("2026-07-03T00:00:00"));
    }

    #[test]
    fn date_add_negative_subtracts() {
        let v = dispatch(
            "date_add",
            Some(s("2026-06-28T00:00:00")),
            vec![Value::Int(-7), s("days")],
        )
        .unwrap();
        assert_eq!(v, s("2026-06-21T00:00:00"));
    }

    #[test]
    fn date_add_months() {
        let v = dispatch(
            "date_add",
            Some(s("2026-01-15T00:00:00")),
            vec![Value::Int(2), s("months")],
        )
        .unwrap();
        assert_eq!(v, s("2026-03-15T00:00:00"));
    }

    #[test]
    fn date_add_years() {
        let v = dispatch(
            "date_add",
            Some(s("2026-06-28T00:00:00")),
            vec![Value::Int(1), s("years")],
        )
        .unwrap();
        assert_eq!(v, s("2027-06-28T00:00:00"));
    }

    #[test]
    fn date_add_accepts_date_only_input() {
        let v = dispatch(
            "date_add",
            Some(s("2026-06-28")),
            vec![Value::Int(3), s("days")],
        )
        .unwrap();
        assert_eq!(v, s("2026-07-01T00:00:00"));
    }

    #[test]
    fn date_add_rejects_unknown_unit() {
        assert!(matches!(
            dispatch(
                "date_add",
                Some(s("2026-06-28")),
                vec![Value::Int(1), s("fortnight")]
            ),
            Err(EnvxError::InvalidArgument { .. })
        ));
    }

    #[test]
    fn date_diff_days_forward() {
        let v = dispatch(
            "date_diff",
            Some(s("2026-06-28T00:00:00")),
            vec![s("2026-07-08T00:00:00"), s("days")],
        )
        .unwrap();
        assert_eq!(v, Value::Int(10));
    }

    #[test]
    fn date_diff_days_backward_is_negative() {
        let v = dispatch(
            "date_diff",
            Some(s("2026-07-08T00:00:00")),
            vec![s("2026-06-28T00:00:00"), s("days")],
        )
        .unwrap();
        assert_eq!(v, Value::Int(-10));
    }

    #[test]
    fn date_diff_years() {
        let v = dispatch(
            "date_diff",
            Some(s("2020-01-01T00:00:00")),
            vec![s("2026-01-01T00:00:00"), s("years")],
        )
        .unwrap();
        assert_eq!(v, Value::Int(6));
    }

    #[test]
    fn date_format_reformats() {
        let v = dispatch(
            "date_format",
            Some(s("2026-06-28T15:30:00")),
            vec![s("DD/MM/YYYY")],
        )
        .unwrap();
        assert_eq!(v, s("28/06/2026"));
    }

    #[test]
    fn date_format_date_only_input() {
        let v = dispatch("date_format", Some(s("2026-06-28")), vec![s("YYYYMMDD")]).unwrap();
        assert_eq!(v, s("20260628"));
    }

    #[test]
    fn year_extracts() {
        assert_eq!(
            dispatch("year", Some(s("2026-06-28")), vec![]).unwrap(),
            Value::Int(2026)
        );
    }

    #[test]
    fn month_extracts() {
        assert_eq!(
            dispatch("month", Some(s("2026-06-28")), vec![]).unwrap(),
            Value::Int(6)
        );
    }

    #[test]
    fn day_extracts() {
        assert_eq!(
            dispatch("day", Some(s("2026-06-28")), vec![]).unwrap(),
            Value::Int(28)
        );
    }

    #[test]
    fn weekday_returns_full_name() {
        let v = dispatch("weekday", Some(s("2026-06-28")), vec![]).unwrap();
        let valid = [
            "Monday",
            "Tuesday",
            "Wednesday",
            "Thursday",
            "Friday",
            "Saturday",
            "Sunday",
        ];
        let out = match v {
            Value::Str(s) => s,
            _ => panic!(),
        };
        assert!(valid.contains(&out.as_str()), "unexpected: {out}");
    }

    #[test]
    fn weekday_known_day() {
        // 2026-01-01 is a Thursday
        let v = dispatch("weekday", Some(s("2026-01-01")), vec![]).unwrap();
        assert_eq!(v, s("Thursday"));
    }

    #[test]
    fn parse_date_rejects_invalid() {
        assert!(matches!(
            dispatch("year", Some(s("not-a-date")), vec![]),
            Err(EnvxError::InvalidArgument { .. })
        ));
    }

    // ── emoji ─────────────────────────────────────────────────────────────────

    #[test]
    fn emoji_panda() {
        assert_eq!(dispatch("emoji", None, vec![s("panda")]).unwrap(), s("🐼"));
    }

    #[test]
    fn emoji_check() {
        assert_eq!(dispatch("emoji", None, vec![s("check")]).unwrap(), s("✅"));
    }

    #[test]
    fn emoji_rocket() {
        assert_eq!(dispatch("emoji", None, vec![s("rocket")]).unwrap(), s("🚀"));
    }

    #[test]
    fn emoji_unknown_is_error() {
        assert!(matches!(
            dispatch("emoji", None, vec![s("unicorn")]),
            Err(EnvxError::InvalidArgument { .. })
        ));
    }

    #[test]
    fn emoji_rejects_no_args() {
        assert!(matches!(
            dispatch("emoji", None, vec![]),
            Err(EnvxError::ArityError { .. })
        ));
    }

    #[test]
    fn me_returns_author() {
        let v = dispatch("me", None, vec![]).unwrap();
        assert!(matches!(v, Value::Str(_)));
    }

    #[test]
    fn me_rejects_args() {
        assert!(matches!(
            dispatch("me", None, vec![s("x")]),
            Err(EnvxError::ArityError { .. })
        ));
    }

    // ── uuid ──────────────────────────────────────────────────────────────────

    fn is_uuid_format(s: &str) -> bool {
        // xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx  (36 chars, 4 dashes)
        s.len() == 36
            && s.as_bytes()[8] == b'-'
            && s.as_bytes()[13] == b'-'
            && s.as_bytes()[18] == b'-'
            && s.as_bytes()[23] == b'-'
    }

    #[test]
    fn uuid_default_is_v4() {
        let v = dispatch("uuid", None, vec![]).unwrap();
        let out = match v {
            Value::Str(s) => s,
            _ => panic!(),
        };
        assert!(is_uuid_format(&out), "bad format: {out}");
        assert_eq!(&out[14..15], "4", "version digit: {out}");
    }

    #[test]
    fn uuid_explicit_v4() {
        let v = dispatch("uuid", None, vec![Value::Int(4)]).unwrap();
        let out = match v {
            Value::Str(s) => s,
            _ => panic!(),
        };
        assert!(is_uuid_format(&out));
        assert_eq!(&out[14..15], "4");
    }

    #[test]
    fn uuid_v7() {
        let v = dispatch("uuid", None, vec![Value::Int(7)]).unwrap();
        let out = match v {
            Value::Str(s) => s,
            _ => panic!(),
        };
        assert!(is_uuid_format(&out), "bad format: {out}");
        assert_eq!(&out[14..15], "7", "version digit: {out}");
    }

    #[test]
    fn uuid_two_calls_differ() {
        let a = dispatch("uuid", None, vec![]).unwrap();
        let b = dispatch("uuid", None, vec![]).unwrap();
        assert_ne!(a, b);
    }

    #[test]
    fn uuid_rejects_unsupported_version() {
        assert!(matches!(
            dispatch("uuid", None, vec![Value::Int(1)]),
            Err(EnvxError::InvalidArgument { .. })
        ));
    }

    #[test]
    fn uuid_rejects_pipe_receiver() {
        assert!(matches!(
            dispatch("uuid", Some(s("x")), vec![]),
            Err(EnvxError::ArityError { .. })
        ));
    }

    #[test]
    fn now_default_format_is_iso8601() {
        let v = dispatch("now", None, vec![]).unwrap();
        let s = match v {
            Value::Str(s) => s,
            _ => panic!("expected Str"),
        };
        // ISO 8601: YYYY-MM-DDTHH:MM:SS (19 chars)
        assert_eq!(s.len(), 19, "default now() format: {s}");
        assert_eq!(&s[4..5], "-");
        assert_eq!(&s[7..8], "-");
        assert_eq!(&s[10..11], "T");
    }

    #[test]
    fn now_yyyymmdd_format() {
        let v = dispatch("now", None, vec![s("YYYYMMDD")]).unwrap();
        let out = match v {
            Value::Str(s) => s,
            _ => panic!(),
        };
        assert_eq!(out.len(), 8, "YYYYMMDD should be 8 chars: {out}");
        assert!(out.chars().all(|c| c.is_ascii_digit()), "all digits: {out}");
    }

    #[test]
    fn now_yyyy_mm_dd_format() {
        let v = dispatch("now", None, vec![s("YYYY-MM-DD")]).unwrap();
        let out = match v {
            Value::Str(s) => s,
            _ => panic!(),
        };
        assert_eq!(out.len(), 10, "YYYY-MM-DD: {out}");
        assert_eq!(&out[4..5], "-");
        assert_eq!(&out[7..8], "-");
    }

    #[test]
    fn now_year_only() {
        let v = dispatch("now", None, vec![s("YYYY")]).unwrap();
        let out = match v {
            Value::Str(s) => s,
            _ => panic!(),
        };
        assert_eq!(out.len(), 4);
        assert!(out.parse::<u32>().is_ok(), "should be a number: {out}");
    }

    #[test]
    fn now_rejects_pipe_receiver() {
        assert!(matches!(
            dispatch("now", Some(s("anything")), vec![]),
            Err(EnvxError::ArityError { .. })
        ));
    }

    #[test]
    fn now_rejects_too_many_args() {
        assert!(matches!(
            dispatch("now", None, vec![s("YYYY"), s("extra")]),
            Err(EnvxError::ArityError { .. })
        ));
    }

    #[test]
    fn secret_default_is_32_alphanumeric() {
        let v = dispatch("secret", None, vec![]).unwrap();
        let s = match v {
            Value::Str(s) => s,
            _ => panic!(),
        };
        assert_eq!(s.len(), 32);
        assert!(
            s.chars().all(|c| c.is_ascii_alphanumeric()),
            "non-alphanumeric: {s}"
        );
    }

    #[test]
    fn secret_custom_length() {
        let v = dispatch("secret", None, vec![Value::Int(64)]).unwrap();
        let s = match v {
            Value::Str(s) => s,
            _ => panic!(),
        };
        assert_eq!(s.len(), 64);
    }

    #[test]
    fn secret_custom_alphabet() {
        let v = dispatch("secret", None, vec![Value::Int(16), s("abcdef0123456789")]).unwrap();
        let out = match v {
            Value::Str(s) => s,
            _ => panic!(),
        };
        assert_eq!(out.len(), 16);
        assert!(
            out.chars().all(|c| "abcdef0123456789".contains(c)),
            "non-hex: {out}"
        );
    }

    #[test]
    fn secret_preset_hex() {
        let v = dispatch("secret", None, vec![Value::Int(32), s("hex")]).unwrap();
        let out = match v {
            Value::Str(s) => s,
            _ => panic!(),
        };
        assert_eq!(out.len(), 32);
        assert!(
            out.chars().all(|c| "0123456789abcdef".contains(c)),
            "non-hex: {out}"
        );
    }

    #[test]
    fn secret_preset_base64() {
        let valid = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        let v = dispatch("secret", None, vec![Value::Int(32), s("base64")]).unwrap();
        let out = match v {
            Value::Str(s) => s,
            _ => panic!(),
        };
        assert_eq!(out.len(), 32);
        assert!(out.chars().all(|c| valid.contains(c)), "non-base64: {out}");
    }

    #[test]
    fn secret_preset_base64url() {
        let valid = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
        let v = dispatch("secret", None, vec![Value::Int(32), s("base64url")]).unwrap();
        let out = match v {
            Value::Str(s) => s,
            _ => panic!(),
        };
        assert_eq!(out.len(), 32);
        assert!(
            out.chars().all(|c| valid.contains(c)),
            "non-base64url: {out}"
        );
    }

    #[test]
    fn secret_preset_alpha() {
        let v = dispatch("secret", None, vec![Value::Int(20), s("alpha")]).unwrap();
        let out = match v {
            Value::Str(s) => s,
            _ => panic!(),
        };
        assert_eq!(out.len(), 20);
        assert!(
            out.chars().all(|c| c.is_ascii_alphabetic()),
            "non-alpha: {out}"
        );
    }

    #[test]
    fn secret_preset_numeric() {
        let v = dispatch("secret", None, vec![Value::Int(10), s("numeric")]).unwrap();
        let out = match v {
            Value::Str(s) => s,
            _ => panic!(),
        };
        assert_eq!(out.len(), 10);
        assert!(
            out.chars().all(|c| c.is_ascii_digit()),
            "non-numeric: {out}"
        );
    }

    #[test]
    fn secret_two_calls_differ() {
        let a = dispatch("secret", None, vec![]).unwrap();
        let b = dispatch("secret", None, vec![]).unwrap();
        // Astronomically unlikely to collide (62^32 possibilities).
        assert_ne!(a, b);
    }

    #[test]
    fn secret_rejects_pipe_receiver() {
        assert!(matches!(
            dispatch("secret", Some(s("x")), vec![]),
            Err(EnvxError::ArityError { .. })
        ));
    }

    #[test]
    fn secret_rejects_zero_length() {
        assert!(matches!(
            dispatch("secret", None, vec![Value::Int(0)]),
            Err(EnvxError::InvalidArgument { .. })
        ));
    }

    #[test]
    fn secret_rejects_empty_alphabet() {
        assert!(matches!(
            dispatch("secret", None, vec![Value::Int(8), s("")]),
            Err(EnvxError::InvalidArgument { .. })
        ));
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
