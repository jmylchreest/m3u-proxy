use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::time::Duration;
use crate::expression_parser::ExpressionParser;
use crate::models::{ExtendedExpression, ActionOperator, Action};
use crate::utils::regex_preprocessor::RegexPreprocessor;
use tracing::{trace, warn};
use regex::Regex;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FieldModification {
    pub field_name: String,
    pub old_value: Option<String>,
    pub new_value: Option<String>,
    pub modification_type: ModificationType,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ModificationType {
    Set,
    SetIfEmpty,
    Append,
    Remove,
    Delete,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuleResult {
    pub rule_applied: bool,
    pub field_modifications: Vec<FieldModification>,
    pub execution_time: Duration,
    pub error: Option<String>,
}

pub trait RuleProcessor<T> {
    fn process_record(&mut self, record: T) -> Result<(T, RuleResult), Box<dyn std::error::Error>>;
    fn get_rule_name(&self) -> &str;
    fn get_rule_id(&self) -> &str;
}

/// Shared regex evaluator with preprocessing optimization for data mapping rules
pub struct RegexEvaluator {
    preprocessor: RegexPreprocessor,
}

impl RegexEvaluator {
    pub fn new(preprocessor: RegexPreprocessor) -> Self {
        Self { preprocessor }
    }
    
    pub fn evaluate_with_preprocessing(&self, pattern: &str, text: &str, context: &str) -> Result<bool, Box<dyn std::error::Error>> {
        // Use preprocessor to check if regex should run
        if !self.preprocessor.should_run_regex(text, pattern, context) {
            return Ok(false);
        }
        
        // Run the actual regex
        match Regex::new(pattern) {
            Ok(regex) => Ok(regex.is_match(text)),
            Err(e) => {
                warn!("Invalid regex pattern '{}': {}, falling back to contains", pattern, e);
                Ok(text.contains(pattern))
            }
        }
    }
    
    pub fn evaluate_with_captures(&self, pattern: &str, text: &str, context: &str) -> Result<(bool, Option<Vec<String>>), Box<dyn std::error::Error>> {
        // Use preprocessor to check if regex should run
        if !self.preprocessor.should_run_regex(text, pattern, context) {
            return Ok((false, None));
        }
        
        // Run the actual regex with captures
        match Regex::new(pattern) {
            Ok(regex) => {
                if let Some(caps) = regex.captures(text) {
                    let capture_strings: Vec<String> = caps.iter()
                        .map(|m| m.map_or("".to_string(), |m| m.as_str().to_string()))
                        .collect();
                    Ok((true, Some(capture_strings)))
                } else {
                    Ok((false, None))
                }
            },
            Err(e) => {
                warn!("Invalid regex pattern '{}': {}, falling back to contains", pattern, e);
                Ok((text.contains(pattern), None))
            }
        }
    }
}

pub struct StreamRuleProcessor {
    pub rule_id: String,
    pub rule_name: String,
    pub expression: String,
    pub parsed_expression: Option<crate::models::ExtendedExpression>,
    pub regex_evaluator: RegexEvaluator,
}

impl StreamRuleProcessor {
    pub fn new(rule_id: String, rule_name: String, expression: String, regex_evaluator: RegexEvaluator) -> Self {
        // Parse expression once during initialization
        let parsed_expression = if expression.trim().is_empty() {
            None
        } else {
            let channel_fields = vec![
                "tvg_id".to_string(),
                "tvg_name".to_string(),
                "tvg_logo".to_string(),
                "tvg_shift".to_string(),
                "group_title".to_string(),
                "channel_name".to_string(),
                "stream_url".to_string(),
            ];
            
            let parser = ExpressionParser::new().with_fields(channel_fields);
            match parser.parse_extended(&expression) {
                Ok(parsed) => {
                    trace!("Successfully pre-parsed expression for rule {}", rule_id);
                    Some(parsed)
                },
                Err(e) => {
                    warn!("Failed to pre-parse expression for rule {}: {}", rule_id, e);
                    None
                }
            }
        };

        Self {
            rule_id,
            rule_name,
            expression,
            parsed_expression,
            regex_evaluator,
        }
    }
    
    /// Evaluate the expression against a channel record using the cached parsed expression
    fn evaluate_expression(&self, record: &crate::models::Channel) -> Result<(crate::models::Channel, Vec<FieldModification>), Box<dyn std::error::Error>> {
        let mut modified_record = record.clone();
        let mut modifications = Vec::new();
        
        // Use cached parsed expression
        let parsed_expression = match &self.parsed_expression {
            Some(expr) => expr,
            None => {
                trace!("Rule {} has no valid parsed expression, skipping", self.rule_id);
                return Ok((modified_record, modifications));
            }
        };
        
        trace!("Evaluating rule {} against channel", self.rule_id);
        
        // Evaluate the parsed expression
        match parsed_expression {
            ExtendedExpression::ConditionWithActions { condition, actions } => {
                // Check if we need captures (only for regex operations)
                let needs_captures = self.condition_tree_needs_captures(&condition);
                
                
                if needs_captures {
                    // Use captures version for regex-based conditions
                    let (condition_result, captures) = self.evaluate_condition_tree_with_captures(&condition, record)?;
                    
                    
                    trace!("Rule {} condition evaluation result: {} captures: {:?}", self.rule_id, condition_result, captures);
                    
                    if condition_result {
                        trace!("Rule {} condition matched, applying {} actions with captures", self.rule_id, actions.len());
                        for action in actions {
                            if let Some(modification) = self.apply_parsed_action_with_captures(&action, &mut modified_record, &record.channel_name, &captures)? {
                                trace!("Rule {} applied action: {} {:?} -> {:?}", 
                                       self.rule_id, &modification.field_name, 
                                       &modification.modification_type, &modification.new_value);
                                modifications.push(modification);
                            }
                        }
                    } else {
                        trace!("Rule {} condition did not match", self.rule_id);
                    }
                } else {
                    // Use simpler/faster version for non-regex conditions
                    let condition_result = self.evaluate_condition_tree(&condition, record)?;
                    trace!("Rule {} condition evaluation result: {} (fast path)", self.rule_id, condition_result);
                    
                    if condition_result {
                        trace!("Rule {} condition matched, applying {} actions (fast path)", self.rule_id, actions.len());
                        for action in actions {
                            if let Some(modification) = self.apply_parsed_action(&action, &mut modified_record, &record.channel_name)? {
                                trace!("Rule {} applied action: {} {:?} -> {:?}", 
                                       self.rule_id, &modification.field_name, 
                                       &modification.modification_type, &modification.new_value);
                                modifications.push(modification);
                            }
                        }
                    } else {
                        trace!("Rule {} condition did not match", self.rule_id);
                    }
                }
            }
            ExtendedExpression::ConditionOnly(condition) => {
                // Just evaluate condition - no actions to apply
                let _matches = self.evaluate_condition_tree(&condition, record)?;
                // No modifications for condition-only expressions
            }
            ExtendedExpression::ConditionalActionGroups(groups) => {
                // Process each conditional action group
                for group in groups {
                    let needs_captures = self.condition_tree_needs_captures(&group.conditions);
                    
                    if needs_captures {
                        let (condition_result, group_captures) = self.evaluate_condition_tree_with_captures(&group.conditions, record)?;
                        if condition_result {
                            for action in &group.actions {
                                if let Some(modification) = self.apply_parsed_action_with_captures(action, &mut modified_record, &record.channel_name, &group_captures)? {
                                    modifications.push(modification);
                                }
                            }
                        }
                    } else {
                        let condition_result = self.evaluate_condition_tree(&group.conditions, record)?;
                        if condition_result {
                            for action in &group.actions {
                                if let Some(modification) = self.apply_parsed_action(action, &mut modified_record, &record.channel_name)? {
                                    modifications.push(modification);
                                }
                            }
                        }
                    }
                }
            }
        }
        
        Ok((modified_record, modifications))
    }
    
    /// Evaluate a condition tree (parsed expression structure)
    fn evaluate_condition_tree(&self, condition: &crate::models::ConditionTree, record: &crate::models::Channel) -> Result<bool, Box<dyn std::error::Error>> {
        self.evaluate_condition_node(&condition.root, record)
    }
    
    /// Evaluate a condition tree and return captures from regex matches
    fn evaluate_condition_tree_with_captures(&self, condition: &crate::models::ConditionTree, record: &crate::models::Channel) -> Result<(bool, Option<Vec<String>>), Box<dyn std::error::Error>> {
        self.evaluate_condition_node_with_captures(&condition.root, record)
    }
    
    /// Check if a condition tree contains regex operators that need capture groups
    fn condition_tree_needs_captures(&self, condition: &crate::models::ConditionTree) -> bool {
        self.condition_node_needs_captures(&condition.root)
    }
    
    /// Check if a condition node contains regex operators recursively
    fn condition_node_needs_captures(&self, node: &crate::models::ConditionNode) -> bool {
        use crate::models::FilterOperator;
        
        match node {
            crate::models::ConditionNode::Condition { operator, .. } => {
                matches!(operator, FilterOperator::Matches | FilterOperator::NotMatches)
            }
            crate::models::ConditionNode::Group { children, .. } => {
                children.iter().any(|child| self.condition_node_needs_captures(child))
            }
        }
    }
    
    /// Evaluate a condition node recursively  
    fn evaluate_condition_node(&self, node: &crate::models::ConditionNode, record: &crate::models::Channel) -> Result<bool, Box<dyn std::error::Error>> {
        self.evaluate_condition_node_with_captures(node, record).map(|(result, _)| result)
    }

    /// Evaluate a condition node and return captures for regex matches
    fn evaluate_condition_node_with_captures(&self, node: &crate::models::ConditionNode, record: &crate::models::Channel) -> Result<(bool, Option<Vec<String>>), Box<dyn std::error::Error>> {
        use crate::models::{LogicalOperator, FilterOperator};
        
        match node {
            crate::models::ConditionNode::Condition { field, operator, value, .. } => {
                let field_value = self.get_field_value(field, record)?;
                let field_value_str = field_value.unwrap_or_default();
                
                
                trace!("Evaluating condition: field='{}' operator='{:?}' value='{}' field_value='{}'", 
                       field, operator, value, field_value_str);
                
                let (matches, captures) = match operator {
                    FilterOperator::Equals => (field_value_str.eq_ignore_ascii_case(value), None),
                    FilterOperator::NotEquals => (!field_value_str.eq_ignore_ascii_case(value), None),
                    FilterOperator::Contains => (field_value_str.to_lowercase().contains(&value.to_lowercase()), None),
                    FilterOperator::NotContains => (!field_value_str.to_lowercase().contains(&value.to_lowercase()), None),
                    FilterOperator::StartsWith => (field_value_str.to_lowercase().starts_with(&value.to_lowercase()), None),
                    FilterOperator::NotStartsWith => (!field_value_str.to_lowercase().starts_with(&value.to_lowercase()), None),
                    FilterOperator::EndsWith => (field_value_str.to_lowercase().ends_with(&value.to_lowercase()), None),
                    FilterOperator::NotEndsWith => (!field_value_str.to_lowercase().ends_with(&value.to_lowercase()), None),
                    FilterOperator::Matches => {
                        let context = format!("data_mapping_rule_{}", self.rule_name);
                        match self.regex_evaluator.evaluate_with_captures(value, &field_value_str, &context) {
                            Ok((matches, captures)) => (matches, captures),
                            Err(e) => {
                                warn!("RULE_PROCESSOR:   > Regex evaluation failed for pattern '{}': {}", value, e);
                                (false, None)
                            }
                        }
                    },
                    FilterOperator::NotMatches => {
                        let context = format!("data_mapping_rule_{}", self.rule_name);
                        match self.regex_evaluator.evaluate_with_preprocessing(value, &field_value_str, &context) {
                            Ok(matches) => {
                                let not_matches = !matches;
                                trace!("Regex evaluation (NOT): pattern='{}' text='{}' matches={}", value, field_value_str, not_matches);
                                (not_matches, None)
                            },
                            Err(e) => {
                                warn!("RULE_PROCESSOR:   > Regex evaluation failed for pattern '{}': {}", value, e);
                                (false, None)
                            }
                        }
                    },
                    FilterOperator::GreaterThan => {
                        match self.compare_values(&field_value_str, value, std::cmp::Ordering::Greater) {
                            Ok(result) => (result, None),
                            Err(e) => {
                                warn!("RULE_PROCESSOR:   > Comparison failed: {}", e);
                                (false, None)
                            }
                        }
                    },
                    FilterOperator::LessThan => {
                        match self.compare_values(&field_value_str, value, std::cmp::Ordering::Less) {
                            Ok(result) => (result, None),
                            Err(e) => {
                                warn!("RULE_PROCESSOR:   > Comparison failed: {}", e);
                                (false, None)
                            }
                        }
                    },
                    FilterOperator::GreaterThanOrEqual => {
                        match self.compare_values(&field_value_str, value, std::cmp::Ordering::Greater) {
                            Ok(result) => {
                                let equal = field_value_str.eq_ignore_ascii_case(value);
                                (result || equal, None)
                            },
                            Err(e) => {
                                warn!("RULE_PROCESSOR:   > Comparison failed: {}", e);
                                (false, None)
                            }
                        }
                    },
                    FilterOperator::LessThanOrEqual => {
                        match self.compare_values(&field_value_str, value, std::cmp::Ordering::Less) {
                            Ok(result) => {
                                let equal = field_value_str.eq_ignore_ascii_case(value);
                                (result || equal, None)
                            },
                            Err(e) => {
                                warn!("RULE_PROCESSOR:   > Comparison failed: {}", e);
                                (false, None)
                            }
                        }
                    },
                };
                
                
                trace!("Condition evaluation result: {}", matches);
                Ok((matches, captures))
            }
            crate::models::ConditionNode::Group { operator, children } => {
                if children.is_empty() {
                    return Ok((true, None)); // Empty group defaults to true
                }
                
                let mut results = Vec::new();
                let mut all_captures: Option<Vec<String>> = None;
                
                for child in children {
                    let (child_result, child_captures) = self.evaluate_condition_node_with_captures(child, record)?;
                    results.push(child_result);
                    
                    // Collect the first non-None captures we find (from regex matches)
                    if all_captures.is_none() && child_captures.is_some() {
                        all_captures = child_captures;
                    }
                }
                
                let group_result = match operator {
                    LogicalOperator::And => results.iter().all(|&r| r),
                    LogicalOperator::Or => results.iter().any(|&r| r),
                };
                
                Ok((group_result, all_captures))
            }
        }
    }
    
    /// Apply a parsed action
    fn apply_parsed_action(&self, action: &Action, record: &mut crate::models::Channel, channel_name: &str) -> Result<Option<FieldModification>, Box<dyn std::error::Error>> {
        let old_value = self.get_field_value(&action.field, record)?;
        
        
        let modification_type = match &action.operator {
            ActionOperator::Set => ModificationType::Set,
            ActionOperator::SetIfEmpty => {
                if old_value.is_some() && !old_value.as_ref().unwrap().is_empty() {
                    return Ok(None); // Don't modify if field has a value
                }
                ModificationType::SetIfEmpty
            },
            ActionOperator::Append => ModificationType::Append,
            ActionOperator::Remove => ModificationType::Remove,
            ActionOperator::Delete => ModificationType::Delete,
        };
        
        match &action.operator {
            ActionOperator::Set | ActionOperator::SetIfEmpty => {
                match &action.value {
                    crate::models::ActionValue::Literal(new_value) => {
                        self.set_field_value(&action.field, new_value, record)?;
                        
                        
                        Ok(Some(FieldModification {
                            field_name: action.field.clone(),
                            old_value: old_value.clone(),
                            new_value: Some(new_value.clone()),
                            modification_type,
                        }))
                    },
                    crate::models::ActionValue::Null => {
                        // Set field to None/empty
                        self.apply_parsed_action(&Action {
                            field: action.field.clone(),
                            operator: ActionOperator::Delete,
                            value: action.value.clone(),
                        }, record, channel_name)
                    },
                    _ => Ok(None), // Other action value types not implemented yet
                }
            },
            ActionOperator::Append => {
                match &action.value {
                    crate::models::ActionValue::Literal(append_value) => {
                        let current_value = old_value.as_ref().map(|s| s.as_str()).unwrap_or_default();
                        let new_value = format!("{}{}", current_value, append_value);
                        self.set_field_value(&action.field, &new_value, record)?;
                        
                        Ok(Some(FieldModification {
                            field_name: action.field.clone(),
                            old_value: old_value.clone(),
                            new_value: Some(new_value),
                            modification_type,
                        }))
                    },
                    _ => Ok(None), // Other action value types not supported for append
                }
            },
            ActionOperator::Delete => {
                // For optional fields, set to None; for required fields, set to empty string
                match action.field.as_str() {
                    "channel_name" | "stream_url" => {
                        self.set_field_value(&action.field, "", record)?;
                        Ok(Some(FieldModification {
                            field_name: action.field.clone(),
                            old_value: old_value.clone(),
                            new_value: Some("".to_string()),
                            modification_type,
                        }))
                    },
                    _ => {
                        // Set optional field to None
                        self.set_optional_field_none(&action.field, record)?;
                        Ok(Some(FieldModification {
                            field_name: action.field.clone(),
                            old_value: old_value.clone(),
                            new_value: None,
                            modification_type,
                        }))
                    }
                }
            },
            ActionOperator::Remove => {
                // Remove specific value from field - for simplicity, treat as delete for now
                self.apply_parsed_action(&Action {
                    field: action.field.clone(),
                    operator: ActionOperator::Delete,
                    value: action.value.clone(),
                }, record, channel_name)
            }
        }
    }
    
    /// Apply a parsed action with capture group substitution
    fn apply_parsed_action_with_captures(&self, action: &Action, record: &mut crate::models::Channel, channel_name: &str, captures: &Option<Vec<String>>) -> Result<Option<FieldModification>, Box<dyn std::error::Error>> {
        let old_value = self.get_field_value(&action.field, record)?;
        
        
        let modification_type = match &action.operator {
            ActionOperator::Set => ModificationType::Set,
            ActionOperator::SetIfEmpty => {
                if old_value.is_some() && !old_value.as_ref().unwrap().is_empty() {
                    return Ok(None); // Don't modify if field has a value
                }
                ModificationType::SetIfEmpty
            },
            ActionOperator::Append => ModificationType::Append,
            ActionOperator::Remove => ModificationType::Remove,
            ActionOperator::Delete => ModificationType::Delete,
        };
        
        match &action.operator {
            ActionOperator::Set | ActionOperator::SetIfEmpty => {
                match &action.value {
                    crate::models::ActionValue::Literal(new_value) => {
                        // Process capture group substitutions
                        let processed_value = self.substitute_capture_groups(new_value, captures);
                        self.set_field_value(&action.field, &processed_value, record)?;
                        
                        
                        Ok(Some(FieldModification {
                            field_name: action.field.clone(),
                            old_value: old_value.clone(),
                            new_value: Some(processed_value),
                            modification_type,
                        }))
                    },
                    crate::models::ActionValue::Null => {
                        // Set field to None/empty
                        self.apply_parsed_action_with_captures(&Action {
                            field: action.field.clone(),
                            operator: ActionOperator::Delete,
                            value: action.value.clone(),
                        }, record, channel_name, captures)
                    },
                    _ => Ok(None), // Other action value types not implemented yet
                }
            },
            ActionOperator::Append => {
                match &action.value {
                    crate::models::ActionValue::Literal(append_value) => {
                        let processed_value = self.substitute_capture_groups(append_value, captures);
                        let current_value = old_value.as_ref().map(|s| s.as_str()).unwrap_or_default();
                        let new_value = format!("{}{}", current_value, processed_value);
                        self.set_field_value(&action.field, &new_value, record)?;
                        
                        Ok(Some(FieldModification {
                            field_name: action.field.clone(),
                            old_value: old_value.clone(),
                            new_value: Some(new_value),
                            modification_type,
                        }))
                    },
                    _ => Ok(None), // Other action value types not supported for append
                }
            },
            ActionOperator::Delete => {
                // For optional fields, set to None; for required fields, set to empty string
                match action.field.as_str() {
                    "channel_name" | "stream_url" => {
                        self.set_field_value(&action.field, "", record)?;
                        Ok(Some(FieldModification {
                            field_name: action.field.clone(),
                            old_value: old_value.clone(),
                            new_value: Some("".to_string()),
                            modification_type,
                        }))
                    },
                    _ => {
                        // Set optional field to None
                        self.set_optional_field_none(&action.field, record)?;
                        Ok(Some(FieldModification {
                            field_name: action.field.clone(),
                            old_value: old_value.clone(),
                            new_value: None,
                            modification_type,
                        }))
                    }
                }
            },
            ActionOperator::Remove => {
                // Remove specific value from field - for simplicity, treat as delete for now
                self.apply_parsed_action_with_captures(&Action {
                    field: action.field.clone(),
                    operator: ActionOperator::Delete,
                    value: action.value.clone(),
                }, record, channel_name, captures)
            }
        }
    }
    
    /// Substitute capture groups ($1, $2, etc.) in a string with actual captured values
    fn substitute_capture_groups(&self, input: &str, captures: &Option<Vec<String>>) -> String {
        if let Some(capture_list) = captures {
            let mut result = input.to_string();
            
            
            // Replace $1, $2, $3, etc. with captured groups
            // Note: captures[0] is the full match, captures[1] is the first group, etc.
            for (i, capture) in capture_list.iter().enumerate().skip(1) { // Skip index 0 (full match)
                let placeholder = format!("${}", i);
                result = result.replace(&placeholder, capture);
            }
            
            
            result
        } else {
            input.to_string()
        }
    }
    
    /// Get a field value from a channel record
    fn get_field_value(&self, field_name: &str, record: &crate::models::Channel) -> Result<Option<String>, Box<dyn std::error::Error>> {
        match field_name {
            "tvg_id" => Ok(record.tvg_id.clone()),
            "tvg_name" => Ok(record.tvg_name.clone()),
            "tvg_logo" => Ok(record.tvg_logo.clone()),
            "tvg_shift" => Ok(record.tvg_shift.clone()),
            "group_title" => Ok(record.group_title.clone()),
            "channel_name" => Ok(Some(record.channel_name.clone())),
            "stream_url" => Ok(Some(record.stream_url.clone())),
            _ => Err(anyhow::anyhow!("Unknown field: {}", field_name).into()),
        }
    }
    
    /// Set a field value on a channel record
    fn set_field_value(&self, field_name: &str, value: &str, record: &mut crate::models::Channel) -> Result<(), Box<dyn std::error::Error>> {
        match field_name {
            "tvg_id" => record.tvg_id = Some(value.to_string()),
            "tvg_name" => record.tvg_name = Some(value.to_string()),
            "tvg_logo" => record.tvg_logo = Some(value.to_string()),
            "tvg_shift" => record.tvg_shift = Some(value.to_string()),
            "group_title" => record.group_title = Some(value.to_string()),
            "channel_name" => record.channel_name = value.to_string(),
            "stream_url" => record.stream_url = value.to_string(),
            _ => return Err(anyhow::anyhow!("Cannot set unknown field: {}", field_name).into()),
        }
        Ok(())
    }
    
    /// Set an optional field to None
    fn set_optional_field_none(&self, field_name: &str, record: &mut crate::models::Channel) -> Result<(), Box<dyn std::error::Error>> {
        match field_name {
            "tvg_id" => record.tvg_id = None,
            "tvg_name" => record.tvg_name = None,
            "tvg_logo" => record.tvg_logo = None,
            "tvg_shift" => record.tvg_shift = None,
            "group_title" => record.group_title = None,
            "channel_name" | "stream_url" => return Err(anyhow::anyhow!("Cannot set required field '{}' to None", field_name).into()),
            _ => return Err(anyhow::anyhow!("Cannot clear unknown field: {}", field_name).into()),
        }
        Ok(())
    }
    
    /// Compare two values using numeric or datetime comparison
    /// First tries to parse as Unix timestamps, then falls back to string comparison
    fn compare_values(&self, field_value: &str, expected_value: &str, ordering: std::cmp::Ordering) -> Result<bool, Box<dyn std::error::Error>> {
        use crate::utils::time::{resolve_time_functions, parse_time_string};
        
        // Resolve any @time: functions in the expected value
        let resolved_expected = resolve_time_functions(expected_value)?;
        
        // Try numeric comparison first (Unix timestamps)
        if let (Ok(field_num), Ok(expected_num)) = (
            parse_time_string(field_value),
            parse_time_string(&resolved_expected)
        ) {
            return Ok(field_num.cmp(&expected_num) == ordering);
        }
        
        // Fall back to lexicographic string comparison
        Ok(field_value.cmp(&resolved_expected) == ordering)
    }
}

impl RuleProcessor<crate::models::Channel> for StreamRuleProcessor {
    fn process_record(&mut self, record: crate::models::Channel) -> Result<(crate::models::Channel, RuleResult), Box<dyn std::error::Error>> {
        let start = std::time::Instant::now();
        
        
        // Parse and evaluate the expression
        let (modified_record, modifications) = match self.evaluate_expression(&record) {
            Ok((rec, mods)) => (rec, mods),
            Err(e) => {
                warn!("RULE_PROCESSOR: Rule evaluation failed: rule_id={} error={}", self.rule_id, e);
                let result = RuleResult {
                    rule_applied: false,
                    field_modifications: vec![],
                    execution_time: start.elapsed(),
                    error: Some(e.to_string()),
                };
                return Ok((record, result));
            }
        };
        
        let execution_time = start.elapsed();
        let rule_applied = !modifications.is_empty();
        
        // Only log when rule actually modifies data
        if rule_applied {
            trace!("RULE_PROCESSOR: {} applied {} modifications to '{}'", 
                   self.rule_name, modifications.len(), record.channel_name);
        }
        
        let result = RuleResult {
            rule_applied,
            field_modifications: modifications,
            execution_time,
            error: None,
        };
        
        Ok((modified_record, result))
    }
    
    fn get_rule_name(&self) -> &str {
        &self.rule_name
    }
    
    fn get_rule_id(&self) -> &str {
        &self.rule_id
    }
}

pub struct EpgRuleProcessor {
    pub rule_id: String,
    pub rule_name: String,
    pub expression: String,
}

impl EpgRuleProcessor {
    pub fn new(rule_id: String, rule_name: String, expression: String) -> Self {
        Self {
            rule_id,
            rule_name,
            expression,
        }
    }
}

// For EPG programs - we'll need to define this type or use existing one
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EpgProgram {
    pub id: String,
    pub channel_id: String,
    pub title: String,
    pub description: Option<String>,
    pub program_icon: Option<String>,
    #[serde(serialize_with = "crate::utils::datetime::serialize_datetime")]
    #[serde(deserialize_with = "crate::utils::datetime::deserialize_datetime")]
    pub start_time: DateTime<Utc>,
    #[serde(serialize_with = "crate::utils::datetime::serialize_datetime")]
    #[serde(deserialize_with = "crate::utils::datetime::deserialize_datetime")]
    pub end_time: DateTime<Utc>,
}

impl RuleProcessor<EpgProgram> for EpgRuleProcessor {
    fn process_record(&mut self, record: EpgProgram) -> Result<(EpgProgram, RuleResult), Box<dyn std::error::Error>> {
        let start = std::time::Instant::now();
        
        // Return the record unchanged - rule processing is handled by data mapping and filtering stages
        let result = RuleResult {
            rule_applied: false,
            field_modifications: vec![],
            execution_time: start.elapsed(),
            error: None,
        };
        
        Ok((record, result))
    }
    
    fn get_rule_name(&self) -> &str {
        &self.rule_name
    }
    
    fn get_rule_id(&self) -> &str {
        &self.rule_id
    }
}