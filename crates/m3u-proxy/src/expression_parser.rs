// Generic expression parser for all pipeline stages
// Supports complex expressions like: (A=B AND (C=D OR E=F) SET field = value)
// Used by: data mapping rules, filter expressions, numbering rules, generation rules

use crate::models::{
    Action, ActionOperator, ActionValue, ConditionNode, ConditionTree, ExtendedExpression,
    FilterOperator, LogicalOperator, ExpressionErrorCategory, ExpressionValidationError,
    ExpressionValidateResult,
};
use anyhow::{Result, anyhow};
use tracing::{trace, warn};

/// Generic expression parser for pipeline stages
/// Handles conditional expressions, actions, and complex boolean logic
#[derive(Debug, Clone)]
pub struct ExpressionParser {
    operators: Vec<String>,
    logical_operators: Vec<String>,
    valid_fields: Vec<String>,
}

impl ExpressionParser {
    pub fn new() -> Self {
        Self {
            operators: vec![
                // Base operators
                "starts_with".to_string(),
                "ends_with".to_string(),
                "contains".to_string(),
                "equals".to_string(),
                "matches".to_string(),
                // Comparison operators (longer operators first to match before shorter ones)
                "greater_than_or_equal".to_string(),
                "less_than_or_equal".to_string(),
                "greater_than".to_string(),
                "less_than".to_string(),
            ],
            logical_operators: vec!["AND".to_string(), "OR".to_string()],
            valid_fields: vec![], // Empty by default, will be set via with_fields
        }
    }

    pub fn with_fields(mut self, fields: Vec<String>) -> Self {
        self.valid_fields = fields;
        self
    }

    /// Create a parser specifically for data mapping expressions
    pub fn for_data_mapping(fields: Vec<String>) -> Self {
        Self::new().with_fields(fields)
    }

    /// Create a parser specifically for filter expressions  
    pub fn for_filtering(fields: Vec<String>) -> Self {
        Self::new().with_fields(fields)
    }

    /// Create a parser specifically for numbering expressions
    pub fn for_numbering(fields: Vec<String>) -> Self {
        Self::new().with_fields(fields)
    }

    /// Create a parser specifically for generation expressions
    pub fn for_generation(fields: Vec<String>) -> Self {
        Self::new().with_fields(fields)
    }

    /// Parse a text expression into a ConditionTree (simple conditions only)
    /// Example: "channel_name contains \"sport\" AND (group_title equals \"HD\" OR group_title equals \"4K\")"
    /// Used by: filter expressions, simple validation
    pub fn parse(&self, expression: &str) -> Result<ConditionTree> {
        let tokens = self.tokenize(expression)?;
        let root = self.parse_expression(&tokens, &mut 0)?;
        Ok(ConditionTree { root })
    }

    /// Parse a text expression into an ExtendedExpression (supports actions)
    /// 
    /// # Basic Syntax
    /// Example: "channel_name contains \"sport\" SET group_title = \"Sports\""
    /// 
    /// # Complex Syntax 
    /// Multiple conditions: "(channel_name matches \"regex\" AND tvg_id contains \"test\" SET tvg_shift = \"$1$2\")"
    /// Multiple groups: "(condition1 SET action1) AND (condition2 SET action2)"
    /// 
    /// # Important: SET actions must come AFTER all conditions in a group
    /// ✅ Valid: "(condition1 AND condition2 SET action)"
    /// ✅ Valid: "(condition1 SET action) AND (condition2 SET action)"
    /// ❌ Invalid: "(condition1 SET action AND condition2)" - SET cannot be followed by more conditions
    /// 
    /// Used by: data mapping rules, complex transformations
    pub fn parse_extended(&self, expression: &str) -> Result<ExtendedExpression> {
        trace!(
            "Parsing expression (length: {} chars)",
            expression.len()
        );

        let tokens = self.tokenize(expression)?;
        trace!("Tokenized into {} tokens", tokens.len());

        let mut pos = 0;

        // Try to parse as conditional action groups first
        if let Ok(groups) = self.parse_conditional_action_groups(&tokens, &mut 0) {
            trace!(
                "Successfully parsed as conditional action groups:\n\
                 │ Expression: '{}'\n\
                 │ Groups: {}\n\
                 │ Total conditions: {}\n\
                 │ Total actions: {}\n\
                 └─ Group breakdown: {}",
                expression,
                groups.len(),
                groups
                    .iter()
                    .map(|g| self.count_conditions(&g.conditions))
                    .sum::<usize>(),
                groups.iter().map(|g| g.actions.len()).sum::<usize>(),
                groups
                    .iter()
                    .enumerate()
                    .map(|(i, g)| format!(
                        "G{}: {}c/{}a",
                        i + 1,
                        self.count_conditions(&g.conditions),
                        g.actions.len()
                    ))
                    .collect::<Vec<_>>()
                    .join(", ")
            );
            trace!("Expression parsed successfully");
            return Ok(ExtendedExpression::ConditionalActionGroups(groups));
        }

        // Fall back to original parsing
        trace!("Conditional groups parsing failed, trying simple condition-action format");
        let condition_root = self.parse_expression(&tokens, &mut pos)?;
        let condition = ConditionTree {
            root: condition_root,
        };

        // Check for SET keyword
        if pos < tokens.len() && matches!(tokens[pos], Token::SetKeyword) {
            pos += 1; // consume SET
            let actions = self.parse_action_list(&tokens, &mut pos)?;

            // Ensure we've consumed all tokens
            if pos < tokens.len() {
                return Err(anyhow!(
                    "Unexpected tokens after actions at position {}",
                    pos
                ));
            }

            trace!(
                "Successfully parsed as condition with actions:\n\
                 │ Expression: '{}'\n\
                 │ Conditions: {}\n\
                 │ Actions: {}\n\
                 └─ Action targets: [{}]",
                expression,
                self.count_conditions(&condition),
                actions.len(),
                actions
                    .iter()
                    .map(|a| a.field.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            );

            trace!("Expression parsed successfully");
            Ok(ExtendedExpression::ConditionWithActions { condition, actions })
        } else {
            // Ensure we've consumed all tokens
            if pos < tokens.len() {
                return Err(anyhow!(
                    "Unexpected tokens after condition at position {}",
                    pos
                ));
            }

            trace!(
                "Successfully parsed as condition only:\n\
                 │ Expression: '{}'\n\
                 └─ Conditions: {}",
                expression,
                self.count_conditions(&condition)
            );

            trace!("Expression parsed successfully");
            Ok(ExtendedExpression::ConditionOnly(condition))
        }
    }

    /// Validate expression and return structured results with position information
    /// This provides detailed validation results suitable for UI display and API responses
    /// Returns structured validation result with multiple detailed errors
    pub fn validate(&self, expression: &str) -> ExpressionValidateResult {
        trace!("Performing structured validation of expression: '{}'", expression);
        
        let mut errors = Vec::new();
        let mut expression_tree = None;
        
        // Step 1: Parse the expression and collect syntax errors
        match self.parse_extended_with_errors(expression) {
            Ok(parsed) => {
                // Step 2: Validate semantic correctness and collect semantic errors
                let semantic_errors = self.validate_extended_with_errors(&parsed);
                errors.extend(semantic_errors);
                
                // If we successfully parsed, serialize the expression tree for display
                if let Ok(tree_json) = serde_json::to_value(&parsed) {
                    expression_tree = Some(tree_json);
                }
            }
            Err(syntax_errors) => {
                // Collect syntax errors from parsing phase
                errors.extend(syntax_errors);
            }
        }
        
        let is_valid = errors.is_empty();
        
        ExpressionValidateResult {
            is_valid,
            errors,
            expression_tree,
        }
    }

    /// Internal parsing method that collects structured errors instead of failing fast
    fn parse_extended_with_errors(&self, expression: &str) -> Result<ExtendedExpression, Vec<ExpressionValidationError>> {
        let mut errors = Vec::new();
        
        // Tokenization with error collection
        let tokens = match self.tokenize_with_errors(expression) {
            Ok(tokens) => tokens,
            Err(tokenize_errors) => {
                errors.extend(tokenize_errors);
                return Err(errors);
            }
        };
        
        // Expression parsing with error collection
        match self.parse_extended_from_tokens(&tokens) {
            Ok(expr) => Ok(expr),
            Err(parse_errors) => {
                errors.extend(parse_errors);
                Err(errors)
            }
        }
    }

    /// Internal tokenization method that collects structured errors
    fn tokenize_with_errors(&self, expression: &str) -> Result<Vec<Token>, Vec<ExpressionValidationError>> {
        let mut tokens = Vec::new();
        let mut errors = Vec::new();
        let mut current_pos = 0;
        let expr = expression.trim();

        // Don't pre-scan - let the proper tokenizer and parser handle all validation
        // This ensures complex expressions with logical operators are handled correctly

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

            // Handle quoted strings with better error reporting
            if remaining.starts_with('"') || remaining.starts_with('\'') {
                let quote_char = remaining.chars().next().unwrap();
                match remaining[1..].find(quote_char) {
                    Some(end_pos) => {
                        let value = remaining[1..end_pos + 1].to_string();
                        tokens.push(Token::Value(value));
                        current_pos += end_pos + 2;
                        continue;
                    }
                    None => {
                        let context = if remaining.len() > 20 {
                            format!("{}...", &remaining[..20])
                        } else {
                            remaining.to_string()
                        };
                        
                        errors.push(ExpressionValidationError {
                            category: ExpressionErrorCategory::Syntax,
                            error_type: "unclosed_quote".to_string(),
                            message: format!("Unclosed {} quote", if quote_char == '"' { "double" } else { "single" }),
                            details: Some(format!("String literal starting at position {current_pos} is not properly closed")),
                            position: Some(current_pos),
                            context: Some(context),
                            suggestion: Some(format!("Add closing {quote_char} quote: {quote_char}...{quote_char}")),
                        });
                        // Skip the problematic token and continue to find more errors
                        current_pos += 1;
                        continue;
                    }
                }
            }

            // Handle logical operators with suggestions for common mistakes
            let mut found_logical = false;
            for logical_op in &self.logical_operators {
                if remaining.to_uppercase().starts_with(logical_op) {
                    let end_pos = logical_op.len();
                    if end_pos == remaining.len()
                        || remaining
                            .chars()
                            .nth(end_pos)
                            .is_none_or(|c| c.is_whitespace() || c == '(' || c == ')')
                    {
                        let operator = match logical_op.as_str() {
                            "AND" | "ALL" => LogicalOperator::And,
                            "OR" | "ANY" => LogicalOperator::Or,
                            _ => {
                                errors.push(ExpressionValidationError {
                                    category: ExpressionErrorCategory::Operator,
                                    error_type: "unknown_logical_operator".to_string(),
                                    message: format!("Unknown logical operator: {logical_op}"),
                                    details: Some("This logical operator is not supported".to_string()),
                                    position: Some(current_pos),
                                    context: Some(logical_op.clone()),
                                    suggestion: Some("Use AND or OR".to_string()),
                                });
                                return Err(errors);
                            }
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

            // Check for common logical operator mistakes
            let common_mistakes = [
                ("&&", "AND"), ("||", "OR"), ("and", "AND"), ("or", "OR"),
                ("&", "AND"), ("|", "OR"), ("AND and", "AND"), ("OR or", "OR")
            ];
            
            for (mistake, correct) in &common_mistakes {
                if remaining.to_uppercase().starts_with(&mistake.to_uppercase()) {
                    let context = if remaining.len() > 10 {
                        format!("{}...", &remaining[..10])
                    } else {
                        remaining.to_string()
                    };
                    
                    errors.push(ExpressionValidationError {
                        category: ExpressionErrorCategory::Operator,
                        error_type: "invalid_logical_operator".to_string(),
                        message: format!("Invalid logical operator: {mistake}"),
                        details: Some(format!("'{mistake}' is not a valid logical operator")),
                        position: Some(current_pos),
                        context: Some(context),
                        suggestion: Some(format!("Use '{correct}' instead")),
                    });
                    return Err(errors);
                }
            }

            // Handle modifiers
            if remaining.to_uppercase().starts_with("NOT") {
                let end_pos = 3;
                if end_pos == remaining.len()
                    || remaining
                        .chars()
                        .nth(end_pos)
                        .is_none_or(|c| c.is_whitespace())
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
                        .is_none_or(|c| c.is_whitespace())
                {
                    tokens.push(Token::Modifier("case_sensitive".to_string()));
                    current_pos += end_pos;
                    continue;
                }
            }

            // Handle SET keyword
            if remaining.to_uppercase().starts_with("SET") {
                let end_pos = 3;
                if end_pos == remaining.len()
                    || remaining
                        .chars()
                        .nth(end_pos)
                        .is_none_or(|c| c.is_whitespace())
                {
                    tokens.push(Token::SetKeyword);
                    current_pos += end_pos;
                    continue;
                }
            }

            // Handle assignment operators
            if remaining.starts_with("+=") {
                tokens.push(Token::AssignmentOp(ActionOperator::Append));
                current_pos += 2;
                continue;
            }
            if remaining.starts_with("-=") {
                tokens.push(Token::AssignmentOp(ActionOperator::Remove));
                current_pos += 2;
                continue;
            }
            if remaining.starts_with("?=") {
                tokens.push(Token::AssignmentOp(ActionOperator::SetIfEmpty));
                current_pos += 2;
                continue;
            }
            if remaining.starts_with("=") {
                tokens.push(Token::AssignmentOp(ActionOperator::Set));
                current_pos += 1;
                continue;
            }

            // Handle comma separator
            if remaining.starts_with(",") {
                tokens.push(Token::Comma);
                current_pos += 1;
                continue;
            }

            // Handle filter operators with suggestions for typos
            let mut found_operator = false;
            for op in &self.operators {
                if remaining.starts_with(op) {
                    let end_pos = op.len();
                    if end_pos == remaining.len()
                        || remaining
                            .chars()
                            .nth(end_pos)
                            .is_none_or(|c| c.is_whitespace() || c == '"' || c == '\'')
                    {
                        let filter_op = match op.as_str() {
                            "contains" => FilterOperator::Contains,
                            "equals" => FilterOperator::Equals,
                            "matches" => FilterOperator::Matches,
                            "starts_with" => FilterOperator::StartsWith,
                            "ends_with" => FilterOperator::EndsWith,
                            "greater_than" => FilterOperator::GreaterThan,
                            "less_than" => FilterOperator::LessThan,
                            "greater_than_or_equal" => FilterOperator::GreaterThanOrEqual,
                            "less_than_or_equal" => FilterOperator::LessThanOrEqual,
                            _ => {
                                errors.push(ExpressionValidationError {
                                    category: ExpressionErrorCategory::Operator,
                                    error_type: "unknown_filter_operator".to_string(),
                                    message: format!("Unknown filter operator: {op}"),
                                    details: Some("This filter operator is not supported".to_string()),
                                    position: Some(current_pos),
                                    context: Some(op.clone()),
                                    suggestion: Some("Available operators: contains, equals, matches, starts_with, ends_with".to_string()),
                                });
                                return Err(errors);
                            }
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

            // Check for common operator typos and suggest corrections
            let common_operator_typos = [
                ("containz", "contains"), ("contians", "contains"), ("contain", "contains"),
                ("equal", "equals"), ("equls", "equals"), ("=", "equals"),
                ("match", "matches"), ("matche", "matches"), ("regex", "matches"),
                ("start_with", "starts_with"), ("begin_with", "starts_with"),
                ("end_with", "ends_with"), ("finish_with", "ends_with"),
                ("!=", "not_equals"), ("not_equal", "not_equals"),
                ("!", "not_"), ("~", "not_"), ("does_not", "not_"),
                // Comparison operators
                (">", "greater_than"), (">=", "greater_than_or_equal"),
                ("<", "less_than"), ("<=", "less_than_or_equal"),
            ];

            for (typo, correct) in &common_operator_typos {
                if remaining.starts_with(typo) {
                    let end_pos = typo.len();
                    if end_pos == remaining.len()
                        || remaining
                            .chars()
                            .nth(end_pos)
                            .is_none_or(|c| c.is_whitespace() || c == '"' || c == '\'')
                    {
                        let context = if remaining.len() > 15 {
                            format!("{}...", &remaining[..15])
                        } else {
                            remaining.to_string()
                        };
                        
                        errors.push(ExpressionValidationError {
                            category: ExpressionErrorCategory::Operator,
                            error_type: "operator_typo".to_string(),
                            message: format!("Unknown operator: {typo}"),
                            details: Some(format!("'{typo}' is not a valid operator. Did you mean '{correct}'?")),
                            position: Some(current_pos),
                            context: Some(context),
                            suggestion: Some(format!("Use '{correct}' instead")),
                        });
                        // Skip the problematic token and continue to find more errors
                        current_pos += 1;
                        continue;
                    }
                }
            }

            // Handle field names
            let word_end = remaining
                .find(|c: char| c.is_whitespace() || c == '(' || c == ')' || c == '"' || c == '\'')
                .unwrap_or(remaining.len());

            if word_end > 0 {
                let word = remaining[..word_end].to_string();
                tokens.push(Token::Field(word));
                current_pos += word_end;
            } else {
                let context = if remaining.len() > 10 {
                    format!("{}...", &remaining[..10])
                } else {
                    remaining.to_string()
                };
                
                errors.push(ExpressionValidationError {
                    category: ExpressionErrorCategory::Syntax,
                    error_type: "unexpected_character".to_string(),
                    message: format!("Unexpected character at position {current_pos}"),
                    details: Some(format!("Character '{}' is not valid in this context", remaining.chars().next().unwrap_or('?'))),
                    position: Some(current_pos),
                    context: Some(context),
                    suggestion: Some("Remove the invalid character or check your syntax".to_string()),
                });
                // Skip the problematic character and continue to find more errors
                current_pos += 1;
                continue;
            }
        }

        if errors.is_empty() {
            Ok(tokens)
        } else {
            Err(errors)
        }
    }




    /// Internal parsing method that works with pre-tokenized input
    fn parse_extended_from_tokens(&self, tokens: &[Token]) -> Result<ExtendedExpression, Vec<ExpressionValidationError>> {
        let mut errors = Vec::new();
        
        // Check for empty token list
        if tokens.is_empty() {
            errors.push(ExpressionValidationError {
                category: ExpressionErrorCategory::Syntax,
                error_type: "empty_expression".to_string(),
                message: "Expression cannot be empty".to_string(),
                details: Some("An expression must contain at least one condition".to_string()),
                position: Some(0),
                context: None,
                suggestion: Some("Example: channel_name contains \"value\"".to_string()),
            });
            return Err(errors);
        }

        // Check for balanced parentheses
        let mut paren_stack = Vec::new();
        for (i, token) in tokens.iter().enumerate() {
            match token {
                Token::LeftParen => paren_stack.push(i),
                Token::RightParen => {
                    if paren_stack.is_empty() {
                        errors.push(ExpressionValidationError {
                            category: ExpressionErrorCategory::Syntax,
                            error_type: "unmatched_closing_parenthesis".to_string(),
                            message: "Unmatched closing parenthesis".to_string(),
                            details: Some(format!("Closing parenthesis at position {i} has no matching opening parenthesis")),
                            position: Some(i),
                            context: Some(")".to_string()),
                            suggestion: Some("Add opening parenthesis or remove this closing parenthesis".to_string()),
                        });
                    } else {
                        paren_stack.pop();
                    }
                }
                _ => {}
            }
        }

        // Check for unclosed parentheses
        for &open_pos in &paren_stack {
            errors.push(ExpressionValidationError {
                category: ExpressionErrorCategory::Syntax,
                error_type: "unclosed_parenthesis".to_string(),
                message: "Unclosed parenthesis".to_string(),
                details: Some(format!("Opening parenthesis at position {open_pos} is never closed")),
                position: Some(open_pos),
                context: Some("(".to_string()),
                suggestion: Some("Add closing parenthesis: (...) or remove the opening parenthesis".to_string()),
            });
        }

        if !errors.is_empty() {
            return Err(errors);
        }

        // Proceed with normal parsing if syntax is valid
        // For now, fall back to original parsing and convert errors
        match self.parse_extended_from_tokens_original(tokens) {
            Ok(expr) => Ok(expr),
            Err(e) => {
                // Convert anyhow errors to structured errors
                errors.push(ExpressionValidationError {
                    category: ExpressionErrorCategory::Syntax,
                    error_type: "parse_error".to_string(),
                    message: "Failed to parse expression".to_string(),
                    details: Some(e.to_string()),
                    position: None,
                    context: None,
                    suggestion: Some("Check your expression syntax".to_string()),
                });
                Err(errors)
            }
        }
    }

    /// Original parsing logic wrapped for error conversion
    fn parse_extended_from_tokens_original(&self, tokens: &[Token]) -> Result<ExtendedExpression> {
        // This is essentially the same as the original parse_extended logic
        // but working with pre-tokenized input instead of tokenizing again
        let mut pos = 0;

        // Try to parse as conditional action groups first
        if let Ok(groups) = self.parse_conditional_action_groups(tokens, &mut 0) {
            return Ok(ExtendedExpression::ConditionalActionGroups(groups));
        }

        // Fall back to original parsing
        let condition_root = self.parse_expression(tokens, &mut pos)?;
        let condition = ConditionTree {
            root: condition_root,
        };

        // Check for SET keyword
        if pos < tokens.len() && matches!(tokens[pos], Token::SetKeyword) {
            pos += 1; // consume SET
            let actions = self.parse_action_list(tokens, &mut pos)?;

            // Ensure we've consumed all tokens
            if pos < tokens.len() {
                return Err(anyhow!(
                    "Unexpected tokens after actions at position {}",
                    pos
                ));
            }

            Ok(ExtendedExpression::ConditionWithActions { condition, actions })
        } else {
            // Ensure we've consumed all tokens
            if pos < tokens.len() {
                return Err(anyhow!(
                    "Unexpected tokens after condition at position {}",
                    pos
                ));
            }

            Ok(ExtendedExpression::ConditionOnly(condition))
        }
    }

    /// Internal validation method that collects structured errors instead of failing fast
    fn validate_extended_with_errors(&self, expression: &ExtendedExpression) -> Vec<ExpressionValidationError> {
        let mut errors = Vec::new();

        match expression {
            ExtendedExpression::ConditionOnly(condition) => {
                errors.extend(self.validate_condition_tree_with_errors(condition));
            }
            ExtendedExpression::ConditionWithActions { condition, actions } => {
                errors.extend(self.validate_condition_tree_with_errors(condition));
                errors.extend(self.validate_actions_with_errors(actions));
            }
            ExtendedExpression::ConditionalActionGroups(groups) => {
                for group in groups.iter() {
                    errors.extend(self.validate_condition_tree_with_errors(&group.conditions));
                    errors.extend(self.validate_actions_with_errors(&group.actions));
                }
            }
        }

        errors
    }

    /// Validate condition tree and collect structured errors
    fn validate_condition_tree_with_errors(&self, condition_tree: &ConditionTree) -> Vec<ExpressionValidationError> {
        self.validate_condition_node_with_errors(&condition_tree.root)
    }

    /// Validate condition nodes recursively and collect structured errors
    fn validate_condition_node_with_errors(&self, condition: &ConditionNode) -> Vec<ExpressionValidationError> {
        let mut errors = Vec::new();

        match condition {
            ConditionNode::Condition { field, operator, value, .. } => {
                // Validate field name
                if let Some(field_error) = self.validate_field_name_with_error(field) {
                    errors.push(field_error);
                }

                // Validate regex patterns for matches operators
                if matches!(operator, FilterOperator::Matches | FilterOperator::NotMatches) {
                    if let Some(regex_error) = self.validate_regex_pattern(value) {
                        errors.push(regex_error);
                    }
                }
            }
            ConditionNode::Group { children, .. } => {
                for child in children {
                    errors.extend(self.validate_condition_node_with_errors(child));
                }
            }
        }

        errors
    }

    /// Validate field name and return structured error if invalid
    fn validate_field_name_with_error(&self, field: &str) -> Option<ExpressionValidationError> {
        if self.valid_fields.is_empty() {
            return None; // Skip validation if no fields configured
        }

        if !self.valid_fields.iter().any(|f| f == field) {
            // Find similar field names for suggestions
            let suggestion = self.find_similar_field_name(field);
            
            Some(ExpressionValidationError {
                category: ExpressionErrorCategory::Field,
                error_type: "unknown_field".to_string(),
                message: format!("Unknown field '{field}'"),
                details: if let Some(ref similar) = suggestion {
                    Some(format!("Field '{field}' is not available. Did you mean '{similar}'?"))
                } else {
                    Some(format!("Field '{field}' is not available for this expression type"))
                },
                position: None, // Would need additional tracking to provide position
                context: Some(field.to_string()),
                suggestion: suggestion.or_else(|| Some(format!("Available fields: {}", self.valid_fields.join(", ")))),
            })
        } else {
            None
        }
    }

    /// Find similar field name using simple edit distance
    fn find_similar_field_name(&self, field: &str) -> Option<String> {
        let mut best_match = None;
        let mut best_score = 0;

        for valid_field in &self.valid_fields {
            let score = self.calculate_similarity(field, valid_field);
            if score > best_score && score >= 60 { // 60% similarity threshold
                best_score = score;
                best_match = Some(valid_field.clone());
            }
        }

        best_match
    }

    /// Calculate similarity percentage between two strings
    fn calculate_similarity(&self, a: &str, b: &str) -> u32 {
        if a == b {
            return 100;
        }

        let a_lower = a.to_lowercase();
        let b_lower = b.to_lowercase();

        // Check if one contains the other
        if a_lower.contains(&b_lower) || b_lower.contains(&a_lower) {
            return 80;
        }

        // Simple character-based similarity
        let max_len = a.len().max(b.len());
        if max_len == 0 {
            return 100;
        }

        let mut common_chars = 0;
        let a_chars: Vec<char> = a_lower.chars().collect();
        let b_chars: Vec<char> = b_lower.chars().collect();

        for (i, &char_a) in a_chars.iter().enumerate() {
            if (i < b_chars.len() && char_a == b_chars[i]) || b_chars.contains(&char_a) {
                common_chars += 1;
            }
        }

        (common_chars * 100 / max_len).min(100) as u32
    }

    /// Validate regex pattern in matches operators
    fn validate_regex_pattern(&self, pattern: &str) -> Option<ExpressionValidationError> {
        // Try to compile the regex pattern
        match regex::Regex::new(pattern) {
            Ok(_) => None, // Valid regex
            Err(e) => Some(ExpressionValidationError {
                category: ExpressionErrorCategory::Value,
                error_type: "invalid_regex".to_string(),
                message: "Invalid regular expression".to_string(),
                details: Some(format!("Regex pattern '{pattern}' is invalid: {e}")),
                position: None,
                context: Some(format!("matches \"{pattern}\"")),
                suggestion: Some("Use valid regex syntax. Example: channel_name matches \"^[a-zA-Z]+$\"".to_string()),
            }),
        }
    }

    /// Validate actions list and collect structured errors
    fn validate_actions_with_errors(&self, actions: &[Action]) -> Vec<ExpressionValidationError> {
        let mut errors = Vec::new();

        for action in actions {
            if let Some(action_error) = self.validate_action_with_error(action) {
                errors.push(action_error);
            }
        }

        errors
    }

    /// Validate single action and return structured error if invalid
    fn validate_action_with_error(&self, action: &Action) -> Option<ExpressionValidationError> {
        // Validate field name
        if let Some(field_error) = self.validate_field_name_with_error(&action.field) {
            return Some(field_error);
        }

        // Validate value length
        if let ActionValue::Literal(literal) = &action.value {
            if literal.len() > 255 {
                return Some(ExpressionValidationError {
                    category: ExpressionErrorCategory::Value,
                    error_type: "value_too_long".to_string(),
                    message: format!("Value for field '{}' is too long", action.field),
                    details: Some(format!("Value is {} characters long, maximum allowed is 255", literal.len())),
                    position: None,
                    context: Some(format!("{} = \"{}...\"", action.field, &literal[..20.min(literal.len())])),
                    suggestion: Some("Shorten the value to 255 characters or less".to_string()),
                });
            }
        }

        None
    }

    /// Parse conditional action groups
    /// Example: "(condition1 SET action1) AND (condition2 SET action2)"
    fn parse_conditional_action_groups(
        &self,
        tokens: &[Token],
        pos: &mut usize,
    ) -> Result<Vec<crate::models::ConditionalActionGroup>> {
        trace!(
            "Attempting to parse conditional action groups from {} tokens",
            tokens.len()
        );
        let mut groups = Vec::new();
        let start_pos = *pos;

        // Parse first group
        if let Ok(group) = self.parse_single_conditional_group(tokens, pos) {
            trace!(
                "Parsed first conditional group with {} conditions and {} actions",
                self.count_conditions(&group.conditions),
                group.actions.len()
            );
            groups.push(group);
        } else {
            trace!(
                "Failed to parse first conditional group, not a conditional action groups expression"
            );
            *pos = start_pos; // Reset position on failure
            return Err(anyhow!("Failed to parse conditional action groups"));
        }

        // Parse additional groups with logical operators
        while *pos < tokens.len() {
            // Look for logical operator
            if let Some(Token::LogicalOp(op)) = tokens.get(*pos) {
                let logical_op = op.clone();
                trace!(
                    "Found logical operator: {:?} at position {}",
                    logical_op, *pos
                );
                *pos += 1; // consume logical operator

                // Parse next group
                if let Ok(group) = self.parse_single_conditional_group(tokens, pos) {
                    trace!(
                        "Parsed additional conditional group with {} conditions and {} actions",
                        self.count_conditions(&group.conditions),
                        group.actions.len()
                    );
                    // Set the logical operator on the previous group
                    if let Some(prev_group) = groups.last_mut() {
                        prev_group.logical_operator = Some(logical_op);
                    }
                    groups.push(group);
                } else {
                    warn!(
                        "Expected conditional group after logical operator {:?} at position {}",
                        op, *pos
                    );
                    return Err(anyhow!("Expected conditional group after logical operator"));
                }
            } else {
                trace!("No more logical operators found, finished parsing groups");
                break;
            }
        }

        if groups.is_empty() {
            return Err(anyhow!("No conditional action groups found"));
        }

        trace!(
            "Successfully parsed {} conditional action groups",
            groups.len()
        );
        Ok(groups)
    }

    /// Parse a single conditional group: (conditions SET actions)
    /// Note: SET actions must come AFTER all conditions - syntax like "(condition SET action AND condition)" is invalid
    fn parse_single_conditional_group(
        &self,
        tokens: &[Token],
        pos: &mut usize,
    ) -> Result<crate::models::ConditionalActionGroup> {
        trace!(
            "Parsing single conditional group starting at position {}",
            *pos
        );

        // Expect opening parenthesis
        if !matches!(tokens.get(*pos), Some(Token::LeftParen)) {
            trace!("No opening parenthesis found at position {}", *pos);
            return Err(anyhow!(
                "Expected opening parenthesis for conditional group"
            ));
        }
        *pos += 1; // consume '('
        trace!("Consumed opening parenthesis");

        // Parse conditions until we hit SET
        let mut condition_tokens = Vec::new();
        let mut paren_depth = 0;
        let mut found_set = false;

        while *pos < tokens.len() {
            match &tokens[*pos] {
                Token::LeftParen => {
                    condition_tokens.push(tokens[*pos].clone());
                    paren_depth += 1;
                    *pos += 1;
                }
                Token::RightParen if paren_depth > 0 => {
                    condition_tokens.push(tokens[*pos].clone());
                    paren_depth -= 1;
                    *pos += 1;
                }
                Token::RightParen if paren_depth == 0 => {
                    // This is the closing paren for our group, but we should have found SET first
                    if !found_set {
                        trace!("Found closing parenthesis before SET keyword");
                        return Err(anyhow!("Expected SET keyword before closing parenthesis"));
                    }
                    break;
                }
                Token::SetKeyword if paren_depth == 0 => {
                    trace!(
                        "Found SET keyword at position {}, collected {} condition tokens",
                        *pos,
                        condition_tokens.len()
                    );
                    found_set = true;
                    *pos += 1; // consume SET
                    break;
                }
                _ => {
                    condition_tokens.push(tokens[*pos].clone());
                    *pos += 1;
                }
            }
        }

        if !found_set {
            trace!("No SET keyword found in conditional group");
            return Err(anyhow!("Expected SET keyword in conditional group"));
        }

        // Parse conditions from collected tokens
        let mut condition_pos = 0;
        let condition_root = self.parse_expression(&condition_tokens, &mut condition_pos)?;
        let conditions = ConditionTree {
            root: condition_root,
        };
        trace!(
            "Parsed conditions with {} total condition nodes",
            self.count_conditions(&conditions)
        );

        // Parse actions until closing parenthesis
        let actions = self.parse_action_list(tokens, pos)?;
        trace!("Parsed {} actions", actions.len());

        // Expect closing parenthesis
        if !matches!(tokens.get(*pos), Some(Token::RightParen)) {
            warn!(
                "Expected closing parenthesis at position {}, found: {:?}",
                *pos,
                tokens.get(*pos)
            );
            return Err(anyhow!(
                "Expected closing parenthesis for conditional group"
            ));
        }
        *pos += 1; // consume ')'
        trace!("Consumed closing parenthesis, conditional group complete");

        Ok(crate::models::ConditionalActionGroup {
            conditions,
            actions,
            logical_operator: None, // Will be set by caller if needed
        })
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
                            .is_none_or(|c| c.is_whitespace() || c == '(' || c == ')')
                    {
                        let operator = match logical_op.as_str() {
                            "AND" | "ALL" => LogicalOperator::And,
                            "OR" | "ANY" => LogicalOperator::Or,
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
                        .is_none_or(|c| c.is_whitespace())
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
                        .is_none_or(|c| c.is_whitespace())
                {
                    tokens.push(Token::Modifier("case_sensitive".to_string()));
                    current_pos += end_pos;
                    continue;
                }
            }

            // Handle SET keyword
            if remaining.to_uppercase().starts_with("SET") {
                let end_pos = 3;
                if end_pos == remaining.len()
                    || remaining
                        .chars()
                        .nth(end_pos)
                        .is_none_or(|c| c.is_whitespace())
                {
                    tokens.push(Token::SetKeyword);
                    current_pos += end_pos;
                    continue;
                }
            }

            // Handle assignment operators (must come before single = check)
            if remaining.starts_with("+=") {
                tokens.push(Token::AssignmentOp(ActionOperator::Append));
                current_pos += 2;
                continue;
            }
            if remaining.starts_with("-=") {
                tokens.push(Token::AssignmentOp(ActionOperator::Remove));
                current_pos += 2;
                continue;
            }
            if remaining.starts_with("?=") {
                tokens.push(Token::AssignmentOp(ActionOperator::SetIfEmpty));
                current_pos += 2;
                continue;
            }
            if remaining.starts_with("=") {
                tokens.push(Token::AssignmentOp(ActionOperator::Set));
                current_pos += 1;
                continue;
            }

            // Handle comma separator
            if remaining.starts_with(",") {
                tokens.push(Token::Comma);
                current_pos += 1;
                continue;
            }

            // Handle filter operators (base operators only - modifiers handled separately)
            let mut found_operator = false;
            for op in &self.operators {
                if remaining.starts_with(op) {
                    // Check that it's a whole word
                    let end_pos = op.len();
                    if end_pos == remaining.len()
                        || remaining
                            .chars()
                            .nth(end_pos)
                            .is_none_or(|c| c.is_whitespace() || c == '"' || c == '\'')
                    {
                        let filter_op = match op.as_str() {
                            "contains" => FilterOperator::Contains,
                            "equals" => FilterOperator::Equals,
                            "matches" => FilterOperator::Matches,
                            "starts_with" => FilterOperator::StartsWith,
                            "ends_with" => FilterOperator::EndsWith,
                            "greater_than" => FilterOperator::GreaterThan,
                            "less_than" => FilterOperator::LessThan,
                            "greater_than_or_equal" => FilterOperator::GreaterThanOrEqual,
                            "less_than_or_equal" => FilterOperator::LessThanOrEqual,
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

                // Apply modifiers to the base operator to get the final operator
                let base_operator = match &tokens[*pos] {
                    Token::Operator(op) => op.clone(),
                    _ => return Err(anyhow!("Expected operator after field '{}'", field)),
                };

                let operator = if negate {
                    match base_operator {
                        FilterOperator::Contains => FilterOperator::NotContains,
                        FilterOperator::Equals => FilterOperator::NotEquals,
                        FilterOperator::Matches => FilterOperator::NotMatches,
                        FilterOperator::StartsWith => FilterOperator::NotStartsWith,
                        FilterOperator::EndsWith => FilterOperator::NotEndsWith,
                        // Already negated operators - double negation becomes positive
                        FilterOperator::NotContains => FilterOperator::Contains,
                        FilterOperator::NotEquals => FilterOperator::Equals,
                        FilterOperator::NotMatches => FilterOperator::Matches,
                        FilterOperator::NotStartsWith => FilterOperator::StartsWith,
                        FilterOperator::NotEndsWith => FilterOperator::EndsWith,
                        // Comparison operators don't have negated versions, so return error
                        FilterOperator::GreaterThan | FilterOperator::LessThan | 
                        FilterOperator::GreaterThanOrEqual | FilterOperator::LessThanOrEqual => {
                            return Err(anyhow!("Cannot negate comparison operators"));
                        }
                    }
                } else {
                    base_operator
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
                    negate: false, // negate is now handled through operator transformation
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

                // Apply modifiers to the base operator to get the final operator
                let base_operator = match &tokens[*pos] {
                    Token::Operator(op) => op.clone(),
                    _ => return Err(anyhow!("Expected operator after field '{}'", field)),
                };

                let operator = if negate {
                    match base_operator {
                        FilterOperator::Contains => FilterOperator::NotContains,
                        FilterOperator::Equals => FilterOperator::NotEquals,
                        FilterOperator::Matches => FilterOperator::NotMatches,
                        FilterOperator::StartsWith => FilterOperator::NotStartsWith,
                        FilterOperator::EndsWith => FilterOperator::NotEndsWith,
                        // Already negated operators - double negation becomes positive
                        FilterOperator::NotContains => FilterOperator::Contains,
                        FilterOperator::NotEquals => FilterOperator::Equals,
                        FilterOperator::NotMatches => FilterOperator::Matches,
                        FilterOperator::NotStartsWith => FilterOperator::StartsWith,
                        FilterOperator::NotEndsWith => FilterOperator::EndsWith,
                        // Comparison operators don't have negated versions, so return error
                        FilterOperator::GreaterThan | FilterOperator::LessThan | 
                        FilterOperator::GreaterThanOrEqual | FilterOperator::LessThanOrEqual => {
                            return Err(anyhow!("Cannot negate comparison operators"));
                        }
                    }
                } else {
                    base_operator
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
                    negate: false, // negate is now handled through operator transformation
                })
            }
            _ => Err(anyhow!(
                "Expected field name, modifier, or opening parenthesis"
            )),
        }
    }

    /// Parse a list of actions separated by commas
    fn parse_action_list(&self, tokens: &[Token], pos: &mut usize) -> Result<Vec<Action>> {
        let mut actions = Vec::new();

        if *pos >= tokens.len() {
            return Err(anyhow!("Expected action after SET keyword"));
        }

        loop {
            let action = self.parse_action(tokens, pos)?;
            actions.push(action);

            // Check for comma to continue, or end of tokens/other token to stop
            if *pos < tokens.len() && matches!(tokens[*pos], Token::Comma) {
                *pos += 1; // consume comma

                // Ensure there's another action after the comma
                if *pos >= tokens.len() {
                    return Err(anyhow!("Expected action after comma"));
                }
                continue;
            } else {
                break;
            }
        }

        Ok(actions)
    }

    /// Parse a single action: field operator value
    fn parse_action(&self, tokens: &[Token], pos: &mut usize) -> Result<Action> {
        // Parse field name
        if *pos >= tokens.len() {
            return Err(anyhow!("Expected field name in action"));
        }

        let field = match &tokens[*pos] {
            Token::Field(name) => name.clone(),
            _ => {
                return Err(anyhow!(
                    "Expected field name in action, found {:?}",
                    tokens[*pos]
                ));
            }
        };
        *pos += 1;

        // Parse assignment operator
        if *pos >= tokens.len() {
            return Err(anyhow!(
                "Expected assignment operator after field '{}'",
                field
            ));
        }

        let operator = match &tokens[*pos] {
            Token::AssignmentOp(op) => op.clone(),
            _ => {
                return Err(anyhow!(
                    "Expected assignment operator after field '{}', found {:?}",
                    field,
                    tokens[*pos]
                ));
            }
        };
        *pos += 1;

        // Parse value
        if *pos >= tokens.len() {
            return Err(anyhow!("Expected value after assignment operator"));
        }

        let value = match &tokens[*pos] {
            Token::Value(val) => ActionValue::Literal(val.clone()),
            _ => {
                return Err(anyhow!(
                    "Expected quoted value after assignment operator, found {:?}",
                    tokens[*pos]
                ));
            }
        };
        *pos += 1;

        Ok(Action {
            field,
            operator,
            value,
        })
    }


}

#[derive(Debug, Clone)]
enum Token {
    // Existing tokens
    Field(String),
    Operator(FilterOperator),
    Value(String),
    LogicalOp(LogicalOperator),
    Modifier(String),
    LeftParen,
    RightParen,

    // New action tokens
    SetKeyword,                   // SET
    AssignmentOp(ActionOperator), // =, +=, ?=, -=
    Comma,                        // ,
}

impl Default for ExpressionParser {
    fn default() -> Self {
        Self::new()
    }
}

impl ExpressionParser {
    /// Helper method to count conditions in a condition tree
    fn count_conditions(&self, tree: &ConditionTree) -> usize {
        self.count_condition_nodes(&tree.root)
    }
    #[allow(clippy::only_used_in_recursion)]
    fn count_condition_nodes(&self, node: &ConditionNode) -> usize {
        match node {
            ConditionNode::Condition { .. } => 1,
            ConditionNode::Group { children, .. } => children
                .iter()
                .map(|child| self.count_condition_nodes(child))
                .sum(),
        }
    }
}

// Backward compatibility type alias
pub type FilterParser = ExpressionParser;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_simple_condition() {
        let parser = ExpressionParser::new();
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
                assert!(!case_sensitive);
                assert!(!negate);
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
                assert!(matches!(operator, FilterOperator::NotContains));
                assert_eq!(value, "BBC");
                assert!(case_sensitive); // case_sensitive modifier was parsed correctly
                assert!(!negate); // negate is false because "not" was incorporated into NotContains
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
                assert!(matches!(operator, LogicalOperator::And));
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
                assert!(matches!(operator, LogicalOperator::And));
                assert_eq!(children.len(), 2);

                // Second child should be another group
                match &children[1] {
                    ConditionNode::Group { operator, children } => {
                        assert!(matches!(operator, LogicalOperator::Or));
                        assert_eq!(children.len(), 2);
                    }
                    _ => panic!("Expected nested OR group"),
                }
            }
            _ => panic!("Expected AND group"),
        }
    }

    #[test]
    fn test_all_operators() {
        let parser = FilterParser::new();
        let test_cases = vec![
            ("channel_name contains \"sport\"", FilterOperator::Contains),
            ("channel_name equals \"BBC One\"", FilterOperator::Equals),
            ("channel_name matches \"^BBC.*\"", FilterOperator::Matches),
            (
                "channel_name starts_with \"BBC\"",
                FilterOperator::StartsWith,
            ),
            ("channel_name ends_with \"HD\"", FilterOperator::EndsWith),
            (
                "channel_name not contains \"adult\"",
                FilterOperator::NotContains,
            ),
            (
                "channel_name not equals \"Test\"",
                FilterOperator::NotEquals,
            ),
            (
                "channel_name not matches \"test.*\"",
                FilterOperator::NotMatches,
            ),
        ];

        for (expression, expected_operator) in test_cases {
            let result = parser.parse(expression).unwrap();
            match result.root {
                ConditionNode::Condition {
                    operator,
                    field,
                    value,
                    ..
                } => {
                    assert!(
                        std::mem::discriminant(&operator)
                            == std::mem::discriminant(&expected_operator),
                        "Expression {expression} failed: got {operator:?}, expected {expected_operator:?}",
                    );
                    assert_eq!(field, "channel_name");
                    assert!(!value.is_empty());
                }
                _ => panic!("Expression '{expression}' should parse to a condition, not a group"),
            }
        }
    }

    #[test]
    fn test_starts_with_and_ends_with_specifically() {
        let parser = FilterParser::new();

        // Test starts_with
        let result = parser.parse("channel_name starts_with \"BBC\"").unwrap();
        match result.root {
            ConditionNode::Condition {
                operator,
                field,
                value,
                ..
            } => {
                assert!(matches!(operator, FilterOperator::StartsWith));
                assert_eq!(field, "channel_name");
                assert_eq!(value, "BBC");
            }
            _ => panic!("Expected starts_with condition"),
        }

        // Test ends_with
        let result = parser.parse("channel_name ends_with \"HD\"").unwrap();
        match result.root {
            ConditionNode::Condition {
                operator,
                field,
                value,
                ..
            } => {
                assert!(matches!(operator, FilterOperator::EndsWith));
                assert_eq!(field, "channel_name");
                assert_eq!(value, "HD");
            }
            _ => panic!("Expected ends_with condition"),
        }
    }

    // Extended parser tests

    #[test]
    fn test_basic_action_syntax() {
        let parser = FilterParser::new();
        let result = parser
            .parse_extended("group_title equals \"\" SET group_title = \"General\"")
            .unwrap();

        match result {
            ExtendedExpression::ConditionWithActions { condition, actions } => {
                // Verify condition
                match condition.root {
                    ConditionNode::Condition {
                        field,
                        operator,
                        value,
                        ..
                    } => {
                        assert_eq!(field, "group_title");
                        assert!(matches!(operator, FilterOperator::Equals));
                        assert_eq!(value, "");
                    }
                    _ => panic!("Expected simple condition"),
                }

                // Verify actions
                assert_eq!(actions.len(), 1);
                assert_eq!(actions[0].field, "group_title");
                assert!(matches!(actions[0].operator, ActionOperator::Set));
                match &actions[0].value {
                    ActionValue::Literal(val) => assert_eq!(val, "General"),
                    _ => panic!("Expected literal value"),
                }
            }
            _ => panic!("Expected condition with actions"),
        }
    }

    #[test]
    fn test_multiple_actions() {
        let parser = FilterParser::new();
        let result = parser.parse_extended("channel_name contains \"sport\" SET group_title = \"Sports\", category = \"entertainment\"").unwrap();

        match result {
            ExtendedExpression::ConditionWithActions { actions, .. } => {
                assert_eq!(actions.len(), 2);

                // First action
                assert_eq!(actions[0].field, "group_title");
                assert!(matches!(actions[0].operator, ActionOperator::Set));
                match &actions[0].value {
                    ActionValue::Literal(val) => assert_eq!(val, "Sports"),
                    _ => panic!("Expected literal value"),
                }

                // Second action
                assert_eq!(actions[1].field, "category");
                assert!(matches!(actions[1].operator, ActionOperator::Set));
                match &actions[1].value {
                    ActionValue::Literal(val) => assert_eq!(val, "entertainment"),
                    _ => panic!("Expected literal value"),
                }
            }
            _ => panic!("Expected condition with actions"),
        }
    }

    #[test]
    fn test_all_assignment_operators() {
        let parser = FilterParser::new();

        // Test Set operator
        let result = parser
            .parse_extended("channel_name contains \"test\" SET group_title = \"Test\"")
            .unwrap();
        if let ExtendedExpression::ConditionWithActions { actions, .. } = result {
            assert!(matches!(actions[0].operator, ActionOperator::Set));
        }

        // Test Append operator
        let result = parser
            .parse_extended("channel_name contains \"test\" SET channel_name += \" [HD]\"")
            .unwrap();
        if let ExtendedExpression::ConditionWithActions { actions, .. } = result {
            assert!(matches!(actions[0].operator, ActionOperator::Append));
        }

        // Test Remove operator
        let result = parser
            .parse_extended("channel_name contains \"test\" SET channel_name -= \"[AD]\"")
            .unwrap();
        if let ExtendedExpression::ConditionWithActions { actions, .. } = result {
            assert!(matches!(actions[0].operator, ActionOperator::Remove));
        }
    }

    #[test]
    fn test_complex_condition_with_actions() {
        let parser = FilterParser::new();
        let result = parser.parse_extended("(channel_name contains \"sport\" OR channel_name contains \"football\") AND language equals \"en\" SET group_title = \"English Sports\"").unwrap();

        match result {
            ExtendedExpression::ConditionWithActions { condition, actions } => {
                // Verify complex condition structure
                match condition.root {
                    ConditionNode::Group { operator, children } => {
                        assert!(matches!(operator, LogicalOperator::And));
                        assert_eq!(children.len(), 2);

                        // First child should be OR group
                        match &children[0] {
                            ConditionNode::Group { operator, children } => {
                                assert!(matches!(operator, LogicalOperator::Or));
                                assert_eq!(children.len(), 2);
                            }
                            _ => panic!("Expected OR group as first child"),
                        }

                        // Second child should be simple condition
                        match &children[1] {
                            ConditionNode::Condition { field, .. } => {
                                assert_eq!(field, "language");
                            }
                            _ => panic!("Expected simple condition as second child"),
                        }
                    }
                    _ => panic!("Expected complex group condition"),
                }

                // Verify action
                assert_eq!(actions.len(), 1);
                assert_eq!(actions[0].field, "group_title");
            }
            _ => panic!("Expected condition with actions"),
        }
    }

    #[test]
    fn test_backward_compatibility() {
        let parser = FilterParser::new();

        // Test that condition-only expressions work with extended parser
        let result = parser
            .parse_extended("channel_name contains \"sport\" AND group_title equals \"TV\"")
            .unwrap();

        match result {
            ExtendedExpression::ConditionOnly(condition) => match condition.root {
                ConditionNode::Group { operator, children } => {
                    assert!(matches!(operator, LogicalOperator::And));
                    assert_eq!(children.len(), 2);
                }
                _ => panic!("Expected group condition"),
            },
            _ => panic!("Expected condition-only expression"),
        }
    }

    #[test]
    fn test_syntax_errors() {
        let parser = FilterParser::new();

        // Missing action after SET
        assert!(
            !parser
                .validate("channel_name contains \"sport\" SET")
                .is_valid
        );

        // Missing assignment operator
        assert!(
            !parser
                .validate("channel_name contains \"sport\" SET group_title \"Sports\"")
                .is_valid
        );

        // Missing value
        assert!(
            !parser
                .validate("channel_name contains \"sport\" SET group_title =")
                .is_valid
        );

        // Missing comma between actions
        assert!(
            !parser
                .validate(
                    "channel_name contains \"sport\" SET group_title = \"Sports\" category = \"TV\""
                )
                .is_valid
        );

        // Unquoted value
        assert!(
            !parser
                .validate("channel_name contains \"sport\" SET group_title = Sports")
                .is_valid
        );
    }

    #[test]
    fn test_semantic_validation() {
        let parser = FilterParser::new();

        // Valid action should pass validation
        assert!(parser.validate("channel_name contains \"sport\" SET group_title = \"Sports\"").is_valid);

        // Note: Field validation is not enabled by default in FilterParser::new()
        // so any field name is accepted - this is the correct behavior
        assert!(parser.validate("channel_name contains \"sport\" SET invalid_field = \"value\"").is_valid);

        // Too long value should fail validation
        let long_value = "a".repeat(300);
        let expr = format!("channel_name contains \"sport\" SET group_title = \"{long_value}\"");
        assert!(!parser.validate(&expr).is_valid);
    }

    #[test]
    fn test_special_characters_in_values() {
        let parser = FilterParser::new();

        // Test with special characters
        let result = parser.parse_extended("channel_name contains \"test\" SET group_title = \"Sports & Entertainment (Premium) [HD]\"").unwrap();

        match result {
            ExtendedExpression::ConditionWithActions { actions, .. } => match &actions[0].value {
                ActionValue::Literal(val) => {
                    assert_eq!(val, "Sports & Entertainment (Premium) [HD]")
                }
                _ => panic!("Expected literal value"),
            },
            _ => panic!("Expected condition with actions"),
        }
    }

    #[test]
    fn test_real_world_scenarios() {
        let parser = FilterParser::new();

        // BBC channel organization
        let result = parser.parse_extended("tvg_id starts_with \"bbc\" SET tvg_logo = \"https://logos.example.com/bbc.png\", group_title = \"BBC Channels\"").unwrap();

        match result {
            ExtendedExpression::ConditionWithActions { actions, .. } => {
                assert_eq!(actions.len(), 2);
                assert_eq!(actions[0].field, "tvg_logo");
                assert_eq!(actions[1].field, "group_title");
                assert!(matches!(actions[0].operator, ActionOperator::Set));
                assert!(matches!(actions[1].operator, ActionOperator::Set));
            }
            _ => panic!("Expected condition with actions"),
        }

        // Channel cleanup
        let result = parser.parse_extended("channel_name contains \"[AD]\" SET channel_name -= \"[AD]\", channel_name -= \"  \", group_title += \" (Audio Description)\"").unwrap();

        match result {
            ExtendedExpression::ConditionWithActions { actions, .. } => {
                assert_eq!(actions.len(), 3);
                assert!(matches!(actions[0].operator, ActionOperator::Remove));
                assert!(matches!(actions[1].operator, ActionOperator::Remove));
                assert!(matches!(actions[2].operator, ActionOperator::Append));
            }
            _ => panic!("Expected condition with actions"),
        }
    }

    #[test]
    fn test_basic_conditional_action_groups() {
        let parser = FilterParser::new();

        // Basic conditional action groups syntax
        let result = parser.parse_extended("(channel_name matches \"BBC\" SET group_title = \"BBC\") AND (channel_name matches \"ITV\" SET group_title = \"ITV\")").unwrap();

        match result {
            ExtendedExpression::ConditionalActionGroups(groups) => {
                assert_eq!(groups.len(), 2);

                // First group: BBC condition and action
                match &groups[0].conditions.root {
                    ConditionNode::Condition {
                        field,
                        operator,
                        value,
                        ..
                    } => {
                        assert_eq!(field, "channel_name");
                        assert!(matches!(operator, crate::models::FilterOperator::Matches));
                        assert_eq!(value, "BBC");
                    }
                    _ => panic!("Expected simple condition for first group"),
                }
                assert_eq!(groups[0].actions.len(), 1);
                assert_eq!(groups[0].actions[0].field, "group_title");
                assert_eq!(groups[0].logical_operator, Some(LogicalOperator::And));

                // Second group: ITV condition and action
                match &groups[1].conditions.root {
                    ConditionNode::Condition {
                        field,
                        operator,
                        value,
                        ..
                    } => {
                        assert_eq!(field, "channel_name");
                        assert!(matches!(operator, crate::models::FilterOperator::Matches));
                        assert_eq!(value, "ITV");
                    }
                    _ => panic!("Expected simple condition for second group"),
                }
                assert_eq!(groups[1].actions.len(), 1);
                assert_eq!(groups[1].actions[0].field, "group_title");
                assert_eq!(groups[1].logical_operator, None);
            }
            _ => panic!("Expected conditional action groups"),
        }
    }

    #[test]
    fn test_complex_conditional_groups_with_regex() {
        let parser = FilterParser::new();

        // Complex example with regex capture groups
        let result = parser.parse_extended("(channel_name matches \"(.+) \\+([0-9]+)\" SET tvg_shift = \"$2\", channel_name = \"$1\") AND (channel_name not matches \".*HD.*\" SET group_title = \"SD Channels\")").unwrap();

        match result {
            ExtendedExpression::ConditionalActionGroups(groups) => {
                assert_eq!(groups.len(), 2);

                // First group with multiple actions
                assert_eq!(groups[0].actions.len(), 2);
                assert_eq!(groups[0].actions[0].field, "tvg_shift");
                assert_eq!(groups[0].actions[1].field, "channel_name");
                match &groups[0].actions[0].value {
                    ActionValue::Literal(v) => assert_eq!(v, "$2"),
                    _ => panic!("Expected literal value"),
                }

                // Second group with NOT operator
                match &groups[1].conditions.root {
                    ConditionNode::Condition {
                        operator, negate, ..
                    } => {
                        assert!(matches!(operator, crate::models::FilterOperator::NotMatches));
                        assert!(!*negate); // NotMatches operator with negate = false
                    }
                    _ => panic!("Expected condition with NotMatches operator"),
                }
            }
            _ => panic!("Expected conditional action groups"),
        }
    }

    #[test]
    fn test_nested_conditions_in_groups() {
        let parser = FilterParser::new();

        // Groups with nested conditions
        let result = parser.parse_extended("(channel_name contains \"sport\" AND group_title equals \"HD\" SET group_title = \"Sports HD\") OR (tvg_id starts_with \"uk.\" SET tvg_logo = \"uk-logo.png\")").unwrap();

        match result {
            ExtendedExpression::ConditionalActionGroups(groups) => {
                assert_eq!(groups.len(), 2);

                // First group should have nested AND condition
                match &groups[0].conditions.root {
                    ConditionNode::Group { operator, children } => {
                        assert!(matches!(operator, LogicalOperator::And));
                        assert_eq!(children.len(), 2);
                    }
                    _ => panic!("Expected group condition with AND"),
                }
                assert_eq!(groups[0].logical_operator, Some(LogicalOperator::Or));

                // Second group should have simple condition
                match &groups[1].conditions.root {
                    ConditionNode::Condition { field, .. } => {
                        assert_eq!(field, "tvg_id");
                    }
                    _ => panic!("Expected simple condition"),
                }
            }
            _ => panic!("Expected conditional action groups"),
        }
    }

    #[test]
    fn test_conditional_groups_syntax_errors() {
        let parser = FilterParser::new();

        // Missing SET keyword
        assert!(
            !parser
                .validate("(channel_name contains \"test\" group_title = \"Test\")")
                .is_valid
        );

        // Missing closing parenthesis
        assert!(
            !parser
                .validate("(channel_name contains \"test\" SET group_title = \"Test\"")
                .is_valid
        );

        // Missing opening parenthesis
        assert!(
            !parser
                .validate("channel_name contains \"test\" SET group_title = \"Test\")")
                .is_valid
        );

        // Empty group
        assert!(
            !parser
                .validate(
                    "() AND (channel_name contains \"test\" SET group_title = \"Test\")"
                )
                .is_valid
        );

        // Missing action after SET
        assert!(
            !parser
                .validate("(channel_name contains \"test\" SET)")
                .is_valid
        );
    }

    #[test]
    fn test_mixed_simple_and_conditional_groups() {
        let parser = FilterParser::new();

        // Should parse as simple expression if no parentheses around conditions
        let result = parser
            .parse_extended("channel_name contains \"test\" SET group_title = \"Test\"")
            .unwrap();
        match result {
            ExtendedExpression::ConditionWithActions { .. } => {
                // This is expected - falls back to simple mode
            }
            _ => panic!("Expected simple condition with actions"),
        }

        // Should parse as conditional groups if parentheses are used
        let result = parser
            .parse_extended("(channel_name contains \"test\" SET group_title = \"Test\")")
            .unwrap();
        match result {
            ExtendedExpression::ConditionalActionGroups(groups) => {
                assert_eq!(groups.len(), 1);
            }
            _ => panic!("Expected conditional action groups"),
        }
    }

    #[test]
    fn test_user_example_syntax() {
        let parser = FilterParser::new();

        // Test complex conditional action groups with proper syntax
        let result = parser.parse_extended("(channel_name matches \"regex\" AND tvg_id matches \"other_regex\" SET tvg_shift = \"$1$2\")").unwrap();

        match result {
            ExtendedExpression::ConditionalActionGroups(groups) => {
                assert_eq!(groups.len(), 1);

                // Should have complex nested conditions (2 conditions joined by AND)
                match &groups[0].conditions.root {
                    ConditionNode::Group { operator, children } => {
                        assert!(matches!(operator, LogicalOperator::And));
                        assert_eq!(children.len(), 2); // Two conditions joined by AND
                    }
                    _ => panic!("Expected group condition with multiple ANDs"),
                }

                // Should have one action with capture group reference
                assert_eq!(groups[0].actions.len(), 1);
                assert_eq!(groups[0].actions[0].field, "tvg_shift");
                match &groups[0].actions[0].value {
                    ActionValue::Literal(v) => assert_eq!(v, "$1$2"),
                    _ => panic!("Expected literal value with capture groups"),
                }
            }
            _ => panic!("Expected conditional action groups"),
        }
    }

    // Tests for extended expressions examples from documentation

    #[test]
    fn test_simple_examples() {
        let parser = FilterParser::new();

        // Example 1: Basic group assignment
        let result = parser
            .parse_extended("channel_name contains \"sport\" SET group_title = \"Sports\"")
            .unwrap();
        match result {
            ExtendedExpression::ConditionWithActions { actions, .. } => {
                assert_eq!(actions.len(), 1);
                assert_eq!(actions[0].field, "group_title");
                match &actions[0].value {
                    ActionValue::Literal(v) => assert_eq!(v, "Sports"),
                    _ => panic!("Expected literal value"),
                }
            }
            _ => panic!("Expected condition with actions"),
        }

        // Example 4: Multiple actions
        let result = parser.parse_extended("channel_name contains \"HD\" SET group_title = \"HD Channels\", tvg_logo = \"https://example.com/hd.png\"").unwrap();
        match result {
            ExtendedExpression::ConditionWithActions { actions, .. } => {
                assert_eq!(actions.len(), 2);
                assert_eq!(actions[0].field, "group_title");
                assert_eq!(actions[1].field, "tvg_logo");
            }
            _ => panic!("Expected condition with multiple actions"),
        }
    }

    #[test]
    fn test_intermediate_examples() {
        let parser = FilterParser::new();

        // Example 7: Multiple condition matching
        let result = parser.parse_extended("channel_name contains \"BBC\" AND channel_name contains \"HD\" SET group_title = \"BBC HD\"").unwrap();
        match result {
            ExtendedExpression::ConditionWithActions { condition, actions } => {
                match &condition.root {
                    ConditionNode::Group { operator, children } => {
                        assert!(matches!(operator, LogicalOperator::And));
                        assert_eq!(children.len(), 2);
                    }
                    _ => panic!("Expected AND group"),
                }
                assert_eq!(actions.len(), 1);
                assert_eq!(actions[0].field, "group_title");
            }
            _ => panic!("Expected condition with actions"),
        }

        // Example 9: Regex with capture groups
        let result = parser
            .parse_extended("channel_name matches \"^([A-Z]+) .*\" SET tvg_id = \"$1\"")
            .unwrap();
        match result {
            ExtendedExpression::ConditionWithActions { actions, .. } => {
                assert_eq!(actions.len(), 1);
                assert_eq!(actions[0].field, "tvg_id");
                match &actions[0].value {
                    ActionValue::Literal(v) => assert_eq!(v, "$1"),
                    _ => panic!("Expected capture group reference"),
                }
            }
            _ => panic!("Expected condition with actions"),
        }

        // Example 10: Timeshift extraction with multiple actions
        let result = parser.parse_extended("channel_name matches \"(.+) \\\\+([0-9]+)\" SET channel_name = \"$1\", tvg_shift = \"$2\"").unwrap();
        match result {
            ExtendedExpression::ConditionWithActions { actions, .. } => {
                assert_eq!(actions.len(), 2);
                assert_eq!(actions[0].field, "channel_name");
                assert_eq!(actions[1].field, "tvg_shift");
                match &actions[1].value {
                    ActionValue::Literal(v) => assert_eq!(v, "$2"),
                    _ => panic!("Expected capture group reference"),
                }
            }
            _ => panic!("Expected condition with actions"),
        }
    }

    #[test]
    fn test_complex_conditional_groups() {
        let parser = FilterParser::new();

        // Example 12: Advanced regional grouping with OR logic
        let expr = "(tvg_id matches \"^(uk|gb)\\.\" SET group_title = \"United Kingdom\") OR (tvg_id matches \"^us\\.\" SET group_title = \"United States\")";
        let result = parser.parse_extended(expr).unwrap();

        match result {
            ExtendedExpression::ConditionalActionGroups(groups) => {
                assert_eq!(groups.len(), 2);

                // First group
                assert_eq!(groups[0].actions.len(), 1);
                assert_eq!(groups[0].actions[0].field, "group_title");
                assert_eq!(groups[0].logical_operator, Some(LogicalOperator::Or));

                // Second group
                assert_eq!(groups[1].actions.len(), 1);
                assert_eq!(groups[1].actions[0].field, "group_title");
                assert_eq!(groups[1].logical_operator, None);
            }
            _ => panic!("Expected conditional action groups"),
        }
    }

    #[test]
    fn test_deeply_nested_conditions() {
        let parser = FilterParser::new();

        // Example 16: Nested conditional logic
        let expr = "((channel_name matches \"^(BBC|ITV|Channel [45])\" AND tvg_id not equals \"\") OR (channel_name matches \"Sky (Sports|Movies|News)\" AND group_title equals \"\")) SET group_title = \"Premium UK\"";
        let result = parser.parse_extended(expr).unwrap();

        match result {
            ExtendedExpression::ConditionWithActions { condition, actions } => {
                // Should have deeply nested structure
                match &condition.root {
                    ConditionNode::Group { operator, children } => {
                        assert!(matches!(operator, LogicalOperator::Or)); // OR at top level
                        assert_eq!(children.len(), 2);

                        // Each child should be an AND group
                        for child in children {
                            match child {
                                ConditionNode::Group { operator, children } => {
                                    assert!(matches!(operator, LogicalOperator::And)); // AND groups
                                    assert_eq!(children.len(), 2);
                                }
                                _ => panic!("Expected AND groups as children"),
                            }
                        }
                    }
                    _ => panic!("Expected top-level OR group"),
                }

                assert_eq!(actions.len(), 1);
                assert_eq!(actions[0].field, "group_title");
            }
            _ => panic!("Expected condition with actions"),
        }
    }

    #[test]
    fn test_multi_stage_processing() {
        let parser = FilterParser::new();

        // Example 17: Multi-stage processing with sequential operations
        let expr = "(channel_name matches \"^\\\\[([A-Z]{2,3})\\\\] (.+)\" SET tvg_id = \"$1\", channel_name = \"$2\") AND (tvg_id matches \"^(BBC|ITV|C4|C5)$\" SET group_title = \"UK Terrestrial\")";
        let result = parser.parse_extended(expr).unwrap();

        match result {
            ExtendedExpression::ConditionalActionGroups(groups) => {
                assert_eq!(groups.len(), 2);

                // First stage: Extract and set multiple fields
                assert_eq!(groups[0].actions.len(), 2);
                assert_eq!(groups[0].actions[0].field, "tvg_id");
                assert_eq!(groups[0].actions[1].field, "channel_name");

                // Second stage: Categorize based on extracted TVG ID
                assert_eq!(groups[1].actions.len(), 1);
                assert_eq!(groups[1].actions[0].field, "group_title");

                assert_eq!(groups[0].logical_operator, Some(LogicalOperator::And));
            }
            _ => panic!("Expected conditional action groups"),
        }
    }

    #[test]
    fn test_logo_assignment_patterns() {
        let parser = FilterParser::new();

        // Example 18: Dynamic logo assignment with capture groups
        let expr = "(channel_name matches \"^(BBC One|BBC Two|BBC Three)\" SET tvg_logo = \"@logo:bbc-$1\") AND (channel_name matches \"^(ITV|ITV2|ITV3)\" SET tvg_logo = \"@logo:itv-$1\")";
        let result = parser.parse_extended(expr).unwrap();

        match result {
            ExtendedExpression::ConditionalActionGroups(groups) => {
                assert_eq!(groups.len(), 2);

                for group in &groups {
                    assert_eq!(group.actions.len(), 1);
                    assert_eq!(group.actions[0].field, "tvg_logo");
                    match &group.actions[0].value {
                        ActionValue::Literal(v) => {
                            assert!(v.starts_with("@logo:"));
                            assert!(v.contains("$1"));
                        }
                        _ => panic!("Expected logo reference with capture group"),
                    }
                }
            }
            _ => panic!("Expected conditional action groups"),
        }
    }

    #[test]
    fn test_comprehensive_normalization() {
        let parser = FilterParser::new();

        // Example 19: Comprehensive channel normalization with multiple stages
        let expr = "(channel_name matches \"^(.+?) *(?:\\\\|| - |: ).*(?:HD|FHD|4K|UHD)\" SET channel_name = \"$1\", group_title = \"High Definition\") AND (channel_name matches \"^(.+?) *\\\\+([0-9]+)h?$\" SET channel_name = \"$1\", tvg_shift = \"$2\")";
        let result = parser.parse_extended(expr).unwrap();

        match result {
            ExtendedExpression::ConditionalActionGroups(groups) => {
                assert_eq!(groups.len(), 2);

                // First stage: HD cleanup
                assert_eq!(groups[0].actions.len(), 2);
                assert_eq!(groups[0].actions[0].field, "channel_name");
                assert_eq!(groups[0].actions[1].field, "group_title");

                // Second stage: Timeshift extraction
                assert_eq!(groups[1].actions.len(), 2);
                assert_eq!(groups[1].actions[0].field, "channel_name");
                assert_eq!(groups[1].actions[1].field, "tvg_shift");
            }
            _ => panic!("Expected conditional action groups"),
        }
    }

    #[test]
    fn test_real_world_provider_examples() {
        let parser = FilterParser::new();

        // Sky UK Channel Normalization
        let expr = "(channel_name matches \"Sky (Sports|Movies|News) (.+)\" SET channel_name = \"Sky $1 $2\", group_title = \"Sky\") AND (channel_name starts_with \"Sky Sports\" SET group_title = \"Sky Sports\")";
        let result = parser.parse_extended(expr).unwrap();

        match result {
            ExtendedExpression::ConditionalActionGroups(groups) => {
                assert_eq!(groups.len(), 2);

                // First group has multiple actions
                assert_eq!(groups[0].actions.len(), 2);
                assert_eq!(groups[0].actions[0].field, "channel_name");
                assert_eq!(groups[0].actions[1].field, "group_title");

                // Second group is more specific categorization
                assert_eq!(groups[1].actions.len(), 1);
                assert_eq!(groups[1].actions[0].field, "group_title");
            }
            _ => panic!("Expected conditional action groups"),
        }

        // US Cable Provider with timeshift
        let expr = "(channel_name matches \"^([A-Z]+) East \\\\+([0-9]+)\" SET channel_name = \"$1 East\", tvg_shift = \"$2\") AND (channel_name contains \"ESPN\" SET group_title = \"ESPN Family\")";
        let result = parser.parse_extended(expr).unwrap();

        match result {
            ExtendedExpression::ConditionalActionGroups(groups) => {
                assert_eq!(groups.len(), 2);

                // Timeshift extraction
                assert_eq!(groups[0].actions.len(), 2);
                assert_eq!(groups[0].actions[1].field, "tvg_shift");

                // ESPN grouping
                assert_eq!(groups[1].actions.len(), 1);
                assert_eq!(groups[1].actions[0].field, "group_title");
            }
            _ => panic!("Expected conditional action groups"),
        }
    }

    #[test]
    fn test_error_cases_for_examples() {
        let parser = FilterParser::new();

        // Test malformed regex
        assert!(
            !parser
                .validate("channel_name matches \"[unclosed\" SET group_title = \"Test\"")
                .is_valid
        );

        // Test missing SET keyword
        assert!(
            !parser
                .validate("channel_name contains \"test\" group_title = \"Test\"")
                .is_valid
        );

        // Test unbalanced parentheses in conditional groups
        assert!(
            !parser
                .validate("(channel_name contains \"test\" SET group_title = \"Test\" AND")
                .is_valid
        );

        // Test empty action value (should be valid)
        assert!(
            parser
                .validate("channel_name contains \"test\" SET group_title = \"\"")
                .is_valid
        );

        // Note: Field validation is not enabled by default in FilterParser::new()
        // so any field name is accepted - this is the correct behavior
        assert!(parser.validate("invalid_field contains \"test\" SET group_title = \"Test\"").is_valid);
    }

    #[test]
    fn test_validate_method_comprehensive() {
        let valid_fields = vec![
            "channel_name".to_string(),
            "group_title".to_string(),
            "stream_url".to_string(),
            "tvg_id".to_string(),
        ];
        let parser = ExpressionParser::new().with_fields(valid_fields);

        // Test valid simple expressions
        assert!(parser.validate("channel_name contains \"HD\"").is_valid);
        assert!(parser.validate("group_title equals \"Sports\"").is_valid);
        assert!(parser.validate("stream_url starts_with \"https\"").is_valid);

        // Test valid complex expressions  
        assert!(parser.validate("(channel_name contains \"HD\" OR group_title equals \"Movies\") AND stream_url starts_with \"https\"").is_valid);
        assert!(parser.validate("channel_name not contains \"SD\" AND tvg_id matches \"^[0-9]+$\"").is_valid);

        // Test invalid field names (should fail with proper field validation)
        assert!(!parser.validate("invalid_field contains \"test\"").is_valid);
        assert!(!parser.validate("chanxnlname contains \"sport\"").is_valid);
        assert!(!parser.validate("group_tittle equals \"Movies\"").is_valid);

        // Test syntax errors
        assert!(!parser.validate("channel_name contains").is_valid);
        assert!(!parser.validate("channel_name \"HD\"").is_valid);
        assert!(!parser.validate("(channel_name contains \"HD\"").is_valid);
        assert!(!parser.validate("channel_name contains \"HD\" AND").is_valid);

        // Test invalid operators
        assert!(!parser.validate("channel_name invalid_op \"HD\"").is_valid);
        assert!(!parser.validate("channel_name == \"HD\"").is_valid); // Should use 'equals'

        // Test empty expressions
        assert!(!parser.validate("").is_valid);
        assert!(!parser.validate("   ").is_valid);
    }

    #[test]
    fn test_field_validation_edge_cases() {
        let valid_fields = vec![
            "channel_name".to_string(),
            "group_title".to_string(),
        ];
        let parser = ExpressionParser::new().with_fields(valid_fields);

        // Test case sensitivity in field names
        assert!(!parser.validate("CHANNEL_NAME contains \"HD\"").is_valid);
        assert!(!parser.validate("Channel_Name contains \"HD\"").is_valid);
        
        // Test partial field name matches (should fail)
        assert!(!parser.validate("channel contains \"HD\"").is_valid);
        assert!(!parser.validate("name contains \"HD\"").is_valid);
        
        // Test field names with special characters (valid fields should work)
        let special_parser = ExpressionParser::new().with_fields(vec![
            "field-with-dash".to_string(),
            "field_with_underscore".to_string(),
            "field123".to_string(),
        ]);
        assert!(special_parser.validate("field-with-dash contains \"test\"").is_valid);
        assert!(special_parser.validate("field_with_underscore contains \"test\"").is_valid);
        assert!(special_parser.validate("field123 contains \"test\"").is_valid);
        
        // Test completely empty field list (should skip validation)
        let no_fields_parser = ExpressionParser::new().with_fields(vec![]);
        assert!(no_fields_parser.validate("any_field contains \"test\"").is_valid);
    }

    #[test]
    fn test_expression_validation_with_actions() {
        let valid_fields = vec![
            "channel_name".to_string(),
            "group_title".to_string(),
            "stream_url".to_string(),
        ];
        let parser = ExpressionParser::new().with_fields(valid_fields);

        // Test valid expressions with actions
        assert!(parser.validate("channel_name contains \"HD\" SET group_title = \"High Definition\"").is_valid);
        // Note: Complex string functions like SUBSTRING are not supported by the current parser
        assert!(parser.validate("stream_url starts_with \"http\" SET stream_url = \"https://example.com\"").is_valid);

        // Test invalid field in condition
        assert!(!parser.validate("invalid_field contains \"test\" SET group_title = \"Test\"").is_valid);
        
        // Test invalid field in action
        assert!(!parser.validate("channel_name contains \"test\" SET invalid_field = \"Test\"").is_valid);
        
        // Test valid condition with invalid action field
        assert!(!parser.validate("channel_name contains \"HD\" SET unknown_field = \"Test\"").is_valid);
    }

    #[test] 
    fn test_validation_error_messages() {
        let valid_fields = vec![
            "channel_name".to_string(),
            "group_title".to_string(),
        ];
        let parser = ExpressionParser::new().with_fields(valid_fields);

        // Test that error messages contain helpful information
        let result = parser.validate("invalid_field contains \"test\"");
        assert!(!result.is_valid, "Expected validation to fail for invalid field");
        assert!(!result.errors.is_empty());
        let error_msg = result.errors[0].message.clone();
        assert!(error_msg.contains("Unknown field") || error_msg.contains("Field"));

        // Test syntax error messages
        let result = parser.validate("channel_name contains");
        assert!(!result.is_valid, "Expected validation to fail for incomplete expression");
        assert!(!result.errors.is_empty());
        let error_msg = result.errors[0].message.clone();
        // Should contain some indication of syntax error
        assert!(!error_msg.is_empty());
    }


    #[test]
    fn test_complex_nested_expressions() {
        let valid_fields = vec![
            "channel_name".to_string(),
            "group_title".to_string(),
            "stream_url".to_string(),
            "tvg_id".to_string(),
        ];
        let parser = ExpressionParser::new().with_fields(valid_fields);

        // Test deeply nested valid expressions
        assert!(parser.validate("((channel_name contains \"HD\" OR channel_name contains \"4K\") AND (group_title equals \"Movies\" OR group_title equals \"Sports\")) OR (stream_url starts_with \"https\" AND tvg_id matches \"^[0-9]+$\")").is_valid);

        // Test nested expression with invalid field
        assert!(!parser.validate("((channel_name contains \"HD\" OR invalid_field contains \"4K\") AND group_title equals \"Movies\")").is_valid);

        // Test multiple logical operators
        assert!(parser.validate("channel_name contains \"HD\" AND group_title equals \"Sports\" OR stream_url starts_with \"https\"").is_valid);
    }

    #[test]
    fn test_structured_validation_basic() {
        let valid_fields = vec![
            "channel_name".to_string(),
            "group_title".to_string(),
            "stream_url".to_string(),
            "tvg_id".to_string(),
        ];
        let parser = ExpressionParser::new().with_fields(valid_fields);

        // Test valid expression
        let result = parser.validate("channel_name contains \"HD\"");
        assert!(result.is_valid);
        assert!(result.errors.is_empty());
        assert!(result.expression_tree.is_some());

        // Test invalid field
        let result = parser.validate("invalid_field contains \"HD\"");
        assert!(!result.is_valid);
        assert_eq!(result.errors.len(), 1);
        assert_eq!(result.errors[0].category, ExpressionErrorCategory::Field);
        assert_eq!(result.errors[0].error_type, "unknown_field");
        assert!(result.errors[0].message.contains("invalid_field"));
    }

    #[test]
    fn test_structured_validation_syntax_errors() {
        let parser = ExpressionParser::new();

        // Test unclosed quote
        let result = parser.validate("channel_name contains \"unclosed");
        assert!(!result.is_valid);
        assert_eq!(result.errors.len(), 1);
        assert_eq!(result.errors[0].category, ExpressionErrorCategory::Syntax);
        assert_eq!(result.errors[0].error_type, "unclosed_quote");
        assert!(result.errors[0].position.is_some());

        // Test unbalanced parentheses
        let result = parser.validate("(channel_name contains \"value\"");
        assert!(!result.is_valid);
        assert_eq!(result.errors.len(), 1);
        assert_eq!(result.errors[0].category, ExpressionErrorCategory::Syntax);
        assert_eq!(result.errors[0].error_type, "unclosed_parenthesis");

        // Test empty expression
        let result = parser.validate("");
        assert!(!result.is_valid);
        assert_eq!(result.errors.len(), 1);
        assert_eq!(result.errors[0].error_type, "empty_expression");
    }

    #[test]
    fn test_structured_validation_operator_errors() {
        let parser = ExpressionParser::new();

        // Test invalid logical operator
        let result = parser.validate("channel_name contains \"test\" && group_title equals \"value\"");
        assert!(!result.is_valid);
        assert_eq!(result.errors.len(), 1);
        assert_eq!(result.errors[0].category, ExpressionErrorCategory::Operator);
        assert_eq!(result.errors[0].error_type, "invalid_logical_operator");
        assert!(result.errors[0].suggestion.as_ref().unwrap().contains("AND"));

        // Test operator typo
        let result = parser.validate("channel_name containz \"test\"");
        assert!(!result.is_valid);
        assert_eq!(result.errors.len(), 1);
        assert_eq!(result.errors[0].category, ExpressionErrorCategory::Operator);
        assert_eq!(result.errors[0].error_type, "operator_typo");
        assert!(result.errors[0].suggestion.as_ref().unwrap().contains("contains"));
    }

    #[test]
    fn test_structured_validation_regex_errors() {
        let parser = ExpressionParser::new();

        // Test invalid regex pattern
        let result = parser.validate("channel_name matches \"[unclosed\"");
        assert!(!result.is_valid);
        assert_eq!(result.errors.len(), 1);
        assert_eq!(result.errors[0].category, ExpressionErrorCategory::Value);
        assert_eq!(result.errors[0].error_type, "invalid_regex");
        assert!(result.errors[0].message.contains("Invalid regular expression"));
    }

    #[test]
    fn test_structured_validation_field_suggestions() {
        let valid_fields = vec![
            "channel_name".to_string(),
            "group_title".to_string(),
            "stream_url".to_string(),
        ];
        let parser = ExpressionParser::new().with_fields(valid_fields);

        // Test field name with typo
        let result = parser.validate("channe_name contains \"test\"");
        assert!(!result.is_valid);
        assert_eq!(result.errors.len(), 1);
        assert_eq!(result.errors[0].category, ExpressionErrorCategory::Field);
        
        // Should suggest similar field name
        let suggestion = result.errors[0].suggestion.as_ref().unwrap();
        assert!(suggestion.contains("channel_name") || suggestion.contains("Available fields"));

        // Test completely unknown field
        let result = parser.validate("unknown_field contains \"test\"");
        assert!(!result.is_valid);
        assert_eq!(result.errors.len(), 1);
        let suggestion = result.errors[0].suggestion.as_ref().unwrap();
        assert!(suggestion.contains("Available fields"));
    }

    #[test]
    fn test_structured_validation_multiple_errors() {
        let valid_fields = vec!["channel_name".to_string()];
        let parser = ExpressionParser::new().with_fields(valid_fields);

        // Expression with multiple errors: invalid field + invalid operator
        let result = parser.validate("invalid_field containz \"test\"");
        assert!(!result.is_valid);
        // Should catch the operator typo first during tokenization
        assert_eq!(result.errors.len(), 1);
        assert_eq!(result.errors[0].category, ExpressionErrorCategory::Operator);
    }

    #[test]
    fn test_structured_validation_actions() {
        let valid_fields = vec![
            "channel_name".to_string(),
            "group_title".to_string(),
        ];
        let parser = ExpressionParser::new().with_fields(valid_fields);

        // Test valid action expression
        let result = parser.validate("channel_name contains \"HD\" SET group_title = \"High Definition\"");
        assert!(result.is_valid);
        assert!(result.errors.is_empty());
        assert!(result.expression_tree.is_some());

        // Test action with invalid field
        let result = parser.validate("channel_name contains \"HD\" SET invalid_field = \"value\"");
        assert!(!result.is_valid);
        assert_eq!(result.errors.len(), 1);
        assert_eq!(result.errors[0].category, ExpressionErrorCategory::Field);

        // Test action with value too long
        let long_value = "a".repeat(300);
        let expression = format!("channel_name contains \"HD\" SET group_title = \"{long_value}\"");
        let result = parser.validate(&expression);
        assert!(!result.is_valid);
        assert_eq!(result.errors.len(), 1);
        assert_eq!(result.errors[0].category, ExpressionErrorCategory::Value);
        assert_eq!(result.errors[0].error_type, "value_too_long");
    }

    #[test]
    fn test_structured_validation_context_and_suggestions() {
        let parser = ExpressionParser::new();

        // Test that context and suggestions are provided
        let result = parser.validate("channel_name containz");
        assert!(!result.is_valid);
        assert_eq!(result.errors.len(), 1);
        
        let error = &result.errors[0];
        assert!(error.context.is_some());
        assert!(error.suggestion.is_some());
        assert!(error.details.is_some());
        
        // Position should be provided for tokenization errors
        if error.error_type == "operator_typo" {
            assert!(error.position.is_some());
        }
    }

    // Comprehensive tests for new comparison operators

    // Time helper parsing enhancement tests
    mod time_helper_tests {
        use super::*;

        /// Test enhanced time parsing with parse_flexible() function
        #[test]
        fn test_time_helper_with_parse_flexible() {
            let parser = FilterParser::new();
            
            // Test various time formats that should work with parse_flexible()
            let time_formats = [
                "@time:2024-01-15T10:30:00Z",           // RFC3339
                "@time:2024-01-15 10:30:00",            // SQLite format
                "@time:15/01/2024 10:30:00",            // European format
                "@time:01/15/2024 10:30:00",            // US format
                "@time:20240115103000 +0000",           // XMLTV format
                "@time:1705315800",                      // Unix timestamp
            ];
            
            for time_format in &time_formats {
                let expr = format!("last_updated greater_than \"{time_format}\"");
                let result = parser.parse(&expr);
                
                assert!(result.is_ok(), "Failed to parse time format: {time_format}");
                
                if let Ok(parsed) = result {
                    if let ConditionNode::Condition { field, operator, value, .. } = &parsed.root {
                        assert_eq!(field, "last_updated");
                        assert!(matches!(operator, FilterOperator::GreaterThan));
                        assert!(value.starts_with("@time:"));
                    } else {
                        panic!("Expected condition node");
                    }
                }
            }
        }

        /// Test time helper validation
        #[test]
        fn test_time_helper_validation() {
            let parser = ExpressionParser::new();
            
            // Test valid time expressions
            let valid_expressions = [
                "last_updated greater_than \"@time:2024-01-15T10:30:00Z\"",
                "created_at greater_than_or_equal \"@time:1705315800\"",
                "updated_at less_than \"@time:2024-01-15 10:30:00\"",
            ];
            
            for expr in &valid_expressions {
                let result = parser.validate(expr);
                assert!(result.is_valid, "Valid time expression should pass: {expr}");
            }
            
            // Test that "not" modifier syntax also works
            let test_expr = "channel_name not contains \"HD\"";
            let result = parser.validate(test_expr);
            assert!(result.is_valid, "Not modifier syntax should work: {test_expr}");
        }

        /// Test time helper error handling
        #[test]
        fn test_time_helper_error_handling() {
            let parser = FilterParser::new();
            
            // Test time helpers that might cause parsing issues
            let expressions_with_potential_issues = [
                "last_updated greater_than \"@time:invalid-date\"",
                "created_at greater_than_or_equal \"@time:\"",  // Empty time value
                "updated_at less_than \"@time:2024-99-99\"", // Invalid date
            ];
            
            for expr in &expressions_with_potential_issues {
                let result = parser.parse(expr);
                // Should parse successfully (validation happens during execution)
                assert!(result.is_ok(), "Expression should parse even with potentially invalid time: {expr}");
            }
        }

        /// Test time helper integration with comparison operators
        #[test]
        fn test_time_helper_with_comparison_operators() {
            let parser = FilterParser::new();
            
            // Test time range queries
            let expr = "last_updated greater_than_or_equal \"@time:2024-01-01T00:00:00Z\" AND last_updated less_than_or_equal \"@time:2024-12-31T23:59:59Z\"";
            let result = parser.parse(expr).unwrap();
            
            // Use logical expression helper to validate structure
            if let ConditionNode::Group { operator, children } = &result.root {
                assert!(matches!(operator, LogicalOperator::And));
                assert_eq!(children.len(), 2);
                
                // Validate first condition
                if let ConditionNode::Condition { field, operator, value, .. } = &children[0] {
                    assert_eq!(field, "last_updated");
                    assert!(matches!(operator, FilterOperator::GreaterThanOrEqual));
                    assert!(value.contains("2024-01-01"));
                }
                
                // Validate second condition
                if let ConditionNode::Condition { field, operator, value, .. } = &children[1] {
                    assert_eq!(field, "last_updated");
                    assert!(matches!(operator, FilterOperator::LessThanOrEqual));
                    assert!(value.contains("2024-12-31"));
                }
            } else {
                panic!("Expected logical group expression");
            }
        }

        /// Test time helper in data mapping context
        #[test]
        fn test_time_helper_in_data_mapping() {
            let parser = FilterParser::new();
            
            let expr = "last_updated greater_than \"@time:2024-01-01T00:00:00Z\" SET group_title = \"Recent Content\"";
            let result = parser.parse_extended(expr).unwrap();
            
            match result {
                ExtendedExpression::ConditionWithActions { condition, actions } => {
                    if let ConditionNode::Condition { field, operator, value, .. } = &condition.root {
                        assert_eq!(field, "last_updated");
                        assert!(matches!(operator, FilterOperator::GreaterThan));
                        assert!(value.starts_with("@time:"));
                    } else {
                        panic!("Expected condition node");
                    }
                    
                    assert_eq!(actions.len(), 1);
                    assert_eq!(actions[0].field, "group_title");
                }
                _ => panic!("Expected condition with actions for time helper data mapping"),
            }
        }
    }
}
