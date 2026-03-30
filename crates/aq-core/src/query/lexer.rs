/// Part of an interpolated string: either literal text or an expression to evaluate.
#[derive(Debug, Clone, PartialEq)]
pub enum InterpPart {
    Lit(String),
    Expr(String), // raw expression text, to be lexed+parsed by the parser
}

/// Token types for the aq query language.
#[derive(Debug, Clone, PartialEq)]
pub enum Token {
    // Literals
    String(String),
    InterpString(Vec<InterpPart>),
    Number(f64),
    Bool(bool),
    Null,

    // Punctuation
    Dot,         // .
    Pipe,        // |
    Colon,       // :
    At,          // @
    LParen,      // (
    RParen,      // )
    LBracket,    // [
    RBracket,    // ]
    LBrace,      // {
    RBrace,      // }
    Comma,       // ,
    Gt,          // >
    Lt,          // <
    Gte,         // >=
    Lte,         // <=
    Eq,          // ==
    NotEq,       // !=
    RegexMatch,  // =~
    DoubleSlash, // // (alternative operator)

    // Keywords / Identifiers
    Ident(String),

    // Built-in keywords
    Select,
    Match,
    Desc,
    Children,
    Parent,
    Ancestors,
    Siblings,
    PrevSibling,
    NextSibling,
    Not,
    And,
    Or,
    If,
    Then,
    Else,
    End,
    True,
    False,

    // Arithmetic
    Plus,
    Minus,
    Star,
    Slash,

    // End
    Eof,
}

/// Span information for error reporting.
#[derive(Debug, Clone, PartialEq)]
pub struct Spanned {
    pub token: Token,
    pub start: usize,
    pub end: usize,
}

/// Tokenize an aq query string.
pub fn lex(input: &str) -> Result<Vec<Spanned>, LexError> {
    let bytes = input.as_bytes();
    let mut tokens = Vec::new();
    let mut pos = 0;

    while pos < bytes.len() {
        let b = bytes[pos];

        // Skip whitespace
        if b.is_ascii_whitespace() {
            pos += 1;
            continue;
        }

        let start = pos;

        let token = match b {
            b'.' => {
                pos += 1;
                Token::Dot
            }
            b'|' => {
                pos += 1;
                Token::Pipe
            }
            b':' => {
                pos += 1;
                Token::Colon
            }
            b'@' => {
                pos += 1;
                Token::At
            }
            b'(' => {
                pos += 1;
                Token::LParen
            }
            b')' => {
                pos += 1;
                Token::RParen
            }
            b'[' => {
                pos += 1;
                Token::LBracket
            }
            b']' => {
                pos += 1;
                Token::RBracket
            }
            b'{' => {
                pos += 1;
                Token::LBrace
            }
            b'}' => {
                pos += 1;
                Token::RBrace
            }
            b',' => {
                pos += 1;
                Token::Comma
            }
            b'+' => {
                pos += 1;
                Token::Plus
            }
            b'-' => {
                pos += 1;
                Token::Minus
            }
            b'*' => {
                pos += 1;
                Token::Star
            }

            // / or //
            b'/' => {
                pos += 1;
                if pos < bytes.len() && bytes[pos] == b'/' {
                    pos += 1;
                    Token::DoubleSlash
                } else {
                    Token::Slash
                }
            }

            // = or == or =~
            b'=' => {
                pos += 1;
                if pos < bytes.len() && bytes[pos] == b'=' {
                    pos += 1;
                    Token::Eq
                } else if pos < bytes.len() && bytes[pos] == b'~' {
                    pos += 1;
                    Token::RegexMatch
                } else {
                    return Err(LexError {
                        position: start,
                        message: "Expected '==' or '=~', got bare '='".into(),
                    });
                }
            }

            // ! (only !=)
            b'!' => {
                pos += 1;
                if pos < bytes.len() && bytes[pos] == b'=' {
                    pos += 1;
                    Token::NotEq
                } else {
                    return Err(LexError {
                        position: start,
                        message: "Expected '!=', got bare '!'".into(),
                    });
                }
            }

            // > or >=
            b'>' => {
                pos += 1;
                if pos < bytes.len() && bytes[pos] == b'=' {
                    pos += 1;
                    Token::Gte
                } else {
                    Token::Gt
                }
            }

            // < or <=
            b'<' => {
                pos += 1;
                if pos < bytes.len() && bytes[pos] == b'=' {
                    pos += 1;
                    Token::Lte
                } else {
                    Token::Lt
                }
            }

            // String literal (possibly with interpolation)
            b'"' => {
                pos += 1;
                let mut s = String::new();
                let mut interp_parts: Vec<InterpPart> = Vec::new();
                let mut has_interp = false;

                loop {
                    if pos >= bytes.len() {
                        return Err(LexError {
                            position: start,
                            message: "Unterminated string literal".into(),
                        });
                    }
                    match bytes[pos] {
                        b'"' => {
                            pos += 1;
                            break;
                        }
                        b'\\' => {
                            pos += 1;
                            if pos >= bytes.len() {
                                return Err(LexError {
                                    position: pos,
                                    message: "Unterminated escape sequence".into(),
                                });
                            }
                            if bytes[pos] == b'(' {
                                // String interpolation: \(expr)
                                has_interp = true;
                                pos += 1; // skip '('

                                // Save accumulated literal text
                                if !s.is_empty() {
                                    interp_parts.push(InterpPart::Lit(std::mem::take(&mut s)));
                                }

                                // Scan forward tracking paren depth to find matching ')'
                                let expr_start = pos;
                                let mut depth = 1u32;
                                while pos < bytes.len() && depth > 0 {
                                    match bytes[pos] {
                                        b'(' => depth += 1,
                                        b')' => depth -= 1,
                                        b'"' => {
                                            // Skip over nested string literals
                                            pos += 1;
                                            while pos < bytes.len() && bytes[pos] != b'"' {
                                                if bytes[pos] == b'\\' {
                                                    pos += 1; // skip escaped char
                                                }
                                                pos += 1;
                                            }
                                            // pos now on closing '"', will be incremented below
                                        }
                                        _ => {}
                                    }
                                    if depth > 0 {
                                        pos += 1;
                                    }
                                }
                                if depth != 0 {
                                    return Err(LexError {
                                        position: expr_start,
                                        message: "Unterminated string interpolation \\(...)".into(),
                                    });
                                }
                                let expr_text = input[expr_start..pos].to_string();
                                interp_parts.push(InterpPart::Expr(expr_text));
                                pos += 1; // skip closing ')'
                            } else {
                                match bytes[pos] {
                                    b'"' => s.push('"'),
                                    b'\\' => s.push('\\'),
                                    b'n' => s.push('\n'),
                                    b't' => s.push('\t'),
                                    b'r' => s.push('\r'),
                                    other => {
                                        s.push('\\');
                                        s.push(other as char);
                                    }
                                }
                                pos += 1;
                            }
                        }
                        _ => {
                            s.push(bytes[pos] as char);
                            pos += 1;
                        }
                    }
                }

                if has_interp {
                    // Push trailing literal text if any
                    if !s.is_empty() {
                        interp_parts.push(InterpPart::Lit(s));
                    }
                    Token::InterpString(interp_parts)
                } else {
                    Token::String(s)
                }
            }

            // Number
            b'0'..=b'9' => {
                let num_start = pos;
                while pos < bytes.len() && bytes[pos].is_ascii_digit() {
                    pos += 1;
                }
                if pos < bytes.len()
                    && bytes[pos] == b'.'
                    && pos + 1 < bytes.len()
                    && bytes[pos + 1].is_ascii_digit()
                {
                    pos += 1; // skip '.'
                    while pos < bytes.len() && bytes[pos].is_ascii_digit() {
                        pos += 1;
                    }
                }
                let num_str = &input[num_start..pos];
                let n: f64 = num_str.parse().map_err(|_| LexError {
                    position: num_start,
                    message: format!("Invalid number: {}", num_str),
                })?;
                Token::Number(n)
            }

            // Identifier or keyword
            b'a'..=b'z' | b'A'..=b'Z' | b'_' => {
                let id_start = pos;
                while pos < bytes.len()
                    && (bytes[pos].is_ascii_alphanumeric() || bytes[pos] == b'_')
                {
                    pos += 1;
                }
                let word = &input[id_start..pos];
                match word {
                    "select" => Token::Select,
                    "match" => Token::Match,
                    "desc" => Token::Desc,
                    "children" => Token::Children,
                    "parent" => Token::Parent,
                    "ancestors" => Token::Ancestors,
                    "siblings" => Token::Siblings,
                    "prev_sibling" => Token::PrevSibling,
                    "next_sibling" => Token::NextSibling,
                    "not" => Token::Not,
                    "and" => Token::And,
                    "or" => Token::Or,
                    "if" => Token::If,
                    "then" => Token::Then,
                    "else" => Token::Else,
                    "end" => Token::End,
                    "true" => Token::True,
                    "false" => Token::False,
                    "null" => Token::Null,
                    _ => Token::Ident(word.to_string()),
                }
            }

            _ => {
                return Err(LexError {
                    position: pos,
                    message: format!("Unexpected character: '{}'", b as char),
                });
            }
        };

        tokens.push(Spanned {
            token,
            start,
            end: pos,
        });
    }

    tokens.push(Spanned {
        token: Token::Eof,
        start: pos,
        end: pos,
    });

    Ok(tokens)
}

#[derive(Debug, thiserror::Error)]
#[error("Lex error at position {position}: {message}")]
pub struct LexError {
    pub position: usize,
    pub message: String,
}

#[path = "lexer_tests.rs"]
#[cfg(test)]
mod tests;
