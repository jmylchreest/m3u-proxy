use crate::filter_parser::FilterParser;
use crate::models::{
    data_mapping::{
        DataMappingRule, DataMappingRuleScope, DataMappingSourceType,
        MappedChannel,
    },
    logo_asset::LogoAsset,
    Channel, ExtendedExpression, FilterOperator, LogicalOperator,
};
use crate::utils::{RegexPreprocessor, RegexPreprocessorConfig};

use chrono::Utc;
use regex::{Regex, RegexBuilder};
use std::collections::HashMap;
use std::time::Instant;
use tracing::{debug, info};
use uuid::Uuid;

/// Default special characters used for regex precheck filtering
/// These characters are considered significant enough to use as first-pass filters
#[allow(dead_code)]
const REGEX_SPECIAL_CHARS: &[char] = &[
    '.', '*', '+', '?', '^', '$', '|', '(', ')', '[', ']', '{', '}', '\\',
];

/// Configuration for the Data Mapping Engine
#[derive(Debug, Clone)]
pub struct DataMappingEngineConfig {
    pub enable_regex_caching: bool,
    pub enable_performance_logging: bool,
    pub max_regex_cache_size: usize,
    pub preprocessor: RegexPreprocessorConfig,
}

impl Default for DataMappingEngineConfig {
    fn default() -> Self {
        Self {
            enable_regex_caching: true,
            enable_performance_logging: false,
            max_regex_cache_size: 1000,
            preprocessor: RegexPreprocessorConfig::default(),
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
    #[allow(dead_code)]
    parser: FilterParser,
    preprocessor: RegexPreprocessor,
}

impl DataMappingEngine {
    pub fn new() -> Self {
        let config = DataMappingEngineConfig::default();
        Self {
            preprocessor: RegexPreprocessor::new(config.preprocessor.clone()),
            config,
            regex_cache: HashMap::new(),
            parser: FilterParser::new(),
        }
    }

    pub fn with_config(config: DataMappingEngineConfig) -> Self {
        Self {
            preprocessor: RegexPreprocessor::new(config.preprocessor.clone()),
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
        let mut performance_stats: HashMap<Uuid, (u128, u128, usize)> = HashMap::new();
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
            let (channel_result, rule_timings) =
                self.apply_rules_to_channel_with_timing(channel, &rules, &logo_assets, base_url)?;
            mapped_channels.push(channel_result);

            // Aggregate timing statistics per rule
            for (rule_id, execution_time_micros) in rule_timings {
                let entry = performance_stats.entry(rule_id).or_insert((0, 0, 0));
                entry.0 += execution_time_micros; // total_execution_time
                entry.2 += 1; // processed_count (number of channels this rule was applied to)
            }
        }

        // Calculate average execution times
        for (_, stats) in performance_stats.iter_mut() {
            if stats.2 > 0 {
                stats.1 = stats.0 / stats.2 as u128; // avg_execution_time = total / count
            }
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

    /// Apply rules to a single channel with timing measurements
    fn apply_rules_to_channel_with_timing(
        &mut self,
        channel: Channel,
        rules: &[DataMappingRule],
        logo_assets: &HashMap<Uuid, LogoAsset>,
        base_url: &str,
    ) -> Result<(MappedChannel, Vec<(Uuid, u128)>), Box<dyn std::error::Error>> {
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
            capture_group_values: HashMap::new(),
        };

        let mut rule_timings = Vec::new();

        // Apply each rule
        for rule in rules.iter() {
            if !rule.is_active {
                continue;
            }

            // Only process rules for the correct source type
            if rule.source_type == DataMappingSourceType::Stream {
                if let Some(expression) = &rule.expression {
                    let rule_start = Instant::now();

                    let applied = self.apply_rule_expression_to_channel(
                        &mut mapped,
                        expression,
                        &rule.source_type,
                        rule,
                        logo_assets,
                        base_url,
                    )?;

                    let rule_duration = rule_start.elapsed();
                    let rule_micros = rule_duration.as_micros();

                    // Only record timing if the rule was actually evaluated (regardless of match)
                    rule_timings.push((rule.id, rule_micros));

                    if applied {
                        debug!(
                            "Rule '{}' applied to channel '{}' in {}μs",
                            rule.name, mapped.original.channel_name, rule_micros
                        );
                    }
                }
            }
        }

        Ok((mapped, rule_timings))
    }

    /// Apply rules to a single channel (legacy method without timing)
    fn apply_rules_to_channel(
        &mut self,
        channel: Channel,
        rules: &[DataMappingRule],
        logo_assets: &HashMap<Uuid, LogoAsset>,
        base_url: &str,
    ) -> Result<MappedChannel, Box<dyn std::error::Error>> {
        let (mapped, _) =
            self.apply_rules_to_channel_with_timing(channel, rules, logo_assets, base_url)?;
        Ok(mapped)
    }

    /// Apply rule expression to channel and return whether any changes were made
    fn apply_rule_expression_to_channel(
        &mut self,
        mapped_channel: &mut MappedChannel,
        expression: &str,
        source_type: &DataMappingSourceType,
        rule: &DataMappingRule,
        logo_assets: &HashMap<Uuid, LogoAsset>,
        base_url: &str,
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

        // Parse the expression using the filter parser
        let parser = FilterParser::new().with_fields(available_fields);
        let parsed = parser.parse_extended(expression)?;

        // Apply the expression based on its type
        match parsed {
            ExtendedExpression::ConditionOnly(condition_tree) => {
                // Just evaluate conditions, no actions to apply
                let mut captures = RegexCaptures::new();
                let matches = self.evaluate_condition_tree_for_channel(
                    &mapped_channel.original,
                    &condition_tree,
                    &mut captures,
                )?;
                if matches {
                    mapped_channel.applied_rules.push(rule.name.clone());
                }
                Ok(matches)
            }
            ExtendedExpression::ConditionWithActions { condition, actions } => {
                // Evaluate condition and apply actions if it matches
                let mut captures = RegexCaptures::new();
                let matches = self.evaluate_condition_tree_for_channel(
                    &mapped_channel.original,
                    &condition,
                    &mut captures,
                )?;
                if matches {
                    mapped_channel.applied_rules.push(rule.name.clone());
                    self.apply_actions_to_channel_with_captures(
                        mapped_channel,
                        &actions,
                        &captures,
                        logo_assets,
                        base_url,
                        &rule.name,
                    )?;
                }
                Ok(matches)
            }
            ExtendedExpression::ConditionalActionGroups(groups) => {
                let mut any_applied = false;

                // Apply each conditional action group
                for group in groups {
                    let mut captures = RegexCaptures::new();
                    let matches = self.evaluate_condition_tree_for_channel(
                        &mapped_channel.original,
                        &group.conditions,
                        &mut captures,
                    )?;
                    if matches {
                        if !any_applied {
                            mapped_channel.applied_rules.push(rule.name.clone());
                        }
                        self.apply_actions_to_channel_with_captures(
                            mapped_channel,
                            &group.actions,
                            &captures,
                            logo_assets,
                            base_url,
                            &rule.name,
                        )?;
                        any_applied = true;
                    }
                }

                Ok(any_applied)
            }
        }
    }

    /// Apply a list of actions to a mapped channel with capture group substitution
    fn apply_actions_to_channel_with_captures(
        &mut self,
        mapped_channel: &mut MappedChannel,
        actions: &[crate::models::Action],
        captures: &RegexCaptures,
        logo_assets: &HashMap<Uuid, LogoAsset>,
        base_url: &str,
        rule_name: &str,
    ) -> Result<(), Box<dyn std::error::Error>> {
        use crate::models::{ActionOperator, ActionValue};

        for action in actions {
            let field_name = &action.field;
            let (value, capture_used) = match &action.value {
                ActionValue::Literal(val) => {
                    // Handle @logo: references
                    if val.starts_with("@logo:") {
                        if let Ok(logo_uuid) = uuid::Uuid::parse_str(&val[6..]) {
                            if let Some(_logo_asset) = logo_assets.get(&logo_uuid) {
                                (crate::utils::logo::LogoUrlGenerator::full(logo_uuid, base_url), None)
                            } else {
                                (val.clone(), None)
                            }
                        } else {
                            (val.clone(), None)
                        }
                    } else {
                        // Handle capture group substitution ($1, $2, etc.)
                        // Check if this contains capture group references
                        if val.contains('$') && Regex::new(r"\$\d+").unwrap().is_match(val) {
                            let (resolved_value, capture_info) =
                                self.substitute_capture_groups(val, captures);
                            // Store resolved value and capture info
                            (resolved_value, capture_info)
                        } else {
                            (val.clone(), None)
                        }
                    }
                }
                ActionValue::Null => {
                    // Explicit null value - clear the field
                    (String::new(), None)
                }
                ActionValue::Function(_) => {
                    // Functions not implemented yet
                    continue;
                }
                ActionValue::Variable(var_ref) => {
                    // Get value from another field
                    let value = self
                        .get_field_value(&mapped_channel.original, &var_ref.field_name)
                        .unwrap_or_default();
                    (value, None)
                }
            };

            // Store capture group information if used
            if let Some(capture_info) = capture_used {
                mapped_channel
                    .capture_group_values
                    .entry(rule_name.to_string())
                    .or_insert_with(HashMap::new)
                    .insert(field_name.clone(), capture_info);
            }

            // Apply the action based on operator and field
            match action.operator {
                ActionOperator::Set => {
                    match field_name.as_str() {
                        "tvg_id" => mapped_channel.mapped_tvg_id = Some(value),
                        "tvg_name" => mapped_channel.mapped_tvg_name = Some(value),
                        "tvg_logo" => mapped_channel.mapped_tvg_logo = Some(value),
                        "tvg_shift" => mapped_channel.mapped_tvg_shift = Some(value),
                        "group_title" => mapped_channel.mapped_group_title = Some(value),
                        "channel_name" => mapped_channel.mapped_channel_name = value,
                        _ => {} // Unknown field
                    }
                }
                ActionOperator::SetIfEmpty => {
                    // Only set the value if the current field is empty/null
                    match field_name.as_str() {
                        "tvg_id" => {
                            let current = mapped_channel
                                .mapped_tvg_id
                                .as_ref()
                                .or(mapped_channel.original.tvg_id.as_ref());
                            if current.is_none() || current.map_or(true, |s| s.trim().is_empty()) {
                                mapped_channel.mapped_tvg_id = Some(value);
                            }
                        }
                        "tvg_name" => {
                            let current = mapped_channel
                                .mapped_tvg_name
                                .as_ref()
                                .or(mapped_channel.original.tvg_name.as_ref());
                            if current.is_none() || current.map_or(true, |s| s.trim().is_empty()) {
                                mapped_channel.mapped_tvg_name = Some(value);
                            }
                        }
                        "tvg_logo" => {
                            let current = mapped_channel
                                .mapped_tvg_logo
                                .as_ref()
                                .or(mapped_channel.original.tvg_logo.as_ref());
                            if current.is_none() || current.map_or(true, |s| s.trim().is_empty()) {
                                mapped_channel.mapped_tvg_logo = Some(value);
                            }
                        }
                        "tvg_shift" => {
                            let current = mapped_channel
                                .mapped_tvg_shift
                                .as_ref()
                                .or(mapped_channel.original.tvg_shift.as_ref());
                            if current.is_none() || current.map_or(true, |s| s.trim().is_empty()) {
                                mapped_channel.mapped_tvg_shift = Some(value);
                            }
                        }
                        "group_title" => {
                            let current = mapped_channel
                                .mapped_group_title
                                .as_ref()
                                .or(mapped_channel.original.group_title.as_ref());
                            if current.is_none() || current.map_or(true, |s| s.trim().is_empty()) {
                                mapped_channel.mapped_group_title = Some(value);
                            }
                        }
                        "channel_name" => {
                            if mapped_channel.mapped_channel_name.trim().is_empty() {
                                mapped_channel.mapped_channel_name = value;
                            }
                        }
                        _ => {} // Unknown field
                    }
                }
                ActionOperator::Append => {
                    match field_name.as_str() {
                        "tvg_id" => {
                            let empty_string = String::new();
                            let current = mapped_channel
                                .mapped_tvg_id
                                .as_ref()
                                .or(mapped_channel.original.tvg_id.as_ref())
                                .unwrap_or(&empty_string);
                            mapped_channel.mapped_tvg_id = Some(format!("{} {}", current, value));
                        }
                        "tvg_name" => {
                            let empty_string = String::new();
                            let current = mapped_channel
                                .mapped_tvg_name
                                .as_ref()
                                .or(mapped_channel.original.tvg_name.as_ref())
                                .unwrap_or(&empty_string);
                            mapped_channel.mapped_tvg_name = Some(format!("{} {}", current, value));
                        }
                        "tvg_logo" => {
                            let empty_string = String::new();
                            let current = mapped_channel
                                .mapped_tvg_logo
                                .as_ref()
                                .or(mapped_channel.original.tvg_logo.as_ref())
                                .unwrap_or(&empty_string);
                            mapped_channel.mapped_tvg_logo = Some(format!("{} {}", current, value));
                        }
                        "tvg_shift" => {
                            let empty_string = String::new();
                            let current = mapped_channel
                                .mapped_tvg_shift
                                .as_ref()
                                .or(mapped_channel.original.tvg_shift.as_ref())
                                .unwrap_or(&empty_string);
                            mapped_channel.mapped_tvg_shift =
                                Some(format!("{} {}", current, value));
                        }
                        "group_title" => {
                            let empty_string = String::new();
                            let current = mapped_channel
                                .mapped_group_title
                                .as_ref()
                                .or(mapped_channel.original.group_title.as_ref())
                                .unwrap_or(&empty_string);
                            mapped_channel.mapped_group_title =
                                Some(format!("{} {}", current, value));
                        }
                        "channel_name" => {
                            mapped_channel.mapped_channel_name =
                                format!("{} {}", mapped_channel.mapped_channel_name, value);
                        }
                        _ => {} // Unknown field
                    }
                }
                ActionOperator::Remove => {
                    // Remove substring from field
                    match field_name.as_str() {
                        "tvg_id" => {
                            if let Some(current) = mapped_channel
                                .mapped_tvg_id
                                .as_ref()
                                .or(mapped_channel.original.tvg_id.as_ref())
                            {
                                mapped_channel.mapped_tvg_id = Some(current.replace(&value, ""));
                            }
                        }
                        "tvg_name" => {
                            if let Some(current) = mapped_channel
                                .mapped_tvg_name
                                .as_ref()
                                .or(mapped_channel.original.tvg_name.as_ref())
                            {
                                mapped_channel.mapped_tvg_name = Some(current.replace(&value, ""));
                            }
                        }
                        "tvg_logo" => {
                            if let Some(current) = mapped_channel
                                .mapped_tvg_logo
                                .as_ref()
                                .or(mapped_channel.original.tvg_logo.as_ref())
                            {
                                mapped_channel.mapped_tvg_logo = Some(current.replace(&value, ""));
                            }
                        }
                        "tvg_shift" => {
                            if let Some(current) = mapped_channel
                                .mapped_tvg_shift
                                .as_ref()
                                .or(mapped_channel.original.tvg_shift.as_ref())
                            {
                                mapped_channel.mapped_tvg_shift = Some(current.replace(&value, ""));
                            }
                        }
                        "group_title" => {
                            if let Some(current) = mapped_channel
                                .mapped_group_title
                                .as_ref()
                                .or(mapped_channel.original.group_title.as_ref())
                            {
                                mapped_channel.mapped_group_title = Some(current.replace(&value, ""));
                            }
                        }
                        "channel_name" => {
                            mapped_channel.mapped_channel_name = mapped_channel.mapped_channel_name.replace(&value, "");
                        }
                        _ => {} // Unknown field
                    }
                }
                ActionOperator::Delete => {
                    // Delete field entirely - set to None/empty
                    match field_name.as_str() {
                        "tvg_id" => mapped_channel.mapped_tvg_id = None,
                        "tvg_name" => mapped_channel.mapped_tvg_name = None,
                        "tvg_logo" => mapped_channel.mapped_tvg_logo = None,
                        "tvg_shift" => mapped_channel.mapped_tvg_shift = None,
                        "group_title" => mapped_channel.mapped_group_title = None,
                        "channel_name" => {
                            // Cannot delete required field channel_name - this should be caught by validation
                            // But if it gets here, we'll leave it unchanged
                        }
                        _ => {} // Unknown field
                    }
                }
            }
        }

        Ok(())
    }

    /// Apply a list of actions to a mapped channel (legacy method without captures)
    #[allow(dead_code)]
    fn apply_actions_to_channel(
        &mut self,
        mapped_channel: &mut MappedChannel,
        actions: &[crate::models::Action],
        logo_assets: &HashMap<Uuid, LogoAsset>,
        base_url: &str,
    ) -> Result<(), Box<dyn std::error::Error>> {
        use crate::models::{ActionOperator, ActionValue};

        for action in actions {
            let field_name = &action.field;
            let value = match &action.value {
                ActionValue::Literal(val) => {
                    // Handle @logo: references
                    if val.starts_with("@logo:") {
                        if let Ok(logo_uuid) = uuid::Uuid::parse_str(&val[6..]) {
                            if let Some(_logo_asset) = logo_assets.get(&logo_uuid) {
                                crate::utils::logo::LogoUrlGenerator::full(logo_uuid, base_url)
                            } else {
                                val.clone()
                            }
                        } else {
                            val.clone()
                        }
                    } else {
                        val.clone()
                    }
                }
                ActionValue::Null => {
                    // Explicit null value - clear the field
                    String::new()
                }
                ActionValue::Function(_) => {
                    // Functions not implemented yet
                    continue;
                }
                ActionValue::Variable(var_ref) => {
                    // Get value from another field
                    self.get_field_value(&mapped_channel.original, &var_ref.field_name)
                        .unwrap_or_default()
                }
            };

            // Apply the action based on operator and field
            match action.operator {
                ActionOperator::Set => {
                    match field_name.as_str() {
                        "tvg_id" => mapped_channel.mapped_tvg_id = Some(value),
                        "tvg_name" => mapped_channel.mapped_tvg_name = Some(value),
                        "tvg_logo" => mapped_channel.mapped_tvg_logo = Some(value),
                        "tvg_shift" => mapped_channel.mapped_tvg_shift = Some(value),
                        "group_title" => mapped_channel.mapped_group_title = Some(value),
                        "channel_name" => mapped_channel.mapped_channel_name = value,
                        _ => {} // Unknown field
                    }
                }
                ActionOperator::SetIfEmpty => {
                    // Only set the value if the current field is empty/null (legacy method without capture tracking)
                    match field_name.as_str() {
                        "tvg_id" => {
                            let current = mapped_channel
                                .mapped_tvg_id
                                .as_ref()
                                .or(mapped_channel.original.tvg_id.as_ref());
                            if current.is_none() || current.map_or(true, |s| s.trim().is_empty()) {
                                mapped_channel.mapped_tvg_id = Some(value);
                            }
                        }
                        "tvg_name" => {
                            let current = mapped_channel
                                .mapped_tvg_name
                                .as_ref()
                                .or(mapped_channel.original.tvg_name.as_ref());
                            if current.is_none() || current.map_or(true, |s| s.trim().is_empty()) {
                                mapped_channel.mapped_tvg_name = Some(value);
                            }
                        }
                        "tvg_logo" => {
                            let current = mapped_channel
                                .mapped_tvg_logo
                                .as_ref()
                                .or(mapped_channel.original.tvg_logo.as_ref());
                            if current.is_none() || current.map_or(true, |s| s.trim().is_empty()) {
                                mapped_channel.mapped_tvg_logo = Some(value);
                            }
                        }
                        "tvg_shift" => {
                            let current = mapped_channel
                                .mapped_tvg_shift
                                .as_ref()
                                .or(mapped_channel.original.tvg_shift.as_ref());
                            if current.is_none() || current.map_or(true, |s| s.trim().is_empty()) {
                                mapped_channel.mapped_tvg_shift = Some(value);
                            }
                        }
                        "group_title" => {
                            let current = mapped_channel
                                .mapped_group_title
                                .as_ref()
                                .or(mapped_channel.original.group_title.as_ref());
                            if current.is_none() || current.map_or(true, |s| s.trim().is_empty()) {
                                mapped_channel.mapped_group_title = Some(value);
                            }
                        }
                        "channel_name" => {
                            if mapped_channel.mapped_channel_name.trim().is_empty() {
                                mapped_channel.mapped_channel_name = value;
                            }
                        }
                        _ => {} // Unknown field
                    }
                }
                ActionOperator::Append => {
                    match field_name.as_str() {
                        "tvg_id" => {
                            let empty_string = String::new();
                            let current = mapped_channel
                                .mapped_tvg_id
                                .as_ref()
                                .or(mapped_channel.original.tvg_id.as_ref())
                                .unwrap_or(&empty_string);
                            mapped_channel.mapped_tvg_id = Some(format!("{} {}", current, value));
                        }
                        "tvg_name" => {
                            let empty_string = String::new();
                            let current = mapped_channel
                                .mapped_tvg_name
                                .as_ref()
                                .or(mapped_channel.original.tvg_name.as_ref())
                                .unwrap_or(&empty_string);
                            mapped_channel.mapped_tvg_name = Some(format!("{} {}", current, value));
                        }
                        "tvg_logo" => {
                            let empty_string = String::new();
                            let current = mapped_channel
                                .mapped_tvg_logo
                                .as_ref()
                                .or(mapped_channel.original.tvg_logo.as_ref())
                                .unwrap_or(&empty_string);
                            mapped_channel.mapped_tvg_logo = Some(format!("{} {}", current, value));
                        }
                        "tvg_shift" => {
                            let empty_string = String::new();
                            let current = mapped_channel
                                .mapped_tvg_shift
                                .as_ref()
                                .or(mapped_channel.original.tvg_shift.as_ref())
                                .unwrap_or(&empty_string);
                            mapped_channel.mapped_tvg_shift =
                                Some(format!("{} {}", current, value));
                        }
                        "group_title" => {
                            let empty_string = String::new();
                            let current = mapped_channel
                                .mapped_group_title
                                .as_ref()
                                .or(mapped_channel.original.group_title.as_ref())
                                .unwrap_or(&empty_string);
                            mapped_channel.mapped_group_title =
                                Some(format!("{} {}", current, value));
                        }
                        "channel_name" => {
                            mapped_channel.mapped_channel_name =
                                format!("{} {}", mapped_channel.mapped_channel_name, value);
                        }
                        _ => {} // Unknown field
                    }
                }
                ActionOperator::Remove => {
                    match field_name.as_str() {
                        "tvg_id" => {
                            let empty_string = String::new();
                            let current = mapped_channel
                                .mapped_tvg_id
                                .as_ref()
                                .or(mapped_channel.original.tvg_id.as_ref())
                                .unwrap_or(&empty_string);
                            mapped_channel.mapped_tvg_id = Some(current.replace(&value, ""));
                        }
                        "tvg_name" => {
                            let empty_string = String::new();
                            let current = mapped_channel
                                .mapped_tvg_name
                                .as_ref()
                                .or(mapped_channel.original.tvg_name.as_ref())
                                .unwrap_or(&empty_string);
                            mapped_channel.mapped_tvg_name = Some(current.replace(&value, ""));
                        }
                        "tvg_logo" => {
                            let empty_string = String::new();
                            let current = mapped_channel
                                .mapped_tvg_logo
                                .as_ref()
                                .or(mapped_channel.original.tvg_logo.as_ref())
                                .unwrap_or(&empty_string);
                            mapped_channel.mapped_tvg_logo = Some(current.replace(&value, ""));
                        }
                        "tvg_shift" => {
                            let empty_string = String::new();
                            let current = mapped_channel
                                .mapped_tvg_shift
                                .as_ref()
                                .or(mapped_channel.original.tvg_shift.as_ref())
                                .unwrap_or(&empty_string);
                            mapped_channel.mapped_tvg_shift = Some(current.replace(&value, ""));
                        }
                        "group_title" => {
                            let empty_string = String::new();
                            let current = mapped_channel
                                .mapped_group_title
                                .as_ref()
                                .or(mapped_channel.original.group_title.as_ref())
                                .unwrap_or(&empty_string);
                            mapped_channel.mapped_group_title = Some(current.replace(&value, ""));
                        }
                        "channel_name" => {
                            mapped_channel.mapped_channel_name =
                                mapped_channel.mapped_channel_name.replace(&value, "");
                        }
                        _ => {} // Unknown field
                    }
                }
                ActionOperator::Delete => {
                    // Delete field entirely - set to None/empty
                    match field_name.as_str() {
                        "tvg_id" => mapped_channel.mapped_tvg_id = None,
                        "tvg_name" => mapped_channel.mapped_tvg_name = None,
                        "tvg_logo" => mapped_channel.mapped_tvg_logo = None,
                        "tvg_shift" => mapped_channel.mapped_tvg_shift = None,
                        "group_title" => mapped_channel.mapped_group_title = None,
                        "channel_name" => {
                            // Cannot delete required field channel_name - this should be caught by validation
                            // But if it gets here, we'll leave it unchanged
                        }
                        _ => {} // Unknown field
                    }
                }
            }
        }

        Ok(())
    }

    /// Evaluate an expression for a channel (kept for backwards compatibility)
    #[allow(dead_code)]
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

        // Parse the expression using the filter parser
        let parser = FilterParser::new().with_fields(available_fields);
        let parsed = parser.parse_extended(expression)?;

        // Evaluate the expression
        let mut dummy_captures = RegexCaptures::new();
        match parsed {
            ExtendedExpression::ConditionOnly(condition_tree) => self
                .evaluate_condition_tree_for_channel(channel, &condition_tree, &mut dummy_captures),
            ExtendedExpression::ConditionWithActions { condition, .. } => {
                self.evaluate_condition_tree_for_channel(channel, &condition, &mut dummy_captures)
            }
            ExtendedExpression::ConditionalActionGroups(groups) => {
                // For multiple groups, evaluate if any group's conditions match
                for group in groups {
                    if self.evaluate_condition_tree_for_channel(
                        channel,
                        &group.conditions,
                        &mut dummy_captures,
                    )? {
                        return Ok(true);
                    }
                }
                Ok(false)
            }
        }
    }

    /// Evaluate a condition tree for a channel
    fn evaluate_condition_tree_for_channel(
        &mut self,
        channel: &Channel,
        condition_tree: &crate::models::ConditionTree,
        captures: &mut RegexCaptures,
    ) -> Result<bool, Box<dyn std::error::Error>> {
        self.evaluate_condition_node_for_channel(channel, &condition_tree.root, captures)
    }

    /// Evaluate a condition node for a channel
    fn evaluate_condition_node_for_channel(
        &mut self,
        channel: &Channel,
        node: &crate::models::ConditionNode,
        captures: &mut RegexCaptures,
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
                    FilterOperator::NotStartsWith => !field_value
                        .to_lowercase()
                        .starts_with(&value.to_lowercase()),
                    FilterOperator::NotEndsWith => {
                        !field_value.to_lowercase().ends_with(&value.to_lowercase())
                    }
                    FilterOperator::Matches => {
                        // Apply first-pass filtering if enabled
                        if !self.preprocessor.should_run_regex(&field_value, value, "Data mapping") {
                            false
                        } else {
                            let regex = self.get_or_create_regex(value, false)?;
                            if let Some(matched) = regex.captures(&field_value) {
                                // Store captures for later use in actions
                                // Skip group 0 (full match) and start with group 1 (first capture group)
                                let capture_count = regex.captures_len();
                                for i in 1..capture_count {
                                    if let Some(group_match) = matched.get(i) {
                                        // Capture group exists (may be empty)
                                        captures.add_capture(
                                            format!("${}", i),
                                            group_match.as_str().to_string(),
                                        );
                                    } else {
                                        // Capture group doesn't exist in this match
                                        captures.add_capture(format!("${}", i), String::new());
                                    }
                                }
                                true
                            } else {
                                false
                            }
                        }
                    }
                    FilterOperator::NotMatches => {
                        // Apply first-pass filtering if enabled
                        if !self.preprocessor.should_run_regex(&field_value, value, "Data mapping") {
                            true // If preprocessing suggests no match, then "not matches" should be true
                        } else {
                            let regex = self.get_or_create_regex(value, false)?;
                            !regex.is_match(&field_value)
                        }
                    }
                };

                Ok(if *negate { !result } else { result })
            }
            crate::models::ConditionNode::Group { operator, children } => {
                if children.is_empty() {
                    return Ok(true);
                }

                let first_result =
                    self.evaluate_condition_node_for_channel(channel, &children[0], captures)?;

                let mut combined_result = first_result;
                for child in children.iter().skip(1) {
                    let child_result =
                        self.evaluate_condition_node_for_channel(channel, child, captures)?;
                    match operator {
                        LogicalOperator::And => {
                            combined_result = combined_result && child_result;
                        }
                        LogicalOperator::Or => {
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


    /// Substitute capture groups ($1, $2, etc.) in a string with actual captured values
    fn substitute_capture_groups(
        &self,
        input: &str,
        captures: &RegexCaptures,
    ) -> (String, Option<String>) {
        let mut result = input.to_string();
        let mut individual_captures = Vec::new();
        let mut has_substitutions = false;

        // Regular expression to find capture group references like $1, $2, etc.
        let capture_pattern = Regex::new(r"\$(\d+)").unwrap();

        // Collect all capture group references first to avoid replacement conflicts
        let mut replacements = Vec::new();

        for cap in capture_pattern.captures_iter(input) {
            let full_match = cap.get(0).unwrap().as_str(); // e.g., "$1"
            let group_num = cap.get(1).unwrap().as_str(); // e.g., "1"
            let group_key = format!("${}", group_num);

            if let Some(captured_value) = captures.get_capture(&group_key) {
                replacements.push((full_match.to_string(), captured_value.clone()));
                individual_captures.push(format!("{}='{}'", group_key, captured_value));
                has_substitutions = true;
            } else {
                // Handle missing capture groups - replace with empty string and mark as empty
                replacements.push((full_match.to_string(), String::new()));
                individual_captures.push(format!("{}=''", group_key));
                has_substitutions = true;
            }
        }

        // Apply replacements in reverse order of length to avoid partial replacements
        replacements.sort_by(|a, b| b.0.len().cmp(&a.0.len()));
        for (placeholder, replacement) in replacements {
            result = result.replace(&placeholder, &replacement);
        }

        let capture_description = if !has_substitutions {
            None
        } else {
            // Format: 'resolved_value' (template, $1='val1', $2='')
            Some(format!(
                "'{}' ({}, {})",
                result, // Resolved value
                input,  // Original template
                individual_captures.join(", ")
            ))
        };

        (result, capture_description)
    }
}

impl Default for DataMappingEngine {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_preprocessing_with_special_chars() {
        let engine = DataMappingEngine::new();
        
        // Test channel with + character (should run regex)
        let should_run = engine.should_run_regex_on_field("UK: ITV 1 +1 ◉", ".*(?:[^0-9]\\\\+([0-9]+)[hH]?(?:[^0-9].*|$)|[^0-9](-[0-9]+)[hH]?(?:[^0-9].*|$)).*");
        assert!(should_run, "Should run regex for channel with + character");
        
        // Test channel without special chars but with pattern that has no literals >= minimum length (should run regex - default for no literals)
        let should_run_no_special = engine.should_run_regex_on_field("BBC One HD", ".*(?:[^0-9]\\\\+([0-9]+)[hH]?(?:[^0-9].*|$)|[^0-9](-[0-9]+)[hH]?(?:[^0-9].*|$)).*");
        assert!(should_run_no_special, "Should run regex when pattern has no significant literals (valid case)");
        
        // Test with - character (should run regex) 
        let should_run_minus = engine.should_run_regex_on_field("Channel -2H", ".*(?:[^0-9]\\\\+([0-9]+)[hH]?(?:[^0-9].*|$)|[^0-9](-[0-9]+)[hH]?(?:[^0-9].*|$)).*");
        assert!(should_run_minus, "Should run regex for channel with - character");
        
        // Test case where we should skip: pattern with significant literals not present in field
        let should_not_run = engine.should_run_regex_on_field("BBC One HD", "channel.*sport.*name");
        assert!(!should_not_run, "Should not run regex when required literals 'sport' not present in field");
        
        // Test case where we should run: pattern with significant literals present in field
        let should_run_literal = engine.should_run_regex_on_field("This is a sports channel name", "channel.*sport.*name");
        assert!(should_run_literal, "Should run regex when required literals present in field");
    }
    
    #[test]
    fn test_preprocessing_disabled() {
        let config = DataMappingEngineConfig {
            enable_first_pass_filtering: false,
            enable_regex_caching: true,
            enable_performance_logging: false,
            max_regex_cache_size: 1000,
            precheck_special_chars: "+-@#$%&*=<>!~`€£{}[].".to_string(),
            minimum_literal_length: 2,
        };
        let mut engine = DataMappingEngine::with_config(config);
        
        // Should always run regex when preprocessing is disabled
        let should_run = engine.should_run_regex_on_field("BBC One HD", ".*complex.*regex.*");
        assert!(should_run, "Should always run regex when preprocessing is disabled");
    }
    
    #[test]
    fn test_literal_strings_extraction() {
        let engine = DataMappingEngine::new();
        
        // Test simple pattern with literal strings
        let literals = engine.extract_literal_strings_from_regex("channel.*name");
        assert!(literals.contains(&"channel".to_string()));
        assert!(literals.contains(&"name".to_string()));
        assert_eq!(literals.len(), 2);
        
        // Test pattern with regex metacharacters
        let literals2 = engine.extract_literal_strings_from_regex(".*sport.*");
        assert!(literals2.contains(&"sport".to_string()));
        assert_eq!(literals2.len(), 1);
        
        // Test timeshift pattern
        let literals3 = engine.extract_literal_strings_from_regex(".*(?:[^0-9]\\\\+([0-9]+)[hH]?(?:[^0-9].*|$)|[^0-9](-[0-9]+)[hH]?(?:[^0-9].*|$)).*");
        // This complex pattern should extract minimal literals due to metacharacters
        println!("Timeshift pattern literals: {:?}", literals3);
        
        // Test pattern with no meaningful literals
        let literals4 = engine.extract_literal_strings_from_regex(".*+.*");
        assert!(literals4.is_empty());
    }
}

impl From<crate::config::DataMappingEngineConfig> for DataMappingEngineConfig {
    fn from(config: crate::config::DataMappingEngineConfig) -> Self {
        Self {
            enable_regex_caching: true,
            enable_performance_logging: false,
            max_regex_cache_size: 1000,
            preprocessor: RegexPreprocessorConfig {
                enable_first_pass_filtering: true,
                precheck_special_chars: config.precheck_special_chars
                    .unwrap_or_else(|| "+-@#$%&*=<>!~`€£{}[].".to_string()),
                minimum_literal_length: config.minimum_literal_length.unwrap_or(2),
            },
        }
    }
}
