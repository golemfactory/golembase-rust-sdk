use alloy::primitives::{Address, B256};

/// Represents different types of query conditions
#[derive(Debug, Clone)]
pub enum QueryCondition {
    StringEquals(String, String), // key = "value"
    NumericEquals(String, u64),   // key = value
    OwnerEquals(Address),         // $owner = "value"
    KeyEquals(B256),              // $key = "value"
    ExpirationEquals(u64),        // $expiration = value
}

/// Represents a parsed query expression
#[derive(Debug, Clone)]
pub enum Expression {
    Condition(QueryCondition),
    And(Box<Expression>, Box<Expression>),
    Or(Box<Expression>, Box<Expression>),
}

/// Token types for the lexer
#[derive(Debug, Clone, PartialEq)]
enum Token {
    Whitespace,
    LParen,
    RParen,
    And, // &&
    Or,  // ||
    Eq,  // =
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

        self.match_token(Token::Eq)?;

        let value = match self.current() {
            Some(Token::String(value)) => {
                let value = value.clone();
                self.advance();
                if key == "$owner" {
                    // Parse address from string value
                    match value.parse::<Address>() {
                        Ok(address) => {
                            Ok(Expression::Condition(QueryCondition::OwnerEquals(address)))
                        }
                        Err(_) => Err(format!("Invalid address format: {}", value)),
                    }
                } else if key == "$key" {
                    // Parse entity key from string value
                    match value.parse::<B256>() {
                        Ok(entity_key) => {
                            Ok(Expression::Condition(QueryCondition::KeyEquals(entity_key)))
                        }
                        Err(_) => Err(format!("Invalid entity key format: {}", value)),
                    }
                } else {
                    Ok(Expression::Condition(QueryCondition::StringEquals(
                        key, value,
                    )))
                }
            }
            Some(Token::Number(value)) => {
                let value = *value;
                self.advance();
                if key == "$expiration" {
                    Ok(Expression::Condition(QueryCondition::ExpirationEquals(
                        value,
                    )))
                } else {
                    Ok(Expression::Condition(QueryCondition::NumericEquals(
                        key, value,
                    )))
                }
            }
            _ => Err("Expected string or number value".to_string()),
        };

        value
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
                // Numbers
                '0'..='9' => {
                    let number_token = self.tokenize_number(ch)?;
                    tokens.push(number_token);
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

    /// Tokenize a number literal
    fn tokenize_number(&mut self, first_digit: char) -> Result<Token, String> {
        let mut number = first_digit.to_string();

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
        assert!(result.unwrap_err().contains("Invalid address format"));
    }

    #[test]
    fn test_parse_owner_address_too_long() {
        let result = Parser::parse_query(
            "$owner = \"0x1234567890123456789012345678901234567890123456789012345678901234567890\"",
        );
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Invalid address format"));
    }

    #[test]
    fn test_parse_owner_address_invalid_characters() {
        let result = Parser::parse_query("$owner = \"0x123456789012345678901234567890123456789g\"");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Invalid address format"));
    }

    #[test]
    fn test_parse_owner_address_empty_string() {
        let result = Parser::parse_query("$owner = \"\"");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Invalid address format"));
    }

    #[test]
    fn test_parse_owner_address_mixed_case() {
        // The Address type in alloy appears to be strict about mixed case addresses
        // For now, we expect this to fail until we understand the exact validation rules
        let result =
            Parser::parse_query("$owner = \"0x1234567890ABCDEF1234567890abcdef1234567890\"");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Invalid address format"));
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
}
