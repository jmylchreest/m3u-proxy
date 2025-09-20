use crate::expression::ExpressionDomain;

use crate::models::{Action, ActionOperator, ExtendedExpression};
use crate::utils::regex_preprocessor::RegexPreprocessor;
use chrono::{DateTime, Utc};
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::time::Duration;
use tracing::{trace, warn};

/// Type alias for regex evaluation result with captures
type RegexCaptureResult = Result<(bool, Option<Vec<String>>), Box<dyn std::error::Error>>;

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
    /// True if at least one condition branch matched (even if no modifications occurred)
    pub condition_matched: bool,
    /// True if any field was actually modified
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

    pub fn evaluate_with_preprocessing(
        &self,
        pattern: &str,
        text: &str,
        context: &str,
    ) -> Result<bool, Box<dyn std::error::Error>> {
        // Use preprocessor to check if regex should run
        if !self.preprocessor.should_run_regex(text, pattern, context) {
            return Ok(false);
        }

        // Run the actual regex
        match Regex::new(pattern) {
            Ok(regex) => Ok(regex.is_match(text)),
            Err(e) => {
                warn!(
                    "Invalid regex pattern '{}': {}, falling back to contains",
                    pattern, e
                );
                Ok(text.contains(pattern))
            }
        }
    }

    pub fn evaluate_with_captures(
        &self,
        pattern: &str,
        text: &str,
        context: &str,
    ) -> RegexCaptureResult {
        // Use preprocessor to check if regex should run
        if !self.preprocessor.should_run_regex(text, pattern, context) {
            return Ok((false, None));
        }

        // Run the actual regex with captures
        match Regex::new(pattern) {
            Ok(regex) => {
                if let Some(caps) = regex.captures(text) {
                    let capture_strings: Vec<String> = caps
                        .iter()
                        .map(|m| m.map_or("".to_string(), |m| m.as_str().to_string()))
                        .collect();
                    Ok((true, Some(capture_strings)))
                } else {
                    Ok((false, None))
                }
            }
            Err(e) => {
                warn!(
                    "Invalid regex pattern '{}': {}, falling back to contains",
                    pattern, e
                );
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
    /// If parsing/validation failed, capture the reason for diagnostics.
    pub parse_error: Option<String>,
    /// Optional runtime source metadata map enabling evaluation of read-only
    /// source_* fields via an evaluation context (not persisted).
    pub source_meta_map: Option<
        std::sync::Arc<
            std::collections::HashMap<uuid::Uuid, crate::pipeline::eval_context::SourceMeta>,
        >,
    >,
}

impl StreamRuleProcessor {
    pub fn new(
        rule_id: String,
        rule_name: String,
        expression: String,
        regex_evaluator: RegexEvaluator,
    ) -> Self {
        let (parsed_expression, parse_error) = if expression.trim().is_empty() {
            (None, None)
        } else {
            match crate::expression::parse_expression_extended(
                crate::expression::ExpressionDomain::StreamRule,
                &expression,
            ) {
                Ok(Some(parsed)) => {
                    // debug: stream rule parsed (removed println)
                    trace!(
                        "[EXPR_PARSE] domain=StreamRule id={} name={} len_raw={} expr='{}'",
                        rule_id,
                        rule_name,
                        expression.len(),
                        &expression
                    );
                    (Some(parsed.extended.clone()), None)
                }
                Ok(None) => (None, None),
                Err(e) => {
                    let msg = format!(
                        "Failed to parse / validate stream rule expression id={} name={} err={}",
                        rule_id, rule_name, e
                    );
                    // debug: stream rule parse error (removed println)
                    warn!("{}", msg);
                    (None, Some(msg))
                }
            }
        };

        Self {
            rule_id,
            rule_name,
            expression,
            parsed_expression,
            regex_evaluator,
            parse_error,
            source_meta_map: None,
        }
    }

    /// Inject (or replace) the runtime source metadata map enabling resolution
    /// of read-only `source_*` fields during expression evaluation.
    pub fn set_source_meta_map(
        &mut self,
        map: std::sync::Arc<
            std::collections::HashMap<uuid::Uuid, crate::pipeline::eval_context::SourceMeta>,
        >,
    ) {
        self.source_meta_map = Some(map);
    }

    /// Convenience builder-style variant for chaining during construction.
    pub fn with_source_meta_map(
        mut self,
        map: std::sync::Arc<
            std::collections::HashMap<uuid::Uuid, crate::pipeline::eval_context::SourceMeta>,
        >,
    ) -> Self {
        self.source_meta_map = Some(map);
        self
    }

    /// Evaluate the expression against a channel record using the cached parsed expression.
    /// This now prepares a ChannelEvalContext which (when downstream helpers are refactored)
    /// allows resolution of injected read-only source_* fields from `source_meta_map`.
    fn evaluate_expression(
        &self,
        record: &crate::models::Channel,
    ) -> Result<(crate::models::Channel, Vec<FieldModification>, bool), Box<dyn std::error::Error>>
    {
        // Build evaluation context (source metadata is optional; trace if missing only in verbose modes)
        let source_meta = self
            .source_meta_map
            .as_ref()
            .and_then(|m| m.get(&record.source_id));
        let _eval_ctx = crate::pipeline::eval_context::ChannelEvalContext::new(record, source_meta);
        let mut modified_record = record.clone();
        let mut modifications = Vec::new();

        // Use cached parsed expression
        let parsed_expression = match &self.parsed_expression {
            Some(expr) => expr,
            None => {
                if let Some(err) = &self.parse_error {
                    warn!(
                        "Rule {} has no valid parsed expression (parse_error='{}'), skipping",
                        self.rule_id, err
                    );
                } else {
                    trace!(
                        "Rule {} has no valid parsed expression, skipping",
                        self.rule_id
                    );
                }
                return Ok((modified_record, modifications, false));
            }
        };

        trace!("Evaluating rule {} against channel", self.rule_id);

        // Evaluate the parsed expression
        let mut condition_matched = false;

        match parsed_expression {
            ExtendedExpression::ConditionWithActions { condition, actions } => {
                // Check if we need captures (only for regex operations)
                let needs_captures = self.condition_tree_needs_captures(condition);

                if needs_captures {
                    // Use captures version for regex-based conditions
                    let (condition_result, captures) =
                        self.evaluate_condition_tree_with_captures(condition, record)?;

                    trace!(
                        "Rule {} condition evaluation result: {} captures: {:?}",
                        self.rule_id, condition_result, captures
                    );

                    if condition_result {
                        condition_matched = true;
                        trace!(
                            "Rule {} condition matched, applying {} actions with captures",
                            self.rule_id,
                            actions.len()
                        );
                        for action in actions {
                            if let Some(modification) = self.apply_parsed_action_with_captures(
                                action,
                                &mut modified_record,
                                &record.channel_name,
                                &captures,
                            )? {
                                trace!(
                                    "Rule {} applied action: {} {:?} -> {:?}",
                                    self.rule_id,
                                    &modification.field_name,
                                    &modification.modification_type,
                                    &modification.new_value
                                );
                                modifications.push(modification);
                            }
                        }
                    } else {
                        trace!("Rule {} condition did not match", self.rule_id);
                    }
                } else {
                    // Use simpler/faster version for non-regex conditions
                    let condition_result = self.evaluate_condition_tree(condition, record)?;
                    trace!(
                        "Rule {} condition evaluation result: {} (fast path)",
                        self.rule_id, condition_result
                    );

                    if condition_result {
                        trace!(
                            "Rule {} condition matched, applying {} actions (fast path)",
                            self.rule_id,
                            actions.len()
                        );
                        for action in actions {
                            if let Some(modification) = self.apply_parsed_action(
                                action,
                                &mut modified_record,
                                &record.channel_name,
                            )? {
                                trace!(
                                    "Rule {} applied action: {} {:?} -> {:?}",
                                    self.rule_id,
                                    &modification.field_name,
                                    &modification.modification_type,
                                    &modification.new_value
                                );
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
                let _matches = self.evaluate_condition_tree(condition, record)?;
                // A pure condition (no actions) counts as matched if true
                if _matches {
                    condition_matched = true;
                }
                // No modifications for condition-only expressions
            }
            ExtendedExpression::ConditionalActionGroups(groups) => {
                // Process each conditional action group
                for group in groups {
                    let needs_captures = self.condition_tree_needs_captures(&group.conditions);

                    if needs_captures {
                        let (condition_result, group_captures) =
                            self.evaluate_condition_tree_with_captures(&group.conditions, record)?;
                        if condition_result {
                            for action in &group.actions {
                                if let Some(modification) = self.apply_parsed_action_with_captures(
                                    action,
                                    &mut modified_record,
                                    &record.channel_name,
                                    &group_captures,
                                )? {
                                    modifications.push(modification);
                                }
                            }
                        }
                    } else {
                        let condition_result =
                            self.evaluate_condition_tree(&group.conditions, record)?;
                        if condition_result {
                            for action in &group.actions {
                                if let Some(modification) = self.apply_parsed_action(
                                    action,
                                    &mut modified_record,
                                    &record.channel_name,
                                )? {
                                    modifications.push(modification);
                                }
                            }
                        }
                    }
                }
            }
        }

        Ok((modified_record, modifications, condition_matched))
    }

    /// Evaluate a condition tree (parsed expression structure)
    fn evaluate_condition_tree(
        &self,
        condition: &crate::models::ConditionTree,
        record: &crate::models::Channel,
    ) -> Result<bool, Box<dyn std::error::Error>> {
        self.evaluate_condition_node(&condition.root, record)
    }

    /// Evaluate a condition tree and return captures from regex matches
    fn evaluate_condition_tree_with_captures(
        &self,
        condition: &crate::models::ConditionTree,
        record: &crate::models::Channel,
    ) -> RegexCaptureResult {
        self.evaluate_condition_node_with_captures(&condition.root, record)
    }

    /// Check if a condition tree contains regex operators that need capture groups
    fn condition_tree_needs_captures(&self, condition: &crate::models::ConditionTree) -> bool {
        self.condition_node_needs_captures(&condition.root)
    }

    #[allow(clippy::only_used_in_recursion)]
    /// Check if a condition node contains regex operators recursively
    fn condition_node_needs_captures(&self, node: &crate::models::ConditionNode) -> bool {
        use crate::models::FilterOperator;

        match node {
            crate::models::ConditionNode::Condition { operator, .. } => {
                matches!(
                    operator,
                    FilterOperator::Matches | FilterOperator::NotMatches
                )
            }
            crate::models::ConditionNode::Group { children, .. } => children
                .iter()
                .any(|child| self.condition_node_needs_captures(child)),
        }
    }

    /// Evaluate a condition node recursively
    fn evaluate_condition_node(
        &self,
        node: &crate::models::ConditionNode,
        record: &crate::models::Channel,
    ) -> Result<bool, Box<dyn std::error::Error>> {
        self.evaluate_condition_node_with_captures(node, record)
            .map(|(result, _)| result)
    }

    /// Evaluate a condition node and return captures for regex matches
    fn evaluate_condition_node_with_captures(
        &self,
        node: &crate::models::ConditionNode,
        record: &crate::models::Channel,
    ) -> RegexCaptureResult {
        use crate::models::{FilterOperator, LogicalOperator};

        match node {
            crate::models::ConditionNode::Condition {
                field,
                operator,
                value,
                ..
            } => {
                let field_value = self.get_field_value(field, record)?;
                let field_value_str = field_value.unwrap_or_default();

                trace!(
                    "Evaluating condition: field='{}' operator='{:?}' value='{}' field_value='{}'",
                    field, operator, value, field_value_str
                );

                let (matches, captures) = match operator {
                    FilterOperator::Equals => (field_value_str.eq_ignore_ascii_case(value), None),
                    FilterOperator::NotEquals => {
                        (!field_value_str.eq_ignore_ascii_case(value), None)
                    }
                    FilterOperator::Contains => (
                        field_value_str
                            .to_lowercase()
                            .contains(&value.to_lowercase()),
                        None,
                    ),
                    FilterOperator::NotContains => (
                        !field_value_str
                            .to_lowercase()
                            .contains(&value.to_lowercase()),
                        None,
                    ),
                    FilterOperator::StartsWith => (
                        field_value_str
                            .to_lowercase()
                            .starts_with(&value.to_lowercase()),
                        None,
                    ),
                    FilterOperator::NotStartsWith => (
                        !field_value_str
                            .to_lowercase()
                            .starts_with(&value.to_lowercase()),
                        None,
                    ),
                    FilterOperator::EndsWith => (
                        field_value_str
                            .to_lowercase()
                            .ends_with(&value.to_lowercase()),
                        None,
                    ),
                    FilterOperator::NotEndsWith => (
                        !field_value_str
                            .to_lowercase()
                            .ends_with(&value.to_lowercase()),
                        None,
                    ),
                    FilterOperator::Matches => {
                        let context = format!("data_mapping_rule_{}", self.rule_name);
                        match self.regex_evaluator.evaluate_with_captures(
                            value,
                            &field_value_str,
                            &context,
                        ) {
                            Ok((matches, captures)) => (matches, captures),
                            Err(e) => {
                                warn!(
                                    "RULE_PROCESSOR:   > Regex evaluation failed for pattern '{}': {}",
                                    value, e
                                );
                                (false, None)
                            }
                        }
                    }
                    FilterOperator::NotMatches => {
                        let context = format!("data_mapping_rule_{}", self.rule_name);
                        match self.regex_evaluator.evaluate_with_preprocessing(
                            value,
                            &field_value_str,
                            &context,
                        ) {
                            Ok(matches) => {
                                let not_matches = !matches;
                                trace!(
                                    "Regex evaluation (NOT): pattern='{}' text='{}' matches={}",
                                    value, field_value_str, not_matches
                                );
                                (not_matches, None)
                            }
                            Err(e) => {
                                warn!(
                                    "RULE_PROCESSOR:   > Regex evaluation failed for pattern '{}': {}",
                                    value, e
                                );
                                (false, None)
                            }
                        }
                    }
                    FilterOperator::GreaterThan => {
                        match self.compare_values(
                            &field_value_str,
                            value,
                            std::cmp::Ordering::Greater,
                        ) {
                            Ok(result) => (result, None),
                            Err(e) => {
                                warn!("RULE_PROCESSOR:   > Comparison failed: {}", e);
                                (false, None)
                            }
                        }
                    }
                    FilterOperator::LessThan => {
                        match self.compare_values(&field_value_str, value, std::cmp::Ordering::Less)
                        {
                            Ok(result) => (result, None),
                            Err(e) => {
                                warn!("RULE_PROCESSOR:   > Comparison failed: {}", e);
                                (false, None)
                            }
                        }
                    }
                    FilterOperator::GreaterThanOrEqual => {
                        match self.compare_values(
                            &field_value_str,
                            value,
                            std::cmp::Ordering::Greater,
                        ) {
                            Ok(result) => {
                                let equal = field_value_str.eq_ignore_ascii_case(value);
                                (result || equal, None)
                            }
                            Err(e) => {
                                warn!("RULE_PROCESSOR:   > Comparison failed: {}", e);
                                (false, None)
                            }
                        }
                    }
                    FilterOperator::LessThanOrEqual => {
                        match self.compare_values(&field_value_str, value, std::cmp::Ordering::Less)
                        {
                            Ok(result) => {
                                let equal = field_value_str.eq_ignore_ascii_case(value);
                                (result || equal, None)
                            }
                            Err(e) => {
                                warn!("RULE_PROCESSOR:   > Comparison failed: {}", e);
                                (false, None)
                            }
                        }
                    }
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
                    let (child_result, child_captures) =
                        self.evaluate_condition_node_with_captures(child, record)?;
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

    /// Apply a parsed action (placeholder edit – full refactor pending eval context integration)
    fn apply_parsed_action(
        &self,
        action: &Action,
        record: &mut crate::models::Channel,
        _channel_name: &str,
    ) -> Result<Option<FieldModification>, Box<dyn std::error::Error>> {
        let old_value = self.get_field_value(&action.field, record)?;

        let modification_type = match &action.operator {
            ActionOperator::Set => ModificationType::Set,
            ActionOperator::SetIfEmpty => {
                if let Some(ref v) = old_value
                    && !v.trim().is_empty()
                {
                    return Ok(None); // Don't modify if field has a (non-whitespace) value
                }
                ModificationType::SetIfEmpty
            }
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
                    }
                    crate::models::ActionValue::Null => {
                        // Set field to None/empty
                        self.apply_parsed_action(
                            &Action {
                                field: action.field.clone(),
                                operator: ActionOperator::Delete,
                                value: action.value.clone(),
                            },
                            record,
                            _channel_name,
                        )
                    }
                    _ => Ok(None), // Other action value types not implemented yet
                }
            }
            ActionOperator::Append => {
                match &action.value {
                    crate::models::ActionValue::Literal(append_value) => {
                        let current_value = old_value.as_deref().unwrap_or_default();
                        let new_value = format!("{current_value}{append_value}");
                        self.set_field_value(&action.field, &new_value, record)?;

                        Ok(Some(FieldModification {
                            field_name: action.field.clone(),
                            old_value: old_value.clone(),
                            new_value: Some(new_value),
                            modification_type,
                        }))
                    }
                    _ => Ok(None), // Other action value types not supported for append
                }
            }
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
                    }
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
            }
            ActionOperator::Remove => {
                // Remove specific value from field - for simplicity, treat as delete for now
                self.apply_parsed_action(
                    &Action {
                        field: action.field.clone(),
                        operator: ActionOperator::Delete,
                        value: action.value.clone(),
                    },
                    record,
                    _channel_name,
                )
            }
        }
    }

    /// Apply a parsed action with capture group substitution
    fn apply_parsed_action_with_captures(
        &self,
        action: &Action,
        record: &mut crate::models::Channel,
        _channel_name: &str,
        captures: &Option<Vec<String>>,
    ) -> Result<Option<FieldModification>, Box<dyn std::error::Error>> {
        let old_value = self.get_field_value(&action.field, record)?;

        let modification_type = match &action.operator {
            ActionOperator::Set => ModificationType::Set,
            ActionOperator::SetIfEmpty => {
                if let Some(ref v) = old_value
                    && !v.trim().is_empty()
                {
                    return Ok(None); // Don't modify if field has a (non-whitespace) value
                }
                ModificationType::SetIfEmpty
            }
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
                    }
                    crate::models::ActionValue::Null => {
                        // Set field to None/empty
                        self.apply_parsed_action_with_captures(
                            &Action {
                                field: action.field.clone(),
                                operator: ActionOperator::Delete,
                                value: action.value.clone(),
                            },
                            record,
                            _channel_name,
                            captures,
                        )
                    }
                    _ => Ok(None), // Other action value types not implemented yet
                }
            }
            ActionOperator::Append => {
                match &action.value {
                    crate::models::ActionValue::Literal(append_value) => {
                        let processed_value =
                            self.substitute_capture_groups(append_value, captures);
                        let current_value = old_value.as_deref().unwrap_or_default();
                        let new_value = format!("{current_value}{processed_value}");
                        self.set_field_value(&action.field, &new_value, record)?;

                        Ok(Some(FieldModification {
                            field_name: action.field.clone(),
                            old_value: old_value.clone(),
                            new_value: Some(new_value),
                            modification_type,
                        }))
                    }
                    _ => Ok(None), // Other action value types not supported for append
                }
            }
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
                    }
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
            }
            ActionOperator::Remove => {
                // Remove specific value from field - for simplicity, treat as delete for now
                self.apply_parsed_action_with_captures(
                    &Action {
                        field: action.field.clone(),
                        operator: ActionOperator::Delete,
                        value: action.value.clone(),
                    },
                    record,
                    _channel_name,
                    captures,
                )
            }
        }
    }

    /// Substitute capture groups ($1, $2, etc.) in a string with actual captured values
    fn substitute_capture_groups(&self, input: &str, captures: &Option<Vec<String>>) -> String {
        if let Some(capture_list) = captures {
            let mut result = input.to_string();

            // Replace $1, $2, $3, etc. with captured groups
            // Note: captures[0] is the full match, captures[1] is the first group, etc.
            for (i, capture) in capture_list.iter().enumerate().skip(1) {
                // Skip index 0 (full match)
                let placeholder = format!("${i}");
                result = result.replace(&placeholder, capture);
            }

            result
        } else {
            input.to_string()
        }
    }

    /// Get a field value (supports canonical + alias + injected source_* meta).
    /// Field name is first canonicalised via the FieldRegistry.
    fn get_field_value(
        &self,
        field_name: &str,
        record: &crate::models::Channel,
    ) -> Result<Option<String>, Box<dyn std::error::Error>> {
        let registry = crate::field_registry::FieldRegistry::global();
        // Resolve alias → canonical (if unknown returns None)
        let canonical = registry
            .canonical_or_none(field_name)
            .ok_or_else(|| anyhow::anyhow!("Unknown field: {}", field_name))?;

        // Handle injected read-only source_* fields using the metadata map (if present)
        if matches!(canonical, "source_name" | "source_type" | "source_url") {
            if let Some(map) = &self.source_meta_map {
                if let Some(meta) = map.get(&record.source_id) {
                    let v = match canonical {
                        "source_name" => &meta.name,
                        "source_type" => &meta.kind,
                        "source_url" => &meta.url_sanitised,
                        _ => unreachable!(),
                    };
                    trace!(
                        "FIELD_VALUE_DEBUG: field='{}' (canonical='{}') value='{}' channel='{}'",
                        field_name, canonical, v, record.channel_name
                    );
                    return Ok(Some(v.clone()));
                }
                trace!(
                    "FIELD_VALUE_DEBUG: missing source meta for channel='{}' source_id={}",
                    record.channel_name, record.source_id
                );
                return Ok(None);
            }
            trace!(
                "FIELD_VALUE_DEBUG: source meta map not set; field='{}' channel='{}'",
                canonical, record.channel_name
            );
            return Ok(None);
        }

        let result = match canonical {
            "tvg_id" => Ok(record.tvg_id.clone()),
            "tvg_name" => Ok(record.tvg_name.clone()),
            "tvg_logo" => Ok(record.tvg_logo.clone()),
            "tvg_shift" => Ok(record.tvg_shift.clone()),
            "tvg_chno" => Ok(record.tvg_chno.clone()),
            "group_title" => Ok(record.group_title.clone()),
            "channel_name" => Ok(Some(record.channel_name.clone())),
            "stream_url" => Ok(Some(record.stream_url.clone())),
            _ => Err(anyhow::anyhow!("Unknown field: {}", canonical).into()),
        };

        if let Ok(ref value) = result {
            trace!(
                "FIELD_VALUE_DEBUG: field='{}' (canonical='{}') value='{:?}' channel='{}'",
                field_name, canonical, value, record.channel_name
            );
        }

        result
    }

    /// Set a field value on a channel record (enforces read-only + alias canonicalisation).
    fn set_field_value(
        &self,
        field_name: &str,
        value: &str,
        record: &mut crate::models::Channel,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let registry = crate::field_registry::FieldRegistry::global();
        let canonical = registry
            .canonical_or_none(field_name)
            .ok_or_else(|| anyhow::anyhow!("Unknown field: {}", field_name))?;

        if registry.is_read_only(canonical) {
            return Err(anyhow::anyhow!("Field '{}' is read-only", canonical).into());
        }

        match canonical {
            "tvg_id" => record.tvg_id = Some(value.to_string()),
            "tvg_name" => record.tvg_name = Some(value.to_string()),
            "tvg_logo" => record.tvg_logo = Some(value.to_string()),
            "tvg_shift" => record.tvg_shift = Some(value.to_string()),
            "tvg_chno" => record.tvg_chno = Some(value.to_string()),
            "group_title" => record.group_title = Some(value.to_string()),
            "channel_name" => record.channel_name = value.to_string(),
            "stream_url" => record.stream_url = value.to_string(),
            _ => return Err(anyhow::anyhow!("Cannot set unknown field: {}", canonical).into()),
        }
        Ok(())
    }

    /// Set an optional field to None (with canonicalisation + read-only guard)
    fn set_optional_field_none(
        &self,
        field_name: &str,
        record: &mut crate::models::Channel,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let registry = crate::field_registry::FieldRegistry::global();
        let canonical = registry
            .canonical_or_none(field_name)
            .ok_or_else(|| anyhow::anyhow!("Unknown field: {}", field_name))?;
        if registry.is_read_only(canonical) {
            return Err(anyhow::anyhow!("Field '{}' is read-only", canonical).into());
        }

        match canonical {
            "tvg_id" => record.tvg_id = None,
            "tvg_name" => record.tvg_name = None,
            "tvg_logo" => record.tvg_logo = None,
            "tvg_shift" => record.tvg_shift = None,
            "tvg_chno" => record.tvg_chno = None,
            "group_title" => record.group_title = None,
            "channel_name" | "stream_url" => {
                return Err(
                    anyhow::anyhow!("Cannot set required field '{}' to None", canonical).into(),
                );
            }
            _ => return Err(anyhow::anyhow!("Cannot clear unknown field: {}", canonical).into()),
        }
        Ok(())
    }

    /// Compare two values using numeric or datetime comparison
    /// First tries to parse as Unix timestamps, then falls back to string comparison
    fn compare_values(
        &self,
        field_value: &str,
        expected_value: &str,
        ordering: std::cmp::Ordering,
    ) -> Result<bool, Box<dyn std::error::Error>> {
        use crate::utils::time::{parse_time_string, resolve_time_functions};

        // Resolve any @time: functions in the expected value
        let resolved_expected = resolve_time_functions(expected_value)?;

        // Try numeric comparison first (Unix timestamps)
        if let (Ok(field_num), Ok(expected_num)) = (
            parse_time_string(field_value),
            parse_time_string(&resolved_expected),
        ) {
            return Ok(field_num.cmp(&expected_num) == ordering);
        }

        // Fall back to lexicographic string comparison
        Ok(field_value.cmp(&resolved_expected) == ordering)
    }
}

impl RuleProcessor<crate::models::Channel> for StreamRuleProcessor {
    fn process_record(
        &mut self,
        record: crate::models::Channel,
    ) -> Result<(crate::models::Channel, RuleResult), Box<dyn std::error::Error>> {
        let start = std::time::Instant::now();

        // Parse and evaluate the expression
        let (modified_record, modifications, condition_matched) =
            match self.evaluate_expression(&record) {
                Ok((rec, mods, cond)) => (rec, mods, cond),
                Err(e) => {
                    warn!(
                        "RULE_PROCESSOR: Rule evaluation failed: rule_id={} error={}",
                        self.rule_id, e
                    );
                    let result = RuleResult {
                        condition_matched: false,
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
            trace!(
                "RULE_PROCESSOR: {} applied {} modifications to '{}'",
                self.rule_name,
                modifications.len(),
                record.channel_name
            );
        }

        let result = RuleResult {
            condition_matched,
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
    pub parsed_expression: Option<crate::models::ExtendedExpression>,
    /// If parsing/validation failed, capture the reason for diagnostics.
    pub parse_error: Option<String>,
    /// Optional runtime source metadata map (for resolving read-only source_* fields)
    pub source_meta_map: Option<
        std::sync::Arc<
            std::collections::HashMap<uuid::Uuid, crate::pipeline::eval_context::SourceMeta>,
        >,
    >,
}

impl EpgRuleProcessor {
    pub fn new(rule_id: String, rule_name: String, expression: String) -> Self {
        // Unified parse via parse_expression_extended (handles preprocessing + validation)
        let (parsed_expression, parse_error) = if expression.trim().is_empty() {
            (None, None)
        } else {
            match crate::expression::parse_expression_extended(
                ExpressionDomain::EpgRule,
                &expression,
            ) {
                Ok(Some(parsed_wrapper)) => {
                    let extended = parsed_wrapper.extended.clone();
                    // debug: epg rule parsed (removed println)
                    trace!(
                        "[EXPR_PARSE] domain=EpgRule id={} name={} len_raw={} expr='{}'",
                        rule_id,
                        rule_name,
                        expression.len(),
                        &expression
                    );
                    // Collect stats
                    fn walk_condition(
                        node: &crate::models::ConditionNode,
                        count: &mut usize,
                        fields: &mut std::collections::HashSet<String>,
                    ) {
                        match node {
                            crate::models::ConditionNode::Condition { field, .. } => {
                                *count += 1;
                                fields.insert(field.clone());
                            }
                            crate::models::ConditionNode::Group { children, .. } => {
                                for c in children {
                                    walk_condition(c, count, fields);
                                }
                            }
                        }
                    }
                    fn collect_stats(
                        expr: &crate::models::ExtendedExpression,
                    ) -> (usize, Vec<String>) {
                        use crate::models::ExtendedExpression;
                        let mut count = 0usize;
                        let mut fields = std::collections::HashSet::new();
                        match expr {
                            ExtendedExpression::ConditionOnly(tree) => {
                                walk_condition(&tree.root, &mut count, &mut fields);
                            }
                            ExtendedExpression::ConditionWithActions { condition, .. } => {
                                walk_condition(&condition.root, &mut count, &mut fields);
                            }
                            ExtendedExpression::ConditionalActionGroups(groups) => {
                                for g in groups {
                                    walk_condition(&g.conditions.root, &mut count, &mut fields);
                                }
                            }
                        }
                        let mut list: Vec<String> = fields.into_iter().collect();
                        list.sort();
                        (count, list)
                    }
                    let (node_count, field_list) = collect_stats(&extended);
                    // debug: epg rule fields (removed println)
                    trace!(
                        "[EXPR_PARSE] domain=EpgRule id={} name={} node_count={} fields=[{}] expr='{}'",
                        rule_id,
                        rule_name,
                        node_count,
                        field_list.join(","),
                        expression
                    );
                    (Some(extended), None)
                }
                Ok(None) => (None, None),
                Err(e) => {
                    let msg = format!(
                        "Failed to parse / validate EPG rule expression id={} name={} err={}",
                        rule_id, rule_name, e
                    );
                    // debug: epg rule parse error (removed println)
                    warn!("{}", msg);
                    (None, Some(msg))
                }
            }
        };

        Self {
            rule_id,
            rule_name,
            expression,
            parsed_expression,
            parse_error,
            source_meta_map: None,
        }
    }

    /// Inject (or replace) the runtime source metadata map enabling resolution
    /// of read-only `source_*` fields during EPG expression evaluation.
    pub fn set_source_meta_map(
        &mut self,
        map: std::sync::Arc<
            std::collections::HashMap<uuid::Uuid, crate::pipeline::eval_context::SourceMeta>,
        >,
    ) {
        self.source_meta_map = Some(map);
    }

    /// Builder-style variant for chaining.
    pub fn with_source_meta_map(
        mut self,
        map: std::sync::Arc<
            std::collections::HashMap<uuid::Uuid, crate::pipeline::eval_context::SourceMeta>,
        >,
    ) -> Self {
        self.source_meta_map = Some(map);
        self
    }
}

// For EPG programs - expanded to support all XMLTV fields for rule processing and data mapping
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct EpgProgram {
    pub id: String,
    pub channel_id: String,
    pub channel_name: String,
    pub title: String,
    pub description: Option<String>,
    pub program_icon: Option<String>,
    #[serde(serialize_with = "crate::utils::datetime::serialize_datetime")]
    #[serde(deserialize_with = "crate::utils::datetime::deserialize_datetime")]
    pub start_time: DateTime<Utc>,
    #[serde(serialize_with = "crate::utils::datetime::serialize_datetime")]
    #[serde(deserialize_with = "crate::utils::datetime::deserialize_datetime")]
    pub end_time: DateTime<Utc>,
    // Extended XMLTV fields for rich metadata support and rule processing
    pub program_category: Option<String>, // <category>
    pub subtitles: Option<String>,        // <sub-title> (episode subtitle)
    pub episode_num: Option<String>,      // Episode number for <episode-num>
    pub season_num: Option<String>,       // Season number for <episode-num>
    pub language: Option<String>,         // <language>
    pub rating: Option<String>,           // <rating>
    pub aspect_ratio: Option<String>,     // Video aspect ratio metadata
}

impl RuleProcessor<EpgProgram> for EpgRuleProcessor {
    fn process_record(
        &mut self,
        record: EpgProgram,
    ) -> Result<(EpgProgram, RuleResult), Box<dyn std::error::Error>> {
        let start = std::time::Instant::now();

        // Evaluate expression using cached parsed expression (same pattern as StreamRuleProcessor)
        let (modified_record, field_modifications, condition_matched) =
            match self.evaluate_expression(&record) {
                Ok((modified, modifications, cond)) => (modified, modifications, cond),
                Err(e) => {
                    return Ok((
                        record,
                        RuleResult {
                            condition_matched: false,
                            rule_applied: false,
                            field_modifications: vec![],
                            execution_time: start.elapsed(),
                            error: Some(e.to_string()),
                        },
                    ));
                }
            };

        let rule_applied = !field_modifications.is_empty();

        let result = RuleResult {
            condition_matched,
            rule_applied,
            field_modifications,
            execution_time: start.elapsed(),
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

impl EpgRuleProcessor {
    /// Evaluate the expression against an EPG program record using the cached parsed expression
    fn evaluate_expression(
        &self,
        record: &EpgProgram,
    ) -> Result<(EpgProgram, Vec<FieldModification>, bool), Box<dyn std::error::Error>> {
        let mut modified_record = record.clone();
        let mut modifications = Vec::new();

        // Use cached parsed expression (same pattern as StreamRuleProcessor)
        let parsed_expression = match &self.parsed_expression {
            Some(expr) => expr,
            None => {
                if let Some(err) = &self.parse_error {
                    // debug: epg rule skip parse_error (removed println)
                    warn!(
                        "EPG Rule {} has no valid parsed expression (parse_error='{}'), skipping",
                        self.rule_id, err
                    );
                } else {
                    // debug: epg rule skip no parsed expression (removed println)
                    trace!(
                        "EPG Rule {} has no valid parsed expression, skipping",
                        self.rule_id
                    );
                }
                return Ok((modified_record, modifications, false));
            }
        };

        // debug: epg rule eval begin (removed println)
        trace!(
            "Evaluating EPG rule {} against program '{}' (id={})",
            self.rule_id, record.title, record.id
        );

        // Evaluate the parsed expression (same structure as StreamRuleProcessor)
        let mut condition_matched = false;

        match parsed_expression {
            ExtendedExpression::ConditionWithActions { condition, actions } => {
                // Check if we need captures (only for regex operations)
                let needs_captures = self.condition_tree_needs_captures(condition);

                if needs_captures {
                    // Use captures version for regex-based conditions
                    let (condition_result, captures) =
                        self.evaluate_condition_tree_with_captures(condition, record)?;

                    trace!(
                        "EPG Rule {} condition evaluation result: {} captures: {:?}",
                        self.rule_id, condition_result, captures
                    );

                    if condition_result {
                        condition_matched = true;
                        trace!(
                            "EPG Rule {} condition matched, applying {} actions with captures",
                            self.rule_id,
                            actions.len()
                        );
                        for action in actions {
                            if let Some(modification) = self.apply_parsed_action_with_captures(
                                action,
                                &mut modified_record,
                                &record.title,
                                &captures,
                            )? {
                                trace!(
                                    "EPG Rule {} applied action: {} {:?} -> {:?}",
                                    self.rule_id,
                                    &modification.field_name,
                                    &modification.modification_type,
                                    &modification.new_value
                                );
                                modifications.push(modification);
                            }
                        }
                    } else {
                        trace!("EPG Rule {} condition did not match", self.rule_id);
                    }
                } else {
                    // Use simpler/faster version for non-regex conditions
                    let condition_result = self.evaluate_condition_tree(condition, record)?;
                    trace!(
                        "EPG Rule {} condition evaluation result: {} (fast path)",
                        self.rule_id, condition_result
                    );

                    if condition_result {
                        condition_matched = true;
                        trace!(
                            "EPG Rule {} condition matched, applying {} actions (fast path)",
                            self.rule_id,
                            actions.len()
                        );
                        for action in actions {
                            if let Some(modification) = self.apply_parsed_action(
                                action,
                                &mut modified_record,
                                &record.title,
                            )? {
                                trace!(
                                    "EPG Rule {} applied action: {} {:?} -> {:?}",
                                    self.rule_id,
                                    &modification.field_name,
                                    &modification.modification_type,
                                    &modification.new_value
                                );
                                modifications.push(modification);
                            }
                        }
                    } else {
                        trace!("EPG Rule {} condition did not match", self.rule_id);
                    }
                }
            }
            ExtendedExpression::ConditionOnly(condition) => {
                // Just evaluate condition - no actions to apply
                let _matches = self.evaluate_condition_tree(condition, record)?;
                if _matches {
                    condition_matched = true;
                }
                // No modifications for condition-only expressions
            }
            ExtendedExpression::ConditionalActionGroups(groups) => {
                // Process each conditional action group
                for group in groups {
                    let needs_captures = self.condition_tree_needs_captures(&group.conditions);

                    if needs_captures {
                        let (condition_result, group_captures) =
                            self.evaluate_condition_tree_with_captures(&group.conditions, record)?;
                        if condition_result {
                            for action in &group.actions {
                                if let Some(modification) = self.apply_parsed_action_with_captures(
                                    action,
                                    &mut modified_record,
                                    &record.title,
                                    &group_captures,
                                )? {
                                    modifications.push(modification);
                                }
                            }
                        }
                    } else {
                        let condition_result =
                            self.evaluate_condition_tree(&group.conditions, record)?;
                        if condition_result {
                            for action in &group.actions {
                                if let Some(modification) = self.apply_parsed_action(
                                    action,
                                    &mut modified_record,
                                    &record.title,
                                )? {
                                    modifications.push(modification);
                                }
                            }
                        }
                    }
                }
            }
        }

        Ok((modified_record, modifications, condition_matched))
    }

    /// Evaluate a condition tree (parsed expression structure)
    fn evaluate_condition_tree(
        &self,
        condition: &crate::models::ConditionTree,
        record: &EpgProgram,
    ) -> Result<bool, Box<dyn std::error::Error>> {
        self.evaluate_condition_node(&condition.root, record)
    }

    /// Evaluate a condition tree and return captures from regex matches
    fn evaluate_condition_tree_with_captures(
        &self,
        condition: &crate::models::ConditionTree,
        record: &EpgProgram,
    ) -> RegexCaptureResult {
        self.evaluate_condition_node_with_captures(&condition.root, record)
    }

    /// Check if a condition tree contains regex operators that need capture groups
    fn condition_tree_needs_captures(&self, condition: &crate::models::ConditionTree) -> bool {
        Self::condition_node_needs_captures(&condition.root)
    }

    /// Check if a condition node contains regex operators recursively
    fn condition_node_needs_captures(node: &crate::models::ConditionNode) -> bool {
        use crate::models::FilterOperator;

        match node {
            crate::models::ConditionNode::Condition { operator, .. } => {
                matches!(
                    operator,
                    FilterOperator::Matches | FilterOperator::NotMatches
                )
            }
            crate::models::ConditionNode::Group { children, .. } => {
                children.iter().any(Self::condition_node_needs_captures)
            }
        }
    }

    /// Evaluate a condition node recursively
    fn evaluate_condition_node(
        &self,
        node: &crate::models::ConditionNode,
        record: &EpgProgram,
    ) -> Result<bool, Box<dyn std::error::Error>> {
        self.evaluate_condition_node_with_captures(node, record)
            .map(|(result, _)| result)
    }

    /// Evaluate a condition node and return captures for regex matches
    fn evaluate_condition_node_with_captures(
        &self,
        node: &crate::models::ConditionNode,
        record: &EpgProgram,
    ) -> RegexCaptureResult {
        use crate::models::{FilterOperator, LogicalOperator};

        match node {
            crate::models::ConditionNode::Condition {
                field,
                operator,
                value,
                ..
            } => {
                let field_value = self.get_field_value(field, record)?;
                let field_value_str = field_value.unwrap_or_default();

                trace!(
                    "[EPG_RULE_COND] rule_id={} field='{}' op={:?} raw_value='{}' record_value='{}'",
                    self.rule_id, field, operator, value, field_value_str
                );

                // debug: epg rule condition (removed println)
                let (matches, captures) = match operator {
                    FilterOperator::Equals => {
                        let m = field_value_str.eq_ignore_ascii_case(value);
                        trace!(
                            "[EPG_RULE_COND_MATCH] rule_id={} field='{}' op=equals compare='{}' record='{}' matched={}",
                            self.rule_id, field, value, field_value_str, m
                        );
                        (m, None)
                    }
                    FilterOperator::NotEquals => {
                        let m = !field_value_str.eq_ignore_ascii_case(value);
                        trace!(
                            "[EPG_RULE_COND_MATCH] rule_id={} field='{}' op=not_equals compare='{}' record='{}' matched={}",
                            self.rule_id, field, value, field_value_str, m
                        );
                        (m, None)
                    }
                    FilterOperator::Contains => {
                        let cmp_left = field_value_str.to_lowercase();
                        let cmp_right = value.to_lowercase();
                        let m = cmp_left.contains(&cmp_right);
                        trace!(
                            "[EPG_RULE_COND_MATCH] rule_id={} field='{}' op=contains needle='{}' haystack='{}' matched={}",
                            self.rule_id, field, value, field_value_str, m
                        );
                        (m, None)
                    }
                    FilterOperator::NotContains => {
                        let cmp_left = field_value_str.to_lowercase();
                        let cmp_right = value.to_lowercase();
                        let m = !cmp_left.contains(&cmp_right);
                        trace!(
                            "[EPG_RULE_COND_MATCH] rule_id={} field='{}' op=not_contains needle='{}' haystack='{}' matched={}",
                            self.rule_id, field, value, field_value_str, m
                        );
                        (m, None)
                    }
                    FilterOperator::StartsWith => {
                        let cmp_left = field_value_str.to_lowercase();
                        let cmp_right = value.to_lowercase();
                        let m = cmp_left.starts_with(&cmp_right);
                        trace!(
                            "[EPG_RULE_COND_MATCH] rule_id={} field='{}' op=starts_with prefix='{}' value='{}' matched={}",
                            self.rule_id, field, value, field_value_str, m
                        );
                        (m, None)
                    }
                    FilterOperator::NotStartsWith => {
                        let cmp_left = field_value_str.to_lowercase();
                        let cmp_right = value.to_lowercase();
                        let m = !cmp_left.starts_with(&cmp_right);
                        trace!(
                            "[EPG_RULE_COND_MATCH] rule_id={} field='{}' op=not_starts_with prefix='{}' value='{}' matched={}",
                            self.rule_id, field, value, field_value_str, m
                        );
                        (m, None)
                    }
                    FilterOperator::EndsWith => {
                        let cmp_left = field_value_str.to_lowercase();
                        let cmp_right = value.to_lowercase();
                        let m = cmp_left.ends_with(&cmp_right);
                        trace!(
                            "[EPG_RULE_COND_MATCH] rule_id={} field='{}' op=ends_with suffix='{}' value='{}' matched={}",
                            self.rule_id, field, value, field_value_str, m
                        );
                        (m, None)
                    }
                    FilterOperator::NotEndsWith => {
                        let cmp_left = field_value_str.to_lowercase();
                        let cmp_right = value.to_lowercase();
                        let m = !cmp_left.ends_with(&cmp_right);
                        trace!(
                            "[EPG_RULE_COND_MATCH] rule_id={} field='{}' op=not_ends_with suffix='{}' value='{}' matched={}",
                            self.rule_id, field, value, field_value_str, m
                        );
                        (m, None)
                    }
                    FilterOperator::Matches => match Regex::new(value) {
                        Ok(regex) => {
                            // debug: epg rule regex (removed println)
                            if let Some(caps) = regex.captures(&field_value_str) {
                                let capture_strings: Vec<String> = caps
                                    .iter()
                                    .map(|m| m.map_or("".to_string(), |m| m.as_str().to_string()))
                                    .collect();
                                (true, Some(capture_strings))
                            } else {
                                (false, None)
                            }
                        }
                        Err(e) => {
                            warn!(
                                "Invalid regex pattern '{}': {}, falling back to contains",
                                value, e
                            );
                            (field_value_str.contains(value), None)
                        }
                    },
                    FilterOperator::NotMatches => match Regex::new(value) {
                        Ok(regex) => (!regex.is_match(&field_value_str), None),
                        Err(_) => (!field_value_str.contains(value), None),
                    },
                    // For EPG programs, these comparison operators may not be relevant in most cases
                    // but we'll provide basic string comparison fallbacks
                    FilterOperator::GreaterThan => (field_value_str > *value, None),
                    FilterOperator::LessThan => (field_value_str < *value, None),
                    FilterOperator::GreaterThanOrEqual => (field_value_str >= *value, None),
                    FilterOperator::LessThanOrEqual => (field_value_str <= *value, None),
                };

                trace!("EPG condition result: {}", matches);
                // debug: epg rule condition result (removed println)
                Ok((matches, captures))
            }
            crate::models::ConditionNode::Group { operator, children } => {
                let mut group_result = match operator {
                    LogicalOperator::And => true, // Start true for AND
                    LogicalOperator::Or => false, // Start false for OR
                };
                let mut group_captures: Option<Vec<String>> = None;

                for child in children {
                    let (child_result, child_captures) =
                        self.evaluate_condition_node_with_captures(child, record)?;

                    // Collect captures from first matching child (for capture group substitution)
                    if child_captures.is_some() && group_captures.is_none() {
                        group_captures = child_captures;
                    }

                    match operator {
                        LogicalOperator::And => {
                            group_result = group_result && child_result;
                            if !group_result {
                                break; // Short-circuit AND
                            }
                        }
                        LogicalOperator::Or => {
                            group_result = group_result || child_result;
                            if group_result {
                                break; // Short-circuit OR
                            }
                        }
                    }
                }

                Ok((group_result, group_captures))
            }
        }
    }

    /// Get field value from an EPG program record (canonical + alias + injected source_* support)
    fn get_field_value(
        &self,
        field_name: &str,
        record: &EpgProgram,
    ) -> Result<Option<String>, Box<dyn std::error::Error>> {
        let registry = crate::field_registry::FieldRegistry::global();
        let canonical = registry
            .canonical_or_none(field_name)
            .ok_or_else(|| anyhow::anyhow!("Unknown EPG field: {}", field_name))?;

        // Handle injected source_* metadata (read-only fields)
        if matches!(canonical, "source_name" | "source_type" | "source_url") {
            if let Some(map) = &self.source_meta_map
                && let Some(meta) = map.get(&record.channel_id.parse().unwrap_or(uuid::Uuid::nil()))
            {
                // NOTE: channel_id here may not be a UUID; if not, metadata will be None.
                // If a direct source_id is required, extend EpgProgram to carry source_id.
                let v = match canonical {
                    "source_name" => &meta.name,
                    "source_type" => &meta.kind,
                    "source_url" => &meta.url_sanitised,
                    _ => unreachable!(),
                };
                return Ok(Some(v.clone()));
            }
            return Ok(None);
        }

        // Map canonical programme fields to record properties
        let value = match canonical {
            // Channel linkage
            "channel_id" => Some(record.channel_id.clone()),
            "channel_name" => Some(record.channel_name.clone()),
            // Programme canonical (British spellings)
            "programme_title" => Some(record.title.clone()),
            "programme_description" => record.description.clone(),
            "programme_category" => record.program_category.clone(),
            "programme_icon" => record.program_icon.clone(),
            "programme_subtitle" => record.subtitles.clone(),
            // Aliased legacy still canonicalised above
            "episode_num" => record.episode_num.clone(),
            "season_num" => record.season_num.clone(),
            "language" => record.language.clone(),
            "rating" => record.rating.clone(),
            "aspect_ratio" => record.aspect_ratio.clone(),
            // Internal / unexposed
            "id" => Some(record.id.clone()),
            other => {
                return Err(anyhow::anyhow!("Unknown canonical EPG field: {}", other).into());
            }
        };

        Ok(value)
    }

    /// Apply parsed action without capture groups
    fn apply_parsed_action(
        &self,
        action: &crate::models::Action,
        record: &mut EpgProgram,
        _context: &str,
    ) -> Result<Option<FieldModification>, Box<dyn std::error::Error>> {
        // Canonicalize the action field name (program_* -> programme_*) before lookups / modifications
        let registry = crate::field_registry::FieldRegistry::global();
        let canonical_field = registry
            .canonical_or_none(&action.field)
            .unwrap_or(action.field.as_str())
            .to_string();

        // Use canonical field for value retrieval
        let old_value = self.get_field_value(&canonical_field, record)?;

        let modification_type = match action.operator {
            ActionOperator::Set => ModificationType::Set,
            ActionOperator::SetIfEmpty => {
                if let Some(ref v) = old_value
                    && !v.trim().is_empty()
                {
                    return Ok(None); // Don't modify if field has a (non-whitespace) value
                }
                ModificationType::SetIfEmpty
            }
            ActionOperator::Append => ModificationType::Append,
            ActionOperator::Remove => ModificationType::Remove,
            ActionOperator::Delete => ModificationType::Delete,
        };

        match &action.operator {
            ActionOperator::Set | ActionOperator::SetIfEmpty => {
                match &action.value {
                    crate::models::ActionValue::Literal(new_value) => {
                        self.set_field_value(&canonical_field, new_value, record)?;

                        Ok(Some(FieldModification {
                            field_name: canonical_field.clone(),
                            old_value: old_value.clone(),
                            new_value: Some(new_value.clone()),
                            modification_type,
                        }))
                    }
                    crate::models::ActionValue::Null => {
                        // Set field to None/empty
                        self.set_field_value(&canonical_field, "", record)?;

                        Ok(Some(FieldModification {
                            field_name: canonical_field.clone(),
                            old_value: old_value.clone(),
                            new_value: None,
                            modification_type,
                        }))
                    }
                    _ => {
                        trace!(
                            "Unsupported action value type for EPG rule {}",
                            self.rule_id
                        );
                        Ok(None)
                    }
                }
            }
            ActionOperator::Append => match &action.value {
                crate::models::ActionValue::Literal(append_value) => {
                    let current_value = old_value.as_deref().unwrap_or("");
                    let new_value = if current_value.is_empty() {
                        append_value.clone()
                    } else {
                        format!("{} {}", current_value, append_value)
                    };

                    self.set_field_value(&canonical_field, &new_value, record)?;

                    Ok(Some(FieldModification {
                        field_name: canonical_field.clone(),
                        old_value: old_value.clone(),
                        new_value: Some(new_value),
                        modification_type,
                    }))
                }
                _ => Ok(None),
            },
            ActionOperator::Remove => {
                // For EPG, remove means clear the field
                self.set_field_value(&canonical_field, "", record)?;

                Ok(Some(FieldModification {
                    field_name: canonical_field.clone(),
                    old_value: old_value.clone(),
                    new_value: None,
                    modification_type,
                }))
            }
            ActionOperator::Delete => {
                // Delete means set to None
                self.set_field_value(&canonical_field, "", record)?;

                Ok(Some(FieldModification {
                    field_name: canonical_field.clone(),
                    old_value: old_value.clone(),
                    new_value: None,
                    modification_type,
                }))
            }
        }
    }

    /// Apply parsed action with capture groups
    fn apply_parsed_action_with_captures(
        &self,
        action: &crate::models::Action,
        record: &mut EpgProgram,
        context: &str,
        captures: &Option<Vec<String>>,
    ) -> Result<Option<FieldModification>, Box<dyn std::error::Error>> {
        let old_value = self.get_field_value(&action.field, record)?;

        let modification_type = match action.operator {
            ActionOperator::Set => ModificationType::Set,
            ActionOperator::SetIfEmpty => {
                if let Some(ref v) = old_value
                    && !v.trim().is_empty()
                {
                    return Ok(None); // Don't modify if field has a (non-whitespace) value
                }
                ModificationType::SetIfEmpty
            }
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
                    }
                    crate::models::ActionValue::Null => {
                        // Set field to None/empty
                        self.apply_parsed_action(
                            &crate::models::Action {
                                field: action.field.clone(),
                                operator: action.operator.clone(),
                                value: crate::models::ActionValue::Null,
                            },
                            record,
                            context,
                        )
                    }
                    _ => Ok(None),
                }
            }
            _ => {
                // For other operators, fall back to non-capture version
                self.apply_parsed_action(action, record, context)
            }
        }
    }

    /// Substitute capture groups in a string
    fn substitute_capture_groups(&self, input: &str, captures: &Option<Vec<String>>) -> String {
        if let Some(capture_list) = captures {
            let mut result = input.to_string();

            // Replace $1, $2, $3, etc. with captured groups
            for (i, capture) in capture_list.iter().enumerate().skip(1) {
                let placeholder = format!("${}", i);
                result = result.replace(&placeholder, capture);
            }

            result
        } else {
            input.to_string()
        }
    }

    /// Set field value in EPG program record (canonicalizing aliases to their programme_* forms)
    fn set_field_value(
        &self,
        field_name: &str,
        value: &str,
        record: &mut EpgProgram,
    ) -> Result<(), Box<dyn std::error::Error>> {
        // Canonicalize (e.g. program_title -> programme_title) before matching
        let registry = crate::field_registry::FieldRegistry::global();
        let canonical = registry.canonical_or_none(field_name).unwrap_or(field_name);

        match canonical {
            // Core identifiers
            "id" => record.id = value.to_string(),
            "channel_id" => record.channel_id = value.to_string(),
            "channel_name" => record.channel_name = value.to_string(),

            // Programme canonical (British spellings)
            "programme_title" => record.title = value.to_string(),
            "programme_description" => {
                if value.is_empty() {
                    record.description = None;
                } else {
                    record.description = Some(value.to_string());
                }
            }
            "programme_category" => {
                if value.is_empty() {
                    record.program_category = None;
                } else {
                    record.program_category = Some(value.to_string());
                }
            }
            "programme_icon" => {
                if value.is_empty() {
                    record.program_icon = None;
                } else {
                    record.program_icon = Some(value.to_string());
                }
            }
            "programme_subtitle" => {
                if value.is_empty() {
                    record.subtitles = None;
                } else {
                    record.subtitles = Some(value.to_string());
                }
            }

            // Episode / season / metadata
            "episode_num" => {
                if value.is_empty() {
                    record.episode_num = None;
                } else {
                    record.episode_num = Some(value.to_string());
                }
            }
            "season_num" => {
                if value.is_empty() {
                    record.season_num = None;
                } else {
                    record.season_num = Some(value.to_string());
                }
            }
            "language" => {
                if value.is_empty() {
                    record.language = None;
                } else {
                    record.language = Some(value.to_string());
                }
            }
            "rating" => {
                if value.is_empty() {
                    record.rating = None;
                } else {
                    record.rating = Some(value.to_string());
                }
            }
            "aspect_ratio" => {
                if value.is_empty() {
                    record.aspect_ratio = None;
                } else {
                    record.aspect_ratio = Some(value.to_string());
                }
            }

            // If we missed a field, surface the original requested name for clarity
            _ => return Err(anyhow::anyhow!("Unknown EPG field: {}", field_name).into()),
        }

        Ok(())
    }
}
