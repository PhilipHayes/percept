use super::lexer::{lex, InterpPart, Spanned, Token};

/// AST representation of an aq query.
///
/// Every node in this enum is a "filter" in jq terms — it takes
/// input node(s) and produces zero or more output nodes/values.
#[derive(Debug, Clone)]
pub enum Expr {
    /// `.` — identity, passes through current node
    Identity,

    /// `.field_name` — access named child by field name
    Field(String),

    /// `@type`, `@text`, `@line`, etc. — metadata accessor
    Meta(MetaField),

    /// `expr | expr` — pipe: feed output of left into right
    Pipe(Box<Expr>, Box<Expr>),

    /// `children`, `children[n]`
    Children(Option<isize>),

    /// `desc`, `desc(n)` — recursive descent, optional max depth
    Descendants(Option<usize>),

    /// `desc:type_name` or `children:type_name` — type-filtered traversal
    TypeFilter {
        axis: Axis,
        types: Vec<String>,
    },

    /// `parent`
    Parent,

    /// `ancestors`
    Ancestors,

    /// `siblings`, `prev_sibling`, `next_sibling`
    Sibling(SiblingKind),

    /// `select(expr)` — keep node if expr is truthy
    Select(Box<Expr>),

    /// `match(pattern)` — structural pattern match
    Match(Pattern),

    /// `{ key: expr, ... }` — object construction
    Object(Vec<(String, Expr)>),

    /// `[expr]` — array construction / collect
    Array(Box<Expr>),

    /// `"string with \(interpolation)"`
    StringInterp(Vec<StringPart>),

    /// Literal value
    Literal(Value),

    /// `expr == expr`, `expr != expr`, etc.
    Compare(Box<Expr>, CmpOp, Box<Expr>),

    /// `expr // expr` — alternative (default if empty/null)
    Alternative(Box<Expr>, Box<Expr>),

    /// `expr + expr`, `expr - expr`, etc.
    Arithmetic(Box<Expr>, ArithOp, Box<Expr>),

    /// `not`, `and`, `or`
    Logic(Box<Expr>, LogicOp, Box<Expr>),
    LogicNot(Box<Expr>),

    /// Built-in function call: `length`, `startswith(expr)`, `group_by(expr)`, etc.
    Builtin(String, Vec<Expr>),

    /// `if cond then a else b end`
    IfThenElse {
        cond: Box<Expr>,
        then_branch: Box<Expr>,
        else_branch: Option<Box<Expr>>,
    },

    /// Internal: concatenation of multiple expressions (used for `[a, b, c]`)
    Concat(Vec<Expr>),

    /// `.[]` — iterate array elements / object values / node children
    Iterate,

    /// `.[n]` — index into array
    Index(isize),
}

#[derive(Debug, Clone)]
pub enum MetaField {
    Type,
    Text,
    Start,
    End,
    Line,
    File,
    SubtreeText,
    Depth,
    Path,
    // Format meta fields (jq-compatible)
    Csv,
    Tsv,
    Json,
}

#[derive(Debug, Clone)]
pub enum Axis {
    Children,
    Descendants(Option<usize>),
    Self_,
}

#[derive(Debug, Clone)]
pub enum SiblingKind {
    All,
    Prev,
    Next,
}

#[derive(Debug, Clone)]
pub enum CmpOp {
    Eq,
    NotEq,
    Lt,
    Gt,
    Lte,
    Gte,
    RegexMatch,
}

#[derive(Debug, Clone)]
pub enum ArithOp {
    Add,
    Sub,
    Mul,
    Div,
}

#[derive(Debug, Clone)]
pub enum LogicOp {
    And,
    Or,
}

#[derive(Debug, Clone)]
pub enum Value {
    String(String),
    Number(f64),
    Bool(bool),
    Null,
    Array(Vec<Value>),
}

#[derive(Debug, Clone)]
pub enum StringPart {
    Literal(String),
    Interpolation(Expr),
}

/// Structural pattern for `match()` expressions.
#[derive(Debug, Clone)]
pub struct Pattern {
    pub steps: Vec<PatternStep>,
}

#[derive(Debug, Clone)]
pub struct PatternStep {
    pub node_type: String,
    pub combinator: Combinator,
    /// Field constraint: `name:(identifier)` → Some(("name", "identifier"))
    pub field_constraint: Option<(String, String)>,
    pub capture_name: Option<String>,
    pub predicates: Vec<PatternPredicate>,
}

#[derive(Debug, Clone)]
pub enum Combinator {
    /// First step (no combinator)
    Root,
    /// `>` — direct child
    Child,
    /// ` ` (space) — any descendant
    Descendant,
}

#[derive(Debug, Clone)]
pub struct PatternPredicate {
    pub field: String,
    pub op: CmpOp,
    pub value: String,
}

// ---------------------------------------------------------------------------
// Parser implementation
// ---------------------------------------------------------------------------

struct Parser<'a> {
    tokens: &'a [Spanned],
    pos: usize,
}

impl<'a> Parser<'a> {
    fn new(tokens: &'a [Spanned]) -> Self {
        Self { tokens, pos: 0 }
    }

    fn peek(&self) -> &Token {
        self.tokens
            .get(self.pos)
            .map(|s| &s.token)
            .unwrap_or(&Token::Eof)
    }

    fn current_pos(&self) -> usize {
        self.tokens.get(self.pos).map(|s| s.start).unwrap_or(0)
    }

    fn advance(&mut self) -> &Token {
        let tok = self.peek();
        if !matches!(tok, Token::Eof) {
            self.pos += 1;
        }
        self.tokens
            .get(self.pos - 1)
            .map(|s| &s.token)
            .unwrap_or(&Token::Eof)
    }

    fn expect(&mut self, expected: &Token) -> Result<(), ParseError> {
        let tok = self.peek().clone();
        if std::mem::discriminant(&tok) == std::mem::discriminant(expected) {
            self.advance();
            Ok(())
        } else {
            Err(ParseError {
                position: self.current_pos(),
                message: format!("Expected {:?}, got {:?}", expected, tok),
            })
        }
    }

    /// Is the current token a "terminator" — something that ends an expression
    /// in contexts like `select(...)`, pipe, etc.
    fn at_terminator(&self) -> bool {
        matches!(
            self.peek(),
            Token::Pipe
                | Token::RParen
                | Token::RBracket
                | Token::RBrace
                | Token::Comma
                | Token::Eof
                | Token::Then
                | Token::Else
                | Token::End
        )
    }

    // -----------------------------------------------------------------------
    // Precedence levels (lowest to highest)
    // -----------------------------------------------------------------------

    fn parse_pipe(&mut self) -> Result<Expr, ParseError> {
        let mut left = self.parse_alt()?;
        while matches!(self.peek(), Token::Pipe) {
            self.advance();
            let right = self.parse_alt()?;
            left = Expr::Pipe(Box::new(left), Box::new(right));
        }
        Ok(left)
    }

    fn parse_alt(&mut self) -> Result<Expr, ParseError> {
        let mut left = self.parse_or()?;
        while matches!(self.peek(), Token::DoubleSlash) {
            self.advance();
            let right = self.parse_or()?;
            left = Expr::Alternative(Box::new(left), Box::new(right));
        }
        Ok(left)
    }

    fn parse_or(&mut self) -> Result<Expr, ParseError> {
        let mut left = self.parse_and()?;
        while matches!(self.peek(), Token::Or) {
            self.advance();
            let right = self.parse_and()?;
            left = Expr::Logic(Box::new(left), LogicOp::Or, Box::new(right));
        }
        Ok(left)
    }

    fn parse_and(&mut self) -> Result<Expr, ParseError> {
        let mut left = self.parse_compare()?;
        while matches!(self.peek(), Token::And) {
            self.advance();
            let right = self.parse_compare()?;
            left = Expr::Logic(Box::new(left), LogicOp::And, Box::new(right));
        }
        Ok(left)
    }

    fn parse_compare(&mut self) -> Result<Expr, ParseError> {
        let left = self.parse_add()?;
        let op = match self.peek() {
            Token::Eq => Some(CmpOp::Eq),
            Token::NotEq => Some(CmpOp::NotEq),
            Token::Lt => Some(CmpOp::Lt),
            Token::Gt => Some(CmpOp::Gt),
            Token::Lte => Some(CmpOp::Lte),
            Token::Gte => Some(CmpOp::Gte),
            Token::RegexMatch => Some(CmpOp::RegexMatch),
            _ => None,
        };
        if let Some(op) = op {
            self.advance();
            let right = self.parse_add()?;
            Ok(Expr::Compare(Box::new(left), op, Box::new(right)))
        } else {
            Ok(left)
        }
    }

    fn parse_add(&mut self) -> Result<Expr, ParseError> {
        let mut left = self.parse_mul()?;
        loop {
            let op = match self.peek() {
                Token::Plus => ArithOp::Add,
                Token::Minus => ArithOp::Sub,
                _ => break,
            };
            self.advance();
            let right = self.parse_mul()?;
            left = Expr::Arithmetic(Box::new(left), op, Box::new(right));
        }
        Ok(left)
    }

    fn parse_mul(&mut self) -> Result<Expr, ParseError> {
        let mut left = self.parse_unary()?;
        loop {
            let op = match self.peek() {
                Token::Star => ArithOp::Mul,
                Token::Slash => ArithOp::Div,
                _ => break,
            };
            self.advance();
            let right = self.parse_unary()?;
            left = Expr::Arithmetic(Box::new(left), op, Box::new(right));
        }
        Ok(left)
    }

    fn parse_unary(&mut self) -> Result<Expr, ParseError> {
        if matches!(self.peek(), Token::Not) {
            // Check if this is postfix `not` (used like `expr | not`)
            // or prefix `not expr`
            self.advance();
            if self.at_terminator() {
                // postfix: acts as a zero-arg builtin
                Ok(Expr::Builtin("not".into(), vec![]))
            } else {
                let inner = self.parse_unary()?;
                Ok(Expr::LogicNot(Box::new(inner)))
            }
        } else if matches!(self.peek(), Token::Minus) {
            self.advance();
            let inner = self.parse_unary()?;
            Ok(Expr::Arithmetic(
                Box::new(Expr::Literal(Value::Number(0.0))),
                ArithOp::Sub,
                Box::new(inner),
            ))
        } else {
            self.parse_atom()
        }
    }

    fn parse_atom(&mut self) -> Result<Expr, ParseError> {
        match self.peek().clone() {
            // . or .field or .:type or .[]
            Token::Dot => {
                self.advance();
                match self.peek().clone() {
                    Token::Ident(name) => {
                        self.advance();
                        Ok(Expr::Field(name))
                    }
                    Token::Colon => self.parse_type_filter(Axis::Self_),
                    Token::LBracket => {
                        self.advance();
                        if matches!(self.peek(), Token::RBracket) {
                            self.advance();
                            Ok(Expr::Iterate)
                        } else {
                            // .[n] — index access
                            let idx = self.parse_index()?;
                            self.expect(&Token::RBracket)?;
                            Ok(Expr::Index(idx))
                        }
                    }
                    _ => Ok(Expr::Identity),
                }
            }

            // @meta
            Token::At => {
                self.advance();
                // Extract meta field name — handle both Ident and keyword tokens
                // since "end", "type" etc. may be lexed as keywords
                let meta_name = match self.peek().clone() {
                    Token::Ident(name) => {
                        self.advance();
                        name
                    }
                    Token::End => {
                        self.advance();
                        "end".to_string()
                    }
                    Token::Not => {
                        self.advance();
                        "not".to_string()
                    }
                    _ => {
                        return Err(ParseError {
                            position: self.current_pos(),
                            message: format!(
                                "Expected meta field name after '@', got {:?}",
                                self.peek()
                            ),
                        });
                    }
                };
                let field = match meta_name.as_str() {
                    "type" => MetaField::Type,
                    "text" => MetaField::Text,
                    "start" => MetaField::Start,
                    "end" => MetaField::End,
                    "line" => MetaField::Line,
                    "file" => MetaField::File,
                    "subtree_text" | "subtree" => MetaField::SubtreeText,
                    "depth" => MetaField::Depth,
                    "path" => MetaField::Path,
                    "csv" => MetaField::Csv,
                    "tsv" => MetaField::Tsv,
                    "json" => MetaField::Json,
                    _ => {
                        return Err(ParseError {
                            position: self.current_pos(),
                            message: format!("Unknown meta field: @{}", meta_name),
                        });
                    }
                };
                Ok(Expr::Meta(field))
            }

            // children, children[n], children:type
            Token::Children => {
                self.advance();
                match self.peek() {
                    Token::Colon => self.parse_type_filter(Axis::Children),
                    Token::LBracket => {
                        self.advance();
                        let idx = self.parse_index()?;
                        self.expect(&Token::RBracket)?;
                        Ok(Expr::Children(Some(idx)))
                    }
                    _ => Ok(Expr::Children(None)),
                }
            }

            // desc, desc(n), desc:type
            Token::Desc => {
                self.advance();
                match self.peek() {
                    Token::Colon => self.parse_type_filter(Axis::Descendants(None)),
                    Token::LParen => {
                        self.advance();
                        let depth = self.parse_positive_int()?;
                        self.expect(&Token::RParen)?;
                        if matches!(self.peek(), Token::Colon) {
                            self.parse_type_filter(Axis::Descendants(Some(depth)))
                        } else {
                            Ok(Expr::Descendants(Some(depth)))
                        }
                    }
                    _ => Ok(Expr::Descendants(None)),
                }
            }

            // parent
            Token::Parent => {
                self.advance();
                Ok(Expr::Parent)
            }

            // ancestors
            Token::Ancestors => {
                self.advance();
                Ok(Expr::Ancestors)
            }

            // siblings
            Token::Siblings => {
                self.advance();
                Ok(Expr::Sibling(SiblingKind::All))
            }

            // prev_sibling
            Token::PrevSibling => {
                self.advance();
                Ok(Expr::Sibling(SiblingKind::Prev))
            }

            // next_sibling
            Token::NextSibling => {
                self.advance();
                Ok(Expr::Sibling(SiblingKind::Next))
            }

            // select(expr)
            Token::Select => {
                self.advance();
                self.expect(&Token::LParen)?;
                let inner = self.parse_pipe()?;
                self.expect(&Token::RParen)?;
                Ok(Expr::Select(Box::new(inner)))
            }

            // { key: expr, ... }
            Token::LBrace => {
                self.advance();
                let mut pairs = Vec::new();
                while !matches!(self.peek(), Token::RBrace | Token::Eof) {
                    let key = match self.peek().clone() {
                        Token::Ident(name) => {
                            self.advance();
                            name
                        }
                        Token::String(name) => {
                            self.advance();
                            name
                        }
                        _ => {
                            return Err(ParseError {
                                position: self.current_pos(),
                                message: format!(
                                    "Expected object key (identifier or string), got {:?}",
                                    self.peek()
                                ),
                            });
                        }
                    };
                    self.expect(&Token::Colon)?;
                    let value = self.parse_pipe()?;
                    pairs.push((key, value));
                    if matches!(self.peek(), Token::Comma) {
                        self.advance();
                    }
                }
                self.expect(&Token::RBrace)?;
                Ok(Expr::Object(pairs))
            }

            // [expr] or [expr, expr, ...] — array collect
            Token::LBracket => {
                self.advance();
                if matches!(self.peek(), Token::RBracket) {
                    // Empty array []
                    self.advance();
                    return Ok(Expr::Literal(Value::Array(vec![])));
                }
                let mut exprs = vec![self.parse_pipe()?];
                while matches!(self.peek(), Token::Comma) {
                    self.advance();
                    if matches!(self.peek(), Token::RBracket) {
                        break; // trailing comma
                    }
                    exprs.push(self.parse_pipe()?);
                }
                self.expect(&Token::RBracket)?;
                if exprs.len() == 1 {
                    Ok(Expr::Array(Box::new(exprs.pop().unwrap())))
                } else {
                    // Multiple comma-separated expressions: concatenate into Concat
                    Ok(Expr::Array(Box::new(Expr::Concat(exprs))))
                }
            }

            // (expr) — grouping
            Token::LParen => {
                self.advance();
                let inner = self.parse_pipe()?;
                self.expect(&Token::RParen)?;
                Ok(inner)
            }

            // Literals
            Token::String(s) => {
                self.advance();
                Ok(Expr::Literal(Value::String(s)))
            }

            // String interpolation: "\(expr) text \(expr)"
            Token::InterpString(parts) => {
                self.advance();
                let mut string_parts = Vec::new();
                for part in parts {
                    match part {
                        InterpPart::Lit(s) => {
                            string_parts.push(StringPart::Literal(s));
                        }
                        InterpPart::Expr(expr_str) => {
                            let sub_tokens = lex(&expr_str).map_err(|e| ParseError {
                                position: self.current_pos(),
                                message: format!("Error lexing interpolation: {}", e),
                            })?;
                            let mut sub_parser = Parser::new(&sub_tokens);
                            let expr = sub_parser.parse_pipe()?;
                            string_parts.push(StringPart::Interpolation(expr));
                        }
                    }
                }
                Ok(Expr::StringInterp(string_parts))
            }
            Token::Number(n) => {
                self.advance();
                Ok(Expr::Literal(Value::Number(n)))
            }
            Token::True => {
                self.advance();
                Ok(Expr::Literal(Value::Bool(true)))
            }
            Token::False => {
                self.advance();
                Ok(Expr::Literal(Value::Bool(false)))
            }
            Token::Null => {
                self.advance();
                Ok(Expr::Literal(Value::Null))
            }

            // match(pattern)
            Token::Match => {
                self.advance();
                self.expect(&Token::LParen)?;
                let pattern = self.parse_pattern()?;
                self.expect(&Token::RParen)?;
                Ok(Expr::Match(pattern))
            }

            // if-then-else (MVP: basic support)
            Token::If => {
                self.advance();
                let cond = self.parse_pipe()?;
                self.expect(&Token::Then)?;
                let then_branch = self.parse_pipe()?;
                let else_branch = if matches!(self.peek(), Token::Else) {
                    self.advance();
                    Some(Box::new(self.parse_pipe()?))
                } else {
                    None
                };
                self.expect(&Token::End)?;
                Ok(Expr::IfThenElse {
                    cond: Box::new(cond),
                    then_branch: Box::new(then_branch),
                    else_branch,
                })
            }

            // Identifier — builtin function or builtin with args
            Token::Ident(name) => {
                self.advance();
                if matches!(self.peek(), Token::LParen) {
                    // builtin(args...)
                    self.advance();
                    let mut args = Vec::new();
                    if !matches!(self.peek(), Token::RParen) {
                        args.push(self.parse_pipe()?);
                        while matches!(self.peek(), Token::Comma) {
                            self.advance();
                            args.push(self.parse_pipe()?);
                        }
                    }
                    self.expect(&Token::RParen)?;
                    Ok(Expr::Builtin(name, args))
                } else {
                    // zero-arg builtin
                    Ok(Expr::Builtin(name, vec![]))
                }
            }

            other => Err(ParseError {
                position: self.current_pos(),
                message: format!("Unexpected token: {:?}", other),
            }),
        }
    }

    // -----------------------------------------------------------------------
    // Helpers
    // -----------------------------------------------------------------------

    // -----------------------------------------------------------------------
    // Pattern parsing for match()
    // -----------------------------------------------------------------------

    /// Parse a structural pattern: `type > type` or `type type` (descendant)
    fn parse_pattern(&mut self) -> Result<Pattern, ParseError> {
        let mut steps = Vec::new();

        // First step is always Root combinator
        steps.push(self.parse_pattern_step(Combinator::Root)?);

        // Subsequent steps
        loop {
            if matches!(self.peek(), Token::Gt) {
                // > means direct child combinator
                self.advance();
                steps.push(self.parse_pattern_step(Combinator::Child)?);
            } else if self.is_pattern_type_start() {
                // Identifier or keyword without > means descendant combinator
                steps.push(self.parse_pattern_step(Combinator::Descendant)?);
            } else {
                break;
            }
        }

        Ok(Pattern { steps })
    }

    /// Check if the current token can start a pattern type name
    fn is_pattern_type_start(&self) -> bool {
        matches!(
            self.peek(),
            Token::Ident(_)
                | Token::Select
                | Token::Match
                | Token::If
                | Token::Then
                | Token::Else
                | Token::End
                | Token::Not
                | Token::And
                | Token::Or
                | Token::Children
                | Token::Desc
                | Token::Parent
                | Token::Ancestors
                | Token::Siblings
                | Token::PrevSibling
                | Token::NextSibling
                | Token::True
                | Token::False
                | Token::Null
        )
    }

    /// Parse a single pattern step: `type_name [predicates] @capture`
    fn parse_pattern_step(&mut self, combinator: Combinator) -> Result<PatternStep, ParseError> {
        let node_type = self.parse_pattern_type_name()?;

        // Optional field constraint: `field_name:(type_name)`
        // Pattern: `function_item name:(identifier) @capture`
        // We need to look ahead to distinguish from a subsequent pattern step.
        // A field constraint is `ident : ( type_name )` — the colon + lparen disambiguate.
        let field_constraint = if let Token::Ident(maybe_field) = self.peek().clone() {
            // Look ahead: check if this is `ident : ( ... )`
            if self.pos + 2 < self.tokens.len()
                && matches!(self.tokens[self.pos + 1].token, Token::Colon)
                && matches!(self.tokens[self.pos + 2].token, Token::LParen)
            {
                let field_name = maybe_field;
                self.advance(); // consume field name
                self.advance(); // consume :
                self.advance(); // consume (
                let child_type = self.parse_pattern_type_name()?;
                self.expect(&Token::RParen)?;
                Some((field_name, child_type))
            } else {
                None
            }
        } else {
            None
        };

        // Optional predicates: `[@text == "value"]`
        let predicates = if matches!(self.peek(), Token::LBracket) {
            self.advance();
            let mut preds = Vec::new();
            while !matches!(self.peek(), Token::RBracket | Token::Eof) {
                preds.push(self.parse_pattern_predicate()?);
                if matches!(self.peek(), Token::And) {
                    self.advance(); // skip optional `and` between predicates
                }
            }
            self.expect(&Token::RBracket)?;
            preds
        } else {
            Vec::new()
        };

        // Optional capture: `@name`
        let capture_name = if matches!(self.peek(), Token::At) {
            self.advance();
            match self.peek().clone() {
                Token::Ident(name) => {
                    self.advance();
                    Some(name)
                }
                _ => {
                    return Err(ParseError {
                        position: self.current_pos(),
                        message: "Expected capture name after '@' in pattern".into(),
                    });
                }
            }
        } else {
            None
        };

        Ok(PatternStep {
            node_type,
            combinator,
            field_constraint,
            capture_name,
            predicates,
        })
    }

    /// Parse a type name in a pattern context.
    /// Accepts identifiers AND keywords since tree-sitter node types can contain
    /// words like "if", "match", "use", etc.
    fn parse_pattern_type_name(&mut self) -> Result<String, ParseError> {
        let name = match self.peek().clone() {
            Token::Ident(name) => name,
            Token::Select => "select".into(),
            Token::Match => "match".into(),
            Token::If => "if".into(),
            Token::Then => "then".into(),
            Token::Else => "else".into(),
            Token::End => "end".into(),
            Token::Not => "not".into(),
            Token::And => "and".into(),
            Token::Or => "or".into(),
            Token::Children => "children".into(),
            Token::Desc => "desc".into(),
            Token::Parent => "parent".into(),
            Token::Ancestors => "ancestors".into(),
            Token::Siblings => "siblings".into(),
            Token::PrevSibling => "prev_sibling".into(),
            Token::NextSibling => "next_sibling".into(),
            Token::True => "true".into(),
            Token::False => "false".into(),
            Token::Null => "null".into(),
            _ => {
                return Err(ParseError {
                    position: self.current_pos(),
                    message: format!("Expected type name in pattern, got {:?}", self.peek()),
                });
            }
        };
        self.advance();

        // Allow compound type names with underscores: `function_item`, `use_declaration`
        // After the first word, check if next is `_` followed by more identifier chars
        // Since the lexer handles `function_item` as a single ident, we only need this
        // for cases where a keyword is the prefix: `use_declaration` → `use` + `_declaration`
        // Check if `_` immediately follows (no whitespace) by checking byte positions
        let mut full_name = name;
        loop {
            // Check if next char in source is underscore (adjacent, no whitespace)
            let current_end = self
                .tokens
                .get(self.pos.wrapping_sub(1))
                .map(|s| s.end)
                .unwrap_or(0);
            let next_start = self
                .tokens
                .get(self.pos)
                .map(|s| s.start)
                .unwrap_or(usize::MAX);

            // If the previous token's end position is immediately followed by an underscore-prefixed ident
            // we need to handle cases like keyword `use` followed by `_declaration` (which lexer sees as Ident("_declaration"))
            if current_end == next_start {
                if let Some(Token::Ident(next_part)) = self.tokens.get(self.pos).map(|s| &s.token) {
                    if next_part.starts_with('_') {
                        full_name.push_str(next_part);
                        self.advance();
                        continue;
                    }
                }
            }
            break;
        }

        Ok(full_name)
    }

    /// Parse a single predicate inside `[...]`: `@field op "value"`
    fn parse_pattern_predicate(&mut self) -> Result<PatternPredicate, ParseError> {
        // Expect @field
        self.expect(&Token::At)?;
        let field = match self.peek().clone() {
            Token::Ident(name) => {
                self.advance();
                name
            }
            Token::End => {
                self.advance();
                "end".to_string()
            }
            _ => {
                return Err(ParseError {
                    position: self.current_pos(),
                    message: format!(
                        "Expected field name after '@' in predicate, got {:?}",
                        self.peek()
                    ),
                });
            }
        };

        // Expect comparison operator
        let op = match self.peek() {
            Token::Eq => CmpOp::Eq,
            Token::NotEq => CmpOp::NotEq,
            Token::RegexMatch => CmpOp::RegexMatch,
            Token::Lt => CmpOp::Lt,
            Token::Gt => CmpOp::Gt,
            Token::Lte => CmpOp::Lte,
            Token::Gte => CmpOp::Gte,
            _ => {
                return Err(ParseError {
                    position: self.current_pos(),
                    message: format!(
                        "Expected comparison operator in predicate, got {:?}",
                        self.peek()
                    ),
                });
            }
        };
        self.advance();

        // Expect string value
        let value = match self.peek().clone() {
            Token::String(s) => {
                self.advance();
                s
            }
            Token::Number(n) => {
                self.advance();
                n.to_string()
            }
            _ => {
                return Err(ParseError {
                    position: self.current_pos(),
                    message: format!("Expected value in predicate, got {:?}", self.peek()),
                });
            }
        };

        Ok(PatternPredicate { field, op, value })
    }

    /// Parse type filter after seeing `:` — e.g., `:function_declaration` or `:(a | b)`
    fn parse_type_filter(&mut self, axis: Axis) -> Result<Expr, ParseError> {
        self.expect(&Token::Colon)?;
        let types = if matches!(self.peek(), Token::LParen) {
            // Multi-type: (type1 | type2 | ...)
            self.advance();
            let mut types = vec![self.parse_type_name()?];
            while matches!(self.peek(), Token::Pipe) {
                self.advance();
                types.push(self.parse_type_name()?);
            }
            self.expect(&Token::RParen)?;
            types
        } else {
            vec![self.parse_type_name()?]
        };
        let type_filter = Expr::TypeFilter { axis, types };
        // Bracket-filter: desc:type[pred] → desc:type | select(pred)
        if matches!(self.peek(), Token::LBracket) {
            self.advance();
            let pred = self.parse_pipe()?;
            self.expect(&Token::RBracket)?;
            Ok(Expr::Pipe(
                Box::new(type_filter),
                Box::new(Expr::Select(Box::new(pred))),
            ))
        } else {
            Ok(type_filter)
        }
    }

    /// Parse a type name (identifier)
    fn parse_type_name(&mut self) -> Result<String, ParseError> {
        match self.peek().clone() {
            Token::Ident(name) => {
                self.advance();
                Ok(name)
            }
            _ => Err(ParseError {
                position: self.current_pos(),
                message: format!("Expected type name, got {:?}", self.peek()),
            }),
        }
    }

    /// Parse an integer index (possibly negative)
    fn parse_index(&mut self) -> Result<isize, ParseError> {
        let negative = if matches!(self.peek(), Token::Minus) {
            self.advance();
            true
        } else {
            false
        };
        match self.peek().clone() {
            Token::Number(n) => {
                self.advance();
                let i = n as isize;
                Ok(if negative { -i } else { i })
            }
            _ => Err(ParseError {
                position: self.current_pos(),
                message: "Expected integer index".into(),
            }),
        }
    }

    /// Parse a positive integer
    fn parse_positive_int(&mut self) -> Result<usize, ParseError> {
        match self.peek().clone() {
            Token::Number(n) => {
                self.advance();
                Ok(n as usize)
            }
            _ => Err(ParseError {
                position: self.current_pos(),
                message: "Expected positive integer".into(),
            }),
        }
    }
}

/// Parse a token stream into an aq expression AST.
pub fn parse(tokens: &[Spanned]) -> Result<Expr, ParseError> {
    let mut parser = Parser::new(tokens);
    let expr = parser.parse_pipe()?;
    if !matches!(parser.peek(), Token::Eof) {
        return Err(ParseError {
            position: parser.current_pos(),
            message: format!("Unexpected token after expression: {:?}", parser.peek()),
        });
    }
    Ok(expr)
}

#[derive(Debug, thiserror::Error)]
#[error("Parse error at position {position}: {message}")]
pub struct ParseError {
    pub position: usize,
    pub message: String,
}

#[path = "parser_tests.rs"]
#[cfg(test)]
mod tests;
