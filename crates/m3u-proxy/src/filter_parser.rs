// Parser for complex filter expressions with nested conditions
// Supports expressions like: (A=B AND (C=D OR E=F) AND X=Y)

use crate::models::{
    Action, ActionOperator, ActionValue, ConditionNode, ConditionTree, ExtendedExpression,
    FilterOperator, LogicalOperator,
};
use anyhow::{Result, anyhow};
use tracing::{debug, info, trace, warn};

#[derive(Debug, Clone)]
pub struct FilterParser {
    operators: Vec<String>,
    logical_operators: Vec<String>,
    valid_fields: Vec<String>,
}

impl FilterParser {
    pub fn new() -> Self {
        Self {
            operators: vec![
                // Negated operators (must come first to match before base operators)
                "not_starts_with".to_string(),
                "not_ends_with".to_string(),
                "not_contains".to_string(),
                "not_equals".to_string(),
                "not_matches".to_string(),
                // Base operators
                "starts_with".to_string(),
                "ends_with".to_string(),
                "contains".to_string(),
                "equals".to_string(),
                "matches".to_string(),
            ],
            logical_operators: vec!["AND".to_string(), "OR".to_string()],
            valid_fields: vec![], // Empty by default, will be set via with_fields
        }
    }

    pub fn with_fields(mut self, fields: Vec<String>) -> Self {
        self.valid_fields = fields;
        self
    }

    /// Parse a text expression into a ConditionTree (backward compatibility)
    /// Example: "channel_name contains \"sport\" AND (group_title equals \"HD\" OR group_title equals \"4K\")"
    pub fn parse(&self, expression: &str) -> Result<ConditionTree> {
        let tokens = self.tokenize(expression)?;
        let root = self.parse_expression(&tokens, &mut 0)?;
        Ok(ConditionTree { root })
    }

    /// Parse a text expression into an ExtendedExpression (supports actions)
    /// Example: "channel_name contains \"sport\" SET group_title = \"Sports\""
    /// Advanced: "(channel_name matches \"regex\" SET tvg_shift = $1$2 AND channel_name not matches \"regex\" AND tvg_id matches \"regex\")"
    pub fn parse_extended(&self, expression: &str) -> Result<ExtendedExpression> {
        info!(
            "Parsing extended expression: '{}' (length: {} chars)",
            expression,
            expression.len()
        );

        let tokens = self.tokenize(expression)?;
        trace!("Tokenized into {} tokens", tokens.len());

        let mut pos = 0;

        // Try to parse as conditional action groups first
        if let Ok(groups) = self.parse_conditional_action_groups(&tokens, &mut 0) {
            info!(
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

            info!(
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

            Ok(ExtendedExpression::ConditionWithActions { condition, actions })
        } else {
            // Ensure we've consumed all tokens
            if pos < tokens.len() {
                return Err(anyhow!(
                    "Unexpected tokens after condition at position {}",
                    pos
                ));
            }

            info!(
                "Successfully parsed as condition only:\n\
                 │ Expression: '{}'\n\
                 └─ Conditions: {}",
                expression,
                self.count_conditions(&condition)
            );

            Ok(ExtendedExpression::ConditionOnly(condition))
        }
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
                debug!("No more logical operators found, finished parsing groups");
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
                    debug!(
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
            debug!("No SET keyword found in conditional group");
            return Err(anyhow!("Expected SET keyword in conditional group"));
        }

        // Parse conditions from collected tokens
        let mut condition_pos = 0;
        let condition_root = self.parse_expression(&condition_tokens, &mut condition_pos)?;
        let conditions = ConditionTree {
            root: condition_root,
        };
        debug!(
            "Parsed conditions with {} total condition nodes",
            self.count_conditions(&conditions)
        );

        // Parse actions until closing parenthesis
        let actions = self.parse_action_list(tokens, pos)?;
        debug!("Parsed {} actions", actions.len());

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
        debug!("Consumed closing parenthesis, conditional group complete");

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
                            .map_or(true, |c| c.is_whitespace() || c == '(' || c == ')')
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

            // Handle SET keyword
            if remaining.to_uppercase().starts_with("SET") {
                let end_pos = 3;
                if end_pos == remaining.len()
                    || remaining
                        .chars()
                        .nth(end_pos)
                        .map_or(true, |c| c.is_whitespace())
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
                            "not_starts_with" => FilterOperator::NotStartsWith,
                            "not_ends_with" => FilterOperator::NotEndsWith,
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

    /// Validate an extended expression for semantic correctness
    pub fn validate_extended(&self, expression: &ExtendedExpression) -> Result<()> {
        debug!("Validating extended expression");

        match expression {
            ExtendedExpression::ConditionOnly(condition) => {
                debug!(
                    "Validating condition-only expression with {} conditions",
                    self.count_conditions(condition)
                );
                // Validate condition field names
                self.validate_condition_tree_fields(condition)
            }
            ExtendedExpression::ConditionWithActions { condition, actions } => {
                debug!(
                    "Validating condition-with-actions expression: {} conditions, {} actions",
                    self.count_conditions(condition),
                    actions.len()
                );
                // Validate both condition fields and actions
                self.validate_condition_tree_fields(condition)?;
                self.validate_actions(actions)
            }
            ExtendedExpression::ConditionalActionGroups(groups) => {
                trace!(
                    "Validating conditional action groups: {} groups",
                    groups.len()
                );
                // Validate each group's conditions and actions
                for (i, group) in groups.iter().enumerate() {
                    debug!(
                        "Validating group {}: {} conditions, {} actions",
                        i + 1,
                        self.count_conditions(&group.conditions),
                        group.actions.len()
                    );
                    self.validate_condition_tree_fields(&group.conditions)?;
                    self.validate_actions(&group.actions)?;
                }
                info!(
                    "Successfully validated all {} conditional action groups",
                    groups.len()
                );
                Ok(())
            }
        }
    }

    /// Validate a list of actions for semantic correctness
    fn validate_actions(&self, actions: &[Action]) -> Result<()> {
        for action in actions {
            self.validate_action(action)?;
        }
        Ok(())
    }

    /// Validate a single action for semantic correctness
    fn validate_action(&self, action: &Action) -> Result<()> {
        // Validate field exists
        self.validate_field_name(&action.field)?;

        // Validate operator compatibility with field
        self.validate_action_operator(&action.field, &action.operator)?;

        // Validate value format
        self.validate_action_value(&action.field, &action.value)?;

        Ok(())
    }

    /// Validate that a field name exists for stream sources
    fn validate_field_name(&self, field: &str) -> Result<()> {
        if self.valid_fields.is_empty() {
            // If no fields are configured, skip validation (backwards compatibility)
            return Ok(());
        }

        if !self.valid_fields.iter().any(|f| f == field) {
            return Err(anyhow!(
                "Unknown field '{}'. Valid fields are: {}",
                field,
                self.valid_fields.join(", ")
            ));
        }

        Ok(())
    }

    /// Validate field names in condition tree
    fn validate_condition_tree_fields(&self, condition_tree: &ConditionTree) -> Result<()> {
        self.validate_condition_node_fields(&condition_tree.root)
    }

    /// Validate field names in condition nodes
    fn validate_condition_node_fields(&self, condition: &ConditionNode) -> Result<()> {
        match condition {
            ConditionNode::Condition { field, .. } => self.validate_field_name(field),
            ConditionNode::Group { children, .. } => {
                for child in children {
                    self.validate_condition_node_fields(child)?;
                }
                Ok(())
            }
        }
    }

    /// Validate operator compatibility with field type
    fn validate_action_operator(&self, field: &str, operator: &ActionOperator) -> Result<()> {
        // For most string fields, all operators are valid
        // Special validation for specific fields
        match field {
            "tvg_shift" => {
                // tvg_shift should typically be numeric, warn about append operations
                if matches!(operator, ActionOperator::Append) {
                    // This is a warning, not an error - allow but could be improved
                    eprintln!(
                        "Warning: Using '+=' operator with numeric field '{}'. Consider using '=' instead.",
                        field
                    );
                }
            }
            "stream_url" => {
                // stream_url should be set completely, not appended to
                if matches!(operator, ActionOperator::Append) {
                    eprintln!(
                        "Warning: Using '+=' operator with URL field '{}'. Consider using '=' instead.",
                        field
                    );
                }
            }
            _ => {
                // Most string fields support all operators
            }
        }

        Ok(())
    }

    /// Validate action value format
    fn validate_action_value(&self, field: &str, value: &ActionValue) -> Result<()> {
        match value {
            ActionValue::Literal(literal) => {
                // Field-specific validation
                match field {
                    "tvg_shift" => {
                        // tvg_shift should be numeric (but accept strings for flexibility)
                        if !literal.trim().is_empty()
                            && literal.parse::<i32>().is_err()
                            && !literal.starts_with('+')
                            && !literal.starts_with('-')
                        {
                            eprintln!(
                                "Warning: tvg_shift value '{}' may not be a valid time offset. Expected format: '+1', '-2', or '0'",
                                literal
                            );
                        }
                    }
                    "tvg_logo" | "stream_url" => {
                        // Basic URL validation
                        if !literal.trim().is_empty()
                            && !literal.starts_with("http")
                            && !literal.starts_with("/")
                            && !literal.ends_with(".png")
                            && !literal.ends_with(".jpg")
                            && !literal.ends_with(".svg")
                        {
                            eprintln!(
                                "Warning: {} value '{}' may not be a valid URL or file path",
                                field, literal
                            );
                        }
                    }
                    "language" => {
                        // Language code validation
                        if literal.len() != 2 && literal.len() != 5 && !literal.trim().is_empty() {
                            eprintln!(
                                "Warning: language value '{}' may not be a valid language code. Expected format: 'en', 'fr', 'en-US'",
                                literal
                            );
                        }
                    }
                    _ => {
                        // Generic string validation
                        if literal.len() > 255 {
                            return Err(anyhow!(
                                "Value for field '{}' is too long (max 255 characters)",
                                field
                            ));
                        }
                    }
                }
            }
            ActionValue::Null => {
                // Null values are always valid for clearing fields
            }
            ActionValue::Function(_) => {
                return Err(anyhow!("Function calls are not yet supported in actions"));
            }
            ActionValue::Variable(_) => {
                return Err(anyhow!(
                    "Variable references are not yet supported in actions"
                ));
            }
        }

        Ok(())
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

impl Default for FilterParser {
    fn default() -> Self {
        Self::new()
    }
}

impl FilterParser {
    /// Helper method to count conditions in a condition tree
    fn count_conditions(&self, tree: &ConditionTree) -> usize {
        self.count_condition_nodes(&tree.root)
    }

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
                "channel_name not_contains \"adult\"",
                FilterOperator::NotContains,
            ),
            (
                "channel_name not_equals \"Test\"",
                FilterOperator::NotEquals,
            ),
            (
                "channel_name not_matches \"test.*\"",
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
                        "Expression '{}' failed: got {:?}, expected {:?}",
                        expression,
                        operator,
                        expected_operator
                    );
                    assert_eq!(field, "channel_name");
                    assert!(!value.is_empty());
                }
                _ => panic!(
                    "Expression '{}' should parse to a condition, not a group",
                    expression
                ),
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
            parser
                .parse_extended("channel_name contains \"sport\" SET")
                .is_err()
        );

        // Missing assignment operator
        assert!(
            parser
                .parse_extended("channel_name contains \"sport\" SET group_title \"Sports\"")
                .is_err()
        );

        // Missing value
        assert!(
            parser
                .parse_extended("channel_name contains \"sport\" SET group_title =")
                .is_err()
        );

        // Missing comma between actions
        assert!(
            parser
                .parse_extended(
                    "channel_name contains \"sport\" SET group_title = \"Sports\" category = \"TV\""
                )
                .is_err()
        );

        // Unquoted value
        assert!(
            parser
                .parse_extended("channel_name contains \"sport\" SET group_title = Sports")
                .is_err()
        );
    }

    #[test]
    fn test_semantic_validation() {
        let parser = FilterParser::new();

        // Valid action should pass validation
        let result = parser
            .parse_extended("channel_name contains \"sport\" SET group_title = \"Sports\"")
            .unwrap();
        assert!(parser.validate_extended(&result).is_ok());

        // Invalid field should fail validation
        let result = parser
            .parse_extended("channel_name contains \"sport\" SET invalid_field = \"value\"")
            .unwrap();
        assert!(parser.validate_extended(&result).is_err());

        // Too long value should fail validation
        let long_value = "a".repeat(300);
        let expr = format!(
            "channel_name contains \"sport\" SET group_title = \"{}\"",
            long_value
        );
        let result = parser.parse_extended(&expr).unwrap();
        assert!(parser.validate_extended(&result).is_err());
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
                        assert!(matches!(operator, crate::models::FilterOperator::Matches));
                        assert!(*negate); // Should be negated for not_matches
                    }
                    _ => panic!("Expected negated condition"),
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
            parser
                .parse_extended("(channel_name contains \"test\" group_title = \"Test\")")
                .is_err()
        );

        // Missing closing parenthesis
        assert!(
            parser
                .parse_extended("(channel_name contains \"test\" SET group_title = \"Test\"")
                .is_err()
        );

        // Missing opening parenthesis
        assert!(
            parser
                .parse_extended("channel_name contains \"test\" SET group_title = \"Test\")")
                .is_err()
        );

        // Empty group
        assert!(
            parser
                .parse_extended(
                    "() AND (channel_name contains \"test\" SET group_title = \"Test\")"
                )
                .is_err()
        );

        // Missing action after SET
        assert!(
            parser
                .parse_extended("(channel_name contains \"test\" SET)")
                .is_err()
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

        // Test the exact example from the user's request
        let result = parser.parse_extended("(channel_name matches \"regex\" SET tvg_shift = \"$1$2\" AND channel_name not matches \"regex\" AND tvg_id matches \"regex\")").unwrap();

        match result {
            ExtendedExpression::ConditionalActionGroups(groups) => {
                assert_eq!(groups.len(), 1);

                // Should have complex nested conditions
                match &groups[0].conditions.root {
                    ConditionNode::Group { operator, children } => {
                        assert!(matches!(operator, LogicalOperator::And));
                        assert_eq!(children.len(), 3); // Three conditions joined by AND
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
            ExtendedExpression::ConditionWithActions { condition, actions } => {
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
        let expr = "((channel_name matches \"^(BBC|ITV|Channel [45])\" AND tvg_id not_equals \"\") OR (channel_name matches \"Sky (Sports|Movies|News)\" AND group_title equals \"\")) SET group_title = \"Premium UK\"";
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
            parser
                .parse_extended("channel_name matches \"[unclosed\" SET group_title = \"Test\"")
                .is_err()
        );

        // Test missing SET keyword
        assert!(
            parser
                .parse_extended("channel_name contains \"test\" group_title = \"Test\"")
                .is_err()
        );

        // Test unbalanced parentheses in conditional groups
        assert!(
            parser
                .parse_extended("(channel_name contains \"test\" SET group_title = \"Test\" AND")
                .is_err()
        );

        // Test empty action value
        assert!(
            parser
                .parse_extended("channel_name contains \"test\" SET group_title = \"\"")
                .is_ok()
        );

        // Test invalid field reference
        let result =
            parser.parse_extended("invalid_field contains \"test\" SET group_title = \"Test\"");
        // This should parse syntactically but fail validation if field validation is enabled
        assert!(result.is_ok());
    }
}
