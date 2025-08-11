//! Filter processor engines for extensible filtering
//!
//! This module provides filter processing capabilities for both channels and EPG programs,
//! with expression parsing, time function support and regex preprocessing optimization.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::Duration;
use crate::models::ConditionTree;
use crate::utils::regex_preprocessor::RegexPreprocessor;
use tracing::{trace, warn};
use regex::Regex;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FilterResult {
    pub include_match: bool,    // Does this record match include criteria?
    pub exclude_match: bool,    // Does this record match exclude criteria? 
    pub execution_time: Duration,
    pub error: Option<String>,
}

/// Generic trait for filter processors
pub trait FilterProcessor<T>: Send + Sync {
    fn process_record(&mut self, record: &T) -> Result<FilterResult, Box<dyn std::error::Error>>;
    fn get_filter_name(&self) -> &str;
    fn get_filter_id(&self) -> &str;
    fn is_inverse(&self) -> bool;
}

/// Shared regex evaluator with preprocessing optimization
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
}

/// Stream/Channel filter processor
pub struct StreamFilterProcessor {
    pub filter_id: String,
    pub filter_name: String,
    pub is_inverse: bool,
    pub condition_tree: Option<ConditionTree>,
    pub regex_evaluator: RegexEvaluator,
    pub time_snapshot: DateTime<Utc>, // Cached time for @time:now() functions
}

impl StreamFilterProcessor {
    pub fn new(
        filter_id: String,
        filter_name: String,
        is_inverse: bool,
        condition_expression: &str,
        regex_evaluator: RegexEvaluator,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        // Parse the condition expression text into a ConditionTree
        let parsed_condition = if condition_expression.trim().is_empty() {
            None
        } else {
            // First resolve time functions if any
            let resolved_expression = Self::resolve_time_functions(condition_expression)?;
            
            // Parse the human-readable expression into a ConditionTree using ExpressionParser
            let parser = crate::expression_parser::ExpressionParser::new()
                .with_fields(vec![
                    "tvg_id".to_string(),
                    "tvg_name".to_string(),
                    "tvg_logo".to_string(),
                    "tvg_shift".to_string(),
                    "group_title".to_string(),
                    "channel_name".to_string(),
                    "stream_url".to_string(),
                ]);
            
            match parser.parse(&resolved_expression) {
                Ok(condition_tree) => {
                    trace!("Successfully parsed filter expression for filter_id={} filter_name={}", filter_id, filter_name);
                    Some(condition_tree)
                },
                Err(e) => {
                    warn!("Failed to parse filter expression filter_id={} filter_name={} error={} expression={}", 
                          filter_id, filter_name, e, resolved_expression);
                    None
                }
            }
        };
        
        Ok(Self {
            filter_id,
            filter_name,
            is_inverse,
            condition_tree: parsed_condition,
            regex_evaluator,
            time_snapshot: Utc::now(), // Snapshot time for this execution
        })
    }
    
    /// Resolve @time: functions in the condition expression
    fn resolve_time_functions(condition_expression: &str) -> Result<String, Box<dyn std::error::Error>> {
        crate::utils::time::resolve_time_functions(condition_expression)
            .map_err(|e| e.into())
    }
    
    /// Evaluate the filter condition against a channel record
    fn evaluate_condition(&self, record: &crate::models::Channel) -> Result<bool, Box<dyn std::error::Error>> {
        let Some(condition_tree) = &self.condition_tree else {
            // No condition means match all
            return Ok(true);
        };
        
        self.evaluate_condition_node(&condition_tree.root, record)
    }
    
    /// Evaluate a condition node recursively
    fn evaluate_condition_node(&self, node: &crate::models::ConditionNode, record: &crate::models::Channel) -> Result<bool, Box<dyn std::error::Error>> {
        use crate::models::{LogicalOperator, FilterOperator};
        
        match node {
            crate::models::ConditionNode::Condition { field, operator, value, .. } => {
                let field_value = self.get_field_value(field, record)?;
                let field_value_str = field_value.unwrap_or_default();
                
                let matches = match operator {
                    FilterOperator::Equals => field_value_str.eq_ignore_ascii_case(value),
                    FilterOperator::NotEquals => !field_value_str.eq_ignore_ascii_case(value),
                    FilterOperator::Contains => field_value_str.to_lowercase().contains(&value.to_lowercase()),
                    FilterOperator::NotContains => !field_value_str.to_lowercase().contains(&value.to_lowercase()),
                    FilterOperator::StartsWith => field_value_str.to_lowercase().starts_with(&value.to_lowercase()),
                    FilterOperator::NotStartsWith => !field_value_str.to_lowercase().starts_with(&value.to_lowercase()),
                    FilterOperator::EndsWith => field_value_str.to_lowercase().ends_with(&value.to_lowercase()),
                    FilterOperator::NotEndsWith => !field_value_str.to_lowercase().ends_with(&value.to_lowercase()),
                    FilterOperator::Matches => {
                        self.regex_evaluator.evaluate_with_preprocessing(value, &field_value_str, &format!("filter_{}", self.filter_name))?
                    },
                    FilterOperator::NotMatches => {
                        !self.regex_evaluator.evaluate_with_preprocessing(value, &field_value_str, &format!("filter_{}", self.filter_name))?
                    },
                    FilterOperator::GreaterThan => {
                        self.compare_values(&field_value_str, value, std::cmp::Ordering::Greater)?
                    },
                    FilterOperator::LessThan => {
                        self.compare_values(&field_value_str, value, std::cmp::Ordering::Less)?
                    },
                    FilterOperator::GreaterThanOrEqual => {
                        let result = self.compare_values(&field_value_str, value, std::cmp::Ordering::Greater)?;
                        let equal = field_value_str.eq_ignore_ascii_case(value);
                        result || equal
                    },
                    FilterOperator::LessThanOrEqual => {
                        let result = self.compare_values(&field_value_str, value, std::cmp::Ordering::Less)?;
                        let equal = field_value_str.eq_ignore_ascii_case(value);
                        result || equal
                    },
                };
                
                Ok(matches)
            }
            crate::models::ConditionNode::Group { operator, children } => {
                if children.is_empty() {
                    return Ok(true); // Empty group defaults to true
                }
                
                let mut results = Vec::new();
                for child in children {
                    results.push(self.evaluate_condition_node(child, record)?);
                }
                
                let group_result = match operator {
                    LogicalOperator::And => results.iter().all(|&r| r),
                    LogicalOperator::Or => results.iter().any(|&r| r),
                };
                
                Ok(group_result)
            }
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

impl FilterProcessor<crate::models::Channel> for StreamFilterProcessor {
    fn process_record(&mut self, record: &crate::models::Channel) -> Result<FilterResult, Box<dyn std::error::Error>> {
        let start = std::time::Instant::now();
        
        let condition_result = match self.evaluate_condition(record) {
            Ok(result) => result,
            Err(e) => {
                warn!("Filter evaluation failed: filter_id={} error={}", self.filter_id, e);
                return Ok(FilterResult {
                    include_match: false,
                    exclude_match: false,
                    execution_time: start.elapsed(),
                    error: Some(e.to_string()),
                });
            }
        };
        
        let execution_time = start.elapsed();
        
        // Determine include/exclude based on filter type and condition result
        let (include_match, exclude_match) = if self.is_inverse {
            // Inverse/exclude filter: if condition matches, this should be excluded
            (false, condition_result)
        } else {
            // Include filter: if condition matches, this should be included
            (condition_result, false)
        };
        
        Ok(FilterResult {
            include_match,
            exclude_match,
            execution_time,
            error: None,
        })
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

/// EPG/Program filter processor (placeholder for now)
pub struct EpgFilterProcessor {
    pub filter_id: String,
    pub filter_name: String,
    pub is_inverse: bool,
    pub regex_evaluator: RegexEvaluator,
}

impl EpgFilterProcessor {
    pub fn new(
        filter_id: String,
        filter_name: String,
        is_inverse: bool,
        _condition_expression: &str,
        regex_evaluator: RegexEvaluator,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        // TODO: Implement EPG filter parsing similar to StreamFilterProcessor
        Ok(Self {
            filter_id,
            filter_name,
            is_inverse,
            regex_evaluator,
        })
    }
}

impl FilterProcessor<crate::pipeline::engines::rule_processor::EpgProgram> for EpgFilterProcessor {
    fn process_record(&mut self, _record: &crate::pipeline::engines::rule_processor::EpgProgram) -> Result<FilterResult, Box<dyn std::error::Error>> {
        let start = std::time::Instant::now();
        
        // TODO: Implement actual EPG filtering logic
        Ok(FilterResult {
            include_match: true, // For now, include all EPG programs
            exclude_match: false,
            execution_time: start.elapsed(),
            error: None,
        })
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

/// Generic filtering engine
pub struct FilteringEngine<T> {
    filter_processors: Vec<Box<dyn FilterProcessor<T>>>,
    performance_stats: HashMap<String, (usize, usize, Duration)>, // (included_count, excluded_count, total_time)
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
    
    /// Process records with sequential filter logic using indices and deduplication
    pub fn process_records(&mut self, input_records: &[T]) -> Result<FilterEngineResult<T>, Box<dyn std::error::Error>> 
    where T: Clone {
        let start_time = std::time::Instant::now();
        let mut filtered_indices = Vec::new();
        
        // Apply filters sequentially in order
        for processor in &mut self.filter_processors {
            let filter_start = std::time::Instant::now();
            let before_count = filtered_indices.len();
            
            if processor.is_inverse() {
                // EXCLUDE filter: scan current filtered indices and remove matches
                let mut remaining_indices = Vec::new();
                for &index in &filtered_indices {
                    let record = &input_records[index];
                    let result = processor.process_record(record)?;
                    if result.exclude_match {
                        // Record is excluded, don't add to remaining
                    } else {
                        remaining_indices.push(index);
                    }
                }
                filtered_indices = remaining_indices;
            } else {
                // INCLUDE filter: if this is the first filter, scan all input records
                // If we already have filtered results, scan only those records
                if filtered_indices.is_empty() {
                    // First INCLUDE filter: scan all input records
                    for (index, record) in input_records.iter().enumerate() {
                        let result = processor.process_record(record)?;
                        if result.include_match {
                            filtered_indices.push(index);
                        }
                    }
                } else {
                    // Subsequent INCLUDE filter: only keep records that also match this filter
                    let mut remaining_indices = Vec::new();
                    for &index in &filtered_indices {
                        let record = &input_records[index];
                        let result = processor.process_record(record)?;
                        if result.include_match {
                            remaining_indices.push(index);
                        }
                    }
                    filtered_indices = remaining_indices;
                }
            }
            
            let after_count = filtered_indices.len();
            let filter_time = filter_start.elapsed();
            
            // Track the actual effect: how many channels were added or removed by this filter
            let (filter_included, filter_excluded) = if processor.is_inverse() {
                // EXCLUDE filter removes channels
                (0, before_count - after_count)
            } else {
                // INCLUDE filter adds channels (for first filter) or keeps channels (for subsequent)
                if before_count == 0 {
                    // First filter: added channels
                    (after_count, 0)
                } else {
                    // Subsequent filter: may reduce the set
                    (after_count, before_count - after_count)
                }
            };
            
            self.performance_stats.insert(
                processor.get_filter_id().to_string(),
                (filter_included, filter_excluded, filter_time)
            );
        }
        
        // Convert indices back to actual records
        let filtered_records: Vec<T> = filtered_indices
            .into_iter()
            .map(|index| input_records[index].clone())
            .collect();
        
        let filtered_count = filtered_records.len();
        
        Ok(FilterEngineResult {
            filtered_records,
            total_input: input_records.len(),
            total_filtered: filtered_count,
            execution_time: start_time.elapsed(),
            filter_stats: self.performance_stats.clone(),
        })
    }
    
    
    pub fn clear_cache(&mut self) {
        // Clear any cached state for next pipeline run
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


/// Type aliases for convenience
pub type ChannelFilteringEngine = FilteringEngine<crate::models::Channel>;
pub type EpgFilteringEngine = FilteringEngine<crate::pipeline::engines::rule_processor::EpgProgram>;