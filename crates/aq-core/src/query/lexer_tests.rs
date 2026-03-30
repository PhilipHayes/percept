#[cfg(test)]
mod tests {
    use crate::query::lexer::*;

    /// Helper: lex and return just the Token variants (no spans)
    fn tokens(input: &str) -> Vec<Token> {
        lex(input).unwrap().into_iter().map(|s| s.token).collect()
    }

    /// Helper: lex and expect an error
    fn lex_err(input: &str) -> LexError {
        lex(input).unwrap_err()
    }

    // -----------------------------------------------------------------------
    // Basic tokens
    // -----------------------------------------------------------------------

    #[test]
    fn empty_input() {
        assert_eq!(tokens(""), vec![Token::Eof]);
    }

    #[test]
    fn whitespace_only() {
        assert_eq!(tokens("   \t\n  "), vec![Token::Eof]);
    }

    #[test]
    fn single_punctuation() {
        assert_eq!(tokens("."), vec![Token::Dot, Token::Eof]);
        assert_eq!(tokens("|"), vec![Token::Pipe, Token::Eof]);
        assert_eq!(tokens(":"), vec![Token::Colon, Token::Eof]);
        assert_eq!(tokens("@"), vec![Token::At, Token::Eof]);
        assert_eq!(tokens("("), vec![Token::LParen, Token::Eof]);
        assert_eq!(tokens(")"), vec![Token::RParen, Token::Eof]);
        assert_eq!(tokens("["), vec![Token::LBracket, Token::Eof]);
        assert_eq!(tokens("]"), vec![Token::RBracket, Token::Eof]);
        assert_eq!(tokens("{"), vec![Token::LBrace, Token::Eof]);
        assert_eq!(tokens("}"), vec![Token::RBrace, Token::Eof]);
        assert_eq!(tokens(","), vec![Token::Comma, Token::Eof]);
        assert_eq!(tokens("+"), vec![Token::Plus, Token::Eof]);
        assert_eq!(tokens("-"), vec![Token::Minus, Token::Eof]);
        assert_eq!(tokens("*"), vec![Token::Star, Token::Eof]);
    }

    #[test]
    fn multi_char_operators() {
        assert_eq!(tokens("=="), vec![Token::Eq, Token::Eof]);
        assert_eq!(tokens("!="), vec![Token::NotEq, Token::Eof]);
        assert_eq!(tokens(">="), vec![Token::Gte, Token::Eof]);
        assert_eq!(tokens("<="), vec![Token::Lte, Token::Eof]);
        assert_eq!(tokens("=~"), vec![Token::RegexMatch, Token::Eof]);
        assert_eq!(tokens("//"), vec![Token::DoubleSlash, Token::Eof]);
        assert_eq!(tokens(">"), vec![Token::Gt, Token::Eof]);
        assert_eq!(tokens("<"), vec![Token::Lt, Token::Eof]);
        assert_eq!(tokens("/"), vec![Token::Slash, Token::Eof]);
    }

    // -----------------------------------------------------------------------
    // Numbers
    // -----------------------------------------------------------------------

    #[test]
    fn integer() {
        assert_eq!(tokens("42"), vec![Token::Number(42.0), Token::Eof]);
    }

    #[test]
    fn decimal_number() {
        assert_eq!(tokens("3.14"), vec![Token::Number(3.14), Token::Eof]);
    }

    #[test]
    fn zero() {
        assert_eq!(tokens("0"), vec![Token::Number(0.0), Token::Eof]);
    }

    #[test]
    fn dot_not_decimal() {
        // `.5` should be Dot followed by Number
        assert_eq!(tokens(".5"), vec![Token::Dot, Token::Number(5.0), Token::Eof]);
    }

    // -----------------------------------------------------------------------
    // Strings
    // -----------------------------------------------------------------------

    #[test]
    fn simple_string() {
        assert_eq!(
            tokens(r#""hello""#),
            vec![Token::String("hello".into()), Token::Eof]
        );
    }

    #[test]
    fn empty_string() {
        assert_eq!(
            tokens(r#""""#),
            vec![Token::String("".into()), Token::Eof]
        );
    }

    #[test]
    fn string_with_escapes() {
        assert_eq!(
            tokens(r#""a\nb\tc""#),
            vec![Token::String("a\nb\tc".into()), Token::Eof]
        );
    }

    #[test]
    fn string_with_escaped_quotes() {
        assert_eq!(
            tokens(r#""say \"hi\"""#),
            vec![Token::String("say \"hi\"".into()), Token::Eof]
        );
    }

    #[test]
    fn unterminated_string() {
        let err = lex_err(r#""hello"#);
        assert!(err.message.contains("Unterminated string"));
    }

    // -----------------------------------------------------------------------
    // String interpolation
    // -----------------------------------------------------------------------

    #[test]
    fn string_interpolation_simple() {
        let toks = tokens(r#""\(.name)""#);
        match &toks[0] {
            Token::InterpString(parts) => {
                assert_eq!(parts.len(), 1);
                assert_eq!(parts[0], InterpPart::Expr(".name".into()));
            }
            other => panic!("Expected InterpString, got {:?}", other),
        }
    }

    #[test]
    fn string_interpolation_with_text() {
        let toks = tokens(r#""hello \(.name) world""#);
        match &toks[0] {
            Token::InterpString(parts) => {
                assert_eq!(parts.len(), 3);
                assert_eq!(parts[0], InterpPart::Lit("hello ".into()));
                assert_eq!(parts[1], InterpPart::Expr(".name".into()));
                assert_eq!(parts[2], InterpPart::Lit(" world".into()));
            }
            other => panic!("Expected InterpString, got {:?}", other),
        }
    }

    #[test]
    fn string_interpolation_multiple() {
        let toks = tokens(r#""\(.a) + \(.b)""#);
        match &toks[0] {
            Token::InterpString(parts) => {
                assert_eq!(parts.len(), 3);
                assert_eq!(parts[0], InterpPart::Expr(".a".into()));
                assert_eq!(parts[1], InterpPart::Lit(" + ".into()));
                assert_eq!(parts[2], InterpPart::Expr(".b".into()));
            }
            other => panic!("Expected InterpString, got {:?}", other),
        }
    }

    #[test]
    fn string_interpolation_nested_parens() {
        let toks = tokens(r#""\(f(x))""#);
        match &toks[0] {
            Token::InterpString(parts) => {
                assert_eq!(parts.len(), 1);
                assert_eq!(parts[0], InterpPart::Expr("f(x)".into()));
            }
            other => panic!("Expected InterpString, got {:?}", other),
        }
    }

    // -----------------------------------------------------------------------
    // Keywords and identifiers
    // -----------------------------------------------------------------------

    #[test]
    fn keywords() {
        assert_eq!(tokens("select"), vec![Token::Select, Token::Eof]);
        assert_eq!(tokens("match"), vec![Token::Match, Token::Eof]);
        assert_eq!(tokens("desc"), vec![Token::Desc, Token::Eof]);
        assert_eq!(tokens("children"), vec![Token::Children, Token::Eof]);
        assert_eq!(tokens("parent"), vec![Token::Parent, Token::Eof]);
        assert_eq!(tokens("ancestors"), vec![Token::Ancestors, Token::Eof]);
        assert_eq!(tokens("siblings"), vec![Token::Siblings, Token::Eof]);
        assert_eq!(tokens("prev_sibling"), vec![Token::PrevSibling, Token::Eof]);
        assert_eq!(tokens("next_sibling"), vec![Token::NextSibling, Token::Eof]);
        assert_eq!(tokens("not"), vec![Token::Not, Token::Eof]);
        assert_eq!(tokens("and"), vec![Token::And, Token::Eof]);
        assert_eq!(tokens("or"), vec![Token::Or, Token::Eof]);
        assert_eq!(tokens("if"), vec![Token::If, Token::Eof]);
        assert_eq!(tokens("then"), vec![Token::Then, Token::Eof]);
        assert_eq!(tokens("else"), vec![Token::Else, Token::Eof]);
        assert_eq!(tokens("end"), vec![Token::End, Token::Eof]);
        assert_eq!(tokens("true"), vec![Token::True, Token::Eof]);
        assert_eq!(tokens("false"), vec![Token::False, Token::Eof]);
        assert_eq!(tokens("null"), vec![Token::Null, Token::Eof]);
    }

    #[test]
    fn identifiers() {
        assert_eq!(
            tokens("length"),
            vec![Token::Ident("length".into()), Token::Eof]
        );
        assert_eq!(
            tokens("my_func_2"),
            vec![Token::Ident("my_func_2".into()), Token::Eof]
        );
        assert_eq!(
            tokens("_private"),
            vec![Token::Ident("_private".into()), Token::Eof]
        );
    }

    #[test]
    fn booleans_and_null() {
        assert_eq!(tokens("true"), vec![Token::True, Token::Eof]);
        assert_eq!(tokens("false"), vec![Token::False, Token::Eof]);
        assert_eq!(tokens("null"), vec![Token::Null, Token::Eof]);
    }

    // -----------------------------------------------------------------------
    // Complex expressions
    // -----------------------------------------------------------------------

    #[test]
    fn pipe_expression() {
        assert_eq!(
            tokens(". | @type"),
            vec![Token::Dot, Token::Pipe, Token::At, Token::Ident("type".into()), Token::Eof]
        );
    }

    #[test]
    fn field_access() {
        assert_eq!(
            tokens(".name"),
            vec![Token::Dot, Token::Ident("name".into()), Token::Eof]
        );
    }

    #[test]
    fn type_filter() {
        assert_eq!(
            tokens("desc:function_item"),
            vec![
                Token::Desc,
                Token::Colon,
                Token::Ident("function_item".into()),
                Token::Eof,
            ]
        );
    }

    #[test]
    fn object_construction() {
        let toks = tokens(r#"{name: .name, line: @line}"#);
        assert_eq!(
            toks,
            vec![
                Token::LBrace,
                Token::Ident("name".into()),
                Token::Colon,
                Token::Dot,
                Token::Ident("name".into()),
                Token::Comma,
                Token::Ident("line".into()),
                Token::Colon,
                Token::At,
                Token::Ident("line".into()),
                Token::RBrace,
                Token::Eof,
            ]
        );
    }

    #[test]
    fn select_expression() {
        let toks = tokens("select(@type == \"function_item\")");
        assert_eq!(
            toks,
            vec![
                Token::Select,
                Token::LParen,
                Token::At,
                Token::Ident("type".into()),
                Token::Eq,
                Token::String("function_item".into()),
                Token::RParen,
                Token::Eof,
            ]
        );
    }

    #[test]
    fn arithmetic_expression() {
        assert_eq!(
            tokens("@end - @start + 1"),
            vec![
                Token::At,
                Token::End,
                Token::Minus,
                Token::At,
                Token::Ident("start".into()),
                Token::Plus,
                Token::Number(1.0),
                Token::Eof,
            ]
        );
    }

    // -----------------------------------------------------------------------
    // Span tracking
    // -----------------------------------------------------------------------

    #[test]
    fn spans_are_correct() {
        let spanned = lex("a | b").unwrap();
        assert_eq!(spanned[0].start, 0);
        assert_eq!(spanned[0].end, 1);
        assert_eq!(spanned[1].start, 2);
        assert_eq!(spanned[1].end, 3);
        assert_eq!(spanned[2].start, 4);
        assert_eq!(spanned[2].end, 5);
    }

    // -----------------------------------------------------------------------
    // Error cases
    // -----------------------------------------------------------------------

    #[test]
    fn bare_equals() {
        let err = lex_err("a = b");
        assert!(err.message.contains("Expected '==' or '=~'"));
    }

    #[test]
    fn bare_bang() {
        let err = lex_err("a ! b");
        assert!(err.message.contains("Expected '!='"));
    }

    #[test]
    fn unexpected_character() {
        let err = lex_err("a $ b");
        assert!(err.message.contains("Unexpected character"));
    }
}
