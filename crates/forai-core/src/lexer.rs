use crate::ast::Span;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InterpPart {
    Lit(String),
    Expr(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TokenKind {
    Ident(String),
    Number(String),
    StringLit(String),
    StringInterp(Vec<InterpPart>),
    RegexLit(String),
    Symbol(char),
    FatArrow,
    EqEq,     // ==
    BangEq,   // !=
    GtEq,     // >=
    LtEq,     // <=
    AmpAmp,   // &&
    PipePipe, // ||
    StarStar,   // **
    DotDot,     // ..
    PlusEq,           // +=
    MinusEq,          // -=
    StarEq,           // *=
    SlashEq,          // /=
    PercentEq,        // %=
    QuestionQuestion, // ??
    Newline,
    Eof,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Token {
    pub kind: TokenKind,
    pub span: Span,
    pub start: usize,
    pub end: usize,
}

#[derive(Debug, Clone)]
pub struct LexError {
    pub message: String,
    pub span: Span,
}

impl std::fmt::Display for LexError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}:{} {}", self.span.line, self.span.col, self.message)
    }
}

impl std::error::Error for LexError {}

/// Strip up to `indent` leading spaces from each line, and drop trailing whitespace-only line.
fn dedent_string(s: &str, indent: usize) -> String {
    let mut lines: Vec<&str> = s.split('\n').collect();
    // Drop last line if whitespace-only (the line before closing """)
    if let Some(last) = lines.last() {
        if last.chars().all(|c| c == ' ' || c == '\t') {
            lines.pop();
        }
    }
    let result: Vec<String> = lines
        .iter()
        .map(|line| {
            let stripped = line.as_bytes();
            let mut skip = 0;
            while skip < indent && skip < stripped.len() && stripped[skip] == b' ' {
                skip += 1;
            }
            line[skip..].to_string()
        })
        .collect();
    result.join("\n")
}

/// Apply dedent to InterpPart::Lit segments: strip `indent` spaces after each \n in literal parts.
fn dedent_interp_parts(parts: Vec<InterpPart>, indent: usize) -> Vec<InterpPart> {
    // Concatenate all parts into a single string with sentinel markers for expressions
    // Then dedent and split back. Simpler: just process Lit parts directly.
    let mut result = Vec::new();
    for (idx, part) in parts.into_iter().enumerate() {
        match part {
            InterpPart::Lit(s) => {
                if idx == 0 {
                    // First lit part: dedent fully (including first line)
                    result.push(InterpPart::Lit(dedent_lit_segment(&s, indent, true)));
                } else {
                    // Subsequent lit parts: only dedent after newlines
                    result.push(InterpPart::Lit(dedent_lit_segment(&s, indent, false)));
                }
            }
            InterpPart::Expr(e) => result.push(InterpPart::Expr(e)),
        }
    }
    // Drop trailing whitespace-only line from last Lit part (the line before closing """)
    if let Some(InterpPart::Lit(s)) = result.last() {
        // Find the last \n — if everything after it is whitespace, strip from that \n onward
        if let Some(last_nl) = s.rfind('\n') {
            let after = &s[last_nl + 1..];
            if after.chars().all(|c| c == ' ' || c == '\t') {
                let trimmed = s[..last_nl].to_string();
                let len = result.len();
                if trimmed.is_empty() {
                    result.pop();
                } else {
                    result[len - 1] = InterpPart::Lit(trimmed);
                }
            }
        } else if s.chars().all(|c| c == ' ' || c == '\t') {
            result.pop();
        }
    }
    result
}

/// Dedent a literal segment: strip `indent` spaces after each \n, and optionally the first line.
fn dedent_lit_segment(s: &str, indent: usize, first_line: bool) -> String {
    let mut result = String::with_capacity(s.len());
    let mut at_line_start = first_line;
    let mut spaces_stripped = 0;
    for ch in s.chars() {
        if at_line_start && ch == ' ' && spaces_stripped < indent {
            spaces_stripped += 1;
            continue;
        }
        if ch == '\n' {
            result.push('\n');
            at_line_start = true;
            spaces_stripped = 0;
            continue;
        }
        at_line_start = false;
        spaces_stripped = 0;
        result.push(ch);
    }
    result
}

pub fn lex(source: &str) -> Result<Vec<Token>, LexError> {
    let bytes = source.as_bytes();
    let mut tokens = Vec::new();
    let mut i = 0usize;
    let mut line = 1usize;
    let mut col = 1usize;

    while i < bytes.len() {
        let b = bytes[i];
        let ch = b as char;

        if ch == '\r' {
            i += 1;
            continue;
        }

        if ch == '\n' {
            tokens.push(Token {
                kind: TokenKind::Newline,
                span: Span { line, col },
                start: i,
                end: i + 1,
            });
            i += 1;
            line += 1;
            col = 1;
            continue;
        }

        if ch == ' ' || ch == '\t' {
            i += 1;
            col += 1;
            continue;
        }

        if ch == '#' {
            while i < bytes.len() {
                let c = bytes[i] as char;
                if c == '\n' {
                    break;
                }
                i += 1;
                col += 1;
            }
            continue;
        }

        if ch == '"' {
            // Check for """ (triple-quote multi-line string)
            if i + 2 < bytes.len() && bytes[i + 1] == b'"' && bytes[i + 2] == b'"' {
                let start = i;
                let start_col = col;
                let start_line = line;
                i += 3; // skip opening """
                col += 3;

                // If next char is \n, skip it (first-line-blank rule)
                if i < bytes.len() && bytes[i] == b'\n' {
                    i += 1;
                    line += 1;
                    col = 1;
                }

                let mut parts: Vec<InterpPart> = Vec::new();
                let mut current = String::new();
                let mut escaped = false;
                let mut has_interp = false;
                let mut found_close = false;

                while i < bytes.len() {
                    let c = bytes[i] as char;

                    if escaped {
                        let mapped = match c {
                            'n' => '\n',
                            't' => '\t',
                            '\\' => '\\',
                            '"' => '"',
                            '#' => '#',
                            other => other,
                        };
                        current.push(mapped);
                        escaped = false;
                        i += 1;
                        col += 1;
                        continue;
                    }

                    if c == '\\' {
                        escaped = true;
                        i += 1;
                        col += 1;
                        continue;
                    }

                    // Check for closing """
                    if c == '"'
                        && i + 2 < bytes.len()
                        && bytes[i + 1] == b'"'
                        && bytes[i + 2] == b'"'
                    {
                        // Compute closing indent: col is 1-based position of first " of closing """
                        let closing_indent = col - 1; // convert to 0-based space count

                        i += 3; // skip closing """
                        col += 3;
                        found_close = true;

                        // Apply dedent to accumulated literal and parts
                        if has_interp {
                            // Push any remaining literal
                            if !current.is_empty() {
                                parts.push(InterpPart::Lit(std::mem::take(&mut current)));
                            }
                            // Apply dedent to Lit parts: strip closing_indent spaces after each \n
                            parts = dedent_interp_parts(parts, closing_indent);
                        } else {
                            // Simple string: dedent the whole thing
                            current = dedent_string(&current, closing_indent);
                        }
                        break;
                    }

                    // Handle interpolation #{expr}
                    if c == '#' && i + 1 < bytes.len() && bytes[i + 1] as char == '{' {
                        has_interp = true;
                        if !current.is_empty() {
                            parts.push(InterpPart::Lit(std::mem::take(&mut current)));
                        }
                        i += 2; // skip '#{'
                        col += 2;
                        let expr_start = i;
                        let mut depth = 1;
                        while i < bytes.len() && depth > 0 {
                            let ec = bytes[i] as char;
                            if ec == '\n' {
                                return Err(LexError {
                                    message: "unterminated interpolation in string".to_string(),
                                    span: Span { line, col },
                                });
                            }
                            if ec == '{' {
                                depth += 1;
                            }
                            if ec == '}' {
                                depth -= 1;
                            }
                            if depth > 0 {
                                i += 1;
                                col += 1;
                            }
                        }
                        let expr_text = source[expr_start..i].trim().to_string();
                        if expr_text.is_empty() {
                            return Err(LexError {
                                message: "empty interpolation expression".to_string(),
                                span: Span { line, col },
                            });
                        }
                        parts.push(InterpPart::Expr(expr_text));
                        i += 1; // skip '}'
                        col += 1;
                        continue;
                    }

                    if c == '\n' {
                        current.push('\n');
                        i += 1;
                        line += 1;
                        col = 1;
                        continue;
                    }

                    current.push(c);
                    i += 1;
                    col += 1;
                }

                if !found_close {
                    return Err(LexError {
                        message: "unterminated triple-quoted string".to_string(),
                        span: Span {
                            line: start_line,
                            col: start_col,
                        },
                    });
                }

                if has_interp {
                    if !current.is_empty() {
                        parts.push(InterpPart::Lit(current));
                    }
                    tokens.push(Token {
                        kind: TokenKind::StringInterp(parts),
                        span: Span {
                            line: start_line,
                            col: start_col,
                        },
                        start,
                        end: i,
                    });
                } else {
                    tokens.push(Token {
                        kind: TokenKind::StringLit(current),
                        span: Span {
                            line: start_line,
                            col: start_col,
                        },
                        start,
                        end: i,
                    });
                }
                continue;
            }

            let start = i;
            let start_col = col;
            i += 1;
            col += 1;
            let mut parts: Vec<InterpPart> = Vec::new();
            let mut current = String::new();
            let mut escaped = false;
            let mut has_interp = false;

            while i < bytes.len() {
                let c = bytes[i] as char;
                if c == '\n' {
                    return Err(LexError {
                        message: "unterminated string literal".to_string(),
                        span: Span {
                            line,
                            col: start_col,
                        },
                    });
                }

                if escaped {
                    let mapped = match c {
                        'n' => '\n',
                        't' => '\t',
                        '\\' => '\\',
                        '"' => '"',
                        '#' => '#',
                        other => other,
                    };
                    current.push(mapped);
                    escaped = false;
                    i += 1;
                    col += 1;
                    continue;
                }

                if c == '\\' {
                    escaped = true;
                    i += 1;
                    col += 1;
                    continue;
                }

                if c == '"' {
                    i += 1;
                    col += 1;
                    break;
                }

                if c == '#' && i + 1 < bytes.len() && bytes[i + 1] as char == '{' {
                    has_interp = true;
                    // Push accumulated literal
                    if !current.is_empty() {
                        parts.push(InterpPart::Lit(std::mem::take(&mut current)));
                    }
                    i += 2; // skip '#{'
                    col += 2;
                    // Scan for matching '}'
                    let expr_start = i;
                    let mut depth = 1;
                    while i < bytes.len() && depth > 0 {
                        let ec = bytes[i] as char;
                        if ec == '\n' {
                            return Err(LexError {
                                message: "unterminated interpolation in string".to_string(),
                                span: Span { line, col },
                            });
                        }
                        if ec == '{' {
                            depth += 1;
                        }
                        if ec == '}' {
                            depth -= 1;
                        }
                        if depth > 0 {
                            i += 1;
                            col += 1;
                        }
                    }
                    let expr_text = source[expr_start..i].trim().to_string();
                    if expr_text.is_empty() {
                        return Err(LexError {
                            message: "empty interpolation expression".to_string(),
                            span: Span { line, col },
                        });
                    }
                    parts.push(InterpPart::Expr(expr_text));
                    i += 1; // skip '}'
                    col += 1;
                    continue;
                }

                current.push(c);
                i += 1;
                col += 1;
            }

            if has_interp {
                if !current.is_empty() {
                    parts.push(InterpPart::Lit(current));
                }
                tokens.push(Token {
                    kind: TokenKind::StringInterp(parts),
                    span: Span {
                        line,
                        col: start_col,
                    },
                    start,
                    end: i,
                });
            } else {
                tokens.push(Token {
                    kind: TokenKind::StringLit(current),
                    span: Span {
                        line,
                        col: start_col,
                    },
                    start,
                    end: i,
                });
            }
            continue;
        }

        if ch.is_ascii_digit() {
            let start = i;
            let start_col = col;
            i += 1;
            col += 1;
            while i < bytes.len() {
                let c = bytes[i] as char;
                if c.is_ascii_digit() || (c == '.' && i + 1 < bytes.len() && bytes[i + 1] != b'.') {
                    i += 1;
                    col += 1;
                } else {
                    break;
                }
            }
            tokens.push(Token {
                kind: TokenKind::Number(source[start..i].to_string()),
                span: Span {
                    line,
                    col: start_col,
                },
                start,
                end: i,
            });
            continue;
        }

        if ch.is_ascii_alphabetic() || ch == '_' {
            let start = i;
            let start_col = col;
            i += 1;
            col += 1;
            while i < bytes.len() {
                let c = bytes[i] as char;
                if c.is_ascii_alphanumeric() || c == '_' {
                    i += 1;
                    col += 1;
                } else {
                    break;
                }
            }
            tokens.push(Token {
                kind: TokenKind::Ident(source[start..i].to_string()),
                span: Span {
                    line,
                    col: start_col,
                },
                start,
                end: i,
            });
            continue;
        }

        if ch == '=' {
            let start = i;
            let start_col = col;
            if i + 1 < bytes.len() {
                let next = bytes[i + 1] as char;
                if next == '>' {
                    i += 2;
                    col += 2;
                    tokens.push(Token {
                        kind: TokenKind::FatArrow,
                        span: Span {
                            line,
                            col: start_col,
                        },
                        start,
                        end: i,
                    });
                    continue;
                }
                if next == '=' {
                    i += 2;
                    col += 2;
                    tokens.push(Token {
                        kind: TokenKind::EqEq,
                        span: Span {
                            line,
                            col: start_col,
                        },
                        start,
                        end: i,
                    });
                    continue;
                }
            }
            i += 1;
            col += 1;
            tokens.push(Token {
                kind: TokenKind::Symbol('='),
                span: Span {
                    line,
                    col: start_col,
                },
                start,
                end: i,
            });
            continue;
        }

        if ch == '!' {
            let start = i;
            let start_col = col;
            if i + 1 < bytes.len() && bytes[i + 1] as char == '=' {
                i += 2;
                col += 2;
                tokens.push(Token {
                    kind: TokenKind::BangEq,
                    span: Span {
                        line,
                        col: start_col,
                    },
                    start,
                    end: i,
                });
                continue;
            }
            i += 1;
            col += 1;
            tokens.push(Token {
                kind: TokenKind::Symbol('!'),
                span: Span {
                    line,
                    col: start_col,
                },
                start,
                end: i,
            });
            continue;
        }

        if ch == '<' {
            let start = i;
            let start_col = col;
            if i + 1 < bytes.len() && bytes[i + 1] as char == '=' {
                i += 2;
                col += 2;
                tokens.push(Token {
                    kind: TokenKind::LtEq,
                    span: Span {
                        line,
                        col: start_col,
                    },
                    start,
                    end: i,
                });
                continue;
            }
            i += 1;
            col += 1;
            tokens.push(Token {
                kind: TokenKind::Symbol('<'),
                span: Span {
                    line,
                    col: start_col,
                },
                start,
                end: i,
            });
            continue;
        }

        if ch == '>' {
            let start = i;
            let start_col = col;
            if i + 1 < bytes.len() && bytes[i + 1] as char == '=' {
                i += 2;
                col += 2;
                tokens.push(Token {
                    kind: TokenKind::GtEq,
                    span: Span {
                        line,
                        col: start_col,
                    },
                    start,
                    end: i,
                });
                continue;
            }
            i += 1;
            col += 1;
            tokens.push(Token {
                kind: TokenKind::Symbol('>'),
                span: Span {
                    line,
                    col: start_col,
                },
                start,
                end: i,
            });
            continue;
        }

        if ch == '&' {
            let start = i;
            let start_col = col;
            if i + 1 < bytes.len() && bytes[i + 1] as char == '&' {
                i += 2;
                col += 2;
                tokens.push(Token {
                    kind: TokenKind::AmpAmp,
                    span: Span {
                        line,
                        col: start_col,
                    },
                    start,
                    end: i,
                });
                continue;
            }
            i += 1;
            col += 1;
            tokens.push(Token {
                kind: TokenKind::Symbol('&'),
                span: Span {
                    line,
                    col: start_col,
                },
                start,
                end: i,
            });
            continue;
        }

        if ch == '|' {
            let start = i;
            let start_col = col;
            if i + 1 < bytes.len() && bytes[i + 1] as char == '|' {
                i += 2;
                col += 2;
                tokens.push(Token {
                    kind: TokenKind::PipePipe,
                    span: Span {
                        line,
                        col: start_col,
                    },
                    start,
                    end: i,
                });
                continue;
            }
            i += 1;
            col += 1;
            tokens.push(Token {
                kind: TokenKind::Symbol('|'),
                span: Span {
                    line,
                    col: start_col,
                },
                start,
                end: i,
            });
            continue;
        }

        if ch == '.' {
            let start = i;
            let start_col = col;
            if i + 1 < bytes.len() && bytes[i + 1] == b'.' {
                i += 2;
                col += 2;
                tokens.push(Token {
                    kind: TokenKind::DotDot,
                    span: Span {
                        line,
                        col: start_col,
                    },
                    start,
                    end: i,
                });
                continue;
            }
            i += 1;
            col += 1;
            tokens.push(Token {
                kind: TokenKind::Symbol('.'),
                span: Span {
                    line,
                    col: start_col,
                },
                start,
                end: i,
            });
            continue;
        }

        if matches!(ch, ':' | ',' | '(' | ')' | '[' | ']') {
            let start = i;
            let start_col = col;
            i += 1;
            col += 1;
            tokens.push(Token {
                kind: TokenKind::Symbol(ch),
                span: Span {
                    line,
                    col: start_col,
                },
                start,
                end: i,
            });
            continue;
        }

        if ch == '*' {
            let start = i;
            let start_col = col;
            if i + 1 < bytes.len() && bytes[i + 1] as char == '=' {
                i += 2;
                col += 2;
                tokens.push(Token {
                    kind: TokenKind::StarEq,
                    span: Span {
                        line,
                        col: start_col,
                    },
                    start,
                    end: i,
                });
                continue;
            }
            if i + 1 < bytes.len() && bytes[i + 1] as char == '*' {
                i += 2;
                col += 2;
                tokens.push(Token {
                    kind: TokenKind::StarStar,
                    span: Span {
                        line,
                        col: start_col,
                    },
                    start,
                    end: i,
                });
                continue;
            }
            i += 1;
            col += 1;
            tokens.push(Token {
                kind: TokenKind::Symbol('*'),
                span: Span {
                    line,
                    col: start_col,
                },
                start,
                end: i,
            });
            continue;
        }

        if ch == '/' {
            if i + 1 < bytes.len() && bytes[i + 1] as char == '=' {
                let start = i;
                let start_col = col;
                i += 2;
                col += 2;
                tokens.push(Token {
                    kind: TokenKind::SlashEq,
                    span: Span {
                        line,
                        col: start_col,
                    },
                    start,
                    end: i,
                });
                continue;
            }
            if i + 1 < bytes.len() {
                let next = bytes[i + 1] as char;
                if !next.is_ascii_whitespace() && next != '\n' {
                    let start = i;
                    let start_col = col;
                    i += 1;
                    col += 1;
                    let mut pattern = String::new();
                    let mut escaped = false;
                    let mut closed = false;
                    while i < bytes.len() {
                        let c = bytes[i] as char;
                        if c == '\n' {
                            break;
                        }
                        if escaped {
                            pattern.push(c);
                            escaped = false;
                            i += 1;
                            col += 1;
                            continue;
                        }
                        if c == '\\' {
                            escaped = true;
                            pattern.push(c);
                            i += 1;
                            col += 1;
                            continue;
                        }
                        if c == '/' {
                            i += 1;
                            col += 1;
                            closed = true;
                            break;
                        }
                        pattern.push(c);
                        i += 1;
                        col += 1;
                    }
                    if !closed {
                        return Err(LexError {
                            message: "unterminated regex literal".to_string(),
                            span: Span {
                                line,
                                col: start_col,
                            },
                        });
                    }
                    tokens.push(Token {
                        kind: TokenKind::RegexLit(pattern),
                        span: Span {
                            line,
                            col: start_col,
                        },
                        start,
                        end: i,
                    });
                    continue;
                }
            }
            let start = i;
            let start_col = col;
            i += 1;
            col += 1;
            tokens.push(Token {
                kind: TokenKind::Symbol('/'),
                span: Span {
                    line,
                    col: start_col,
                },
                start,
                end: i,
            });
            continue;
        }

        if ch == '+' {
            let start = i;
            let start_col = col;
            if i + 1 < bytes.len() && bytes[i + 1] as char == '=' {
                i += 2;
                col += 2;
                tokens.push(Token {
                    kind: TokenKind::PlusEq,
                    span: Span {
                        line,
                        col: start_col,
                    },
                    start,
                    end: i,
                });
                continue;
            }
            i += 1;
            col += 1;
            tokens.push(Token {
                kind: TokenKind::Symbol('+'),
                span: Span {
                    line,
                    col: start_col,
                },
                start,
                end: i,
            });
            continue;
        }

        if ch == '-' {
            let start = i;
            let start_col = col;
            if i + 1 < bytes.len() && bytes[i + 1] as char == '=' {
                i += 2;
                col += 2;
                tokens.push(Token {
                    kind: TokenKind::MinusEq,
                    span: Span {
                        line,
                        col: start_col,
                    },
                    start,
                    end: i,
                });
                continue;
            }
            i += 1;
            col += 1;
            tokens.push(Token {
                kind: TokenKind::Symbol('-'),
                span: Span {
                    line,
                    col: start_col,
                },
                start,
                end: i,
            });
            continue;
        }

        if ch == '%' {
            let start = i;
            let start_col = col;
            if i + 1 < bytes.len() && bytes[i + 1] as char == '=' {
                i += 2;
                col += 2;
                tokens.push(Token {
                    kind: TokenKind::PercentEq,
                    span: Span {
                        line,
                        col: start_col,
                    },
                    start,
                    end: i,
                });
                continue;
            }
            i += 1;
            col += 1;
            tokens.push(Token {
                kind: TokenKind::Symbol('%'),
                span: Span {
                    line,
                    col: start_col,
                },
                start,
                end: i,
            });
            continue;
        }

        if ch == '?' {
            let start = i;
            let start_col = col;
            if i + 1 < bytes.len() && bytes[i + 1] as char == '?' {
                i += 2;
                col += 2;
                tokens.push(Token {
                    kind: TokenKind::QuestionQuestion,
                    span: Span {
                        line,
                        col: start_col,
                    },
                    start,
                    end: i,
                });
                continue;
            }
            i += 1;
            col += 1;
            tokens.push(Token {
                kind: TokenKind::Symbol('?'),
                span: Span {
                    line,
                    col: start_col,
                },
                start,
                end: i,
            });
            continue;
        }

        if ch.is_ascii_punctuation() {
            let start = i;
            let start_col = col;
            i += 1;
            col += 1;
            tokens.push(Token {
                kind: TokenKind::Symbol(ch),
                span: Span {
                    line,
                    col: start_col,
                },
                start,
                end: i,
            });
            continue;
        }

        return Err(LexError {
            message: format!("unexpected character `{ch}`"),
            span: Span { line, col },
        });
    }

    tokens.push(Token {
        kind: TokenKind::Eof,
        span: Span { line, col },
        start: source.len(),
        end: source.len(),
    });

    Ok(tokens)
}

#[cfg(test)]
mod tests {
    use super::{InterpPart, TokenKind, lex};

    #[test]
    fn lexes_regex_literal() {
        let tokens = lex("/@/").expect("should lex");
        assert_eq!(tokens.len(), 2); // RegexLit + Eof
        assert_eq!(tokens[0].kind, TokenKind::RegexLit("@".to_string()));
    }

    #[test]
    fn lexes_regex_with_escape() {
        let tokens = lex(r"/\d+/").expect("should lex");
        assert_eq!(tokens[0].kind, TokenKind::RegexLit(r"\d+".to_string()));
    }

    #[test]
    fn lexes_regex_in_context() {
        let tokens = lex(":matches => /@/\n").expect("should lex");
        let kinds: Vec<_> = tokens.iter().map(|t| &t.kind).collect();
        assert!(matches!(kinds[0], TokenKind::Symbol(':')));
        assert!(matches!(kinds[1], TokenKind::Ident(s) if s == "matches"));
        assert!(matches!(kinds[2], TokenKind::FatArrow));
        assert_eq!(*kinds[3], TokenKind::RegexLit("@".to_string()));
    }

    #[test]
    fn lexes_multi_char_operators() {
        let tokens = lex("== != >= <= && || **").expect("should lex");
        let kinds: Vec<_> = tokens.iter().map(|t| &t.kind).collect();
        assert_eq!(*kinds[0], TokenKind::EqEq);
        assert_eq!(*kinds[1], TokenKind::BangEq);
        assert_eq!(*kinds[2], TokenKind::GtEq);
        assert_eq!(*kinds[3], TokenKind::LtEq);
        assert_eq!(*kinds[4], TokenKind::AmpAmp);
        assert_eq!(*kinds[5], TokenKind::PipePipe);
        assert_eq!(*kinds[6], TokenKind::StarStar);
    }

    #[test]
    fn lexes_arithmetic_symbols() {
        let tokens = lex("+ - * / %").expect("should lex");
        let kinds: Vec<_> = tokens.iter().map(|t| &t.kind).collect();
        assert_eq!(*kinds[0], TokenKind::Symbol('+'));
        assert_eq!(*kinds[1], TokenKind::Symbol('-'));
        assert_eq!(*kinds[2], TokenKind::Symbol('*'));
        assert_eq!(*kinds[3], TokenKind::Symbol('/'));
        assert_eq!(*kinds[4], TokenKind::Symbol('%'));
    }

    #[test]
    fn lexes_eq_vs_fat_arrow_vs_eqeq() {
        let tokens = lex("= => ==").expect("should lex");
        let kinds: Vec<_> = tokens.iter().map(|t| &t.kind).collect();
        assert_eq!(*kinds[0], TokenKind::Symbol('='));
        assert_eq!(*kinds[1], TokenKind::FatArrow);
        assert_eq!(*kinds[2], TokenKind::EqEq);
    }

    #[test]
    fn lexes_plain_string_unchanged() {
        let tokens = lex(r#""hello world""#).expect("should lex");
        assert_eq!(
            tokens[0].kind,
            TokenKind::StringLit("hello world".to_string())
        );
    }

    #[test]
    fn lexes_string_interpolation() {
        let tokens = lex(r##""hello #{name}!""##).expect("should lex");
        assert!(matches!(&tokens[0].kind, TokenKind::StringInterp(parts) if parts.len() == 3));
        if let TokenKind::StringInterp(parts) = &tokens[0].kind {
            assert_eq!(parts[0], InterpPart::Lit("hello ".to_string()));
            assert_eq!(parts[1], InterpPart::Expr("name".to_string()));
            assert_eq!(parts[2], InterpPart::Lit("!".to_string()));
        }
    }

    #[test]
    fn lexes_escaped_hash() {
        let tokens = lex(r##""hello \#{name}""##).expect("should lex");
        // \# produces literal # — no interpolation
        assert_eq!(
            tokens[0].kind,
            TokenKind::StringLit(r##"hello #{name}"##.to_string())
        );
    }

    #[test]
    fn lexes_bare_braces_as_literal() {
        let tokens = lex(r#""regex {4} test""#).expect("should lex");
        assert_eq!(
            tokens[0].kind,
            TokenKind::StringLit("regex {4} test".to_string())
        );
    }

    #[test]
    fn lexes_multiple_interps() {
        let tokens = lex(r##""#{a} and #{b}""##).expect("should lex");
        if let TokenKind::StringInterp(parts) = &tokens[0].kind {
            assert_eq!(parts.len(), 3);
            assert_eq!(parts[0], InterpPart::Expr("a".to_string()));
            assert_eq!(parts[1], InterpPart::Lit(" and ".to_string()));
            assert_eq!(parts[2], InterpPart::Expr("b".to_string()));
        } else {
            panic!("expected StringInterp");
        }
    }

    #[test]
    fn rejects_empty_interpolation() {
        let result = lex(r##""hello #{}""##);
        assert!(result.is_err());
    }

    // === Triple-quoted string tests ===

    #[test]
    fn lexes_triple_quote_basic() {
        let src = "x = \"\"\"\nhello\nworld\n\"\"\"";
        let tokens = lex(src).expect("should lex");
        // Ident(x), Symbol(=), StringLit, Eof
        assert_eq!(tokens.len(), 4);
        assert_eq!(
            tokens[2].kind,
            TokenKind::StringLit("hello\nworld".to_string())
        );
    }

    #[test]
    fn lexes_triple_quote_with_dedent() {
        let src = "x = \"\"\"\n    hello\n    world\n    \"\"\"";
        let tokens = lex(src).expect("should lex");
        assert_eq!(
            tokens[2].kind,
            TokenKind::StringLit("hello\nworld".to_string())
        );
    }

    #[test]
    fn lexes_triple_quote_partial_dedent() {
        // closing """ at 2-space indent, content at 4-space → strips 2 spaces
        let src = "x = \"\"\"\n    hello\n      deeper\n  \"\"\"";
        let tokens = lex(src).expect("should lex");
        assert_eq!(
            tokens[2].kind,
            TokenKind::StringLit("  hello\n    deeper".to_string())
        );
    }

    #[test]
    fn lexes_triple_quote_with_interpolation() {
        let src = "x = \"\"\"\n<h1>#{title}</h1>\n\"\"\"";
        let tokens = lex(src).expect("should lex");
        if let TokenKind::StringInterp(parts) = &tokens[2].kind {
            assert_eq!(parts.len(), 3);
            assert_eq!(parts[0], InterpPart::Lit("<h1>".to_string()));
            assert_eq!(parts[1], InterpPart::Expr("title".to_string()));
            assert_eq!(parts[2], InterpPart::Lit("</h1>".to_string()));
        } else {
            panic!("expected StringInterp, got {:?}", tokens[2].kind);
        }
    }

    #[test]
    fn lexes_triple_quote_with_dedented_interpolation() {
        let src = "x = \"\"\"\n    <h1>#{title}</h1>\n    \"\"\"";
        let tokens = lex(src).expect("should lex");
        if let TokenKind::StringInterp(parts) = &tokens[2].kind {
            assert_eq!(parts[0], InterpPart::Lit("<h1>".to_string()));
            assert_eq!(parts[1], InterpPart::Expr("title".to_string()));
            assert_eq!(parts[2], InterpPart::Lit("</h1>".to_string()));
        } else {
            panic!("expected StringInterp, got {:?}", tokens[2].kind);
        }
    }

    #[test]
    fn lexes_triple_quote_bare_quotes_inside() {
        let src = "x = \"\"\"\nsay \"hello\" and \"\"bye\"\"\n\"\"\"";
        let tokens = lex(src).expect("should lex");
        assert_eq!(
            tokens[2].kind,
            TokenKind::StringLit("say \"hello\" and \"\"bye\"\"".to_string())
        );
    }

    #[test]
    fn lexes_triple_quote_escape_sequences() {
        let src = "x = \"\"\"\nhello\\nworld\\t!\n\"\"\"";
        let tokens = lex(src).expect("should lex");
        assert_eq!(
            tokens[2].kind,
            TokenKind::StringLit("hello\nworld\t!".to_string())
        );
    }

    #[test]
    fn lexes_triple_quote_escaped_hash() {
        let src = "x = \"\"\"\n\\#{not_interp}\n\"\"\"";
        let tokens = lex(src).expect("should lex");
        assert_eq!(
            tokens[2].kind,
            TokenKind::StringLit("#{not_interp}".to_string())
        );
    }

    #[test]
    fn rejects_unterminated_triple_quote() {
        let src = "x = \"\"\"\nhello\nworld\n";
        let result = lex(src);
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .message
                .contains("unterminated triple-quoted")
        );
    }

    #[test]
    fn lexes_empty_triple_quote() {
        let src = "x = \"\"\"\n\"\"\"";
        let tokens = lex(src).expect("should lex");
        assert_eq!(tokens[2].kind, TokenKind::StringLit("".to_string()));
    }

    #[test]
    fn lexes_triple_quote_same_line_content() {
        // Content starts on same line as opening """
        let src = "x = \"\"\"hello\n\"\"\"";
        let tokens = lex(src).expect("should lex");
        assert_eq!(tokens[2].kind, TokenKind::StringLit("hello".to_string()));
    }

    #[test]
    fn lexes_dot_dot() {
        let tokens = lex("1..10").expect("should lex");
        let kinds: Vec<_> = tokens.iter().map(|t| &t.kind).collect();
        assert_eq!(*kinds[0], TokenKind::Number("1".to_string()));
        assert_eq!(*kinds[1], TokenKind::DotDot);
        assert_eq!(*kinds[2], TokenKind::Number("10".to_string()));
    }

    #[test]
    fn lexes_compound_assignment_operators() {
        let tokens = lex("x += 1").expect("should lex");
        let kinds: Vec<_> = tokens.iter().map(|t| &t.kind).collect();
        assert!(matches!(kinds[0], TokenKind::Ident(s) if s == "x"));
        assert_eq!(*kinds[1], TokenKind::PlusEq);
        assert!(matches!(kinds[2], TokenKind::Number(s) if s == "1"));

        let tokens = lex("x -= 2").expect("should lex");
        assert_eq!(tokens[1].kind, TokenKind::MinusEq);

        let tokens = lex("x *= 3").expect("should lex");
        assert_eq!(tokens[1].kind, TokenKind::StarEq);

        let tokens = lex("x /= 4").expect("should lex");
        assert_eq!(tokens[1].kind, TokenKind::SlashEq);

        let tokens = lex("x %= 5").expect("should lex");
        assert_eq!(tokens[1].kind, TokenKind::PercentEq);
    }

    #[test]
    fn lexes_plus_alone_still_works() {
        let tokens = lex("a + b").expect("should lex");
        let kinds: Vec<_> = tokens.iter().map(|t| &t.kind).collect();
        assert_eq!(*kinds[1], TokenKind::Symbol('+'));
    }

    #[test]
    fn lexes_minus_alone_still_works() {
        let tokens = lex("a - b").expect("should lex");
        assert_eq!(tokens[1].kind, TokenKind::Symbol('-'));
    }

    #[test]
    fn lexes_percent_alone_still_works() {
        let tokens = lex("a % b").expect("should lex");
        assert_eq!(tokens[1].kind, TokenKind::Symbol('%'));
    }

    #[test]
    fn lexes_single_dot_unchanged() {
        let tokens = lex("a.b").expect("should lex");
        let kinds: Vec<_> = tokens.iter().map(|t| &t.kind).collect();
        assert_eq!(*kinds[1], TokenKind::Symbol('.'));
    }

    #[test]
    fn lexes_question_question() {
        let tokens = lex("a ?? b").expect("should lex");
        let kinds: Vec<_> = tokens.iter().map(|t| &t.kind).collect();
        assert!(matches!(kinds[0], TokenKind::Ident(s) if s == "a"));
        assert_eq!(*kinds[1], TokenKind::QuestionQuestion);
        assert!(matches!(kinds[2], TokenKind::Ident(s) if s == "b"));
    }

    #[test]
    fn lexes_single_question_still_works() {
        let tokens = lex("a ? b : c").expect("should lex");
        assert_eq!(tokens[1].kind, TokenKind::Symbol('?'));
    }
}
