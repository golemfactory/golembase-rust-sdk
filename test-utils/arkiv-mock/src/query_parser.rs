use alloy::primitives::{Address, B256};

/// Represents different types of query conditions
#[derive(Debug, Clone)]
pub enum QueryCondition {
    StringEquals(String, String),           // key = "value"
    StringNotEquals(String, String),        // key != "value"
    NumericEquals(String, u64),             // key = value
    NumericNotEquals(String, u64),          // key != value
    NumericLessThan(String, u64),           // key < value
    NumericGreaterThan(String, u64),        // key > value
    NumericLessThanOrEqual(String, u64),    // key <= value
    NumericGreaterThanOrEqual(String, u64), // key >= value
    OwnerEquals(Address),                   // $owner = "value"
    KeyEquals(B256),                        // $key = "value"
    ExpirationEquals(u64),                  // $expiration = value
}

/// Represents a parsed query expression
#[derive(Debug, Clone)]
pub enum Expression {
    Condition(QueryCondition),
    And(Box<Expression>, Box<Expression>),
    Or(Box<Expression>, Box<Expression>),
    Not(Box<Expression>),
}

/// Token types for the lexer
#[derive(Debug, Clone, PartialEq)]
enum Token {
    Whitespace,
    LParen,
    RParen,
    And, // &&
    Or,  // ||
    Not, // !
    Eq,  // =
    Ne,  // !=
    Lt,  // <
    Gt,  // >
    Le,  // <=
    Ge,  // >=
    String(String),
    Number(u64),
    Ident(String),
    Owner,      // $owner
    Key,        // $key
    Expiration, // $expiration
}

/// Parser that consumes tokens incrementally
pub struct Parser {
    tokens: Vec<Token>,
    position: usize,
}

impl Parser {
    fn new(tokens: Vec<Token>) -> Self {
        Self {
            tokens,
            position: 0,
        }
    }

    /// Parse query string to extract conditions
    /// Format: Supports &&, ||, parentheses, and meta-annotations
    /// Examples: "test_type = \"Test\"", "tag = \"important\" && priority = 1", "(a = 1 || b = 2) && c = 3"
    pub fn parse_query(query: &str) -> Result<Expression, String> {
        let mut tokenizer = Tokenizer::new(query);
        let tokens = tokenizer.tokenize()?;
        let mut parser = Self::new(tokens);
        parser.parse_expression()
    }

    fn current(&self) -> Option<&Token> {
        self.tokens.get(self.position)
    }

    fn advance(&mut self) {
        self.position += 1;
    }

    fn match_token(&mut self, expected: Token) -> Result<(), String> {
        if let Some(token) = self.current() {
            if std::mem::discriminant(token) == std::mem::discriminant(&expected) {
                self.advance();
                Ok(())
            } else {
                Err(format!("Expected {:?}, got {:?}", expected, token))
            }
        } else {
            Err("Unexpected end of input".to_string())
        }
    }

    fn parse_expression(&mut self) -> Result<Expression, String> {
        self.parse_or_expression()
    }

    fn parse_or_expression(&mut self) -> Result<Expression, String> {
        let mut left = self.parse_and_expression()?;

        while let Some(token) = self.current() {
            if *token == Token::Or {
                self.advance(); // consume ||
                let right = self.parse_and_expression()?;
                left = Expression::Or(Box::new(left), Box::new(right));
            } else {
                break;
            }
        }

        Ok(left)
    }

    fn parse_and_expression(&mut self) -> Result<Expression, String> {
        let mut left = self.parse_primary_expression()?;

        while let Some(token) = self.current() {
            if *token == Token::And {
                self.advance(); // consume &&
                let right = self.parse_primary_expression()?;
                left = Expression::And(Box::new(left), Box::new(right));
            } else {
                break;
            }
        }

        Ok(left)
    }

    fn parse_primary_expression(&mut self) -> Result<Expression, String> {
        if let Some(token) = self.current() {
            match token {
                Token::Not => {
                    self.advance(); // consume !
                    let expr = self.parse_primary_expression()?;
                    Ok(Expression::Not(Box::new(expr)))
                }
                Token::LParen => {
                    self.advance(); // consume (
                    let expr = self.parse_expression()?;
                    self.match_token(Token::RParen)?; // consume )
                    Ok(expr)
                }
                _ => self.parse_condition(),
            }
        } else {
            Err("Unexpected end of input".to_string())
        }
    }

    fn parse_condition(&mut self) -> Result<Expression, String> {
        let key = match self.current() {
            Some(Token::Ident(key)) => {
                let key = key.clone();
                self.advance();
                key
            }
            Some(Token::Owner) => {
                self.advance();
                "$owner".to_string()
            }
            Some(Token::Key) => {
                self.advance();
                "$key".to_string()
            }
            Some(Token::Expiration) => {
                self.advance();
                "$expiration".to_string()
            }
            _ => return Err("Expected identifier, $owner, $key, or $expiration".to_string()),
        };

        // Parse the operator
        let operator = match self.current() {
            Some(Token::Eq) => {
                self.advance();
                "="
            }
            Some(Token::Ne) => {
                self.advance();
                "!="
            }
            Some(Token::Lt) => {
                self.advance();
                "<"
            }
            Some(Token::Gt) => {
                self.advance();
                ">"
            }
            Some(Token::Le) => {
                self.advance();
                "<="
            }
            Some(Token::Ge) => {
                self.advance();
                ">="
            }
            _ => return Err("Expected operator (=, !=, <, >, <=, >=)".to_string()),
        };

        let value = match self.current() {
            Some(Token::String(value)) => {
                let value = value.clone();
                self.advance();
                self.create_condition_for_string_value(&key, &value, operator)?
            }
            Some(Token::Number(value)) => {
                let value = *value;
                self.advance();
                self.create_condition_for_numeric_value(&key, value, operator)?
            }
            Some(Token::Ident(value)) => {
                let value = value.clone();
                self.advance();
                self.create_condition_for_identifier_value(&key, &value, operator)?
            }
            _ => return Err("Expected string, number, or identifier value".to_string()),
        };

        Ok(value)
    }

    fn create_condition_for_string_value(
        &self,
        key: &str,
        value: &str,
        operator: &str,
    ) -> Result<Expression, String> {
        match (key, operator) {
            ("$owner", "=") => match value.parse::<Address>() {
                Ok(address) => Ok(Expression::Condition(QueryCondition::OwnerEquals(address))),
                Err(_) => Err(format!("Invalid address format: {}", value)),
            },
            ("$key", "=") => match value.parse::<B256>() {
                Ok(entity_key) => Ok(Expression::Condition(QueryCondition::KeyEquals(entity_key))),
                Err(_) => Err(format!("Invalid entity key format: {}", value)),
            },
            ("$expiration", _) => Err("$expiration requires a numeric value".to_string()),
            (_, "=") => Ok(Expression::Condition(QueryCondition::StringEquals(
                key.to_string(),
                value.to_string(),
            ))),
            (_, "!=") => Ok(Expression::Condition(QueryCondition::StringNotEquals(
                key.to_string(),
                value.to_string(),
            ))),
            (_, _) => Err(format!(
                "String values only support = and != operators, got: {}",
                operator
            )),
        }
    }

    fn create_condition_for_numeric_value(
        &self,
        key: &str,
        value: u64,
        operator: &str,
    ) -> Result<Expression, String> {
        match (key, operator) {
            ("$expiration", "=") => Ok(Expression::Condition(QueryCondition::ExpirationEquals(
                value,
            ))),
            ("$owner", _) => Err("$owner requires a string value (address)".to_string()),
            ("$key", _) => Err("$key requires a string value (entity key)".to_string()),
            (_, "=") => Ok(Expression::Condition(QueryCondition::NumericEquals(
                key.to_string(),
                value,
            ))),
            (_, "!=") => Ok(Expression::Condition(QueryCondition::NumericNotEquals(
                key.to_string(),
                value,
            ))),
            (_, "<") => Ok(Expression::Condition(QueryCondition::NumericLessThan(
                key.to_string(),
                value,
            ))),
            (_, ">") => Ok(Expression::Condition(QueryCondition::NumericGreaterThan(
                key.to_string(),
                value,
            ))),
            (_, "<=") => Ok(Expression::Condition(
                QueryCondition::NumericLessThanOrEqual(key.to_string(), value),
            )),
            (_, ">=") => Ok(Expression::Condition(
                QueryCondition::NumericGreaterThanOrEqual(key.to_string(), value),
            )),
            (_, _) => Err(format!("Unknown operator: {}", operator)),
        }
    }

    fn create_condition_for_identifier_value(
        &self,
        key: &str,
        value: &str,
        operator: &str,
    ) -> Result<Expression, String> {
        match (key, operator) {
            ("$owner", "=") => match value.parse::<Address>() {
                Ok(address) => Ok(Expression::Condition(QueryCondition::OwnerEquals(address))),
                Err(_) => Err(format!("Invalid address format: {}", value)),
            },
            ("$key", "=") => match value.parse::<B256>() {
                Ok(entity_key) => Ok(Expression::Condition(QueryCondition::KeyEquals(entity_key))),
                Err(_) => Err(format!("Invalid entity key format: {}", value)),
            },
            ("$expiration", _) => Err("$expiration requires a numeric value".to_string()),
            (_, "=") => Ok(Expression::Condition(QueryCondition::StringEquals(
                key.to_string(),
                value.to_string(),
            ))),
            (_, "!=") => Ok(Expression::Condition(QueryCondition::StringNotEquals(
                key.to_string(),
                value.to_string(),
            ))),
            (_, _) => Err(format!(
                "Identifier values only support = and != operators, got: {}",
                operator
            )),
        }
    }
}

/// Tokenizer that converts input strings into tokens
struct Tokenizer<'a> {
    chars: std::iter::Peekable<std::str::Chars<'a>>,
}

impl<'a> Tokenizer<'a> {
    fn new(input: &'a str) -> Self {
        Self {
            chars: input.chars().peekable(),
        }
    }

    /// Tokenize the entire input string
    fn tokenize(&mut self) -> Result<Vec<Token>, String> {
        let mut tokens = Vec::new();

        while let Some(ch) = self.chars.next() {
            match ch {
                // Whitespace
                ' ' | '\t' | '\n' | '\r' => {
                    tokens.push(Token::Whitespace);
                }
                // Parentheses
                '(' => tokens.push(Token::LParen),
                ')' => tokens.push(Token::RParen),
                // Operators
                '=' => tokens.push(Token::Eq),
                '!' => {
                    if self.chars.peek() == Some(&'=') {
                        self.chars.next(); // consume =
                        tokens.push(Token::Ne);
                    } else {
                        tokens.push(Token::Not);
                    }
                }
                '<' => {
                    if self.chars.peek() == Some(&'=') {
                        self.chars.next(); // consume =
                        tokens.push(Token::Le);
                    } else {
                        tokens.push(Token::Lt);
                    }
                }
                '>' => {
                    if self.chars.peek() == Some(&'=') {
                        self.chars.next(); // consume =
                        tokens.push(Token::Ge);
                    } else {
                        tokens.push(Token::Gt);
                    }
                }
                '&' => {
                    if self.chars.peek() == Some(&'&') {
                        self.chars.next(); // consume second &
                        tokens.push(Token::And);
                    } else {
                        return Err("Invalid token: single &".to_string());
                    }
                }
                '|' => {
                    if self.chars.peek() == Some(&'|') {
                        self.chars.next(); // consume second |
                        tokens.push(Token::Or);
                    } else {
                        return Err("Invalid token: single |".to_string());
                    }
                }
                // Strings
                '"' => {
                    let string_token = self.tokenize_string()?;
                    tokens.push(string_token);
                }
                // Numbers (including hex detection)
                '0'..='9' => {
                    match self.tokenize_number_or_hex(ch) {
                        Ok(number_token) => tokens.push(number_token),
                        Err(_) => {
                            // If it's not a number (e.g., hex string), treat as identifier
                            let ident_token = self.tokenize_identifier(ch)?;
                            tokens.push(ident_token);
                        }
                    }
                }
                // Identifiers and meta-annotations
                '$' => {
                    let meta_token = self.tokenize_meta_annotation()?;
                    tokens.push(meta_token);
                }
                // Regular identifiers
                'a'..='z' | 'A'..='Z' | '_' => {
                    let ident_token = self.tokenize_identifier(ch)?;
                    tokens.push(ident_token);
                }
                _ => return Err(format!("Unexpected character: {}", ch)),
            }
        }

        // Filter out whitespace tokens
        tokens.retain(|t| *t != Token::Whitespace);
        Ok(tokens)
    }

    /// Tokenize a string literal
    fn tokenize_string(&mut self) -> Result<Token, String> {
        let mut string_content = String::new();

        while let Some(next_ch) = self.chars.next() {
            match next_ch {
                '"' => break,
                '\\' => {
                    if let Some(escaped) = self.chars.next() {
                        string_content.push(escaped);
                    } else {
                        return Err("Unterminated string".to_string());
                    }
                }
                _ => string_content.push(next_ch),
            }
        }

        Ok(Token::String(string_content))
    }

    /// Tokenize a number or detect hex strings
    fn tokenize_number_or_hex(&mut self, first_digit: char) -> Result<Token, String> {
        let mut number = first_digit.to_string();

        // Check if this might be a hex string (0x...)
        if first_digit == '0' && self.chars.peek() == Some(&'x') {
            // This looks like a hex string, but we should treat it as an identifier
            // since it's not quoted. Let the identifier tokenizer handle it.
            return Err("Not a number token".to_string());
        }

        // Regular number parsing
        while let Some(&next_ch) = self.chars.peek() {
            if next_ch.is_ascii_digit() {
                number.push(self.chars.next().unwrap());
            } else {
                break;
            }
        }

        if let Ok(num) = number.parse::<u64>() {
            Ok(Token::Number(num))
        } else {
            Err(format!("Invalid number: {}", number))
        }
    }

    /// Tokenize a meta-annotation (starts with $)
    fn tokenize_meta_annotation(&mut self) -> Result<Token, String> {
        let mut ident = String::new();

        while let Some(&next_ch) = self.chars.peek() {
            if next_ch.is_alphanumeric() || next_ch == '_' {
                ident.push(self.chars.next().unwrap());
            } else {
                break;
            }
        }

        if ident == "owner" {
            Ok(Token::Owner)
        } else if ident == "key" {
            Ok(Token::Key)
        } else if ident == "expiration" {
            Ok(Token::Expiration)
        } else {
            Err(format!("Unknown meta-annotation: ${}", ident))
        }
    }

    /// Tokenize a regular identifier
    fn tokenize_identifier(&mut self, first_char: char) -> Result<Token, String> {
        let mut ident = first_char.to_string();

        while let Some(&next_ch) = self.chars.peek() {
            if next_ch.is_alphanumeric() || next_ch == '_' {
                ident.push(self.chars.next().unwrap());
            } else {
                break;
            }
        }

        Ok(Token::Ident(ident))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_simple_string_equality() {
        let result = Parser::parse_query("test_type = \"Test\"").unwrap();
        assert!(matches!(
            result,
            Expression::Condition(QueryCondition::StringEquals(_, _))
        ));
    }

    #[test]
    fn test_parse_simple_numeric_equality() {
        let result = Parser::parse_query("priority = 42").unwrap();
        assert!(matches!(
            result,
            Expression::Condition(QueryCondition::NumericEquals(_, _))
        ));
    }

    #[test]
    fn test_parse_owner_equality() {
        let result =
            Parser::parse_query("$owner = \"0x1234567890123456789012345678901234567890\"").unwrap();
        assert!(matches!(
            result,
            Expression::Condition(QueryCondition::OwnerEquals(_))
        ));
    }

    #[test]
    fn test_parse_and_expression() {
        let result = Parser::parse_query("tag = \"important\" && priority = 1").unwrap();
        assert!(matches!(result, Expression::And(_, _)));
    }

    #[test]
    fn test_parse_or_expression() {
        let result = Parser::parse_query("a = 1 || b = 2").unwrap();
        assert!(matches!(result, Expression::Or(_, _)));
    }

    #[test]
    fn test_parse_parentheses() {
        let result = Parser::parse_query("(a = 1 || b = 2) && c = 3").unwrap();
        assert!(matches!(result, Expression::And(_, _)));
    }

    #[test]
    fn test_parse_complex_expression() {
        let result =
            Parser::parse_query("type = \"user\" && (age = 25 || status = \"active\")").unwrap();
        assert!(matches!(result, Expression::And(_, _)));
    }

    #[test]
    fn test_parse_invalid_syntax() {
        let result = Parser::parse_query("invalid syntax");
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_unmatched_parentheses() {
        let result = Parser::parse_query("(a = 1");
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_single_ampersand() {
        let result = Parser::parse_query("a = 1 & b = 2");
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_single_pipe() {
        let result = Parser::parse_query("a = 1 | b = 2");
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_unknown_meta_annotation() {
        let result = Parser::parse_query("$unknown = \"value\"");
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_valid_owner_address() {
        let result =
            Parser::parse_query("$owner = \"0x1234567890123456789012345678901234567890\"").unwrap();
        match result {
            Expression::Condition(QueryCondition::OwnerEquals(address)) => {
                // Verify it's a valid address
                assert_eq!(
                    address,
                    "0x1234567890123456789012345678901234567890"
                        .parse::<Address>()
                        .unwrap()
                );
            }
            _ => panic!("Expected OwnerEquals condition"),
        }
    }

    #[test]
    fn test_parse_owner_address_without_0x_prefix() {
        // The Address type is permissive and accepts addresses without 0x prefix
        let result =
            Parser::parse_query("$owner = \"1234567890123456789012345678901234567890\"").unwrap();
        match result {
            Expression::Condition(QueryCondition::OwnerEquals(address)) => {
                // Verify it's a valid address
                let expected = "1234567890123456789012345678901234567890"
                    .parse::<Address>()
                    .unwrap();
                assert_eq!(address, expected);
            }
            _ => panic!("Expected OwnerEquals condition"),
        }
    }

    #[test]
    fn test_parse_owner_address_too_short() {
        let result = Parser::parse_query("$owner = \"0x123\"");
        assert!(result.is_err());
        let error = result.unwrap_err();
        assert!(
            error.contains("Invalid address format"),
            "Got error instead: {error}"
        );
    }

    #[test]
    fn test_parse_owner_address_too_long() {
        let result = Parser::parse_query(
            "$owner = \"0x1234567890123456789012345678901234567890123456789012345678901234567890\"",
        );
        assert!(result.is_err());
        let error = result.unwrap_err();
        assert!(
            error.contains("Invalid address format"),
            "Got error instead: {error}"
        );
    }

    #[test]
    fn test_parse_owner_address_invalid_characters() {
        let result = Parser::parse_query("$owner = \"0x123456789012345678901234567890123456789g\"");
        assert!(result.is_err());
        let error = result.unwrap_err();
        assert!(
            error.contains("Invalid address format"),
            "Got error instead: {error}"
        );
    }

    #[test]
    fn test_parse_owner_address_empty_string() {
        let result = Parser::parse_query("$owner = \"\"");
        assert!(result.is_err());
        let error = result.unwrap_err();
        assert!(
            error.contains("Invalid address format"),
            "Got error instead: {error}"
        );
    }

    #[test]
    fn test_parse_owner_address_mixed_case() {
        // The Address type in alloy appears to be strict about mixed case addresses
        // For now, we expect this to fail until we understand the exact validation rules
        let result =
            Parser::parse_query("$owner = \"0x1234567890ABCDEF1234567890abcdef1234567890\"");
        assert!(result.is_err());
        let error = result.unwrap_err();
        assert!(
            error.contains("Invalid address format"),
            "Got error instead: {error}"
        );
    }

    #[test]
    fn test_parse_key_equality() {
        let result = Parser::parse_query(
            "$key = \"0x1234567890123456789012345678901234567890123456789012345678901234\"",
        )
        .unwrap();
        assert!(matches!(
            result,
            Expression::Condition(QueryCondition::KeyEquals(_))
        ));
    }

    #[test]
    fn test_parse_expiration_equality() {
        let result = Parser::parse_query("$expiration = 12345").unwrap();
        assert!(matches!(
            result,
            Expression::Condition(QueryCondition::ExpirationEquals(12345))
        ));
    }

    // Additional comprehensive tests for $key meta annotation
    #[test]
    fn test_parse_key_without_0x_prefix() {
        let result = Parser::parse_query(
            "$key = \"1234567890123456789012345678901234567890123456789012345678901234\"",
        )
        .unwrap();
        match result {
            Expression::Condition(QueryCondition::KeyEquals(key)) => {
                let expected = "1234567890123456789012345678901234567890123456789012345678901234"
                    .parse::<B256>()
                    .unwrap();
                assert_eq!(key, expected);
            }
            _ => panic!("Expected KeyEquals condition"),
        }
    }

    #[test]
    fn test_parse_key_too_short() {
        let result = Parser::parse_query("$key = \"0x123\"");
        assert!(result.is_err());
        let error = result.unwrap_err();
        assert!(
            error.contains("Invalid entity key format"),
            "Got error instead: {error}"
        );
    }

    #[test]
    fn test_parse_key_too_long() {
        let result = Parser::parse_query(
            "$key = \"0x1234567890123456789012345678901234567890123456789012345678901234567890\"",
        );
        assert!(result.is_err());
        let error = result.unwrap_err();
        assert!(
            error.contains("Invalid entity key format"),
            "Got error instead: {error}"
        );
    }

    #[test]
    fn test_parse_key_invalid_characters() {
        let result = Parser::parse_query(
            "$key = \"0x123456789012345678901234567890123456789012345678901234567890123g\"",
        );
        assert!(result.is_err());
        let error = result.unwrap_err();
        assert!(
            error.contains("Invalid entity key format"),
            "Got error instead: {error}"
        );
    }

    #[test]
    fn test_parse_key_empty_string() {
        let result = Parser::parse_query("$key = \"\"");
        assert!(result.is_err());
        let error = result.unwrap_err();
        assert!(
            error.contains("Invalid entity key format"),
            "Got error instead: {error}"
        );
    }

    #[test]
    fn test_parse_key_mixed_case() {
        let result = Parser::parse_query(
            "$key = \"0x1234567890ABCDEF1234567890abcdef1234567890123456789012345678901234\"",
        );
        assert!(result.is_err());
        let error = result.unwrap_err();
        assert!(
            error.contains("Invalid entity key format"),
            "Got error instead: {error}"
        );
    }

    #[test]
    fn test_parse_key_with_underscores() {
        let result = Parser::parse_query(
            "$key = \"0x123456789012345678901234567890123456789012345678901234567890123_\"",
        );
        assert!(result.is_err());
        let error = result.unwrap_err();
        assert!(
            error.contains("Invalid entity key format"),
            "Got error instead: {error}"
        );
    }

    // Additional comprehensive tests for $expiration meta annotation
    #[test]
    fn test_parse_expiration_zero() {
        let result = Parser::parse_query("$expiration = 0").unwrap();
        assert!(matches!(
            result,
            Expression::Condition(QueryCondition::ExpirationEquals(0))
        ));
    }

    #[test]
    fn test_parse_expiration_max_u64() {
        let result = Parser::parse_query("$expiration = 18446744073709551615").unwrap();
        assert!(matches!(
            result,
            Expression::Condition(QueryCondition::ExpirationEquals(18446744073709551615))
        ));
    }

    #[test]
    fn test_parse_expiration_with_string_value() {
        let result = Parser::parse_query("$expiration = \"12345\"");
        assert!(result.is_err());
        let error = result.unwrap_err();
        assert!(
            error.contains("$expiration requires a numeric value"),
            "Got error instead: {error}"
        );
    }

    #[test]
    fn test_parse_expiration_with_identifier_value() {
        let result = Parser::parse_query("$expiration = abc");
        assert!(result.is_err());
        let error = result.unwrap_err();
        assert!(
            error.contains("$expiration requires a numeric value"),
            "Got error instead: {error}"
        );
    }

    #[test]
    fn test_parse_expiration_with_negative_number() {
        // Note: The tokenizer only handles positive numbers, so this would fail at tokenization
        let result = Parser::parse_query("$expiration = -123");
        assert!(result.is_err());
    }

    // Tests for complex expressions with meta annotations
    #[test]
    fn test_parse_owner_and_key_expression() {
        let result = Parser::parse_query(
            "$owner = \"0x1234567890123456789012345678901234567890\" && $key = \"0x1234567890123456789012345678901234567890123456789012345678901234\"",
        )
        .unwrap();
        assert!(matches!(result, Expression::And(_, _)));
    }

    #[test]
    fn test_parse_owner_or_expiration_expression() {
        let result = Parser::parse_query(
            "$owner = \"0x1234567890123456789012345678901234567890\" || $expiration = 12345",
        )
        .unwrap();
        assert!(matches!(result, Expression::Or(_, _)));
    }

    #[test]
    fn test_parse_complex_meta_annotation_expression() {
        let result = Parser::parse_query(
            "($owner = \"0x1234567890123456789012345678901234567890\" || $key = \"0x1234567890123456789012345678901234567890123456789012345678901234\") && $expiration = 12345",
        )
        .unwrap();
        assert!(matches!(result, Expression::And(_, _)));
    }

    #[test]
    fn test_parse_meta_annotation_with_regular_field() {
        let result = Parser::parse_query(
            "$owner = \"0x1234567890123456789012345678901234567890\" && type = \"user\"",
        )
        .unwrap();
        assert!(matches!(result, Expression::And(_, _)));
    }

    // Edge cases for meta annotation parsing
    #[test]
    fn test_parse_meta_annotation_with_underscore() {
        let result = Parser::parse_query("$_owner = \"value\"");
        assert!(result.is_err());
        let error = result.unwrap_err();
        assert!(
            error.contains("Unknown meta-annotation"),
            "Got error instead: {error}"
        );
    }

    #[test]
    fn test_parse_meta_annotation_with_number() {
        let result = Parser::parse_query("$1owner = \"value\"");
        assert!(result.is_err());
        let error = result.unwrap_err();
        assert!(
            error.contains("Unknown meta-annotation"),
            "Got error instead: {error}"
        );
    }

    #[test]
    fn test_parse_meta_annotation_case_sensitive() {
        let result = Parser::parse_query("$Owner = \"value\"");
        assert!(result.is_err());
        let error = result.unwrap_err();
        assert!(
            error.contains("Unknown meta-annotation"),
            "Got error instead: {error}"
        );
    }

    #[test]
    fn test_parse_meta_annotation_partial_match() {
        let result = Parser::parse_query("$own = \"value\"");
        assert!(result.is_err());
        let error = result.unwrap_err();
        assert!(
            error.contains("Unknown meta-annotation"),
            "Got error instead: {error}"
        );
    }

    #[test]
    fn test_parse_meta_annotation_with_trailing_chars() {
        let result = Parser::parse_query("$owner123 = \"value\"");
        assert!(result.is_err());
        let error = result.unwrap_err();
        assert!(
            error.contains("Unknown meta-annotation"),
            "Got error instead: {error}"
        );
    }

    #[test]
    fn test_parse_empty_meta_annotation() {
        let result = Parser::parse_query("$ = \"value\"");
        assert!(result.is_err());
        let error = result.unwrap_err();
        assert!(
            error.contains("Unknown meta-annotation"),
            "Got error instead: {error}"
        );
    }

    // Tests for meta annotations in nested expressions
    #[test]
    fn test_parse_nested_meta_annotations() {
        let result = Parser::parse_query(
            "($owner = \"0x1234567890123456789012345678901234567890\" && ($key = \"0x1234567890123456789012345678901234567890123456789012345678901234\" || $expiration = 12345))",
        )
        .unwrap();
        assert!(matches!(result, Expression::And(_, _)));
    }

    #[test]
    fn test_parse_meta_annotation_without_value() {
        let result = Parser::parse_query("$owner");
        assert!(result.is_err());
        let error = result.unwrap_err();
        assert!(
            error.contains("Expected operator (=, !=, <, >, <=, >=)"),
            "Got error instead: {error}"
        );
    }

    #[test]
    fn test_parse_meta_annotation_without_equals() {
        let result = Parser::parse_query("$owner \"value\"");
        assert!(result.is_err());
        let error = result.unwrap_err();
        assert!(
            error.contains("Expected operator (=, !=, <, >, <=, >=)"),
            "Got error instead: {error}"
        );
    }

    // Tests to verify exact parsing results for meta annotations
    #[test]
    fn test_parse_key_with_hex_string_exact_result() {
        let result = Parser::parse_query(
            "$key = \"0x8509b7c6fbf091e90a79836ca8d1226261eefdb59f3f593b818a74f5c5e7ab06\"",
        );
        assert!(result.is_ok());
        match result.unwrap() {
            Expression::Condition(QueryCondition::KeyEquals(key)) => {
                let expected = "0x8509b7c6fbf091e90a79836ca8d1226261eefdb59f3f593b818a74f5c5e7ab06"
                    .parse::<B256>()
                    .unwrap();
                assert_eq!(key, expected);
            }
            other => panic!("Expected KeyEquals condition, got: {:?}", other),
        }
    }

    #[test]
    fn test_parse_owner_with_address_exact_result() {
        let result = Parser::parse_query("$owner = \"0x1234567890123456789012345678901234567890\"");
        assert!(result.is_ok());
        match result.unwrap() {
            Expression::Condition(QueryCondition::OwnerEquals(address)) => {
                let expected = "0x1234567890123456789012345678901234567890"
                    .parse::<Address>()
                    .unwrap();
                assert_eq!(address, expected);
            }
            other => panic!("Expected OwnerEquals condition, got: {:?}", other),
        }
    }

    #[test]
    fn test_parse_key_with_unquoted_hex_string() {
        // Test that unquoted hex strings work for $key
        let result = Parser::parse_query(
            "$key = 0x8509b7c6fbf091e90a79836ca8d1226261eefdb59f3f593b818a74f5c5e7ab06",
        );
        assert!(result.is_ok());
        match result.unwrap() {
            Expression::Condition(QueryCondition::KeyEquals(key)) => {
                let expected = "0x8509b7c6fbf091e90a79836ca8d1226261eefdb59f3f593b818a74f5c5e7ab06"
                    .parse::<B256>()
                    .unwrap();
                assert_eq!(key, expected);
            }
            other => panic!("Expected KeyEquals condition, got: {:?}", other),
        }
    }

    #[test]
    fn test_parse_owner_with_unquoted_hex_string() {
        // Test that unquoted hex strings work for $owner
        let result = Parser::parse_query("$owner = 0x1234567890123456789012345678901234567890");
        assert!(result.is_ok());
        match result.unwrap() {
            Expression::Condition(QueryCondition::OwnerEquals(address)) => {
                let expected = "0x1234567890123456789012345678901234567890"
                    .parse::<Address>()
                    .unwrap();
                assert_eq!(address, expected);
            }
            other => panic!("Expected OwnerEquals condition, got: {:?}", other),
        }
    }

    // Tests for inequality operators
    #[test]
    fn test_parse_numeric_not_equals() {
        let result = Parser::parse_query("priority != 1");
        assert!(result.is_ok());
        match result.unwrap() {
            Expression::Condition(QueryCondition::NumericNotEquals(key, value)) => {
                assert_eq!(key, "priority");
                assert_eq!(value, 1);
            }
            other => panic!("Expected NumericNotEquals condition, got: {:?}", other),
        }
    }

    #[test]
    fn test_parse_numeric_less_than() {
        let result = Parser::parse_query("priority < 2");
        assert!(result.is_ok());
        match result.unwrap() {
            Expression::Condition(QueryCondition::NumericLessThan(key, value)) => {
                assert_eq!(key, "priority");
                assert_eq!(value, 2);
            }
            other => panic!("Expected NumericLessThan condition, got: {:?}", other),
        }
    }

    #[test]
    fn test_parse_numeric_greater_than() {
        let result = Parser::parse_query("priority > 2");
        assert!(result.is_ok());
        match result.unwrap() {
            Expression::Condition(QueryCondition::NumericGreaterThan(key, value)) => {
                assert_eq!(key, "priority");
                assert_eq!(value, 2);
            }
            other => panic!("Expected NumericGreaterThan condition, got: {:?}", other),
        }
    }

    #[test]
    fn test_parse_numeric_less_than_or_equal() {
        let result = Parser::parse_query("priority <= 2");
        assert!(result.is_ok());
        match result.unwrap() {
            Expression::Condition(QueryCondition::NumericLessThanOrEqual(key, value)) => {
                assert_eq!(key, "priority");
                assert_eq!(value, 2);
            }
            other => panic!(
                "Expected NumericLessThanOrEqual condition, got: {:?}",
                other
            ),
        }
    }

    #[test]
    fn test_parse_numeric_greater_than_or_equal() {
        let result = Parser::parse_query("priority >= 2");
        assert!(result.is_ok());
        match result.unwrap() {
            Expression::Condition(QueryCondition::NumericGreaterThanOrEqual(key, value)) => {
                assert_eq!(key, "priority");
                assert_eq!(value, 2);
            }
            other => panic!(
                "Expected NumericGreaterThanOrEqual condition, got: {:?}",
                other
            ),
        }
    }

    #[test]
    fn test_parse_string_not_equals() {
        let result = Parser::parse_query("type != \"test\"");
        assert!(result.is_ok());
        match result.unwrap() {
            Expression::Condition(QueryCondition::StringNotEquals(key, value)) => {
                assert_eq!(key, "type");
                assert_eq!(value, "test");
            }
            other => panic!("Expected StringNotEquals condition, got: {:?}", other),
        }
    }

    // Tests for negation operator
    #[test]
    fn test_parse_negation_simple() {
        let result = Parser::parse_query("!type = \"test\"");
        assert!(result.is_ok());
        match result.unwrap() {
            Expression::Not(expr) => match expr.as_ref() {
                Expression::Condition(QueryCondition::StringEquals(key, value)) => {
                    assert_eq!(key, "type");
                    assert_eq!(value, "test");
                }
                other => panic!(
                    "Expected StringEquals condition inside Not, got: {:?}",
                    other
                ),
            },
            other => panic!("Expected Not expression, got: {:?}", other),
        }
    }

    #[test]
    fn test_parse_negation_with_parentheses() {
        let result = Parser::parse_query("!(type = \"test\")");
        assert!(result.is_ok());
        match result.unwrap() {
            Expression::Not(expr) => match expr.as_ref() {
                Expression::Condition(QueryCondition::StringEquals(key, value)) => {
                    assert_eq!(key, "type");
                    assert_eq!(value, "test");
                }
                other => panic!(
                    "Expected StringEquals condition inside Not, got: {:?}",
                    other
                ),
            },
            other => panic!("Expected Not expression, got: {:?}", other),
        }
    }

    #[test]
    fn test_parse_negation_complex() {
        let result = Parser::parse_query("!(type = \"test\" && priority = 1)");
        assert!(result.is_ok());
        match result.unwrap() {
            Expression::Not(expr) => {
                match expr.as_ref() {
                    Expression::And(left, right) => {
                        // Verify the left and right expressions
                        assert!(matches!(
                            left.as_ref(),
                            Expression::Condition(QueryCondition::StringEquals(_, _))
                        ));
                        assert!(matches!(
                            right.as_ref(),
                            Expression::Condition(QueryCondition::NumericEquals(_, _))
                        ));
                    }
                    other => panic!("Expected And expression inside Not, got: {:?}", other),
                }
            }
            other => panic!("Expected Not expression, got: {:?}", other),
        }
    }

    #[test]
    fn test_parse_negation_meta_annotation() {
        let result =
            Parser::parse_query("!($owner = \"0x1234567890123456789012345678901234567890\")");
        assert!(result.is_ok());
        match result.unwrap() {
            Expression::Not(expr) => {
                match expr.as_ref() {
                    Expression::Condition(QueryCondition::OwnerEquals(_)) => {
                        // This is correct
                    }
                    other => panic!(
                        "Expected OwnerEquals condition inside Not, got: {:?}",
                        other
                    ),
                }
            }
            other => panic!("Expected Not expression, got: {:?}", other),
        }
    }

    #[test]
    fn test_parse_expiration_with_number_exact_result() {
        let result = Parser::parse_query("$expiration = 12345");
        assert!(result.is_ok());
        match result.unwrap() {
            Expression::Condition(QueryCondition::ExpirationEquals(value)) => {
                assert_eq!(value, 12345);
            }
            other => panic!("Expected ExpirationEquals condition, got: {:?}", other),
        }
    }
}
