// Parser for complex filter expressions with nested conditions
// Supports expressions like: (A=B AND (C=D OR E=F) AND X=Y)

use crate::models::{ConditionNode, ConditionTree, FilterOperator, LogicalOperator};
use anyhow::{anyhow, Result};

#[derive(Debug, Clone)]
pub struct FilterParser {
    operators: Vec<String>,
    logical_operators: Vec<String>,
}

impl FilterParser {
    pub fn new() -> Self {
        Self {
            operators: vec![
                "contains".to_string(),
                "equals".to_string(),
                "matches".to_string(),
                "starts_with".to_string(),
                "ends_with".to_string(),
                "not_contains".to_string(),
                "not_equals".to_string(),
                "not_matches".to_string(),
            ],
            logical_operators: vec![
                "AND".to_string(),
                "OR".to_string(),
                "ALL".to_string(),
                "ANY".to_string(),
            ],
        }
    }

    /// Parse a text expression into a ConditionTree
    /// Example: "channel_name contains \"sport\" AND (group_title equals \"HD\" OR group_title equals \"4K\")"
    pub fn parse(&self, expression: &str) -> Result<ConditionTree> {
        let tokens = self.tokenize(expression)?;
        let root = self.parse_expression(&tokens, &mut 0)?;
        Ok(ConditionTree { root })
    }

    /// Tokenize the input string into components
    fn tokenize(&self, expression: &str) -> Result<Vec<Token>> {
        let mut tokens = Vec::new();
        let mut current_pos = 0;
        let expr = expression.trim();

        while current_pos < expr.len() {
            let remaining = &expr[current_pos..];

            // Skip whitespace
            if remaining.starts_with(char::is_whitespace) {
                current_pos += 1;
                continue;
            }

            // Handle parentheses
            if remaining.starts_with('(') {
                tokens.push(Token::LeftParen);
                current_pos += 1;
                continue;
            }
            if remaining.starts_with(')') {
                tokens.push(Token::RightParen);
                current_pos += 1;
                continue;
            }

            // Handle quoted strings
            if remaining.starts_with('"') || remaining.starts_with('\'') {
                let quote_char = remaining.chars().next().unwrap();
                let end_pos = remaining[1..]
                    .find(quote_char)
                    .ok_or_else(|| anyhow!("Unmatched quote at position {}", current_pos))?;
                let value = remaining[1..end_pos + 1].to_string();
                tokens.push(Token::Value(value));
                current_pos += end_pos + 2;
                continue;
            }

            // Handle logical operators (AND, OR, ALL, ANY)
            let mut found_logical = false;
            for logical_op in &self.logical_operators {
                if remaining.to_uppercase().starts_with(logical_op) {
                    // Check that it's a whole word
                    let end_pos = logical_op.len();
                    if end_pos == remaining.len()
                        || remaining
                            .chars()
                            .nth(end_pos)
                            .map_or(true, |c| c.is_whitespace() || c == '(' || c == ')')
                    {
                        let operator = match logical_op.as_str() {
                            "AND" | "ALL" => LogicalOperator::All,
                            "OR" | "ANY" => LogicalOperator::Any,
                            _ => return Err(anyhow!("Unknown logical operator: {}", logical_op)),
                        };
                        tokens.push(Token::LogicalOp(operator));
                        current_pos += end_pos;
                        found_logical = true;
                        break;
                    }
                }
            }
            if found_logical {
                continue;
            }

            // Handle modifiers first
            if remaining.to_uppercase().starts_with("NOT") {
                let end_pos = 3;
                if end_pos == remaining.len()
                    || remaining
                        .chars()
                        .nth(end_pos)
                        .map_or(true, |c| c.is_whitespace())
                {
                    tokens.push(Token::Modifier("not".to_string()));
                    current_pos += end_pos;
                    continue;
                }
            }

            if remaining.to_uppercase().starts_with("CASE_SENSITIVE") {
                let end_pos = 14;
                if end_pos == remaining.len()
                    || remaining
                        .chars()
                        .nth(end_pos)
                        .map_or(true, |c| c.is_whitespace())
                {
                    tokens.push(Token::Modifier("case_sensitive".to_string()));
                    current_pos += end_pos;
                    continue;
                }
            }

            // Handle filter operators
            let mut found_operator = false;
            for op in &self.operators {
                if remaining.starts_with(op) {
                    // Check that it's a whole word
                    let end_pos = op.len();
                    if end_pos == remaining.len()
                        || remaining
                            .chars()
                            .nth(end_pos)
                            .map_or(true, |c| c.is_whitespace() || c == '"' || c == '\'')
                    {
                        let filter_op = match op.as_str() {
                            "contains" => FilterOperator::Contains,
                            "equals" => FilterOperator::Equals,
                            "matches" => FilterOperator::Matches,
                            "starts_with" => FilterOperator::StartsWith,
                            "ends_with" => FilterOperator::EndsWith,
                            "not_contains" => FilterOperator::NotContains,
                            "not_equals" => FilterOperator::NotEquals,
                            "not_matches" => FilterOperator::NotMatches,
                            _ => return Err(anyhow!("Unknown filter operator: {}", op)),
                        };
                        tokens.push(Token::Operator(filter_op));
                        current_pos += end_pos;
                        found_operator = true;
                        break;
                    }
                }
            }

            if found_operator {
                continue;
            }

            // Handle field names (anything else that's not whitespace or special chars)
            let word_end = remaining
                .find(|c: char| c.is_whitespace() || c == '(' || c == ')' || c == '"' || c == '\'')
                .unwrap_or(remaining.len());

            if word_end > 0 {
                let word = remaining[..word_end].to_string();
                tokens.push(Token::Field(word));
                current_pos += word_end;
            } else {
                return Err(anyhow!("Unexpected character at position {}", current_pos));
            }
        }

        Ok(tokens)
    }

    /// Parse a complete expression
    fn parse_expression(&self, tokens: &[Token], pos: &mut usize) -> Result<ConditionNode> {
        // Parse the left-hand side
        let left = self.parse_term(tokens, pos)?;

        // Check if there's a logical operator
        if *pos < tokens.len() {
            if let Token::LogicalOp(op) = &tokens[*pos] {
                let operator = op.clone();
                *pos += 1;

                let mut children = vec![left];

                // Parse the right side
                let right = self.parse_expression(tokens, pos)?;
                children.push(right);

                // Handle multiple conditions with the same operator
                while *pos < tokens.len() {
                    if let Token::LogicalOp(next_op) = &tokens[*pos] {
                        if std::mem::discriminant(next_op) == std::mem::discriminant(&operator) {
                            *pos += 1;
                            let next_term = self.parse_expression(tokens, pos)?;
                            children.push(next_term);
                        } else {
                            break;
                        }
                    } else {
                        break;
                    }
                }

                return Ok(ConditionNode::Group { operator, children });
            }
        }

        Ok(left)
    }

    /// Parse a term (either a condition or a parenthesized expression)
    fn parse_term(&self, tokens: &[Token], pos: &mut usize) -> Result<ConditionNode> {
        if *pos >= tokens.len() {
            return Err(anyhow!("Unexpected end of expression"));
        }

        match &tokens[*pos] {
            Token::LeftParen => {
                *pos += 1; // consume '('
                let node = self.parse_expression(tokens, pos)?;

                if *pos >= tokens.len() || !matches!(tokens[*pos], Token::RightParen) {
                    return Err(anyhow!("Missing closing parenthesis"));
                }
                *pos += 1; // consume ')'
                Ok(node)
            }
            Token::Field(field) => {
                let field = field.clone();
                *pos += 1;

                // Parse modifiers (not, case_sensitive) before operator
                let mut negate = false;
                let mut case_sensitive = false;

                while *pos < tokens.len() {
                    match &tokens[*pos] {
                        Token::Modifier(modifier) => {
                            match modifier.as_str() {
                                "not" => {
                                    if negate {
                                        return Err(anyhow!("Duplicate 'not' modifier"));
                                    }
                                    negate = true;
                                }
                                "case_sensitive" => {
                                    if case_sensitive {
                                        return Err(anyhow!("Duplicate 'case_sensitive' modifier"));
                                    }
                                    case_sensitive = true;
                                }
                                _ => return Err(anyhow!("Unknown modifier: {}", modifier)),
                            }
                            *pos += 1;
                        }
                        _ => break, // Not a modifier, continue to operator parsing
                    }
                }

                if *pos >= tokens.len() {
                    return Err(anyhow!("Expected operator after field '{}'", field));
                }

                let operator = match &tokens[*pos] {
                    Token::Operator(op) => op.clone(),
                    _ => return Err(anyhow!("Expected operator after field '{}'", field)),
                };
                *pos += 1;

                if *pos >= tokens.len() {
                    return Err(anyhow!("Expected value after operator"));
                }

                let value = match &tokens[*pos] {
                    Token::Value(val) => val.clone(),
                    _ => return Err(anyhow!("Expected value after operator")),
                };
                *pos += 1;

                Ok(ConditionNode::Condition {
                    field,
                    operator,
                    value,
                    case_sensitive,
                    negate,
                })
            }
            Token::Modifier(_) => {
                // Handle cases where modifiers come before field name
                let mut negate = false;
                let mut case_sensitive = false;

                // Parse all modifiers first
                while *pos < tokens.len() {
                    match &tokens[*pos] {
                        Token::Modifier(modifier) => {
                            match modifier.as_str() {
                                "not" => {
                                    if negate {
                                        return Err(anyhow!("Duplicate 'not' modifier"));
                                    }
                                    negate = true;
                                }
                                "case_sensitive" => {
                                    if case_sensitive {
                                        return Err(anyhow!("Duplicate 'case_sensitive' modifier"));
                                    }
                                    case_sensitive = true;
                                }
                                _ => return Err(anyhow!("Unknown modifier: {}", modifier)),
                            }
                            *pos += 1;
                        }
                        _ => break, // Not a modifier, continue to field parsing
                    }
                }

                if *pos >= tokens.len() {
                    return Err(anyhow!("Expected field name after modifiers"));
                }

                let field = match &tokens[*pos] {
                    Token::Field(field) => field.clone(),
                    _ => return Err(anyhow!("Expected field name after modifiers")),
                };
                *pos += 1;

                if *pos >= tokens.len() {
                    return Err(anyhow!("Expected operator after field '{}'", field));
                }

                let operator = match &tokens[*pos] {
                    Token::Operator(op) => op.clone(),
                    _ => return Err(anyhow!("Expected operator after field '{}'", field)),
                };
                *pos += 1;

                if *pos >= tokens.len() {
                    return Err(anyhow!("Expected value after operator"));
                }

                let value = match &tokens[*pos] {
                    Token::Value(val) => val.clone(),
                    _ => return Err(anyhow!("Expected value after operator")),
                };
                *pos += 1;

                Ok(ConditionNode::Condition {
                    field,
                    operator,
                    value,
                    case_sensitive,
                    negate,
                })
            }
            _ => Err(anyhow!(
                "Expected field name, modifier, or opening parenthesis"
            )),
        }
    }
}

#[derive(Debug, Clone)]
enum Token {
    Field(String),
    Operator(FilterOperator),
    Value(String),
    LogicalOp(LogicalOperator),
    Modifier(String),
    LeftParen,
    RightParen,
}

impl Default for FilterParser {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_simple_condition() {
        let parser = FilterParser::new();
        let result = parser.parse("channel_name contains \"sport\"").unwrap();

        match result.root {
            ConditionNode::Condition {
                field,
                operator,
                value,
                case_sensitive,
                negate,
            } => {
                assert_eq!(field, "channel_name");
                assert!(matches!(operator, FilterOperator::Contains));
                assert_eq!(value, "sport");
                assert_eq!(case_sensitive, false);
                assert_eq!(negate, false);
            }
            _ => panic!("Expected condition node"),
        }
    }

    #[test]
    fn test_condition_with_modifiers() {
        let parser = FilterParser::new();
        let result = parser
            .parse("channel_name not case_sensitive contains \"BBC\"")
            .unwrap();

        match result.root {
            ConditionNode::Condition {
                field,
                operator,
                value,
                case_sensitive,
                negate,
            } => {
                assert_eq!(field, "channel_name");
                assert!(matches!(operator, FilterOperator::Contains));
                assert_eq!(value, "BBC");
                assert_eq!(case_sensitive, true);
                assert_eq!(negate, true);
            }
            _ => panic!("Expected condition node"),
        }
    }

    #[test]
    fn test_and_expression() {
        let parser = FilterParser::new();
        let result = parser
            .parse("channel_name contains \"sport\" AND group_title equals \"HD\"")
            .unwrap();

        match result.root {
            ConditionNode::Group { operator, children } => {
                assert!(matches!(operator, LogicalOperator::All));
                assert_eq!(children.len(), 2);
            }
            _ => panic!("Expected AND group"),
        }
    }

    #[test]
    fn test_nested_expression() {
        let parser = FilterParser::new();
        let result = parser.parse("channel_name contains \"news\" AND (group_title equals \"HD\" OR group_title equals \"4K\")").unwrap();

        match result.root {
            ConditionNode::Group { operator, children } => {
                assert!(matches!(operator, LogicalOperator::All));
                assert_eq!(children.len(), 2);

                // Second child should be another group
                match &children[1] {
                    ConditionNode::Group { operator, children } => {
                        assert!(matches!(operator, LogicalOperator::Any));
                        assert_eq!(children.len(), 2);
                    }
                    _ => panic!("Expected nested OR group"),
                }
            }
            _ => panic!("Expected AND group"),
        }
    }
}
