use std::path::{Path, PathBuf};

use miette::NamedSource;

use crate::{
    ast::{EnvxFile, Expr, FnCall, Segment, Span, Statement, Template},
    error::{EnvxError, Result},
    lexer::{lex_expr, lex_file, ExprToken, FileToken, Spanned},
    value::Value,
};

// ─── Public entry point ───────────────────────────────────────────────────────

/// Parse a `.envx` source string into an `EnvxFile` AST.
///
/// Runs Phases A + B in sequence:
///   A) `lex_file` → `Vec<Spanned<FileToken>>`
///   B) `FileParser` → `EnvxFile { statements }`
///
/// `file_path` must be the absolute path of the file being parsed; it is used
/// to resolve relative `@import` paths and to attach origin information to each
/// `Statement::Entry`.
pub fn parse(source: &str, filename: &str, file_path: PathBuf) -> Result<EnvxFile> {
    let tokens = lex_file(source, filename)?;
    let mut p = FileParser::new(&tokens, source, filename, &file_path);
    let statements = p.parse_statements()?;
    Ok(EnvxFile { path: file_path, source: source.to_string(), statements })
}

// ─── File-level parser (Phase B outer) ───────────────────────────────────────

struct FileParser<'s> {
    tokens: &'s [Spanned<FileToken>],
    pos: usize,
    source: &'s str,
    filename: &'s str,
    file_path: &'s Path,
}

impl<'s> FileParser<'s> {
    fn new(
        tokens: &'s [Spanned<FileToken>],
        source: &'s str,
        filename: &'s str,
        file_path: &'s Path,
    ) -> Self {
        Self { tokens, pos: 0, source, filename, file_path }
    }

    /// Drive the outer parse loop: one statement per logical line.
    fn parse_statements(&mut self) -> Result<Vec<Statement>> {
        let mut stmts = Vec::new();
        while self.pos < self.tokens.len() {
            let (tok, span) = self.tokens[self.pos].clone();
            match tok {
                // Blank lines and comments carry no semantics.
                FileToken::Newline | FileToken::Comment => {
                    self.pos += 1;
                }
                FileToken::Import => {
                    self.pos += 1;
                    stmts.push(self.parse_import(span)?);
                }
                FileToken::Ident => {
                    // The key text lives in the source at the token's span.
                    let key = self.source[span.start..span.end].to_string();
                    self.pos += 1;
                    stmts.push(self.parse_entry(key, span)?);
                }
                other => {
                    return Err(EnvxError::UnexpectedToken {
                        expected: "variable assignment (`KEY = \"…\"`) or `@import`".into(),
                        found: format!("{:?}", other),
                        src: self.named(),
                        span: span.into(),
                    });
                }
            }
        }
        Ok(stmts)
    }

    /// Parse the path string that follows an `@import` keyword.
    fn parse_import(&mut self, import_span: Span) -> Result<Statement> {
        let (tok, content_span) = self.require_next("import path string")?;
        match tok {
            FileToken::StringContent(raw_path) => {
                self.skip_newline();
                // Resolve the path relative to the directory that contains this file.
                let parent = self.file_path.parent().unwrap_or(Path::new("."));
                let resolved = parent.join(&raw_path);
                Ok(Statement::Import {
                    raw_path,
                    resolved,
                    span: import_span.merge(content_span),
                })
            }
            other => Err(EnvxError::UnexpectedToken {
                expected: "import path string".into(),
                found: format!("{:?}", other),
                src: self.named(),
                span: content_span.into(),
            }),
        }
    }

    /// Parse `= "value"` after a key identifier has already been consumed.
    fn parse_entry(&mut self, key: String, key_span: Span) -> Result<Statement> {
        // Consume `=`
        let (eq_tok, eq_span) = self.require_next("`=`")?;
        if !matches!(eq_tok, FileToken::Eq) {
            return Err(EnvxError::UnexpectedToken {
                expected: "`=`".into(),
                found: format!("{:?}", eq_tok),
                src: self.named(),
                span: eq_span.into(),
            });
        }

        // Consume the value — either a quoted string or a bare unquoted word.
        let (val_tok, val_span) = self.require_next("value")?;
        match val_tok {
            FileToken::StringContent(raw) => {
                self.skip_newline();
                // `val_span` covers the opening `"` + content + closing `"`.
                // The raw content begins one byte after the opening quote.
                let base_offset = val_span.start + 1;
                let template = self.scan_template(&raw, base_offset)?;
                Ok(Statement::Entry {
                    key,
                    template,
                    source: self.file_path.to_path_buf(),
                    span: key_span.merge(val_span),
                })
            }
            // Bare (unquoted) value: `ENV = development`, `PORT = 8080`, etc.
            // Collect consecutive non-Newline tokens and reconstruct the full
            // text from source byte offsets (spaces are preserved this way even
            // though logos skips them between tokens).
            FileToken::Ident | FileToken::BareChunk => {
                let val_start = val_span.start;
                let mut val_end = val_span.end;
                loop {
                    match self.tokens.get(self.pos) {
                        Some((FileToken::Newline, _))
                        | Some((FileToken::Comment, _))
                        | None => break,
                        Some((_, span)) => {
                            val_end = span.end;
                            self.pos += 1;
                        }
                    }
                }
                self.skip_newline();
                let raw = self.source[val_start..val_end].trim_end().to_string();
                let template = Template {
                    segments: vec![Segment::Literal(raw)],
                    span: Span::new(val_start, val_end),
                };
                Ok(Statement::Entry {
                    key,
                    template,
                    source: self.file_path.to_path_buf(),
                    span: key_span.merge(Span::new(val_start, val_end)),
                })
            }
            other => Err(EnvxError::UnexpectedToken {
                expected: "value (quoted string or bare word)".into(),
                found: format!("{:?}", other),
                src: self.named(),
                span: val_span.into(),
            }),
        }
    }

    // ─── Template scanner (Phase B inner setup) ───────────────────────────────

    /// Split a raw string value (the text between `"…"`) into `Segment`s.
    ///
    /// Scans `raw` looking for `${{` … `}}` delimiters, producing:
    ///   - `Segment::Literal`  for text outside `${{ }}`
    ///   - `Segment::Expr`     for each parsed expression block
    ///
    /// Single-quoted strings within expressions (`'prod'`) are treated as opaque
    /// so that a `}` inside a string literal never closes the block early.
    ///
    /// `base_offset` is the byte position of `raw[0]` within the parent file,
    /// used to shift all spans so error diagnostics point into the original source.
    fn scan_template(&self, raw: &str, base_offset: usize) -> Result<Template> {
        let mut segments: Vec<Segment> = Vec::new();
        let bytes = raw.as_bytes();
        let len = bytes.len();
        let mut pos = 0;
        let mut lit_start = 0;

        while pos < len {
            if pos + 3 <= len && &raw[pos..pos + 3] == "${{" {
                // ── Flush preceding literal text ──────────────────────────────
                if pos > lit_start {
                    segments.push(Segment::Literal(raw[lit_start..pos].to_string()));
                }

                let open_span = Span::new(base_offset + pos, base_offset + pos + 3);
                let expr_start = pos + 3;

                // ── Locate the closing `}}` ───────────────────────────────────
                // Track single-quoted strings so `'}'` inside them is ignored.
                let mut inner = expr_start;
                let mut in_str = false;
                while inner < len {
                    if in_str {
                        if bytes[inner] == b'\'' {
                            in_str = false;
                        }
                        inner += 1;
                    } else if bytes[inner] == b'\'' {
                        in_str = true;
                        inner += 1;
                    } else if inner + 2 <= len && &raw[inner..inner + 2] == "}}" {
                        break;
                    } else {
                        inner += 1;
                    }
                }

                if inner >= len {
                    return Err(EnvxError::UnclosedExpression {
                        src: self.named(),
                        span: open_span.into(),
                    });
                }

                let expr_content = &raw[expr_start..inner];
                let expr_base = base_offset + expr_start;

                if expr_content.trim().is_empty() {
                    let block_len = (inner + 2) - pos;
                    return Err(EnvxError::EmptyExpression {
                        src: self.named(),
                        span: (base_offset + pos, block_len).into(),
                    });
                }

                // ── Tokenize + parse the expression ───────────────────────────
                let expr_tokens = lex_expr(expr_content, expr_base, self.filename)?;
                let expr = parse_expr_tokens(&expr_tokens, self.source, self.filename)?;
                segments.push(Segment::Expr(expr));

                pos = inner + 2; // advance past `}}`
                lit_start = pos;
            } else {
                pos += 1;
            }
        }

        // Flush trailing literal text after the last `}}`.
        if lit_start < len {
            segments.push(Segment::Literal(raw[lit_start..].to_string()));
        }
        // Guard: an empty string `""` produces one empty literal so that
        // `Template::segments` is never empty.
        if segments.is_empty() {
            segments.push(Segment::Literal(String::new()));
        }

        let span = Span::new(base_offset, base_offset + len);
        Ok(Template { segments, span })
    }

    // ─── Helpers ─────────────────────────────────────────────────────────────

    /// Advance past an optional trailing `\n` (so the caller doesn't have to).
    fn skip_newline(&mut self) {
        if let Some((FileToken::Newline, _)) = self.tokens.get(self.pos).map(|(t, s)| (t, *s)) {
            self.pos += 1;
        }
    }

    /// Consume the next token, returning it. Returns an error if at EOF.
    fn require_next(&mut self, expected: &str) -> Result<(FileToken, Span)> {
        match self.tokens.get(self.pos) {
            Some((tok, span)) => {
                let result = (tok.clone(), *span);
                self.pos += 1;
                Ok(result)
            }
            None => Err(EnvxError::UnexpectedToken {
                expected: expected.into(),
                found: "end of file".into(),
                src: self.named(),
                span: (self.source.len(), 0).into(),
            }),
        }
    }

    fn named(&self) -> NamedSource<String> {
        NamedSource::new(self.filename, self.source.to_string())
    }
}

// ─── Expression token → AST (Phase B inner parser) ───────────────────────────

/// Parse a flat slice of `ExprToken`s into a single root `Expr`.
///
/// Called once per `${{ }}` block, after `lex_expr` has already tokenized the
/// block's content. All tokens must be consumed; leftover tokens are an error.
fn parse_expr_tokens(
    tokens: &[Spanned<ExprToken>],
    source: &str,
    filename: &str,
) -> Result<Expr> {
    let mut p = ExprParser::new(tokens, source, filename);
    let expr = p.parse_pipe_chain()?;

    if p.pos < p.tokens.len() {
        let (extra, span) = p.tokens[p.pos].clone();
        return Err(EnvxError::UnexpectedToken {
            expected: "end of expression or `|`".into(),
            found: format!("{:?}", extra),
            src: NamedSource::new(filename, source.to_string()),
            span: span.into(),
        });
    }
    Ok(expr)
}

// ─── Expression parser ────────────────────────────────────────────────────────

struct ExprParser<'s> {
    tokens: &'s [Spanned<ExprToken>],
    pos: usize,
    source: &'s str,
    filename: &'s str,
}

impl<'s> ExprParser<'s> {
    fn new(tokens: &'s [Spanned<ExprToken>], source: &'s str, filename: &'s str) -> Self {
        Self { tokens, pos: 0, source, filename }
    }

    /// Peek at the current token without consuming it (returns a clone).
    fn peek(&self) -> Option<(ExprToken, Span)> {
        self.tokens.get(self.pos).map(|(t, s)| (t.clone(), *s))
    }

    /// Consume and return the current token.
    fn advance(&mut self) -> Option<(ExprToken, Span)> {
        let (tok, span) = self.tokens.get(self.pos)?;
        let result = (tok.clone(), *span);
        self.pos += 1;
        Some(result)
    }

    /// Span of one past the last token — used for "unexpected EOF" diagnostics.
    fn eof_span(&self) -> Span {
        self.tokens.last().map(|(_, s)| Span::new(s.end, s.end)).unwrap_or_default()
    }

    fn named(&self) -> NamedSource<String> {
        NamedSource::new(self.filename, self.source.to_string())
    }

    // ── Grammar ───────────────────────────────────────────────────────────────
    //
    //   expr        := pipe_chain
    //   pipe_chain  := primary  ( '|'  fn_name  ( '(' arg_list ')' )? )*
    //   primary     := VarRef
    //                | StrLit | IntLit | 'true' | 'false'
    //                | Ident '(' arg_list ')'
    //                | 'if' pipe_chain 'then' pipe_chain 'else' pipe_chain
    //   arg_list    := ( pipe_chain ( ',' pipe_chain )* )?

    /// Top-level rule: left-associative chain of `|` pipe steps.
    ///
    /// `$NAME | trim | lower` parses as
    /// `Pipe { Pipe { VarRef("NAME"), trim() }, lower() }`.
    fn parse_pipe_chain(&mut self) -> Result<Expr> {
        let mut expr = self.parse_primary()?;

        while let Some((ExprToken::Pipe, _)) = self.peek() {
            self.advance(); // consume `|`

            // After `|` must come an identifier (function name).
            match self.advance() {
                Some((ExprToken::Ident(name), fn_span)) => {
                    // Explicit argument list is optional for no-arg functions
                    // like `| trim` and `| lower`.
                    let args = if let Some((ExprToken::LParen, _)) = self.peek() {
                        self.advance(); // consume `(`
                        let a = self.parse_arg_list()?;
                        self.consume(ExprToken::RParen, ")")?;
                        a
                    } else {
                        Vec::new()
                    };

                    let pipe_span = expr.span().merge(fn_span);
                    let func = FnCall { name, args, span: fn_span };
                    expr = Expr::Pipe { expr: Box::new(expr), func, span: pipe_span };
                }
                Some((other, span)) => {
                    return Err(EnvxError::UnexpectedToken {
                        expected: "function name after `|`".into(),
                        found: format!("{:?}", other),
                        src: self.named(),
                        span: span.into(),
                    });
                }
                None => {
                    return Err(EnvxError::UnexpectedToken {
                        expected: "function name after `|`".into(),
                        found: "end of expression".into(),
                        src: self.named(),
                        span: self.eof_span().into(),
                    });
                }
            }
        }
        Ok(expr)
    }

    /// Parse one non-pipe atom.
    fn parse_primary(&mut self) -> Result<Expr> {
        match self.advance() {
            // ── Variable reference: `$NAME` ───────────────────────────────────
            Some((ExprToken::VarRef(name), span)) => {
                Ok(Expr::VarRef { name, span })
            }

            // ── Literals ─────────────────────────────────────────────────────
            Some((ExprToken::StrLit(s), span)) => {
                Ok(Expr::Literal(Value::Str(s), span))
            }
            Some((ExprToken::IntLit(n), span)) => {
                Ok(Expr::Literal(Value::Int(n), span))
            }
            Some((ExprToken::True, span)) => {
                Ok(Expr::Literal(Value::Bool(true), span))
            }
            Some((ExprToken::False, span)) => {
                Ok(Expr::Literal(Value::Bool(false), span))
            }

            // ── Direct function call: `name(args…)` ───────────────────────────
            // Includes the special `ENV('KEY')` → `Expr::EnvLookup` rewrite.
            Some((ExprToken::Ident(name), fn_span)) => {
                self.parse_fn_call(name, fn_span)
            }

            // ── Conditional: `if cond then val else val` ──────────────────────
            Some((ExprToken::If, if_span)) => {
                self.parse_if_expr(if_span)
            }

            Some((other, span)) => Err(EnvxError::UnexpectedToken {
                expected: "expression ($VAR, literal, function call, or `if …`)".into(),
                found: format!("{:?}", other),
                src: self.named(),
                span: span.into(),
            }),
            None => Err(EnvxError::UnexpectedToken {
                expected: "expression".into(),
                found: "end of expression".into(),
                src: self.named(),
                span: self.eof_span().into(),
            }),
        }
    }

    /// Parse `name '(' arg_list ')'`.
    ///
    /// Rewrites `ENV('KEY')` into `Expr::EnvLookup { key }` at parse time so
    /// the evaluator never has to special-case it in the dispatch table.
    fn parse_fn_call(&mut self, name: String, fn_span: Span) -> Result<Expr> {
        self.consume(ExprToken::LParen, "(")?;
        let args = self.parse_arg_list()?;
        let close_span = self.consume(ExprToken::RParen, ")")?;
        let call_span = fn_span.merge(close_span);

        if name == "ENV" {
            return match args.as_slice() {
                [Expr::Literal(Value::Str(key), _)] => {
                    Ok(Expr::EnvLookup { key: key.clone(), span: call_span })
                }
                _ => Err(EnvxError::ArityError {
                    func: "ENV".into(),
                    expected: "exactly 1 string literal".into(),
                    got: args.len(),
                }),
            };
        }

        Ok(Expr::Call(FnCall { name, args, span: call_span }))
    }

    /// Parse `if <cond> then <then_val> else <else_val>`.
    fn parse_if_expr(&mut self, if_span: Span) -> Result<Expr> {
        let cond = Box::new(self.parse_pipe_chain()?);
        self.consume(ExprToken::Then, "then")?;
        let then_val = Box::new(self.parse_pipe_chain()?);
        self.consume(ExprToken::Else, "else")?;
        let else_val = Box::new(self.parse_pipe_chain()?);
        let span = if_span.merge(else_val.span());
        Ok(Expr::IfExpr { cond, then_val, else_val, span })
    }

    /// Parse a comma-separated argument list.
    /// Returns an empty `Vec` for `f()`.
    fn parse_arg_list(&mut self) -> Result<Vec<Expr>> {
        if let Some((ExprToken::RParen, _)) = self.peek() {
            return Ok(Vec::new());
        }
        let mut args = vec![self.parse_pipe_chain()?];
        while let Some((ExprToken::Comma, _)) = self.peek() {
            self.advance();
            args.push(self.parse_pipe_chain()?);
        }
        Ok(args)
    }

    /// Consume the next token, asserting it matches `expected` (by value).
    /// Returns the consumed token's `Span` for parent span construction.
    ///
    /// Only called with unit variants that carry no data (LParen, RParen,
    /// Comma, Then, Else) so the PartialEq comparison is unambiguous.
    fn consume(&mut self, expected: ExprToken, expected_str: &str) -> Result<Span> {
        match self.advance() {
            Some((tok, span)) if tok == expected => Ok(span),
            Some((other, span)) => Err(EnvxError::UnexpectedToken {
                expected: format!("`{}`", expected_str),
                found: format!("{:?}", other),
                src: self.named(),
                span: span.into(),
            }),
            None => Err(EnvxError::UnexpectedToken {
                expected: format!("`{}`", expected_str),
                found: "end of expression".into(),
                src: self.named(),
                span: self.eof_span().into(),
            }),
        }
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn path() -> PathBuf {
        PathBuf::from("/test/app.envx")
    }

    fn parse_ok(src: &str) -> EnvxFile {
        parse(src, "test.envx", path()).expect("parse failed")
    }

    fn first_entry(src: &str) -> (String, Template) {
        let file = parse_ok(src);
        if let Statement::Entry { key, template, .. } = file.statements.into_iter().next().unwrap() {
            (key, template)
        } else {
            panic!("expected Entry statement");
        }
    }

    // ── Basic assignments ─────────────────────────────────────────────────────

    #[test]
    fn literal_assignment() {
        let (key, tmpl) = first_entry("NAME = \"John Doe\"\n");
        assert_eq!(key, "NAME");
        assert_eq!(tmpl.segments.len(), 1);
        assert!(matches!(&tmpl.segments[0], Segment::Literal(s) if s == "John Doe"));
    }

    #[test]
    fn empty_string() {
        let (_, tmpl) = first_entry("X = \"\"\n");
        assert_eq!(tmpl.segments.len(), 1);
        assert!(matches!(&tmpl.segments[0], Segment::Literal(s) if s.is_empty()));
    }

    #[test]
    fn multiple_statements() {
        let file = parse_ok("A = \"1\"\nB = \"2\"\n");
        assert_eq!(file.statements.len(), 2);
    }

    #[test]
    fn comments_and_blank_lines_skipped() {
        let src = "# header comment\n\nA = \"1\"\n# trailing\n";
        let file = parse_ok(src);
        assert_eq!(file.statements.len(), 1);
    }

    // ── Template scanning ─────────────────────────────────────────────────────

    #[test]
    fn template_leading_expr() {
        // "${{ $USER }}@company.com" → [Expr(VarRef), Literal("@company.com")]
        let (_, tmpl) = first_entry("EMAIL = \"${{ $USER }}@company.com\"\n");
        assert_eq!(tmpl.segments.len(), 2);
        assert!(matches!(&tmpl.segments[0], Segment::Expr(Expr::VarRef { name, .. }) if name == "USER"));
        assert!(matches!(&tmpl.segments[1], Segment::Literal(s) if s == "@company.com"));
    }

    #[test]
    fn template_trailing_expr() {
        // "prefix${{ $X }}" → [Literal("prefix"), Expr(VarRef)]
        let (_, tmpl) = first_entry("Y = \"prefix${{ $X }}\"\n");
        assert_eq!(tmpl.segments.len(), 2);
        assert!(matches!(&tmpl.segments[0], Segment::Literal(s) if s == "prefix"));
        assert!(matches!(&tmpl.segments[1], Segment::Expr(Expr::VarRef { .. })));
    }

    #[test]
    fn template_multiple_exprs() {
        let (_, tmpl) = first_entry("Z = \"${{ $A }}-${{ $B }}\"\n");
        assert_eq!(tmpl.segments.len(), 3);
        assert!(matches!(&tmpl.segments[1], Segment::Literal(s) if s == "-"));
    }

    // ── Pipe chains ───────────────────────────────────────────────────────────

    #[test]
    fn pipe_single() {
        let (_, tmpl) = first_entry("X = \"${{ $NAME | trim }}\"\n");
        if let Segment::Expr(Expr::Pipe { func, .. }) = &tmpl.segments[0] {
            assert_eq!(func.name, "trim");
            assert!(func.args.is_empty());
        } else {
            panic!("expected Pipe");
        }
    }

    #[test]
    fn pipe_chain_left_associative() {
        // $NAME | trim | lower  →  Pipe(Pipe(VarRef, trim), lower)
        let (_, tmpl) = first_entry("X = \"${{ $NAME | trim | lower }}\"\n");
        if let Segment::Expr(Expr::Pipe { expr, func, .. }) = &tmpl.segments[0] {
            assert_eq!(func.name, "lower");
            assert!(matches!(expr.as_ref(), Expr::Pipe { func, .. } if func.name == "trim"));
        } else {
            panic!("expected nested Pipe");
        }
    }

    #[test]
    fn pipe_with_args() {
        // $NAME | replace(' ', '_')
        let (_, tmpl) =
            first_entry("USERNAME = \"${{ $NAME | replace(' ', '_') }}\"\n");
        if let Segment::Expr(Expr::Pipe { func, .. }) = &tmpl.segments[0] {
            assert_eq!(func.name, "replace");
            assert_eq!(func.args.len(), 2);
            assert!(matches!(&func.args[0], Expr::Literal(Value::Str(s), _) if s == " "));
            assert!(matches!(&func.args[1], Expr::Literal(Value::Str(s), _) if s == "_"));
        } else {
            panic!("expected replace pipe");
        }
    }

    // ── Direct function calls ─────────────────────────────────────────────────

    #[test]
    fn direct_fn_call_with_args() {
        let (_, tmpl) = first_entry("X = \"${{ concat($A, '-', $B) }}\"\n");
        if let Segment::Expr(Expr::Call(fn_call)) = &tmpl.segments[0] {
            assert_eq!(fn_call.name, "concat");
            assert_eq!(fn_call.args.len(), 3);
        } else {
            panic!("expected Call");
        }
    }

    // ── ENV lookup ────────────────────────────────────────────────────────────

    #[test]
    fn env_lookup_rewrite() {
        // ENV('PORT') → EnvLookup, not Call
        let (_, tmpl) = first_entry("PORT = \"${{ ENV('PORT') }}\"\n");
        assert!(
            matches!(&tmpl.segments[0], Segment::Expr(Expr::EnvLookup { key, .. }) if key == "PORT")
        );
    }

    #[test]
    fn env_with_default_pipe() {
        // ENV('PORT') | default(8080)
        let (_, tmpl) = first_entry("PORT = \"${{ ENV('PORT') | default(8080) }}\"\n");
        if let Segment::Expr(Expr::Pipe { expr, func, .. }) = &tmpl.segments[0] {
            assert!(matches!(expr.as_ref(), Expr::EnvLookup { key, .. } if key == "PORT"));
            assert_eq!(func.name, "default");
            assert!(matches!(&func.args[0], Expr::Literal(Value::Int(8080), _)));
        } else {
            panic!("expected Pipe(EnvLookup, default)");
        }
    }

    // ── If/then/else ──────────────────────────────────────────────────────────

    #[test]
    fn if_then_else_basic() {
        let src =
            "DB = \"${{ if eq($APP_ENV, 'prod') then 'api.db.com' else 'localhost' }}\"\n";
        let (_, tmpl) = first_entry(src);
        if let Segment::Expr(Expr::IfExpr { cond, then_val, else_val, .. }) = &tmpl.segments[0] {
            assert!(matches!(cond.as_ref(), Expr::Call(f) if f.name == "eq"));
            assert!(
                matches!(then_val.as_ref(), Expr::Literal(Value::Str(s), _) if s == "api.db.com")
            );
            assert!(
                matches!(else_val.as_ref(), Expr::Literal(Value::Str(s), _) if s == "localhost")
            );
        } else {
            panic!("expected IfExpr");
        }
    }

    #[test]
    fn if_then_else_with_pipe_branches() {
        // `if true then $A | trim else $B | lower` — pipes bind tighter than then/else
        let src = "X = \"${{ if true then $A | trim else $B }}\"\n";
        let (_, tmpl) = first_entry(src);
        if let Segment::Expr(Expr::IfExpr { then_val, .. }) = &tmpl.segments[0] {
            assert!(matches!(then_val.as_ref(), Expr::Pipe { func, .. } if func.name == "trim"));
        } else {
            panic!("expected IfExpr");
        }
    }

    // ── Boolean and integer literals ──────────────────────────────────────────

    #[test]
    fn bool_literal() {
        let (_, tmpl) = first_entry("X = \"${{ true }}\"\n");
        assert!(
            matches!(&tmpl.segments[0], Segment::Expr(Expr::Literal(Value::Bool(true), _)))
        );
    }

    #[test]
    fn int_literal() {
        let (_, tmpl) = first_entry("X = \"${{ 42 }}\"\n");
        assert!(
            matches!(&tmpl.segments[0], Segment::Expr(Expr::Literal(Value::Int(42), _)))
        );
    }

    // ── Import directive ──────────────────────────────────────────────────────

    #[test]
    fn import_directive() {
        let src = "@import \"./base.envx\"\n";
        let file = parse_ok(src);
        assert_eq!(file.statements.len(), 1);
        if let Statement::Import { raw_path, resolved, .. } = &file.statements[0] {
            assert_eq!(raw_path, "./base.envx");
            assert!(resolved.ends_with("base.envx"));
        } else {
            panic!("expected Import");
        }
    }

    // ── Bare (unquoted) values ────────────────────────────────────────────────

    #[test]
    fn bare_word_value() {
        let (key, tmpl) = first_entry("ENV = development\n");
        assert_eq!(key, "ENV");
        assert!(matches!(&tmpl.segments[0], Segment::Literal(s) if s == "development"));
    }

    #[test]
    fn bare_numeric_value() {
        let (_, tmpl) = first_entry("PORT = 8080\n");
        assert!(matches!(&tmpl.segments[0], Segment::Literal(s) if s == "8080"));
    }

    #[test]
    fn bare_value_with_special_chars() {
        let (_, tmpl) = first_entry("HOST = localhost:8080\n");
        assert!(matches!(&tmpl.segments[0], Segment::Literal(s) if s == "localhost:8080"));
    }

    #[test]
    fn bare_multiword_value() {
        let (_, tmpl) = first_entry("GREETING = hello world\n");
        assert!(matches!(&tmpl.segments[0], Segment::Literal(s) if s == "hello world"));
    }

    #[test]
    fn bare_value_stops_at_comment() {
        let (_, tmpl) = first_entry("ENV = production # this is prod\n");
        assert!(matches!(&tmpl.segments[0], Segment::Literal(s) if s == "production"));
    }

    // ── Error cases ───────────────────────────────────────────────────────────

    #[test]
    fn error_unclosed_expression() {
        let src = "X = \"${{ $NAME\"\n";
        assert!(parse(src, "test.envx", path()).is_err());
    }

    #[test]
    fn error_empty_expression() {
        let src = "X = \"${{   }}\"\n";
        assert!(parse(src, "test.envx", path()).is_err());
    }

    #[test]
    fn error_missing_eq() {
        let src = "X \"value\"\n";
        assert!(parse(src, "test.envx", path()).is_err());
    }

    #[test]
    fn error_missing_pipe_rhs() {
        let src = "X = \"${{ $NAME | }}\"\n";
        assert!(parse(src, "test.envx", path()).is_err());
    }

    #[test]
    fn error_env_wrong_arg_type() {
        // ENV must take a string literal, not a variable reference
        let src = "X = \"${{ ENV($KEY) }}\"\n";
        assert!(parse(src, "test.envx", path()).is_err());
    }

    // ── Full .envx example (integration) ─────────────────────────────────────

    #[test]
    fn full_example_file() {
        let src = r#"
NAME = "John Doe"
USERNAME = "${{ $NAME | trim | replace(' ', '_') | lower }}"
EMAIL = "${{ $USERNAME }}@company.com"
PORT = "${{ ENV('PORT') | default(8080) }}"
APP_ENV = "prod"
DB_HOST = "${{ if eq($APP_ENV, 'prod') then 'api.db.com' else 'localhost' }}"
"#;
        let file = parse_ok(src);
        assert_eq!(file.statements.len(), 6);
        // All are Entry statements
        assert!(file.statements.iter().all(|s| matches!(s, Statement::Entry { .. })));
    }
}
