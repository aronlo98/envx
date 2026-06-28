use logos::{Lexer, Logos};
use miette::NamedSource;

use crate::ast::Span;
use crate::error::{EnvxError, Result};

/// A token paired with its source byte-offset span.
pub type Spanned<T> = (T, Span);

// ─── Outer file tokens ────────────────────────────────────────────────────────

/// Tokens produced when scanning the outer structure of a `.envx` file.
///
/// The lexer is intentionally coarse at this level: it identifies the key,
/// the `=`, the `@import` keyword, and captures the raw string value (content
/// between `"..."` with quotes stripped). The parser (Phase B) is responsible
/// for splitting string values into template segments and parsing expressions.
///
/// Horizontal whitespace and carriage-returns are skipped automatically by
/// the `skip` rule so the parser never sees them.
#[derive(Logos, Debug, Clone, PartialEq)]
#[logos(skip r"[ \t\r]+")] // skip horizontal whitespace and \r
pub enum FileToken {
    /// `@import` directive keyword.
    #[token("@import")]
    Import,

    /// A bare identifier: the variable key (e.g. `APP_ENV`) or other unquoted
    /// word. Matches `[A-Za-z_][A-Za-z0-9_]*`.
    #[regex(r"[A-Za-z_][A-Za-z0-9_]*")]
    Ident,

    /// The `=` assignment operator.
    #[token("=")]
    Eq,

    /// A double-quoted string value.
    ///
    /// When logos matches the opening `"`, the `scan_string` callback takes
    /// over, scanning forward for the closing `"` and bumping the lexer past
    /// it. The associated `String` is the raw content between the quotes
    /// (quotes stripped; escape sequences preserved verbatim for the template
    /// scanner to handle later).
    ///
    /// Returning `None` from the callback signals an unterminated string,
    /// which logos converts to `Err(())` on that token.
    #[token("\"", scan_string)]
    StringContent(String),

    /// A line comment beginning with `#`. The entire comment text (including
    /// the `#`) is consumed and discarded — comments carry no semantics.
    #[regex(r"#[^\n]*")]
    Comment,

    /// A newline character. Significant because `.envx` is line-oriented:
    /// each assignment or directive occupies exactly one logical line.
    #[token("\n")]
    Newline,
}

/// Logos callback for `FileToken::StringContent`.
///
/// Called immediately after logos has matched the opening `"`. Scans the
/// remaining input byte-by-byte looking for the closing `"`, respecting the
/// single-level escape `\"` (a literal double-quote inside the string).
///
/// On success: bumps the lexer past the closing `"` and returns the raw
/// content between the quotes.
/// On failure (unterminated string): returns `None`, causing logos to emit
/// `Err(())` for this token.
fn scan_string(lex: &mut Lexer<FileToken>) -> Option<String> {
    let rest = lex.remainder();
    let bytes = rest.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'"' => {
                // Found the closing quote.
                let content = rest[..i].to_string();
                lex.bump(i + 1); // consume content + closing quote
                return Some(content);
            }
            // `\"` inside the string: skip both characters so the escaped
            // quote does not terminate the string.
            b'\\' if i + 1 < bytes.len() => i += 2,
            _ => i += 1,
        }
    }
    // Reached end of input without a closing `"` → unterminated string.
    None
}

// ─── Inner expression tokens ──────────────────────────────────────────────────

/// Tokens produced when scanning the content inside a `${{ }}` expression block.
///
/// Whitespace within expressions is skipped. Keyword tokens (`if`, `then`,
/// `else`, `true`, `false`) are listed as `#[token]` rules before the generic
/// `Ident` `#[regex]` rule. Because logos gives higher priority to literal
/// token matches than to regex matches of the same length, `if` followed by a
/// non-identifier character is always tokenized as `If`, never as `Ident("if")`.
/// `iface` is correctly tokenized as `Ident("iface")` because the regex matches
/// 5 characters while the literal `if` only matches 2 (longest match wins).
#[derive(Logos, Debug, Clone, PartialEq)]
#[logos(skip r"[ \t]+")] // skip horizontal whitespace inside expressions
pub enum ExprToken {
    // ── Keywords ──────────────────────────────────────────────────────────────
    #[token("if")]    If,
    #[token("then")]  Then,
    #[token("else")]  Else,
    #[token("true")]  True,
    #[token("false")] False,

    // ── Values ────────────────────────────────────────────────────────────────

    /// A variable reference: `$NAME`.
    /// The associated `String` is the variable name **without** the leading `$`.
    #[regex(r"\$[A-Za-z_][A-Za-z0-9_]*", |lex| lex.slice()[1..].to_string())]
    VarRef(String),

    /// A generic identifier: a function name (`trim`, `lower`, `replace`,
    /// `ENV`, `eq`, `default`, …) or any word not matched as a keyword.
    #[regex(r"[A-Za-z_][A-Za-z0-9_]*", |lex| lex.slice().to_string())]
    Ident(String),

    /// A single-quoted string literal used inside expressions: `'prod'`, `' '`.
    /// The associated `String` is the content **without** the surrounding quotes.
    /// Double-quotes are not used inside expressions to avoid ambiguity with the
    /// outer template string delimiter.
    #[regex(r"'[^']*'", |lex| {
        let s = lex.slice();
        s[1..s.len() - 1].to_string()
    })]
    StrLit(String),

    /// An integer literal: `8080`, `-1`.
    /// Capped at 18 digits by the regex to guarantee the parse never overflows
    /// `i64`. Returns `None` (lex error) if the parse somehow fails.
    #[regex(r"-?[0-9]{1,18}", |lex| lex.slice().parse::<i64>().ok())]
    IntLit(i64),

    // ── Operators & punctuation ───────────────────────────────────────────────

    /// Pipe operator — chains the output of one expression into a function:
    /// `$NAME | trim | lower`.
    #[token("|")]  Pipe,

    /// Opening parenthesis for function arguments: `replace(' ', '_')`.
    #[token("(")]  LParen,

    /// Closing parenthesis.
    #[token(")")]  RParen,

    /// Argument separator inside function calls.
    #[token(",")]  Comma,
}

// ─── Public tokenization API ──────────────────────────────────────────────────

/// Tokenize the outer structure of a `.envx` source file.
///
/// `source`   — full source text of the file.
/// `filename` — used in error diagnostics (e.g. `"app.envx"`).
///
/// Returns a flat list of `(FileToken, Span)` pairs in source order.
/// Comments are included in the token stream (so the parser can skip them)
/// but carry no data.
pub fn lex_file(source: &str, filename: &str) -> Result<Vec<Spanned<FileToken>>> {
    let mut tokens = Vec::new();
    let mut lex = FileToken::lexer(source);

    while let Some(result) = lex.next() {
        let ls = lex.span(); // byte range within `source`
        let span = Span::new(ls.start, ls.end);

        match result {
            Ok(tok) => tokens.push((tok, span)),
            Err(()) => {
                // Distinguish unterminated strings from other unexpected chars:
                // `StringContent`'s callback returns `None` when the closing `"`
                // is missing, but the *opening* `"` is what logos matched before
                // calling the callback. The opening `"` is at `ls.start` in the
                // original source.
                let ch = source.as_bytes().get(ls.start).copied();
                if ch == Some(b'"') {
                    return Err(EnvxError::UnterminatedString {
                        src: named(filename, source),
                        span: (ls.start, 1).into(),
                    });
                }
                return Err(EnvxError::UnexpectedChar {
                    src: named(filename, source),
                    span: (ls.start, (ls.end - ls.start).max(1)).into(),
                });
            }
        }
    }
    Ok(tokens)
}

/// Tokenize the content of a `${{ }}` expression block.
///
/// `source`      — the raw expression text (no `${{` / `}}` delimiters).
/// `base_offset` — byte offset of `source`'s first character within the
///                 parent file. Added to every span so that error diagnostics
///                 point into the original source rather than the expression
///                 substring.
/// `filename`    — used in error diagnostics.
pub fn lex_expr(
    source: &str,
    base_offset: usize,
    filename: &str,
) -> Result<Vec<Spanned<ExprToken>>> {
    let mut tokens = Vec::new();
    let mut lex = ExprToken::lexer(source);

    while let Some(result) = lex.next() {
        let ls = lex.span();
        // Translate local offsets into file-level offsets.
        let span = Span::new(base_offset + ls.start, base_offset + ls.end);

        match result {
            Ok(tok) => tokens.push((tok, span)),
            Err(()) => {
                return Err(EnvxError::UnexpectedChar {
                    src: named(filename, source),
                    // Report position relative to `source` for display.
                    span: (ls.start, 1).into(),
                });
            }
        }
    }
    Ok(tokens)
}

// ─── Helper ───────────────────────────────────────────────────────────────────

/// Construct a `NamedSource` from a filename and source string.
/// Used throughout this module when building diagnostics.
#[inline]
fn named(filename: &str, source: &str) -> NamedSource<String> {
    NamedSource::new(filename, source.to_string())
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lex_simple_assignment() {
        let src = "NAME = \"John Doe\"\n";
        let tokens = lex_file(src, "test.envx").unwrap();
        let kinds: Vec<_> = tokens.iter().map(|(t, _)| t).collect();
        assert!(matches!(kinds[0], FileToken::Ident));
        assert!(matches!(kinds[1], FileToken::Eq));
        assert!(matches!(kinds[2], FileToken::StringContent(_)));
        assert!(matches!(kinds[3], FileToken::Newline));
    }

    #[test]
    fn string_content_is_stripped() {
        let src = "X = \"hello world\"\n";
        let tokens = lex_file(src, "test.envx").unwrap();
        let content = tokens.iter().find_map(|(t, _)| {
            if let FileToken::StringContent(s) = t { Some(s.clone()) } else { None }
        });
        assert_eq!(content.unwrap(), "hello world");
    }

    #[test]
    fn string_with_template_block() {
        // The outer lexer treats the ${{ }} as raw text inside the string.
        // The parser (Phase B) will later split it into segments.
        let src = "X = \"${{ $NAME | trim }}\"\n";
        let tokens = lex_file(src, "test.envx").unwrap();
        let content = tokens.iter().find_map(|(t, _)| {
            if let FileToken::StringContent(s) = t { Some(s.clone()) } else { None }
        });
        assert_eq!(content.unwrap(), "${{ $NAME | trim }}");
    }

    #[test]
    fn unterminated_string_errors() {
        let src = "X = \"oops\n";
        assert!(lex_file(src, "test.envx").is_err());
    }

    #[test]
    fn lex_import_directive() {
        let src = "@import \"./base.envx\"\n";
        let tokens = lex_file(src, "test.envx").unwrap();
        assert!(matches!(tokens[0].0, FileToken::Import));
        assert!(matches!(&tokens[1].0, FileToken::StringContent(s) if s == "./base.envx"));
    }

    #[test]
    fn lex_expr_pipe_chain() {
        let src = "$NAME | trim | lower";
        let tokens = lex_expr(src, 0, "test").unwrap();
        assert!(matches!(&tokens[0].0, ExprToken::VarRef(n) if n == "NAME"));
        assert!(matches!(tokens[1].0, ExprToken::Pipe));
        assert!(matches!(&tokens[2].0, ExprToken::Ident(n) if n == "trim"));
        assert!(matches!(tokens[3].0, ExprToken::Pipe));
        assert!(matches!(&tokens[4].0, ExprToken::Ident(n) if n == "lower"));
    }

    #[test]
    fn lex_expr_if_then_else() {
        let src = "if eq($APP_ENV, 'prod') then 'api' else 'localhost'";
        let tokens = lex_expr(src, 0, "test").unwrap();
        assert!(matches!(tokens[0].0, ExprToken::If));
        assert!(matches!(&tokens[1].0, ExprToken::Ident(n) if n == "eq"));
        assert!(matches!(tokens[2].0, ExprToken::LParen));
        assert!(matches!(&tokens[3].0, ExprToken::VarRef(n) if n == "APP_ENV"));
        assert!(matches!(tokens[4].0, ExprToken::Comma));
        assert!(matches!(&tokens[5].0, ExprToken::StrLit(s) if s == "prod"));
        assert!(matches!(tokens[6].0, ExprToken::RParen));
        assert!(matches!(tokens[7].0, ExprToken::Then));
        assert!(matches!(&tokens[8].0, ExprToken::StrLit(s) if s == "api"));
        assert!(matches!(tokens[9].0, ExprToken::Else));
        assert!(matches!(&tokens[10].0, ExprToken::StrLit(s) if s == "localhost"));
    }

    #[test]
    fn lex_expr_int_and_default() {
        let src = "ENV('PORT') | default(8080)";
        let tokens = lex_expr(src, 0, "test").unwrap();
        assert!(matches!(&tokens[0].0, ExprToken::Ident(n) if n == "ENV"));
        assert!(matches!(tokens[1].0, ExprToken::LParen));
        assert!(matches!(&tokens[2].0, ExprToken::StrLit(s) if s == "PORT"));
        assert!(matches!(tokens[3].0, ExprToken::RParen));
        assert!(matches!(tokens[4].0, ExprToken::Pipe));
        assert!(matches!(&tokens[5].0, ExprToken::Ident(n) if n == "default"));
        assert!(matches!(tokens[6].0, ExprToken::LParen));
        assert!(matches!(&tokens[7].0, ExprToken::IntLit(8080)));
        assert!(matches!(tokens[8].0, ExprToken::RParen));
    }

    #[test]
    fn keyword_not_matched_as_ident_prefix() {
        // `iface` must be Ident, not If + Ident("ace")
        let src = "iface";
        let tokens = lex_expr(src, 0, "test").unwrap();
        assert_eq!(tokens.len(), 1);
        assert!(matches!(&tokens[0].0, ExprToken::Ident(n) if n == "iface"));
    }

    #[test]
    fn comment_skipped_in_file() {
        let src = "# this is a comment\nX = \"1\"\n";
        let tokens = lex_file(src, "test.envx").unwrap();
        // Comment token is present but parser will skip it
        let has_comment = tokens.iter().any(|(t, _)| matches!(t, FileToken::Comment));
        assert!(has_comment);
    }
}
