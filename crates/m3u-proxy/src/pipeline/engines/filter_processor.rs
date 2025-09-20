/*!
 * Filter processor engines for extensible filtering (Stream + EPG)
 *
 * Refactored so BOTH StreamFilterProcessor and EpgFilterProcessor use the
 * unified expression abstraction (ParsedExpression) provided by
 * `crate::expression`. This keeps canonical field lists + alias handling
 * DRY and future-proofs adding modifiers (e.g. case_sensitive) and action
 * semantics without changing processors again.
 *
 * The legacy logic that stored only a ConditionTree has been replaced by
 * an Option<ParsedExpression>. An empty / whitespace-only expression is
 * treated as "match all".
 *
 * NOTE: Validation of unknown fields now occurs inside parse_expression_extended
 * (via domain-scoped field checking) for both Stream and EPG filters—no further
 * constructor changes required here.
 */

use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::Duration;
use tracing::{trace, warn};

// Helper to collect condition node statistics for logging
fn collect_condition_stats(node: &crate::models::ConditionNode) -> (usize, Vec<String>) {
    use crate::models::ConditionNode;
    use std::collections::HashSet;
    let mut count = 0usize;
    let mut fields = Vec::new();
    let mut seen = HashSet::new();

    fn walk(
        node: &ConditionNode,
        count: &mut usize,
        fields: &mut Vec<String>,
        seen: &mut HashSet<String>,
    ) {
        match node {
            ConditionNode::Condition { field, .. } => {
                *count += 1;
                if seen.insert(field.clone()) {
                    fields.push(field.clone());
                }
            }
            ConditionNode::Group { children, .. } => {
                for c in children {
                    walk(c, count, fields, seen);
                }
            }
        }
    }

    walk(node, &mut count, &mut fields, &mut seen);
    (count, fields)
}

use crate::expression::{ExpressionDomain, ParsedExpression, parse_expression_extended};
use crate::models::{ConditionNode, FilterOperator, LogicalOperator};
use crate::utils::regex_preprocessor::RegexPreprocessor;

// -------------------------------------------------------------------------------------------------
// Shared Filter Result & Trait
// -------------------------------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FilterResult {
    pub include_match: bool,
    pub exclude_match: bool,
    pub execution_time: Duration,
    pub error: Option<String>,
}

pub trait FilterProcessor<T>: Send + Sync {
    fn process_record(&mut self, record: &T) -> Result<FilterResult, Box<dyn std::error::Error>>;
    fn get_filter_name(&self) -> &str;
    fn get_filter_id(&self) -> &str;
    fn is_inverse(&self) -> bool;
}

// -------------------------------------------------------------------------------------------------
// Regex Evaluator (shared)
// -------------------------------------------------------------------------------------------------

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
        if !self.preprocessor.should_run_regex(text, pattern, context) {
            return Ok(false);
        }
        match Regex::new(pattern) {
            Ok(regex) => Ok(regex.is_match(text)),
            Err(e) => {
                warn!(
                    "Invalid regex pattern '{}': {}, falling back to substring contains",
                    pattern, e
                );
                Ok(text.contains(pattern))
            }
        }
    }
}

// -------------------------------------------------------------------------------------------------
// Stream (Channel) Filter Processor
// -------------------------------------------------------------------------------------------------

pub struct StreamFilterProcessor {
    filter_id: String,
    filter_name: String,
    is_inverse: bool,
    parsed: Option<ParsedExpression>,
    regex_evaluator: RegexEvaluator,
}

impl StreamFilterProcessor {
    pub fn new(
        filter_id: String,
        filter_name: String,
        is_inverse: bool,
        condition_expression: &str,
        regex_evaluator: RegexEvaluator,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let parsed = if condition_expression.trim().is_empty() {
            None
        } else {
            match parse_expression_extended(ExpressionDomain::StreamFilter, condition_expression) {
                Ok(opt) => {
                    if opt.is_some() {
                        if let Some(parsed) = &opt {
                            let (node_count, mut fields) =
                                collect_condition_stats(&parsed.condition_tree().root);
                            fields.sort();
                            trace!(
                                "[EXPR_PARSE] domain=StreamFilter id={} name={} node_count={} fields=[{}] expr='{}'",
                                filter_id,
                                filter_name,
                                node_count,
                                fields.join(","),
                                condition_expression
                            );
                        } else {
                            trace!(
                                "[EXPR_PARSE] domain=StreamFilter id={} name={} node_count=0 fields=[] expr='{}'",
                                filter_id, filter_name, condition_expression
                            );
                        }
                    }
                    opt
                }
                Err(e) => {
                    return Err(format!(
                        "Failed to parse stream filter expression ({}): {}",
                        filter_id, e
                    )
                    .into());
                }
            }
        };

        Ok(Self {
            filter_id,
            filter_name,
            is_inverse,
            parsed,
            regex_evaluator,
        })
    }

    fn evaluate_condition(
        &self,
        record: &crate::models::Channel,
    ) -> Result<bool, Box<dyn std::error::Error>> {
        let Some(parsed) = &self.parsed else {
            return Ok(true);
        };
        self.evaluate_condition_node(&parsed.condition_tree().root, record)
    }

    fn evaluate_condition_node(
        &self,
        node: &ConditionNode,
        record: &crate::models::Channel,
    ) -> Result<bool, Box<dyn std::error::Error>> {
        match node {
            ConditionNode::Condition {
                field,
                operator,
                value,
                case_sensitive,
                ..
            } => {
                let field_value = self.get_field_value(field, record)?.unwrap_or_default();

                // Apply case sensitivity
                let (left_cmp, right_cmp) = if *case_sensitive {
                    (field_value.clone(), value.clone())
                } else {
                    (field_value.to_lowercase(), value.to_lowercase())
                };

                let matches = match operator {
                    FilterOperator::Equals => {
                        let result = if *case_sensitive {
                            field_value == *value
                        } else {
                            field_value.eq_ignore_ascii_case(value)
                        };
                        tracing::trace!(
                            "[FILTER_DEBUG] kind=stream op=equals field={} case_sensitive={} field_value='{}' compare='{}' result={}",
                            field,
                            case_sensitive,
                            field_value,
                            value,
                            result
                        );
                        result
                    }
                    FilterOperator::NotEquals => {
                        if *case_sensitive {
                            field_value != *value
                        } else {
                            !field_value.eq_ignore_ascii_case(value)
                        }
                    }
                    FilterOperator::Contains => left_cmp.contains(&right_cmp),
                    FilterOperator::NotContains => !left_cmp.contains(&right_cmp),
                    FilterOperator::StartsWith => left_cmp.starts_with(&right_cmp),
                    FilterOperator::NotStartsWith => !left_cmp.starts_with(&right_cmp),
                    FilterOperator::EndsWith => left_cmp.ends_with(&right_cmp),
                    FilterOperator::NotEndsWith => !left_cmp.ends_with(&right_cmp),
                    FilterOperator::Matches => self.regex_evaluator.evaluate_with_preprocessing(
                        value,
                        &field_value,
                        &self.filter_id,
                    )?,
                    FilterOperator::NotMatches => !self
                        .regex_evaluator
                        .evaluate_with_preprocessing(value, &field_value, &self.filter_id)?,
                    FilterOperator::GreaterThan => {
                        self.compare_values(&field_value, value, std::cmp::Ordering::Greater)?
                    }
                    FilterOperator::LessThan => {
                        self.compare_values(&field_value, value, std::cmp::Ordering::Less)?
                    }
                    FilterOperator::GreaterThanOrEqual => {
                        let gt =
                            self.compare_values(&field_value, value, std::cmp::Ordering::Greater)?;
                        gt || (if *case_sensitive {
                            field_value == *value
                        } else {
                            field_value.eq_ignore_ascii_case(value)
                        })
                    }
                    FilterOperator::LessThanOrEqual => {
                        let lt =
                            self.compare_values(&field_value, value, std::cmp::Ordering::Less)?;
                        lt || (if *case_sensitive {
                            field_value == *value
                        } else {
                            field_value.eq_ignore_ascii_case(value)
                        })
                    }
                };
                Ok(matches)
            }
            ConditionNode::Group { operator, children } => {
                if children.is_empty() {
                    return Ok(true);
                }
                let mut results = Vec::with_capacity(children.len());
                for child in children {
                    results.push(self.evaluate_condition_node(child, record)?);
                }
                let res = match operator {
                    LogicalOperator::And => results.iter().all(|&r| r),
                    LogicalOperator::Or => results.iter().any(|&r| r),
                };
                Ok(res)
            }
        }
    }

    fn get_field_value(
        &self,
        field_name: &str,
        record: &crate::models::Channel,
    ) -> Result<Option<String>, Box<dyn std::error::Error>> {
        let v = match field_name {
            "tvg_id" => record.tvg_id.clone(),
            "tvg_name" => record.tvg_name.clone(),
            "tvg_logo" => record.tvg_logo.clone(),
            "tvg_shift" => record.tvg_shift.clone(),
            "group_title" => record.group_title.clone(),
            "channel_name" => Some(record.channel_name.clone()),
            "stream_url" => Some(record.stream_url.clone()),
            "source_name" => None, // Provided at higher layers if needed
            "source_type" => None,
            "source_url" => None,
            _ => {
                return Err(anyhow::anyhow!(
                    "Unknown stream/channel field referenced in filter: {}",
                    field_name
                )
                .into());
            }
        };
        Ok(v)
    }

    fn compare_values(
        &self,
        field_value: &str,
        compare_value: &str,
        ordering: std::cmp::Ordering,
    ) -> Result<bool, Box<dyn std::error::Error>> {
        if let (Ok(a), Ok(b)) = (field_value.parse::<f64>(), compare_value.parse::<f64>()) {
            Ok(a.partial_cmp(&b).unwrap_or(std::cmp::Ordering::Equal) == ordering)
        } else {
            Ok(field_value.cmp(compare_value) == ordering)
        }
    }
}

impl FilterProcessor<crate::models::Channel> for StreamFilterProcessor {
    fn process_record(
        &mut self,
        record: &crate::models::Channel,
    ) -> Result<FilterResult, Box<dyn std::error::Error>> {
        let start = std::time::Instant::now();
        match self.evaluate_condition(record) {
            Ok(matched) => {
                let include_match = if self.is_inverse { !matched } else { matched };
                Ok(FilterResult {
                    include_match,
                    exclude_match: !include_match,
                    execution_time: start.elapsed(),
                    error: None,
                })
            }
            Err(e) => {
                warn!(
                    "STREAM filter evaluation error filter_id={} name={} err={}",
                    self.filter_id, self.filter_name, e
                );
                Ok(FilterResult {
                    include_match: false,
                    exclude_match: true,
                    execution_time: start.elapsed(),
                    error: Some(e.to_string()),
                })
            }
        }
    }

    fn get_filter_name(&self) -> &str {
        &self.filter_name
    }
    fn get_filter_id(&self) -> &str {
        &self.filter_id
    }
    fn is_inverse(&self) -> bool {
        self.is_inverse
    }
}

// -------------------------------------------------------------------------------------------------
// EPG (Program) Filter Processor
// -------------------------------------------------------------------------------------------------

pub struct EpgFilterProcessor {
    filter_id: String,
    filter_name: String,
    is_inverse: bool,
    parsed: Option<ParsedExpression>,
    regex_evaluator: RegexEvaluator,
}

impl EpgFilterProcessor {
    pub fn new(
        filter_id: String,
        filter_name: String,
        is_inverse: bool,
        condition_expression: &str,
        regex_evaluator: RegexEvaluator,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let parsed = if condition_expression.trim().is_empty() {
            None
        } else {
            match parse_expression_extended(ExpressionDomain::EpgFilter, condition_expression) {
                Ok(opt) => {
                    if opt.is_some() {
                        if let Some(parsed) = &opt {
                            let (node_count, mut fields) =
                                collect_condition_stats(&parsed.condition_tree().root);
                            fields.sort();
                            trace!(
                                "[EXPR_PARSE] domain=EpgFilter id={} name={} node_count={} fields=[{}] expr='{}'",
                                filter_id,
                                filter_name,
                                node_count,
                                fields.join(","),
                                condition_expression
                            );
                        } else {
                            trace!(
                                "[EXPR_PARSE] domain=EpgFilter id={} name={} node_count=0 fields=[] expr='{}'",
                                filter_id, filter_name, condition_expression
                            );
                        }
                    }
                    opt
                }
                Err(e) => {
                    return Err(format!(
                        "Failed to parse EPG filter expression ({}): {}",
                        filter_id, e
                    )
                    .into());
                }
            }
        };

        Ok(Self {
            filter_id,
            filter_name,
            is_inverse,
            parsed,
            regex_evaluator,
        })
    }

    fn evaluate_condition(
        &self,
        record: &crate::pipeline::engines::rule_processor::EpgProgram,
    ) -> Result<bool, Box<dyn std::error::Error>> {
        let Some(parsed) = &self.parsed else {
            return Ok(true);
        };
        self.evaluate_condition_node(&parsed.condition_tree().root, record)
    }

    fn evaluate_condition_node(
        &self,
        node: &ConditionNode,
        record: &crate::pipeline::engines::rule_processor::EpgProgram,
    ) -> Result<bool, Box<dyn std::error::Error>> {
        match node {
            ConditionNode::Condition {
                field,
                operator,
                value,
                case_sensitive,
                ..
            } => {
                let field_value = self.get_field_value(field, record)?.unwrap_or_default();
                let (left_cmp, right_cmp) = if *case_sensitive {
                    (field_value.clone(), value.clone())
                } else {
                    (field_value.to_lowercase(), value.to_lowercase())
                };

                let matches = match operator {
                    FilterOperator::Equals => {
                        let result = if *case_sensitive {
                            field_value == *value
                        } else {
                            field_value.eq_ignore_ascii_case(value)
                        };
                        tracing::trace!(
                            "[FILTER_DEBUG] kind=epg op=equals field={} case_sensitive={} field_value='{}' compare='{}' result={}",
                            field,
                            case_sensitive,
                            field_value,
                            value,
                            result
                        );
                        result
                    }
                    FilterOperator::NotEquals => {
                        if *case_sensitive {
                            field_value != *value
                        } else {
                            !field_value.eq_ignore_ascii_case(value)
                        }
                    }
                    FilterOperator::Contains => left_cmp.contains(&right_cmp),
                    FilterOperator::NotContains => !left_cmp.contains(&right_cmp),
                    FilterOperator::StartsWith => left_cmp.starts_with(&right_cmp),
                    FilterOperator::NotStartsWith => !left_cmp.starts_with(&right_cmp),
                    FilterOperator::EndsWith => left_cmp.ends_with(&right_cmp),
                    FilterOperator::NotEndsWith => !left_cmp.ends_with(&right_cmp),
                    FilterOperator::Matches => self.regex_evaluator.evaluate_with_preprocessing(
                        value,
                        &field_value,
                        &self.filter_id,
                    )?,
                    FilterOperator::NotMatches => !self
                        .regex_evaluator
                        .evaluate_with_preprocessing(value, &field_value, &self.filter_id)?,
                    FilterOperator::GreaterThan => {
                        self.compare_values(&field_value, value, std::cmp::Ordering::Greater)?
                    }
                    FilterOperator::LessThan => {
                        self.compare_values(&field_value, value, std::cmp::Ordering::Less)?
                    }
                    FilterOperator::GreaterThanOrEqual => {
                        let gt =
                            self.compare_values(&field_value, value, std::cmp::Ordering::Greater)?;
                        gt || (if *case_sensitive {
                            field_value == *value
                        } else {
                            field_value.eq_ignore_ascii_case(value)
                        })
                    }
                    FilterOperator::LessThanOrEqual => {
                        let lt =
                            self.compare_values(&field_value, value, std::cmp::Ordering::Less)?;
                        lt || (if *case_sensitive {
                            field_value == *value
                        } else {
                            field_value.eq_ignore_ascii_case(value)
                        })
                    }
                };
                Ok(matches)
            }
            ConditionNode::Group { operator, children } => {
                if children.is_empty() {
                    return Ok(true);
                }
                let mut results = Vec::with_capacity(children.len());
                for child in children {
                    results.push(self.evaluate_condition_node(child, record)?);
                }
                let res = match operator {
                    LogicalOperator::And => results.iter().all(|&r| r),
                    LogicalOperator::Or => results.iter().any(|&r| r),
                };
                Ok(res)
            }
        }
    }

    fn get_field_value(
        &self,
        field_name: &str,
        record: &crate::pipeline::engines::rule_processor::EpgProgram,
    ) -> Result<Option<String>, Box<dyn std::error::Error>> {
        let v = match field_name {
            "channel_id" => Some(record.channel_id.clone()),
            "channel_name" => Some(record.channel_name.clone()),
            "programme_title" | "program_title" | "title" => Some(record.title.clone()),
            "programme_description" | "program_description" | "description" => {
                record.description.clone()
            }
            "programme_category" | "program_category" => record.program_category.clone(),
            "programme_icon" | "program_icon" => record.program_icon.clone(),
            "programme_subtitle" | "subtitles" => record.subtitles.clone(),
            "episode_num" => record.episode_num.clone(),
            "season_num" => record.season_num.clone(),
            "language" => record.language.clone(),
            "rating" => record.rating.clone(),
            "aspect_ratio" => record.aspect_ratio.clone(),
            // Extend with time fields if needed
            _ => {
                return Err(anyhow::anyhow!(
                    "Unknown EPG field referenced in filter: {}",
                    field_name
                )
                .into());
            }
        };
        Ok(v)
    }

    fn compare_values(
        &self,
        field_value: &str,
        compare_value: &str,
        ordering: std::cmp::Ordering,
    ) -> Result<bool, Box<dyn std::error::Error>> {
        if let (Ok(a), Ok(b)) = (field_value.parse::<f64>(), compare_value.parse::<f64>()) {
            Ok(a.partial_cmp(&b).unwrap_or(std::cmp::Ordering::Equal) == ordering)
        } else {
            Ok(field_value.cmp(compare_value) == ordering)
        }
    }
}

impl FilterProcessor<crate::pipeline::engines::rule_processor::EpgProgram> for EpgFilterProcessor {
    fn process_record(
        &mut self,
        record: &crate::pipeline::engines::rule_processor::EpgProgram,
    ) -> Result<FilterResult, Box<dyn std::error::Error>> {
        let start = std::time::Instant::now();
        match self.evaluate_condition(record) {
            Ok(matched) => {
                let include_match = if self.is_inverse { !matched } else { matched };
                Ok(FilterResult {
                    include_match,
                    exclude_match: !include_match,
                    execution_time: start.elapsed(),
                    error: None,
                })
            }
            Err(e) => {
                warn!(
                    "EPG filter evaluation error filter_id={} name={} err={}",
                    self.filter_id, self.filter_name, e
                );
                Ok(FilterResult {
                    include_match: false,
                    exclude_match: true,
                    execution_time: start.elapsed(),
                    error: Some(e.to_string()),
                })
            }
        }
    }

    fn get_filter_name(&self) -> &str {
        &self.filter_name
    }
    fn get_filter_id(&self) -> &str {
        &self.filter_id
    }
    fn is_inverse(&self) -> bool {
        self.is_inverse
    }
}

// -------------------------------------------------------------------------------------------------
// Generic Filtering Engine
// -------------------------------------------------------------------------------------------------

pub struct FilteringEngine<T> {
    filter_processors: Vec<Box<dyn FilterProcessor<T>>>,
    performance_stats: HashMap<String, (usize, usize, Duration)>,
}

impl<T> Default for FilteringEngine<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T> FilteringEngine<T> {
    pub fn new() -> Self {
        Self {
            filter_processors: Vec::new(),
            performance_stats: HashMap::new(),
        }
    }

    pub fn add_filter_processor(&mut self, processor: Box<dyn FilterProcessor<T>>) {
        self.filter_processors.push(processor);
    }

    pub fn process_records(
        &mut self,
        input_records: &[T],
    ) -> Result<FilterEngineResult<T>, Box<dyn std::error::Error>>
    where
        T: Clone,
    {
        let start_time = std::time::Instant::now();
        let mut filtered_indices: Vec<usize> = Vec::new();

        for processor in &mut self.filter_processors {
            let before_count = filtered_indices.len();
            if processor.is_inverse() {
                // EXCLUDE filter
                let mut remaining = Vec::with_capacity(filtered_indices.len());
                let iter = if filtered_indices.is_empty() {
                    (0..input_records.len()).collect::<Vec<_>>()
                } else {
                    filtered_indices.clone()
                };
                for idx in iter {
                    let record = &input_records[idx];
                    let r = processor.process_record(record)?;
                    if !r.exclude_match {
                        remaining.push(idx);
                    }
                }
                filtered_indices = remaining;
            } else {
                // INCLUDE filter
                if filtered_indices.is_empty() {
                    for (idx, record) in input_records.iter().enumerate() {
                        let r = processor.process_record(record)?;
                        if r.include_match {
                            filtered_indices.push(idx);
                        }
                    }
                } else {
                    let mut keep = Vec::with_capacity(filtered_indices.len());
                    for idx in &filtered_indices {
                        let r = processor.process_record(&input_records[*idx])?;
                        if r.include_match {
                            keep.push(*idx);
                        }
                    }
                    filtered_indices = keep;
                }
            }

            let after_count = filtered_indices.len();
            let included;
            let excluded;
            if processor.is_inverse() {
                included = 0;
                excluded = before_count.saturating_sub(after_count);
            } else if before_count == 0 {
                included = after_count;
                excluded = 0;
            } else {
                included = after_count;
                excluded = before_count.saturating_sub(after_count);
            }
            self.performance_stats.insert(
                processor.get_filter_id().to_string(),
                (included, excluded, start_time.elapsed()),
            );
        }

        let filtered_records = filtered_indices
            .into_iter()
            .map(|i| input_records[i].clone())
            .collect::<Vec<_>>();
        let total_filtered = filtered_records.len();

        Ok(FilterEngineResult {
            filtered_records,
            total_input: input_records.len(),
            total_filtered,
            execution_time: start_time.elapsed(),
            filter_stats: self.performance_stats.clone(),
        })
    }

    pub fn has_filters(&self) -> bool {
        !self.filter_processors.is_empty()
    }

    pub fn should_include(&mut self, record: &T) -> Result<bool, Box<dyn std::error::Error>> {
        if self.filter_processors.is_empty() {
            return Ok(true);
        }
        let mut include_any = false;
        let mut has_include = false;
        for p in &mut self.filter_processors {
            let r = p.process_record(record)?;
            if p.is_inverse() {
                if r.exclude_match {
                    return Ok(false);
                }
            } else {
                has_include = true;
                if r.include_match {
                    include_any = true;
                }
            }
        }
        Ok(if has_include { include_any } else { true })
    }

    pub fn clear_cache(&mut self) {
        self.performance_stats.clear();
    }
}

#[derive(Debug)]
pub struct FilterEngineResult<T> {
    pub filtered_records: Vec<T>,
    pub total_input: usize,
    pub total_filtered: usize,
    pub execution_time: Duration,
    pub filter_stats: HashMap<String, (usize, usize, Duration)>,
}

pub type ChannelFilteringEngine = FilteringEngine<crate::models::Channel>;
pub type EpgFilteringEngine = FilteringEngine<crate::pipeline::engines::rule_processor::EpgProgram>;

// -------------------------------------------------------------------------------------------------
// Tests
// -------------------------------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::Channel;
    use crate::pipeline::engines::rule_processor::EpgProgram;
    use crate::utils::regex_preprocessor::{RegexPreprocessor, RegexPreprocessorConfig};
    use chrono::{DateTime, Utc};
    use uuid::Uuid;

    fn regex_eval() -> RegexEvaluator {
        let pre = RegexPreprocessor::new(RegexPreprocessorConfig::default());
        RegexEvaluator::new(pre)
    }

    fn sample_channel(name: &str, group: &str, url: &str) -> Channel {
        Channel {
            id: Uuid::new_v4(),
            source_id: Uuid::new_v4(),
            tvg_id: Some(format!("tvg_{name}")),
            tvg_name: Some(name.to_string()),
            tvg_chno: Some("1".into()),
            tvg_logo: None,
            tvg_shift: None,
            group_title: Some(group.to_string()),
            channel_name: name.to_string(),
            stream_url: url.to_string(),
            video_codec: None,
            audio_codec: None,
            resolution: None,
            probe_method: None,
            last_probed_at: None,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }
    }

    fn sample_epg_program(title: &str, channel_id: &str, category: Option<&str>) -> EpgProgram {
        EpgProgram {
            id: format!("prog_{}", title.to_lowercase().replace(' ', "_")),
            channel_id: channel_id.to_string(),
            channel_name: format!("Channel {}", channel_id),
            title: title.to_string(),
            description: Some(format!("Description for {}", title)),
            program_icon: None,
            start_time: DateTime::parse_from_rfc3339("2024-01-01T12:00:00Z")
                .unwrap()
                .with_timezone(&Utc),
            end_time: DateTime::parse_from_rfc3339("2024-01-01T13:00:00Z")
                .unwrap()
                .with_timezone(&Utc),
            program_category: category.map(|c| c.to_string()),
            subtitles: Some("English".to_string()),
            episode_num: Some("1".to_string()),
            season_num: Some("1".to_string()),
            language: Some("en".to_string()),
            rating: Some("TV-PG".to_string()),
            aspect_ratio: Some("16:9".to_string()),
        }
    }

    // --- Stream filter tests ---

    #[test]
    fn test_stream_filter_processor_basic_matching() {
        let mut proc = StreamFilterProcessor::new(
            "test".into(),
            "Basic".into(),
            false,
            r#"channel_name equals "News Channel""#,
            regex_eval(),
        )
        .unwrap();

        let channel = sample_channel("News Channel", "News", "http://example.com/news");
        let r = proc.process_record(&channel).unwrap();
        assert!(r.include_match);
        assert!(!r.exclude_match);
    }

    #[test]
    fn test_stream_filter_processor_no_match() {
        let mut proc = StreamFilterProcessor::new(
            "test".into(),
            "NoMatch".into(),
            false,
            r#"channel_name equals "Sports Channel""#,
            regex_eval(),
        )
        .unwrap();
        let channel = sample_channel("News Channel", "News", "http://example.com/news");
        let r = proc.process_record(&channel).unwrap();
        assert!(!r.include_match);
        assert!(r.exclude_match);
    }

    #[test]
    fn test_stream_filter_processor_inverse_filter() {
        let mut proc = StreamFilterProcessor::new(
            "test".into(),
            "Inverse".into(),
            true,
            r#"channel_name equals "News Channel""#,
            regex_eval(),
        )
        .unwrap();
        let ch = sample_channel("News Channel", "News", "http://example.com/news");
        let r = proc.process_record(&ch).unwrap();
        assert!(!r.include_match);
        assert!(r.exclude_match);
    }

    #[test]
    fn test_stream_filter_processor_regex_matching() {
        let mut proc = StreamFilterProcessor::new(
            "test".into(),
            "Regex".into(),
            false,
            r#"channel_name matches "^News.*""#,
            regex_eval(),
        )
        .unwrap();
        let ch = sample_channel("News Channel 1", "News", "http://example.com/n1");
        let r = proc.process_record(&ch).unwrap();
        assert!(r.include_match);
    }

    #[test]
    fn test_stream_filter_processor_group_filtering() {
        let mut proc = StreamFilterProcessor::new(
            "test".into(),
            "Group".into(),
            false,
            r#"group_title equals "Entertainment""#,
            regex_eval(),
        )
        .unwrap();
        let ch = sample_channel("Movie Channel", "Entertainment", "http://movies");
        let r = proc.process_record(&ch).unwrap();
        assert!(r.include_match);
    }

    // --- EPG filter tests ---

    #[test]
    fn test_epg_filter_processor_basic_matching() {
        let mut proc = EpgFilterProcessor::new(
            "epg_basic".into(),
            "Epg Basic".into(),
            false,
            r#"program_title equals "Breaking News""#,
            regex_eval(),
        )
        .unwrap();
        let p = sample_epg_program("Breaking News", "ch1", Some("News"));
        let r = proc.process_record(&p).unwrap();
        assert!(r.include_match);
        assert!(!r.exclude_match);
    }

    #[test]
    fn test_epg_filter_processor_category_filtering() {
        let mut proc = EpgFilterProcessor::new(
            "epg_cat".into(),
            "Epg Cat".into(),
            false,
            r#"program_category equals "Movies""#,
            regex_eval(),
        )
        .unwrap();
        let movie = sample_epg_program("Action Hero", "ch2", Some("Movies"));
        let r = proc.process_record(&movie).unwrap();
        assert!(r.include_match);
        let news = sample_epg_program("Evening News", "ch2", Some("News"));
        let r2 = proc.process_record(&news).unwrap();
        assert!(!r2.include_match);
        assert!(r2.exclude_match);
    }

    #[test]
    fn test_epg_filter_processor_channel_filtering() {
        let mut proc = EpgFilterProcessor::new(
            "epg_channel".into(),
            "Epg Channel".into(),
            false,
            r#"channel_id equals "ch1""#,
            regex_eval(),
        )
        .unwrap();
        let p = sample_epg_program("Show", "ch1", Some("News"));
        let r = proc.process_record(&p).unwrap();
        assert!(r.include_match);
    }

    #[test]
    fn test_epg_filter_processor_regex_title_matching() {
        let mut proc = EpgFilterProcessor::new(
            "epg_regex".into(),
            "Epg Regex".into(),
            false,
            r#"program_title matches "^Movie:.*""#,
            regex_eval(),
        )
        .unwrap();
        let p = sample_epg_program("Movie: Action Hero", "c2", Some("Movies"));
        let r = proc.process_record(&p).unwrap();
        assert!(r.include_match);
        let p2 = sample_epg_program("Breaking News", "c2", Some("News"));
        let r2 = proc.process_record(&p2).unwrap();
        assert!(!r2.include_match);
    }

    // --- Engine tests (Stream) ---

    #[test]
    fn test_filtering_engine_has_filters() {
        let mut engine = FilteringEngine::<Channel>::new();
        assert!(!engine.has_filters());
        let proc = StreamFilterProcessor::new(
            "test".into(),
            "Test".into(),
            false,
            r#"channel_name equals "Test""#,
            regex_eval(),
        )
        .unwrap();
        engine.add_filter_processor(Box::new(proc));
        assert!(engine.has_filters());
    }

    #[test]
    fn test_filtering_engine_should_include_no_filters() {
        let mut engine = FilteringEngine::<Channel>::new();
        let ch = sample_channel("Any", "Group", "http://test");
        assert!(engine.should_include(&ch).unwrap());
    }

    #[test]
    fn test_filtering_engine_should_include_with_include_filter() {
        let mut engine = FilteringEngine::<Channel>::new();
        let proc = StreamFilterProcessor::new(
            "inc".into(),
            "Include".into(),
            false,
            r#"group_title equals "News""#,
            regex_eval(),
        )
        .unwrap();
        engine.add_filter_processor(Box::new(proc));
        let news = sample_channel("News Show", "News", "http://news");
        assert!(engine.should_include(&news).unwrap());
        let movie = sample_channel("Movie Show", "Movies", "http://mov");
        assert!(!engine.should_include(&movie).unwrap());
    }

    #[test]
    fn test_filtering_engine_should_include_with_exclude_filter() {
        let mut engine = FilteringEngine::<Channel>::new();
        let proc = StreamFilterProcessor::new(
            "exc".into(),
            "Exclude".into(),
            true,
            r#"group_title equals "Adult""#,
            regex_eval(),
        )
        .unwrap();
        engine.add_filter_processor(Box::new(proc));
        let adult = sample_channel("Adult Show", "Adult", "http://adult");
        assert!(!engine.should_include(&adult).unwrap());
        let news = sample_channel("News Show", "News", "http://news");
        assert!(engine.should_include(&news).unwrap());
    }

    #[test]
    fn test_epg_filtering_engine_with_programs() {
        let mut engine = FilteringEngine::<EpgProgram>::new();
        let proc = EpgFilterProcessor::new(
            "epg_movies".into(),
            "Movies".into(),
            false,
            r#"program_category equals "Movies""#,
            regex_eval(),
        )
        .unwrap();
        engine.add_filter_processor(Box::new(proc));
        let mov = sample_epg_program("Action Movie", "c1", Some("Movies"));
        assert!(engine.should_include(&mov).unwrap());
        let news = sample_epg_program("Evening News", "c1", Some("News"));
        assert!(!engine.should_include(&news).unwrap());
    }

    #[test]
    fn test_filtering_engine_combined_include_exclude() {
        let mut engine = FilteringEngine::<Channel>::new();
        let inc = StreamFilterProcessor::new(
            "inc".into(),
            "Include News".into(),
            false,
            r#"group_title equals "News""#,
            regex_eval(),
        )
        .unwrap();
        let exc = StreamFilterProcessor::new(
            "exc".into(),
            "Exclude Adult".into(),
            true,
            r#"channel_name matches ".*Adult.*""#,
            regex_eval(),
        )
        .unwrap();
        engine.add_filter_processor(Box::new(inc));
        engine.add_filter_processor(Box::new(exc));

        let news_channel = sample_channel("CNN News", "News", "http://cnn");
        assert!(engine.should_include(&news_channel).unwrap());

        let adult_news = sample_channel("Adult News", "News", "http://adult");
        assert!(!engine.should_include(&adult_news).unwrap());

        let movie_channel = sample_channel("Movie Channel", "Movies", "http://mov");
        assert!(!engine.should_include(&movie_channel).unwrap());
    }

    #[test]
    fn test_filter_processor_invalid_expression() {
        let r = StreamFilterProcessor::new(
            "bad".into(),
            "Bad".into(),
            false,
            "invalid expression syntax",
            regex_eval(),
        );
        assert!(r.is_err());
    }

    #[test]
    fn test_filter_processor_empty_expression() {
        let mut proc =
            StreamFilterProcessor::new("empty".into(), "Empty".into(), false, "", regex_eval())
                .unwrap();
        let ch = sample_channel("Test", "Group", "http://test");
        let r = proc.process_record(&ch).unwrap();
        // Empty => include
        assert!(r.include_match);
    }

    // --- Alias / Case-insensitive tests for EPG ---

    #[test]
    fn test_epg_filter_alias_resolution_program_title() {
        let mut proc = EpgFilterProcessor::new(
            "alias".into(),
            "Alias".into(),
            false,
            r#"program_title contains "Match""#,
            regex_eval(),
        )
        .unwrap();
        let p = sample_epg_program("Weekend Match", "Sports123", None);
        let r = proc.process_record(&p).unwrap();
        assert!(r.include_match, "Alias should canonicalize");
    }

    #[test]
    fn test_epg_filter_channel_id_contains_sport_case_insensitive() {
        let mut proc = EpgFilterProcessor::new(
            "sport".into(),
            "Sport".into(),
            false,
            r#"channel_id contains "sport""#,
            regex_eval(),
        )
        .unwrap();
        let match_prog = sample_epg_program("Show A", "BeInSports1.fr", None);
        let other_prog = sample_epg_program("Show B", "NewsWorld.fr", None);
        let r1 = proc.process_record(&match_prog).unwrap();
        let r2 = proc.process_record(&other_prog).unwrap();
        assert!(r1.include_match);
        assert!(!r2.include_match);
    }

    #[test]
    fn test_stream_filter_case_sensitive_equals() {
        // Should match only with exact case when case_sensitive modifier is used
        let mut proc = StreamFilterProcessor::new(
            "cs_eq".into(),
            "CaseSensitiveEquals".into(),
            false,
            r#"channel_name case_sensitive equals "News Channel""#,
            regex_eval(),
        )
        .unwrap();
        let ch_exact = sample_channel("News Channel", "News", "http://example.com/n1");
        let ch_diff = sample_channel("NEWS CHANNEL", "News", "http://example.com/n2");
        assert!(proc.process_record(&ch_exact).unwrap().include_match);
        assert!(!proc.process_record(&ch_diff).unwrap().include_match);
    }

    #[test]
    fn test_stream_filter_case_insensitive_default_equals() {
        // Without modifier, equals should be case-insensitive
        let mut proc = StreamFilterProcessor::new(
            "ci_eq".into(),
            "CaseInsensitiveEquals".into(),
            false,
            r#"channel_name equals "News Channel""#,
            regex_eval(),
        )
        .unwrap();
        let ch_upper = sample_channel("NEWS CHANNEL", "News", "http://example.com/n3");
        assert!(proc.process_record(&ch_upper).unwrap().include_match);
    }

    #[test]
    fn test_epg_filter_case_sensitive_contains() {
        let mut proc = EpgFilterProcessor::new(
            "epg_cs".into(),
            "EpgCaseSensitive".into(),
            false,
            r#"program_title case_sensitive contains "Match""#,
            regex_eval(),
        )
        .unwrap();
        let prog_exact = sample_epg_program("Weekend Match", "Sports123", None);
        let prog_lower = sample_epg_program("Weekend match", "Sports123", None);
        assert!(proc.process_record(&prog_exact).unwrap().include_match);
        assert!(!proc.process_record(&prog_lower).unwrap().include_match);
    }

    #[test]
    fn test_epg_filter_case_insensitive_default_contains() {
        let mut proc = EpgFilterProcessor::new(
            "epg_ci".into(),
            "EpgCaseInsensitive".into(),
            false,
            r#"program_title contains "Match""#,
            regex_eval(),
        )
        .unwrap();
        let prog_lower = sample_epg_program("Weekend match", "Sports123", None);
        assert!(proc.process_record(&prog_lower).unwrap().include_match);
        // Removed invalid assertion referencing undefined variables (excluded, programs, included)
    }

    #[test]
    fn test_epg_filter_unknown_field_suggestion() {
        // Intentionally misspelled field: program_titel (should suggest programme_title)
        let result = EpgFilterProcessor::new(
            "bad_epg".into(),
            "Bad EPG".into(),
            false,
            r#"program_titel contains "News""#,
            regex_eval(),
        );
        assert!(result.is_err(), "Expected parse error for unknown field");
        if let Err(e) = result {
            let msg = e.to_string();
            assert!(
                msg.contains("programme_title")
                    || msg.contains("program")
                    || msg.contains("unknown field"),
                "Expected error message to contain a suggestion for 'programme_title', got: {msg}"
            );
        }
    }

    #[test]
    fn test_epg_filter_engine_multi_program_sport_contains() {
        // Build filtering engine with a single EPG filter
        let mut engine = FilteringEngine::<EpgProgram>::new();
        let proc = EpgFilterProcessor::new(
            "sport_filter".into(),
            "Sport Filter".into(),
            false,
            r#"channel_id contains "sport""#,
            regex_eval(),
        )
        .unwrap();
        engine.add_filter_processor(Box::new(proc));

        // Fixture programs
        let p1 = sample_epg_program("Morning Show", "BeInSports1.fr", Some("Sports"));
        let p2 = sample_epg_program("Late Night", "NewsWorld.fr", Some("News"));
        let p3 = sample_epg_program("Highlights", "eurosport2", Some("Sports"));
        let p4 = sample_epg_program("Documentary", "HistoryPlus", Some("Documentary"));

        let programs = vec![p1, p2, p3, p4];

        let mut included = 0;
        let mut excluded = 0;
        for prog in &programs {
            match engine.should_include(prog) {
                Ok(true) => included += 1,
                Ok(false) => excluded += 1,
                Err(e) => {
                    panic!("Unexpected filter error for program {:?}: {}", prog.id, e);
                }
            }
        }

        // Expect BeInSports1.fr and eurosport2 to match (case-insensitive contains)
        assert_eq!(included, 2, "Expected exactly two programs to be included");
        assert_eq!(excluded, programs.len() - included);
    }
}
