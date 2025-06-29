use crate::filter_parser::FilterParser;
use crate::models::{
    data_mapping::{
        DataMappingRule, DataMappingRuleScope, DataMappingSourceType, EpgDataMappingResult,
        MappedChannel, MappedEpgChannel, MappedEpgProgram,
    },
    logo_asset::LogoAsset,
    Channel, EpgChannel, EpgProgram, ExtendedExpression, FilterOperator, LogicalOperator,
};

use chrono::Utc;
use regex::{Regex, RegexBuilder};
use std::collections::HashMap;
use std::time::Instant;
use tracing::{debug, info};
use uuid::Uuid;

/// Default special characters used for regex precheck filtering
/// These characters are considered significant enough to use as first-pass filters
const REGEX_SPECIAL_CHARS: &[char] = &[
    '.', '*', '+', '?', '^', '$', '|', '(', ')', '[', ']', '{', '}', '\\',
];

/// Configuration for the Data Mapping Engine
#[derive(Debug, Clone)]
pub struct DataMappingEngineConfig {
    pub enable_first_pass_filtering: bool,
    pub enable_regex_caching: bool,
    pub enable_performance_logging: bool,
    pub max_regex_cache_size: usize,
}

impl Default for DataMappingEngineConfig {
    fn default() -> Self {
        Self {
            enable_first_pass_filtering: true,
            enable_regex_caching: true,
            enable_performance_logging: false,
            max_regex_cache_size: 1000,
        }
    }
}

/// Holds captured regex groups during rule evaluation
#[derive(Debug, Clone)]
pub struct RegexCaptures {
    pub captures: HashMap<String, String>,
}

impl RegexCaptures {
    pub fn new() -> Self {
        Self {
            captures: HashMap::new(),
        }
    }

    pub fn add_capture(&mut self, key: String, value: String) {
        self.captures.insert(key, value);
    }

    pub fn get_capture(&self, key: &str) -> Option<&String> {
        self.captures.get(key)
    }
}

/// Main data mapping engine responsible for applying transformation rules
/// to stream and EPG data
pub struct DataMappingEngine {
    config: DataMappingEngineConfig,
    regex_cache: HashMap<String, Regex>,
    parser: FilterParser,
}

impl DataMappingEngine {
    pub fn new() -> Self {
        Self {
            config: DataMappingEngineConfig::default(),
            regex_cache: HashMap::new(),
            parser: FilterParser::new(),
        }
    }

    pub fn with_config(config: DataMappingEngineConfig) -> Self {
        Self {
            config,
            regex_cache: HashMap::new(),
            parser: FilterParser::new(),
        }
    }

    /// Apply data mapping rules to a list of channels
    pub fn apply_mapping_rules(
        &mut self,
        channels: Vec<Channel>,
        rules: Vec<DataMappingRule>,
        logo_assets: HashMap<Uuid, LogoAsset>,
        _source_id: Uuid,
        base_url: &str,
    ) -> Result<(Vec<MappedChannel>, HashMap<Uuid, (u128, u128, usize)>), Box<dyn std::error::Error>>
    {
        let start_time = Instant::now();
        let performance_stats = HashMap::new();
        let mut mapped_channels = Vec::with_capacity(channels.len());

        if self.config.enable_performance_logging {
            info!(
                "Starting data mapping for {} channels with {} rules",
                channels.len(),
                rules.len()
            );
        }

        // Process each channel
        for channel in channels {
            let channel_result =
                self.apply_rules_to_channel(channel, &rules, &logo_assets, base_url)?;
            mapped_channels.push(channel_result);
        }

        let total_duration = start_time.elapsed();

        if self.config.enable_performance_logging {
            info!(
                "Data mapping completed in {:?} for {} channels",
                total_duration,
                mapped_channels.len()
            );
        }

        Ok((mapped_channels, performance_stats))
    }

    /// Apply rules to a single channel
    fn apply_rules_to_channel(
        &mut self,
        channel: Channel,
        rules: &[DataMappingRule],
        _logo_assets: &HashMap<Uuid, LogoAsset>,
        _base_url: &str,
    ) -> Result<MappedChannel, Box<dyn std::error::Error>> {
        let mut mapped = MappedChannel {
            original: channel.clone(),
            mapped_tvg_id: channel.tvg_id.clone(),
            mapped_tvg_name: channel.tvg_name.clone(),
            mapped_tvg_logo: channel.tvg_logo.clone(),
            mapped_tvg_shift: channel.tvg_shift.clone(),
            mapped_group_title: channel.group_title.clone(),
            mapped_channel_name: channel.channel_name.clone(),
            applied_rules: Vec::new(),
            is_removed: false,
        };

        // Apply each rule
        for rule in rules.iter() {
            if !rule.is_active {
                continue;
            }

            // Only process rules for the correct source type
            if rule.source_type == DataMappingSourceType::Stream {
                if let Some(expression) = &rule.expression {
                    let conditions_match = self.evaluate_expression_for_channel(
                        &mapped.original,
                        expression,
                        &rule.source_type,
                    )?;

                    if conditions_match {
                        // Rule matched - mark as applied
                        mapped.applied_rules.push(rule.name.clone());

                        debug!(
                            "Rule '{}' matched for channel '{}'",
                            rule.name, mapped.original.channel_name
                        );
                    }
                }
            }
        }

        Ok(mapped)
    }

    /// Evaluate an expression for a channel
    fn evaluate_expression_for_channel(
        &mut self,
        channel: &Channel,
        expression: &str,
        source_type: &DataMappingSourceType,
    ) -> Result<bool, Box<dyn std::error::Error>> {
        // Get available fields for this source type
        let available_fields = match source_type {
            DataMappingSourceType::Stream => vec![
                "tvg_id".to_string(),
                "tvg_name".to_string(),
                "tvg_logo".to_string(),
                "tvg_shift".to_string(),
                "group_title".to_string(),
                "channel_name".to_string(),
            ],
            DataMappingSourceType::Epg => vec![
                "channel_id".to_string(),
                "channel_name".to_string(),
                "channel_logo".to_string(),
                "channel_group".to_string(),
                "language".to_string(),
            ],
        };

        let parser = FilterParser::new().with_fields(available_fields);
        let parsed = parser.parse_extended(expression)?;

        // Evaluate the expression
        match parsed {
            ExtendedExpression::ConditionOnly(condition_tree) => {
                self.evaluate_condition_tree_for_channel(channel, &condition_tree)
            }
            ExtendedExpression::ConditionWithActions { condition, .. } => {
                self.evaluate_condition_tree_for_channel(channel, &condition)
            }
            ExtendedExpression::ConditionalActionGroups(groups) => {
                // For multiple groups, evaluate the first one's conditions
                if let Some(first_group) = groups.first() {
                    self.evaluate_condition_tree_for_channel(channel, &first_group.conditions)
                } else {
                    Ok(false)
                }
            }
        }
    }

    /// Evaluate a condition tree for a channel
    fn evaluate_condition_tree_for_channel(
        &mut self,
        channel: &Channel,
        condition_tree: &crate::models::ConditionTree,
    ) -> Result<bool, Box<dyn std::error::Error>> {
        self.evaluate_condition_node_for_channel(channel, &condition_tree.root)
    }

    /// Evaluate a condition node for a channel
    fn evaluate_condition_node_for_channel(
        &mut self,
        channel: &Channel,
        node: &crate::models::ConditionNode,
    ) -> Result<bool, Box<dyn std::error::Error>> {
        match node {
            crate::models::ConditionNode::Condition {
                field,
                operator,
                value,
                case_sensitive: _,
                negate,
            } => {
                let field_value = self.get_field_value(channel, field).unwrap_or_default();

                let result = match operator {
                    FilterOperator::Equals => field_value.to_lowercase() == value.to_lowercase(),
                    FilterOperator::NotEquals => field_value.to_lowercase() != value.to_lowercase(),
                    FilterOperator::Contains => {
                        field_value.to_lowercase().contains(&value.to_lowercase())
                    }
                    FilterOperator::NotContains => {
                        !field_value.to_lowercase().contains(&value.to_lowercase())
                    }
                    FilterOperator::StartsWith => field_value
                        .to_lowercase()
                        .starts_with(&value.to_lowercase()),
                    FilterOperator::EndsWith => {
                        field_value.to_lowercase().ends_with(&value.to_lowercase())
                    }
                    FilterOperator::Matches => {
                        let regex = self.get_or_create_regex(value, false)?;
                        regex.is_match(&field_value)
                    }
                    FilterOperator::NotMatches => {
                        let regex = self.get_or_create_regex(value, false)?;
                        !regex.is_match(&field_value)
                    }
                };

                Ok(if *negate { !result } else { result })
            }
            crate::models::ConditionNode::Group { operator, children } => {
                if children.is_empty() {
                    return Ok(true);
                }

                let first_result =
                    self.evaluate_condition_node_for_channel(channel, &children[0])?;

                let mut combined_result = first_result;
                for child in children.iter().skip(1) {
                    let child_result = self.evaluate_condition_node_for_channel(channel, child)?;
                    match operator {
                        LogicalOperator::And | LogicalOperator::All => {
                            combined_result = combined_result && child_result;
                        }
                        LogicalOperator::Or | LogicalOperator::Any => {
                            combined_result = combined_result || child_result;
                        }
                    }
                }

                Ok(combined_result)
            }
        }
    }

    /// Get field value from channel
    fn get_field_value(&self, channel: &Channel, field: &str) -> Option<String> {
        match field {
            "tvg_id" => channel.tvg_id.clone(),
            "tvg_name" => channel.tvg_name.clone(),
            "tvg_logo" => channel.tvg_logo.clone(),
            "tvg_shift" => channel.tvg_shift.clone(),
            "group_title" => channel.group_title.clone(),
            "channel_name" => Some(channel.channel_name.clone()),
            _ => None,
        }
    }

    /// Get or create a regex, with caching if enabled
    fn get_or_create_regex(
        &mut self,
        pattern: &str,
        case_insensitive: bool,
    ) -> Result<&Regex, Box<dyn std::error::Error>> {
        let cache_key = format!("{}:{}", pattern, case_insensitive);

        if self.config.enable_regex_caching && self.regex_cache.contains_key(&cache_key) {
            return Ok(self.regex_cache.get(&cache_key).unwrap());
        }

        let mut builder = RegexBuilder::new(pattern);
        builder.case_insensitive(case_insensitive);
        let regex = builder.build()?;

        if self.config.enable_regex_caching {
            // Limit cache size to prevent memory bloat
            if self.regex_cache.len() >= self.config.max_regex_cache_size {
                self.regex_cache.clear();
            }
            self.regex_cache.insert(cache_key.clone(), regex);
            Ok(self.regex_cache.get(&cache_key).unwrap())
        } else {
            // Store temporarily for this evaluation
            self.regex_cache.insert(cache_key.clone(), regex);
            Ok(self.regex_cache.get(&cache_key).unwrap())
        }
    }

    /// Apply EPG mapping rules (simplified version)
    pub fn apply_epg_mapping_rules(
        &mut self,
        channels: Vec<EpgChannel>,
        programs: Vec<EpgProgram>,
        _rules: Vec<DataMappingRule>,
        _logo_assets: HashMap<Uuid, LogoAsset>,
        _source_id: Uuid,
        _base_url: &str,
    ) -> Result<EpgDataMappingResult, Box<dyn std::error::Error>> {
        let start_time = Instant::now();

        // For now, return a simple result with no transformations applied
        // This would need to be implemented for full EPG support
        let mapped_channels: Vec<MappedEpgChannel> = channels
            .into_iter()
            .map(|channel| MappedEpgChannel {
                original: channel.clone(),
                mapped_channel_id: channel.channel_id.clone(),
                mapped_channel_name: channel.channel_name.clone(),
                mapped_channel_logo: channel.channel_logo.clone(),
                mapped_channel_group: channel.channel_group.clone(),
                mapped_language: channel.language.clone(),
                applied_rules: Vec::new(),
                clone_group_id: None,
                is_primary_clone: true,
                timeshift_offset: None,
            })
            .collect();

        let mapped_programs: Vec<MappedEpgProgram> = programs
            .into_iter()
            .map(|program| MappedEpgProgram {
                original: program.clone(),
                mapped_channel_id: program.channel_id.clone(),
                mapped_channel_name: program.channel_name.clone(),
                mapped_program_title: program.program_title.clone(),
                mapped_program_description: program.program_description.clone(),
                mapped_program_category: program.program_category.clone(),
                mapped_start_time: program.start_time,
                mapped_end_time: program.end_time,
                applied_rules: Vec::new(),
            })
            .collect();

        let result = EpgDataMappingResult {
            mapped_channels,
            mapped_programs,
            clone_groups: HashMap::new(),
            total_mutations: 0,
            channels_affected: 0,
            programs_affected: 0,
        };

        let duration = start_time.elapsed();
        if self.config.enable_performance_logging {
            info!("EPG mapping completed in {:?}", duration);
        }

        Ok(result)
    }

    /// Test a mapping rule with given channels (simplified version)
    pub fn test_mapping_rule(
        &mut self,
        channels: Vec<Channel>,
        logo_assets: HashMap<Uuid, LogoAsset>,
        base_url: &str,
        expression: &str,
    ) -> Result<Vec<MappedChannel>, Box<dyn std::error::Error>> {
        let test_rule = DataMappingRule {
            id: Uuid::new_v4(),
            name: "Test Rule".to_string(),
            description: None,
            source_type: DataMappingSourceType::Stream,
            scope: DataMappingRuleScope::Individual,
            sort_order: 0,
            is_active: true,
            expression: Some(expression.to_string()),
            created_at: Utc::now(),
            updated_at: Utc::now(),
        };

        let mut mapped_channels = Vec::new();
        for channel in channels {
            let mapped =
                self.apply_rules_to_channel(channel, &[test_rule.clone()], &logo_assets, base_url)?;
            mapped_channels.push(mapped);
        }

        Ok(mapped_channels)
    }

    /// Get EPG channel field value
    fn get_epg_channel_field_value(&self, channel: &EpgChannel, field: &str) -> Option<String> {
        match field {
            "channel_id" => Some(channel.channel_id.clone()),
            "channel_name" => Some(channel.channel_name.clone()),
            "channel_logo" => channel.channel_logo.clone(),
            "channel_group" => channel.channel_group.clone(),
            "language" => channel.language.clone(),
            _ => None,
        }
    }
}

impl Default for DataMappingEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl From<crate::config::DataMappingEngineConfig> for DataMappingEngineConfig {
    fn from(config: crate::config::DataMappingEngineConfig) -> Self {
        Self {
            enable_first_pass_filtering: true,
            enable_regex_caching: true,
            enable_performance_logging: false,
            max_regex_cache_size: 1000,
        }
    }
}
