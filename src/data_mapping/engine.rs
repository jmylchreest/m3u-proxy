use crate::models::{
    data_mapping::{
        DataMappingAction, DataMappingActionType, DataMappingCondition, DataMappingFieldInfo,
        DataMappingRule, DataMappingRuleScope, DataMappingRuleWithDetails, DataMappingSourceType,
        EpgDataMappingResult, MappedChannel, MappedEpgChannel, MappedEpgProgram,
    },
    logo_asset::LogoAsset,
    Channel, EpgChannel, EpgProgram, FilterOperator, LogicalOperator,
};

use chrono::Utc;
use regex::{Regex, RegexBuilder};
use std::collections::HashMap;
use std::time::Instant;
use tracing::{debug, info, warn};
use uuid::Uuid;

/// Default special characters used for regex precheck filtering
/// These characters are considered significant enough to use as first-pass filters
const DEFAULT_PRECHECK_SPECIAL_CHARS: &str = "+-@#$%&*=<>!~`€£{}[]";

/// Configuration for DataMappingEngine optimization parameters
#[derive(Debug, Clone)]
pub struct DataMappingEngineConfig {
    /// Special characters used for regex precheck filtering
    /// These characters are considered significant enough to use as first-pass filters
    pub precheck_special_chars: String,
    /// Minimum length required for literal strings in regex precheck
    /// Strings shorter than this will not be used for precheck filtering
    pub minimum_literal_length: usize,
}

impl Default for DataMappingEngineConfig {
    fn default() -> Self {
        Self {
            precheck_special_chars: DEFAULT_PRECHECK_SPECIAL_CHARS.to_string(),
            minimum_literal_length: 2,
        }
    }
}

impl From<crate::config::DataMappingEngineConfig> for DataMappingEngineConfig {
    fn from(config: crate::config::DataMappingEngineConfig) -> Self {
        Self {
            precheck_special_chars: config
                .precheck_special_chars
                .unwrap_or_else(|| DEFAULT_PRECHECK_SPECIAL_CHARS.to_string()),
            minimum_literal_length: config.minimum_literal_length.unwrap_or(2),
        }
    }
}

/// Stores regex capture groups from condition evaluation
#[derive(Debug, Clone, Default)]
pub struct RegexCaptures {
    pub captures: HashMap<usize, String>, // group index -> captured value
}

impl RegexCaptures {
    pub fn new() -> Self {
        Self {
            captures: HashMap::new(),
        }
    }

    /// Substitute regex capture groups in a value string
    /// Supports $1, $2, etc. syntax
    pub fn substitute_captures(&self, value: &str) -> String {
        if self.captures.is_empty() {
            return value.to_string();
        }

        let mut result = value.to_string();
        // Sort by group index in descending order to avoid replacing $1 when we have $10
        let mut sorted_captures: Vec<_> = self.captures.iter().collect();
        sorted_captures.sort_by(|a, b| b.0.cmp(a.0));

        for (group_index, captured_value) in sorted_captures {
            let placeholder = format!("${}", group_index);
            if result.contains(&placeholder) {
                result = result.replace(&placeholder, captured_value);
                debug!(
                    "Substituted {} -> '{}' in template",
                    placeholder, captured_value
                );
            }
        }

        // Remove any remaining unreplaced placeholders (e.g., $2 when only $1 was captured)
        // This handles cases like "$1$2" where only group 1 has a capture
        let placeholder_regex = regex::Regex::new(r"\$\d+").unwrap();
        result = placeholder_regex.replace_all(&result, "").to_string();

        debug!(
            "Template substitution: '{}' -> '{}' with {} captures",
            value,
            result,
            self.captures.len()
        );
        result
    }
}

pub struct DataMappingEngine {
    regex_cache: HashMap<String, Regex>,
    rule_stats: HashMap<String, (u128, usize)>, // (total_time_micros, channels_processed)
    precheck_special_chars: String,
    minimum_literal_length: usize,
}

impl DataMappingEngine {
    pub fn new() -> Self {
        Self::with_config(DataMappingEngineConfig::default())
    }

    pub fn with_config(config: DataMappingEngineConfig) -> Self {
        Self {
            regex_cache: HashMap::new(),
            rule_stats: HashMap::new(),
            precheck_special_chars: config.precheck_special_chars,
            minimum_literal_length: config.minimum_literal_length,
        }
    }

    pub fn apply_mapping_rules(
        &mut self,
        channels: Vec<Channel>,
        rules: Vec<DataMappingRuleWithDetails>,
        logo_assets: HashMap<Uuid, LogoAsset>,
        source_id: Uuid,
        base_url: &str,
    ) -> Result<Vec<MappedChannel>, Box<dyn std::error::Error>> {
        let mut mapped_channels = Vec::with_capacity(channels.len());

        let active_rules: Vec<_> = rules
            .into_iter()
            .filter(|rule| rule.rule.is_active)
            .collect();

        let overall_start = Instant::now();
        info!(
            "Starting data mapping for source {} with {} channels and {} active rules",
            source_id,
            channels.len(),
            active_rules.len()
        );

        let total_channels = channels.len();
        let total_mutations = 0;
        let mut channels_affected = 0;

        for channel in channels {
            let original_name = channel.channel_name.clone();
            let record_start = Instant::now();
            let mapped = self.apply_rules_to_channel_with_filtering(
                channel,
                &active_rules,
                &logo_assets,
                base_url,
            )?;
            let record_duration = record_start.elapsed();

            debug!(
                "record_processing_time={}μs for channel '{}'",
                record_duration.as_micros(),
                original_name
            );

            if !mapped.applied_rules.is_empty() {
                channels_affected += 1;
                debug!(
                    "Channel '{}' affected by {} rule(s): {:?}",
                    original_name,
                    mapped.applied_rules.len(),
                    mapped.applied_rules
                );
            }

            mapped_channels.push(mapped);
        }

        // Filter out channels marked for removal
        let total_before_filtering = mapped_channels.len();
        mapped_channels.retain(|channel| !channel.is_removed);
        let removed_count = total_before_filtering - mapped_channels.len();

        if removed_count > 0 {
            info!("Removed {} channels marked for deletion", removed_count);
        }

        let overall_duration = overall_start.elapsed();

        info!(
            "total_processing_time={:.2}ms source={} channels_affected={} total_mutations={}",
            overall_duration.as_secs_f64() * 1000.0,
            source_id,
            channels_affected,
            total_mutations
        );

        // Log detailed rule performance statistics
        info!("=== Rule Performance Summary ===");
        info!(
            "Active rules: {} | Rule stats collected: {}",
            active_rules.len(),
            self.rule_stats.len()
        );

        // Add overall precheck vs regex timing summary
        let _total_precheck_time: u128 = 0; // Will be calculated from rule stats
        let _total_regex_time: u128 = 0; // Will be calculated from rule stats
        info!("Overall: {} channels processed", total_channels);
        info!(
            "Processing time: {:.2}ms total, {:.2}ms avg per channel",
            overall_duration.as_secs_f64() * 1000.0,
            (overall_duration.as_secs_f64() * 1000.0) / total_channels as f64
        );

        if self.rule_stats.is_empty() {
            info!("No rule statistics to display (rule_stats is empty)");
        } else {
            let mut sorted_rules: Vec<_> = self.rule_stats.iter().collect();
            sorted_rules.sort_by(|a, b| b.1 .0.cmp(&a.1 .0)); // Sort by total time descending

            for (rule_name, (total_time_micros, channels_processed)) in sorted_rules {
                let avg_time_micros = if *channels_processed > 0 {
                    total_time_micros / (*channels_processed as u128)
                } else {
                    0
                };

                let time_display = if *total_time_micros >= 1000 {
                    format!("{:.2}ms", *total_time_micros as f64 / 1000.0)
                } else {
                    format!("{}μs", total_time_micros)
                };

                let avg_display = if avg_time_micros >= 1000 {
                    format!("{:.2}ms", avg_time_micros as f64 / 1000.0)
                } else {
                    format!("{}μs", avg_time_micros)
                };

                info!(
                    "  rule_processing_time={} rule='{}' avg_per_channel={} channels_processed={}",
                    time_display, rule_name, avg_display, channels_processed
                );
            }
        }
        info!("=== End Performance Summary ===");

        // Clear stats for next run
        self.rule_stats.clear();

        Ok(mapped_channels)
    }

    /// Get current rule performance statistics before they are cleared
    pub fn get_rule_stats(&self) -> &HashMap<String, (u128, usize)> {
        &self.rule_stats
    }

    /// Get rule performance statistics and calculate averages
    pub fn get_rule_performance_summary(&self) -> HashMap<String, (u128, u128, usize)> {
        self.rule_stats
            .iter()
            .map(|(rule_name, (total_time_micros, channels_processed))| {
                let avg_time_micros = if *channels_processed > 0 {
                    total_time_micros / (*channels_processed as u128)
                } else {
                    0
                };
                (
                    rule_name.clone(),
                    (*total_time_micros, avg_time_micros, *channels_processed),
                )
            })
            .collect()
    }

    /// Check if a rule contains any regex conditions
    fn rule_contains_regex_conditions(&self, rule: &DataMappingRuleWithDetails) -> bool {
        rule.conditions.iter().any(|condition| {
            matches!(
                condition.operator,
                FilterOperator::Matches | FilterOperator::NotMatches
            )
        })
    }

    /// Apply rules to channel with first-pass filtering for regex rules
    fn apply_rules_to_channel_with_filtering(
        &mut self,
        channel: Channel,
        rules: &[DataMappingRuleWithDetails],
        logo_assets: &HashMap<Uuid, LogoAsset>,
        base_url: &str,
    ) -> Result<MappedChannel, Box<dyn std::error::Error>> {
        let mut mapped = MappedChannel {
            mapped_tvg_id: channel.tvg_id.clone(),
            mapped_tvg_name: channel.tvg_name.clone(),
            mapped_tvg_logo: channel.tvg_logo.clone(),
            mapped_tvg_shift: channel.tvg_shift.clone(),
            mapped_group_title: channel.group_title.clone(),
            mapped_channel_name: channel.channel_name.clone(),
            applied_rules: Vec::new(),
            is_removed: false,
            original: channel,
        };

        let mut total_precheck_time = 0u128;
        let mut total_regex_time = 0u128;
        let mut precheck_passed = 0usize;
        let mut precheck_filtered = 0usize;
        let mut regex_evaluated = 0usize;

        for rule in rules.iter() {
            let rule_start = Instant::now();

            // Check if this is a regex rule and apply first-pass filtering
            let is_regex_rule = self.rule_contains_regex_conditions(rule);
            let should_evaluate = if is_regex_rule {
                let precheck_start = Instant::now();

                // Apply first-pass filter for regex rules
                let filter_result =
                    self.first_pass_regex_filter(&mapped.original, &rule.conditions);

                let precheck_duration = precheck_start.elapsed().as_micros();
                total_precheck_time += precheck_duration;

                if filter_result {
                    precheck_passed += 1;
                } else {
                    precheck_filtered += 1;
                }
                filter_result
            } else {
                // Always evaluate non-regex rules
                true
            };

            if should_evaluate {
                let regex_start = Instant::now();

                let (conditions_match, captures) =
                    self.evaluate_rule_conditions(&mapped.original, &rule.conditions)?;

                if is_regex_rule {
                    regex_evaluated += 1;
                    let regex_duration = regex_start.elapsed().as_micros();
                    total_regex_time += regex_duration;
                }

                if conditions_match {
                    debug!(
                        "{} rule '{}' conditions matched for channel '{}'",
                        if is_regex_rule { "Regex" } else { "Simple" },
                        rule.rule.name,
                        mapped.original.channel_name
                    );

                    let mutations = self.apply_rule_actions(
                        &mut mapped,
                        &rule.actions,
                        logo_assets,
                        base_url,
                        &captures,
                    )?;
                    mapped.applied_rules.push(rule.rule.id);

                    if mutations > 0 {
                        debug!(
                            "{} rule '{}' applied {} mutation(s) to channel '{}'",
                            if is_regex_rule { "Regex" } else { "Simple" },
                            rule.rule.name,
                            mutations,
                            mapped.original.channel_name
                        );
                    }
                }
            }

            let rule_duration = rule_start.elapsed().as_micros();

            // Track rule performance statistics
            let entry = self
                .rule_stats
                .entry(rule.rule.name.clone())
                .or_insert((0, 0));
            entry.0 += rule_duration;
            entry.1 += 1;

            debug!(
                "Rule stats updated: '{}' total_time={}μs processed_channels={}",
                rule.rule.name, entry.0, entry.1
            );
        }

        debug!(
            "'{}': precheck({}μs, {}/{}) regex({}μs, {})",
            mapped.original.channel_name,
            total_precheck_time,
            precheck_passed,
            precheck_passed + precheck_filtered,
            total_regex_time,
            regex_evaluated
        );

        Ok(mapped)
    }

    /// First-pass filter for regex rules using simple string operations
    fn first_pass_regex_filter(
        &self,
        channel: &Channel,
        conditions: &[DataMappingCondition],
    ) -> bool {
        let mut has_regex_conditions = false;
        let mut any_precheck_passed = false;

        for condition in conditions {
            let field_value = self.get_field_value(channel, &condition.field_name);
            let Some(field_value) = field_value else {
                debug!(
                    "PRECHECK: Field '{}' not found on channel '{}' - skipping condition",
                    condition.field_name, channel.channel_name
                );
                continue;
            };

            // Extract simple patterns from regex that we can check quickly
            if matches!(
                condition.operator,
                FilterOperator::Matches | FilterOperator::NotMatches
            ) {
                has_regex_conditions = true;
                let precheck_result = self.quick_regex_precheck(
                    &field_value.to_lowercase(),
                    &condition.value.to_lowercase(),
                );
                if precheck_result {
                    any_precheck_passed = true;
                }
            } else {
                // Non-regex condition always passes precheck
                return true;
            }
        }

        if has_regex_conditions {
            any_precheck_passed
        } else {
            true
        }
    }

    /// Quick regex pre-check using simple string operations
    fn quick_regex_precheck(&self, field_value: &str, regex_pattern: &str) -> bool {
        // If precheck is disabled (empty special chars and 0 literal length), always pass through
        if self.precheck_special_chars.is_empty() && self.minimum_literal_length == 0 {
            return true;
        }
        // For very simple patterns, check if they're present as literals
        if !regex_pattern
            .chars()
            .any(|c| r".*+?^$[]{}()|\\".contains(c))
        {
            // Pure literal string - check if it's contained in the field
            return field_value.contains(regex_pattern);
        }

        // Extract meaningful patterns from the regex
        let mut required_chars = Vec::new(); // Single special chars that must be present
        let mut literal_strings = Vec::new(); // Multi-char strings that must be present

        // Parse the regex pattern to extract useful precheck patterns
        let mut chars = regex_pattern.chars().peekable();
        let mut current_literal = String::new();

        while let Some(ch) = chars.next() {
            match ch {
                // Regex metacharacters that break literal sequences
                '.' | '*' | '+' | '?' | '^' | '$' | '{' | '}' | '(' | ')' | '|' => {
                    self.save_current_literal(
                        &mut current_literal,
                        &mut literal_strings,
                        &mut required_chars,
                    );
                }
                // Handle character classes [...]
                '[' => {
                    self.save_current_literal(
                        &mut current_literal,
                        &mut literal_strings,
                        &mut required_chars,
                    );
                    // Skip everything until the closing ]
                    let mut bracket_depth = 1;
                    while let Some(bracket_ch) = chars.next() {
                        match bracket_ch {
                            '[' => bracket_depth += 1,
                            ']' => {
                                bracket_depth -= 1;
                                if bracket_depth == 0 {
                                    break;
                                }
                            }
                            _ => {} // Skip everything inside brackets
                        }
                    }
                }
                ']' => {
                    // Shouldn't happen if brackets are balanced, but handle gracefully
                    self.save_current_literal(
                        &mut current_literal,
                        &mut literal_strings,
                        &mut required_chars,
                    );
                }
                // Handle escaped characters
                '\\' => {
                    if let Some(&next_char) = chars.peek() {
                        if "dDwWsSbBnrtf".contains(next_char) {
                            // Character class like \d, \w, etc. - end current literal
                            chars.next();
                            self.save_current_literal(
                                &mut current_literal,
                                &mut literal_strings,
                                &mut required_chars,
                            );
                        } else {
                            // Escaped literal character - add to current literal
                            if let Some(escaped_char) = chars.next() {
                                current_literal.push(escaped_char);
                            }
                        }
                    } else {
                        self.save_current_literal(
                            &mut current_literal,
                            &mut literal_strings,
                            &mut required_chars,
                        );
                    }
                }
                // Regular characters
                c => {
                    current_literal.push(c);
                }
            }
        }

        // Handle any remaining literal
        self.save_current_literal(
            &mut current_literal,
            &mut literal_strings,
            &mut required_chars,
        );

        // Check if field contains required patterns
        let has_required_chars =
            required_chars.is_empty() || required_chars.iter().any(|&ch| field_value.contains(ch));
        let has_required_strings =
            literal_strings.is_empty() || literal_strings.iter().all(|s| field_value.contains(s));

        let result = if required_chars.is_empty() && literal_strings.is_empty() {
            true // No specific requirements - let full regex handle it
        } else {
            has_required_chars && has_required_strings
        };

        // Single concise debug log - include filter result and timing if available
        debug!(
            "PRECHECK: '{}' vs '{}' -> {} (chars:{:?}→{}, strings:{:?}→{}) {}",
            field_value,
            regex_pattern,
            result,
            required_chars,
            has_required_chars,
            literal_strings,
            has_required_strings,
            if !result { "FILTERED" } else { "" }
        );

        result
    }

    /// Helper function to save the current literal and extract special characters
    pub fn save_current_literal(
        &self,
        current_literal: &mut String,
        literal_strings: &mut Vec<String>,
        required_chars: &mut Vec<char>,
    ) {
        if current_literal.is_empty() {
            return;
        }

        // Check if this is a special character that's useful for prefiltering
        if current_literal.len() == 1 {
            let ch = current_literal.chars().next().unwrap();
            if !self.precheck_special_chars.is_empty() && self.precheck_special_chars.contains(ch) {
                required_chars.push(ch);
            }
        } else if self.minimum_literal_length > 0
            && current_literal.len() >= self.minimum_literal_length
        {
            // Multi-character string - check for mixed content
            let has_special = current_literal
                .chars()
                .any(|c| self.precheck_special_chars.contains(c));
            let has_alpha = current_literal.chars().any(|c| c.is_alphabetic());

            if has_special && has_alpha {
                // Mixed content - extract special chars and alpha parts separately
                let mut alpha_part = String::new();
                for ch in current_literal.chars() {
                    if self.precheck_special_chars.contains(ch) {
                        if !required_chars.contains(&ch) {
                            required_chars.push(ch);
                        }
                        // End any alpha sequence
                        if self.minimum_literal_length > 0
                            && alpha_part.len() >= self.minimum_literal_length
                        {
                            literal_strings.push(alpha_part.clone());
                        }
                        alpha_part.clear();
                    } else if ch.is_alphanumeric() || ch.is_whitespace() {
                        alpha_part.push(ch);
                    } else {
                        // Other punctuation
                        if self.minimum_literal_length > 0
                            && alpha_part.len() >= self.minimum_literal_length
                        {
                            literal_strings.push(alpha_part.clone());
                        }
                        alpha_part.clear();
                    }
                }
                if self.minimum_literal_length > 0
                    && alpha_part.len() >= self.minimum_literal_length
                {
                    literal_strings.push(alpha_part);
                }
            } else if has_special {
                // Only special chars - extract each one
                for ch in current_literal.chars() {
                    if !self.precheck_special_chars.is_empty()
                        && self.precheck_special_chars.contains(ch)
                        && !required_chars.contains(&ch)
                    {
                        required_chars.push(ch);
                    }
                }
            } else {
                // Pure text - add as literal string
                let trimmed = current_literal.trim();
                if self.minimum_literal_length > 0 && trimmed.len() >= self.minimum_literal_length {
                    literal_strings.push(trimmed.to_string());
                }
            }
        }

        current_literal.clear();
    }

    /// Apply mapping rules to EPG channels and programs
    pub fn apply_epg_mapping_rules(
        &mut self,
        channels: Vec<EpgChannel>,
        programs: Vec<EpgProgram>,
        rules: Vec<DataMappingRuleWithDetails>,
        logo_assets: HashMap<Uuid, LogoAsset>,
        source_id: Uuid,
        base_url: &str,
    ) -> Result<EpgDataMappingResult, Box<dyn std::error::Error>> {
        let mut mapped_channels = Vec::with_capacity(channels.len());
        let mut mapped_programs = Vec::with_capacity(programs.len());
        let mut clone_groups: HashMap<String, Vec<Uuid>> = HashMap::new();

        let active_rules: Vec<_> = rules
            .into_iter()
            .filter(|rule| {
                rule.rule.is_active && rule.rule.source_type == DataMappingSourceType::Epg
            })
            .collect();

        info!(
            "Starting EPG data mapping for source {} with {} channels, {} programs and {} active EPG rules",
            source_id,
            channels.len(),
            programs.len(),
            active_rules.len()
        );

        let mut total_mutations = 0;
        let mut channels_affected = 0;
        let mut programs_affected = 0;

        // First pass: Process channels and identify clones
        for channel in channels {
            let mapped =
                self.apply_epg_rules_to_channel(channel, &active_rules, &logo_assets, base_url)?;

            if !mapped.applied_rules.is_empty() {
                channels_affected += 1;
                total_mutations += mapped.applied_rules.len();
            }

            // Group channels by their clone group for deduplication
            if let Some(clone_group_id) = &mapped.clone_group_id {
                clone_groups
                    .entry(clone_group_id.clone())
                    .or_insert_with(Vec::new)
                    .push(mapped.original.id);
            }

            mapped_channels.push(mapped);
        }

        // Second pass: Process programs with timeshift adjustments
        for program in programs {
            // Find the corresponding mapped channel
            if let Some(mapped_channel) = mapped_channels
                .iter()
                .find(|ch| ch.original.channel_id == program.channel_id)
            {
                let mapped_program =
                    self.apply_epg_rules_to_program(program, mapped_channel, &active_rules)?;

                if !mapped_program.applied_rules.is_empty() {
                    programs_affected += 1;
                    total_mutations += mapped_program.applied_rules.len();
                }

                mapped_programs.push(mapped_program);
            }
        }

        info!(
            "EPG data mapping completed for source {}: {} channels affected, {} programs affected, {} total mutations applied, {} clone groups created",
            source_id,
            channels_affected,
            programs_affected,
            total_mutations,
            clone_groups.len()
        );

        Ok(EpgDataMappingResult {
            mapped_channels,
            mapped_programs,
            clone_groups,
            total_mutations,
            channels_affected,
            programs_affected,
        })
    }

    /// Convert mapped channels back to regular channels for database storage
    pub fn mapped_to_channels(mapped_channels: Vec<MappedChannel>) -> Vec<Channel> {
        mapped_channels
            .into_iter()
            .map(|mapped| Channel {
                id: mapped.original.id,
                source_id: mapped.original.source_id,
                tvg_id: mapped.mapped_tvg_id,
                tvg_name: mapped.mapped_tvg_name,
                tvg_logo: mapped.mapped_tvg_logo,
                tvg_shift: mapped.mapped_tvg_shift,
                group_title: mapped.mapped_group_title,
                channel_name: mapped.mapped_channel_name,
                stream_url: mapped.original.stream_url,
                created_at: mapped.original.created_at,
                updated_at: mapped.original.updated_at,
            })
            .collect()
    }

    pub fn evaluate_rule_conditions(
        &mut self,
        channel: &Channel,
        conditions: &[DataMappingCondition],
    ) -> Result<(bool, RegexCaptures), Box<dyn std::error::Error>> {
        if conditions.is_empty() {
            return Ok((true, RegexCaptures::new()));
        }

        let mut captures = RegexCaptures::new();
        let (mut result, first_captures) = self.evaluate_condition(channel, &conditions[0])?;

        // Use captures from the first matching condition
        if result {
            captures = first_captures;
        }

        for i in 1..conditions.len() {
            let (condition_result, condition_captures) =
                self.evaluate_condition(channel, &conditions[i])?;

            let logical_op = conditions[i]
                .logical_operator
                .as_ref()
                .unwrap_or(&LogicalOperator::And);

            // Support both old (and/or) and new (all/any) formats
            if logical_op.is_and_like() {
                result = result && condition_result;
                // For AND, only keep captures if both conditions match
                if !result {
                    captures = RegexCaptures::new();
                }
            } else {
                let previous_result = result;
                result = result || condition_result;
                // For OR, use captures from the first matching condition
                if !previous_result && condition_result {
                    captures = condition_captures;
                }
            }
        }

        Ok((result, captures))
    }

    fn evaluate_condition(
        &mut self,
        channel: &Channel,
        condition: &DataMappingCondition,
    ) -> Result<(bool, RegexCaptures), Box<dyn std::error::Error>> {
        let field_value = self.get_field_value(channel, &condition.field_name);
        let Some(field_value) = field_value else {
            return Ok((false, RegexCaptures::new()));
        };

        match condition.operator {
            FilterOperator::Equals => {
                let matches = field_value.to_lowercase() == condition.value.to_lowercase();
                Ok((matches, RegexCaptures::new()))
            }
            FilterOperator::NotEquals => {
                let matches = field_value.to_lowercase() != condition.value.to_lowercase();
                Ok((matches, RegexCaptures::new()))
            }
            FilterOperator::Contains => {
                let matches = field_value
                    .to_lowercase()
                    .contains(&condition.value.to_lowercase());
                Ok((matches, RegexCaptures::new()))
            }
            FilterOperator::NotContains => {
                let matches = !field_value
                    .to_lowercase()
                    .contains(&condition.value.to_lowercase());
                Ok((matches, RegexCaptures::new()))
            }
            FilterOperator::StartsWith => {
                let matches = field_value
                    .to_lowercase()
                    .starts_with(&condition.value.to_lowercase());
                Ok((matches, RegexCaptures::new()))
            }
            FilterOperator::EndsWith => {
                let matches = field_value
                    .to_lowercase()
                    .ends_with(&condition.value.to_lowercase());
                Ok((matches, RegexCaptures::new()))
            }
            FilterOperator::Matches => {
                let regex = self.get_or_create_regex(&condition.value, false)?; // case_insensitive by default
                debug!(
                    "Evaluating regex pattern '{}' against field value '{}' for field '{}'",
                    condition.value, field_value, condition.field_name
                );
                if let Some(captures_match) = regex.captures(&field_value) {
                    let mut captures = RegexCaptures::new();

                    // Store captured groups (skip group 0 which is the full match)
                    for i in 1..captures_match.len() {
                        if let Some(capture) = captures_match.get(i) {
                            captures.captures.insert(i, capture.as_str().to_string());
                            debug!("Captured group {}: '{}'", i, capture.as_str());
                        }
                    }

                    debug!(
                        "Regex match found with {} capture groups",
                        captures.captures.len()
                    );
                    Ok((true, captures))
                } else {
                    debug!("Regex match failed");
                    Ok((false, RegexCaptures::new()))
                }
            }
            FilterOperator::NotMatches => {
                let regex = self.get_or_create_regex(&condition.value, false)?; // case_insensitive by default
                let matches = !regex.is_match(&field_value);
                Ok((matches, RegexCaptures::new()))
            }
        }
    }

    fn apply_rule_actions(
        &self,
        mapped: &mut MappedChannel,
        actions: &[DataMappingAction],
        logo_assets: &HashMap<Uuid, LogoAsset>,
        base_url: &str,
        captures: &RegexCaptures,
    ) -> Result<usize, Box<dyn std::error::Error>> {
        let mut mutations = 0;

        for action in actions {
            let old_value = self.get_mapped_field_value(mapped, &action.target_field);
            let mut action_applied = false;

            match action.action_type {
                DataMappingActionType::SetValue => {
                    if let Some(value) = &action.value {
                        // Validate field is valid for stream source type
                        if !DataMappingFieldInfo::is_valid_field_for_source_type(
                            &action.target_field,
                            &DataMappingSourceType::Stream,
                        ) {
                            warn!(
                                "Invalid field '{}' for Stream source type",
                                action.target_field
                            );
                            continue;
                        }
                        // Substitute regex capture groups in the value
                        let substituted_value = captures.substitute_captures(value);
                        debug!(
                            "SetValue action: field='{}' template='{}' captures={:?} substituted='{}'",
                            action.target_field, value, captures.captures, substituted_value
                        );
                        self.set_field_value(
                            mapped,
                            &action.target_field,
                            Some(substituted_value.clone()),
                        );
                        action_applied = true;
                        debug!(
                            "SetValue: {} '{}' -> '{}' (original template: '{}')",
                            action.target_field,
                            old_value.unwrap_or_else(|| "null".to_string()),
                            substituted_value,
                            value
                        );
                    }
                }
                DataMappingActionType::SetDefaultIfEmpty => {
                    if let Some(value) = &action.value {
                        // Validate field is valid for stream source type
                        if !DataMappingFieldInfo::is_valid_field_for_source_type(
                            &action.target_field,
                            &DataMappingSourceType::Stream,
                        ) {
                            warn!(
                                "Invalid field '{}' for Stream source type",
                                action.target_field
                            );
                            continue;
                        }
                        let current = self.get_mapped_field_value(mapped, &action.target_field);
                        if current.is_none() || current.as_ref().map_or(true, |s| s.is_empty()) {
                            // Substitute regex capture groups in the value
                            let substituted_value = captures.substitute_captures(value);
                            self.set_field_value(
                                mapped,
                                &action.target_field,
                                Some(substituted_value.clone()),
                            );
                            action_applied = true;
                            debug!(
                                "SetDefaultIfEmpty: {} '{}' -> '{}' (original: '{}')",
                                action.target_field,
                                old_value.unwrap_or_else(|| "empty".to_string()),
                                substituted_value,
                                value
                            );
                        }
                    }
                }
                DataMappingActionType::SetLogo => {
                    if let Some(logo_id) = action.logo_asset_id {
                        if let Some(logo_asset) = logo_assets.get(&logo_id) {
                            let logo_url = crate::utils::generate_logo_url(base_url, logo_asset.id);
                            self.set_field_value(
                                mapped,
                                &action.target_field,
                                Some(logo_url.clone()),
                            );
                            action_applied = true;
                            debug!(
                                "SetLogo: {} '{}' -> logo '{}' ({})",
                                action.target_field,
                                old_value.unwrap_or_else(|| "null".to_string()),
                                logo_asset.name,
                                logo_url
                            );
                        } else {
                            warn!(
                                "SetLogo: Logo asset {} not found for field {}",
                                logo_id, action.target_field
                            );
                        }
                    }
                }

                DataMappingActionType::TimeshiftEpg => {
                    // For stream channels, set tvg-shift field
                    if let Some(timeshift_minutes) = action.timeshift_minutes {
                        let timeshift_hours = timeshift_minutes / 60;
                        let timeshift_value = if timeshift_hours > 0 {
                            format!("+{}", timeshift_hours)
                        } else {
                            timeshift_hours.to_string()
                        };
                        self.set_field_value(mapped, "tvg_shift", Some(timeshift_value));
                        action_applied = true;
                        debug!("TimeshiftEpg: Set tvg_shift to {} hours", timeshift_hours);
                    }
                }
                DataMappingActionType::DeduplicateStreamUrls => {
                    // This action will be handled at the group level, not individual channel level
                    // We'll mark it as applied but the actual deduplication happens later
                    action_applied = true;
                    debug!("DeduplicateStreamUrls: Action marked for group processing");
                }
                DataMappingActionType::RemoveChannel => {
                    // Mark channel for removal from the final output
                    mapped.is_removed = true;
                    action_applied = true;
                    debug!(
                        "RemoveChannel: Channel '{}' marked for removal",
                        mapped.original.channel_name
                    );
                }
            }

            if action_applied {
                mutations += 1;
            }
        }

        Ok(mutations)
    }

    fn get_field_value(&self, channel: &Channel, field_name: &str) -> Option<String> {
        match field_name {
            "channel_name" => Some(channel.channel_name.clone()),
            "tvg_id" => channel.tvg_id.clone(),
            "tvg_name" => channel.tvg_name.clone(),
            "tvg_logo" => channel.tvg_logo.clone(),
            "tvg_shift" => channel.tvg_shift.clone(),
            "group_title" => channel.group_title.clone(),
            "stream_url" => Some(channel.stream_url.clone()),
            _ => None,
        }
    }

    fn get_mapped_field_value(&self, mapped: &MappedChannel, field_name: &str) -> Option<String> {
        match field_name {
            "channel_name" => Some(mapped.mapped_channel_name.clone()),
            "tvg_id" => mapped.mapped_tvg_id.clone(),
            "tvg_name" => mapped.mapped_tvg_name.clone(),
            "tvg_logo" => mapped.mapped_tvg_logo.clone(),
            "tvg_shift" => mapped.mapped_tvg_shift.clone(),
            "group_title" => mapped.mapped_group_title.clone(),
            "stream_url" => Some(mapped.original.stream_url.clone()),
            _ => None,
        }
    }

    fn set_field_value(&self, mapped: &mut MappedChannel, field_name: &str, value: Option<String>) {
        match field_name {
            "channel_name" => {
                if let Some(val) = value {
                    mapped.mapped_channel_name = val;
                }
            }
            "tvg_id" => mapped.mapped_tvg_id = value,
            "tvg_name" => mapped.mapped_tvg_name = value,
            "tvg_logo" => mapped.mapped_tvg_logo = value,
            "tvg_shift" => mapped.mapped_tvg_shift = value,
            "group_title" => mapped.mapped_group_title = value,
            _ => {}
        }
    }

    fn get_or_create_regex(
        &mut self,
        pattern: &str,
        case_sensitive: bool,
    ) -> Result<&Regex, Box<dyn std::error::Error>> {
        let cache_key = format!("{}:{}", pattern, case_sensitive);
        if !self.regex_cache.contains_key(&cache_key) {
            let regex = if case_sensitive {
                RegexBuilder::new(pattern).build()?
            } else {
                RegexBuilder::new(pattern).case_insensitive(true).build()?
            };
            self.regex_cache.insert(cache_key.clone(), regex);
        }
        Ok(self.regex_cache.get(&cache_key).unwrap())
    }

    pub fn test_mapping_rule(
        &mut self,
        channels: Vec<Channel>,
        conditions: Vec<DataMappingCondition>,
        actions: Vec<DataMappingAction>,
        logo_assets: HashMap<Uuid, LogoAsset>,
        base_url: &str,
    ) -> Result<Vec<MappedChannel>, Box<dyn std::error::Error>> {
        let test_rule = DataMappingRuleWithDetails {
            rule: DataMappingRule {
                id: Uuid::new_v4(),
                name: "Test Rule".to_string(),
                description: None,
                source_type: DataMappingSourceType::Stream, // Default to Stream for existing test
                scope: DataMappingRuleScope::Individual,
                sort_order: 0,
                is_active: true,
                created_at: Utc::now(),
                updated_at: Utc::now(),
            },
            conditions,
            actions,
            expression: Some("test expression".to_string()),
        };

        let mut mapped_channels = Vec::new();
        for channel in channels {
            // Apply the same filtering logic as the main mapping pipeline
            let mapped = self.apply_rules_to_channel_with_filtering(
                channel,
                &[test_rule.clone()],
                &logo_assets,
                base_url,
            )?;

            // Only include channels that had rules applied
            if !mapped.applied_rules.is_empty() {
                mapped_channels.push(mapped);
            }
        }

        Ok(mapped_channels)
    }

    fn apply_epg_rules_to_channel(
        &mut self,
        channel: EpgChannel,
        rules: &[DataMappingRuleWithDetails],
        logo_assets: &HashMap<Uuid, LogoAsset>,
        base_url: &str,
    ) -> Result<MappedEpgChannel, Box<dyn std::error::Error>> {
        let mut mapped = MappedEpgChannel {
            mapped_channel_id: channel.channel_id.clone(),
            mapped_channel_name: channel.channel_name.clone(),
            mapped_channel_logo: channel.channel_logo.clone(),
            mapped_channel_group: channel.channel_group.clone(),
            mapped_language: channel.language.clone(),
            applied_rules: Vec::new(),
            clone_group_id: None,
            is_primary_clone: false,
            timeshift_offset: None,
            original: channel,
        };

        for rule in rules.iter() {
            if self.evaluate_epg_channel_conditions(&mapped.original, &rule.conditions)? {
                debug!(
                    "EPG Rule '{}' conditions matched for channel '{}'",
                    rule.rule.name, mapped.original.channel_name
                );

                let mutations = self.apply_epg_channel_actions(
                    &mut mapped,
                    &rule.actions,
                    logo_assets,
                    base_url,
                )?;
                mapped.applied_rules.push(rule.rule.id);

                if mutations > 0 {
                    info!(
                        "EPG Rule '{}' applied {} mutation(s) to channel '{}'",
                        rule.rule.name, mutations, mapped.original.channel_name
                    );
                }
            }
        }

        Ok(mapped)
    }

    fn apply_epg_rules_to_program(
        &mut self,
        program: EpgProgram,
        mapped_channel: &MappedEpgChannel,
        _rules: &[DataMappingRuleWithDetails],
    ) -> Result<MappedEpgProgram, Box<dyn std::error::Error>> {
        let mut mapped_start_time = program.start_time;
        let mut mapped_end_time = program.end_time;

        // Apply timeshift if the channel has a timeshift offset
        if let Some(timeshift_minutes) = mapped_channel.timeshift_offset {
            let timeshift_duration = chrono::Duration::minutes(timeshift_minutes as i64);
            mapped_start_time = program.start_time + timeshift_duration;
            mapped_end_time = program.end_time + timeshift_duration;
        }

        Ok(MappedEpgProgram {
            mapped_channel_id: mapped_channel.mapped_channel_id.clone(),
            mapped_channel_name: mapped_channel.mapped_channel_name.clone(),
            mapped_program_title: program.program_title.clone(),
            mapped_program_description: program.program_description.clone(),
            mapped_program_category: program.program_category.clone(),
            mapped_start_time,
            mapped_end_time,
            applied_rules: Vec::new(), // Programs inherit channel rules for now
            original: program,
        })
    }

    fn evaluate_epg_channel_conditions(
        &mut self,
        channel: &EpgChannel,
        conditions: &[DataMappingCondition],
    ) -> Result<bool, Box<dyn std::error::Error>> {
        if conditions.is_empty() {
            return Ok(true);
        }

        let mut result = self.evaluate_epg_channel_condition(channel, &conditions[0])?;

        for i in 1..conditions.len() {
            let condition_result = self.evaluate_epg_channel_condition(channel, &conditions[i])?;

            let logical_op = conditions[i]
                .logical_operator
                .as_ref()
                .unwrap_or(&LogicalOperator::And);

            if logical_op.is_and_like() {
                result = result && condition_result;
            } else {
                result = result || condition_result;
            }
        }

        Ok(result)
    }

    fn evaluate_epg_channel_condition(
        &mut self,
        channel: &EpgChannel,
        condition: &DataMappingCondition,
    ) -> Result<bool, Box<dyn std::error::Error>> {
        let field_value = self.get_epg_channel_field_value(channel, &condition.field_name);
        let Some(field_value) = field_value else {
            return Ok(false);
        };

        match condition.operator {
            FilterOperator::Equals => {
                Ok(field_value.to_lowercase() == condition.value.to_lowercase())
            }
            FilterOperator::NotEquals => {
                Ok(field_value.to_lowercase() != condition.value.to_lowercase())
            }
            FilterOperator::Contains => Ok(field_value
                .to_lowercase()
                .contains(&condition.value.to_lowercase())),
            FilterOperator::NotContains => Ok(!field_value
                .to_lowercase()
                .contains(&condition.value.to_lowercase())),
            FilterOperator::StartsWith => Ok(field_value
                .to_lowercase()
                .starts_with(&condition.value.to_lowercase())),
            FilterOperator::EndsWith => Ok(field_value
                .to_lowercase()
                .ends_with(&condition.value.to_lowercase())),
            FilterOperator::Matches => {
                let regex = self.get_or_create_regex(&condition.value, false)?;
                Ok(regex.is_match(&field_value))
            }
            FilterOperator::NotMatches => {
                let regex = self.get_or_create_regex(&condition.value, false)?;
                Ok(!regex.is_match(&field_value))
            }
        }
    }

    fn apply_epg_channel_actions(
        &mut self,
        mapped: &mut MappedEpgChannel,
        actions: &[DataMappingAction],
        logo_assets: &HashMap<Uuid, LogoAsset>,
        base_url: &str,
    ) -> Result<usize, Box<dyn std::error::Error>> {
        let mut mutations = 0;

        for action in actions {
            let old_value = self.get_mapped_epg_channel_field_value(mapped, &action.target_field);
            let mut action_applied = false;

            match action.action_type {
                DataMappingActionType::SetValue => {
                    if let Some(value) = &action.value {
                        self.set_epg_channel_field_value(
                            mapped,
                            &action.target_field,
                            Some(value.clone()),
                        );
                        action_applied = true;
                        debug!(
                            "EPG SetValue: {} '{}' -> '{}'",
                            action.target_field,
                            old_value.unwrap_or_else(|| "null".to_string()),
                            value
                        );
                    }
                }
                DataMappingActionType::SetDefaultIfEmpty => {
                    if let Some(value) = &action.value {
                        let current =
                            self.get_mapped_epg_channel_field_value(mapped, &action.target_field);
                        if current.is_none() || current.as_ref().map_or(true, |s| s.is_empty()) {
                            self.set_epg_channel_field_value(
                                mapped,
                                &action.target_field,
                                Some(value.clone()),
                            );
                            action_applied = true;
                            debug!(
                                "EPG SetDefaultIfEmpty: {} '{}' -> '{}'",
                                action.target_field,
                                old_value.unwrap_or_else(|| "empty".to_string()),
                                value
                            );
                        }
                    }
                }
                DataMappingActionType::SetLogo => {
                    if let Some(logo_id) = action.logo_asset_id {
                        if let Some(logo_asset) = logo_assets.get(&logo_id) {
                            let logo_url = crate::utils::generate_logo_url(base_url, logo_asset.id);
                            self.set_epg_channel_field_value(
                                mapped,
                                &action.target_field,
                                Some(logo_url.clone()),
                            );
                            action_applied = true;
                            debug!(
                                "EPG SetLogo: {} '{}' -> logo '{}' ({})",
                                action.target_field,
                                old_value.unwrap_or_else(|| "null".to_string()),
                                logo_asset.name,
                                logo_url
                            );
                        }
                    }
                }

                DataMappingActionType::TimeshiftEpg => {
                    if let Some(timeshift_minutes) = action.timeshift_minutes {
                        mapped.timeshift_offset = Some(timeshift_minutes);
                        action_applied = true;
                        debug!(
                            "EPG TimeshiftEpg: Channel '{}' timeshift set to {} minutes",
                            mapped.original.channel_name, timeshift_minutes
                        );
                    }
                }
                DataMappingActionType::DeduplicateStreamUrls => {
                    // This action is for stream channels, not EPG channels
                    debug!("DeduplicateStreamUrls action is for stream channels, not EPG channels");
                }
                DataMappingActionType::RemoveChannel => {
                    // This action is for stream channels, not EPG channels
                    debug!("RemoveChannel action is for stream channels, not EPG channels");
                }
            }

            if action_applied {
                mutations += 1;
            }
        }

        Ok(mutations)
    }

    fn get_epg_channel_field_value(
        &self,
        channel: &EpgChannel,
        field_name: &str,
    ) -> Option<String> {
        match field_name {
            "channel_id" => Some(channel.channel_id.clone()),
            "channel_name" => Some(channel.channel_name.clone()),
            "channel_logo" => channel.channel_logo.clone(),
            "channel_group" => channel.channel_group.clone(),
            "language" => channel.language.clone(),
            _ => None,
        }
    }

    fn get_mapped_epg_channel_field_value(
        &self,
        mapped: &MappedEpgChannel,
        field_name: &str,
    ) -> Option<String> {
        match field_name {
            "channel_id" => Some(mapped.mapped_channel_id.clone()),
            "channel_name" => Some(mapped.mapped_channel_name.clone()),
            "channel_logo" => mapped.mapped_channel_logo.clone(),
            "channel_group" => mapped.mapped_channel_group.clone(),
            "language" => mapped.mapped_language.clone(),
            _ => None,
        }
    }

    fn set_epg_channel_field_value(
        &self,
        mapped: &mut MappedEpgChannel,
        field_name: &str,
        value: Option<String>,
    ) {
        match field_name {
            "channel_id" => {
                if let Some(val) = value {
                    mapped.mapped_channel_id = val;
                }
            }
            "channel_name" => {
                if let Some(val) = value {
                    mapped.mapped_channel_name = val;
                }
            }
            "channel_logo" => mapped.mapped_channel_logo = value,
            "channel_group" => mapped.mapped_channel_group = value,
            "language" => mapped.mapped_language = value,
            _ => {}
        }
    }

    /// Convert mapped EPG channels back to regular EPG channels for database storage
    pub fn mapped_epg_channels_to_channels(
        mapped_channels: Vec<MappedEpgChannel>,
    ) -> Vec<EpgChannel> {
        mapped_channels
            .into_iter()
            .map(|mapped| EpgChannel {
                id: mapped.original.id,
                source_id: mapped.original.source_id,
                channel_id: mapped.mapped_channel_id,
                channel_name: mapped.mapped_channel_name,
                channel_logo: mapped.mapped_channel_logo,
                channel_group: mapped.mapped_channel_group,
                language: mapped.mapped_language,
                created_at: mapped.original.created_at,
                updated_at: mapped.original.updated_at,
            })
            .collect()
    }

    /// Convert mapped EPG programs back to regular EPG programs for database storage
    pub fn mapped_epg_programs_to_programs(
        mapped_programs: Vec<MappedEpgProgram>,
    ) -> Vec<EpgProgram> {
        mapped_programs
            .into_iter()
            .map(|mapped| EpgProgram {
                id: mapped.original.id,
                source_id: mapped.original.source_id,
                channel_id: mapped.mapped_channel_id,
                channel_name: mapped.mapped_channel_name,
                program_title: mapped.mapped_program_title,
                program_description: mapped.mapped_program_description,
                program_category: mapped.mapped_program_category,
                start_time: mapped.mapped_start_time,
                end_time: mapped.mapped_end_time,
                episode_num: mapped.original.episode_num,
                season_num: mapped.original.season_num,
                rating: mapped.original.rating,
                language: mapped.original.language,
                subtitles: mapped.original.subtitles,
                aspect_ratio: mapped.original.aspect_ratio,
                created_at: mapped.original.created_at,
                updated_at: mapped.original.updated_at,
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_quick_regex_precheck() {
        let engine = DataMappingEngine::new();

        // Test cases: (field_value, regex_pattern, expected_result, description)
        let test_cases = vec![
            // Simple literal patterns should work
            ("bbc one", "bbc", true, "Simple literal should match"),
            ("channel 4", "channel", true, "Simple literal should match"),
            // Regex with literal parts that should be extracted
            (
                "bbc one +1",
                r"bbc.*\+1",
                true,
                "Should extract 'bbc' as literal part",
            ),
            (
                "cnn +24",
                r".*\+(\d+)",
                true,
                "Should allow complex regex to pass through",
            ),
            // Timeshift regex pattern from default rule
            (
                "bbc one +1",
                r"(.+?)\s*\+\s*(\d+)",
                true,
                "Complex pattern should pass through",
            ),
            (
                "itv +24",
                r"(.+?)\s*\+\s*(\d+)",
                true,
                "Complex pattern should pass through",
            ),
            // Non-matching cases with clear literals
            ("discovery", "bbc", false, "No literal match should fail"),
            ("channel 5", "bbc", false, "Different literal should fail"),
            // Edge cases
            ("", "test", false, "Empty field should not match"),
            ("test", "", true, "Empty regex should pass through"),
            ("test", r".*", true, "Pure wildcard should pass through"),
            (
                "test",
                r"\d+",
                true,
                "Pure character class should pass through",
            ),
            // Test literal extraction from complex patterns
            (
                "bbc news",
                r"bbc.*news",
                true,
                "Should extract 'bbc' literal",
            ),
            (
                "sky sports",
                r"sky.+sports",
                true,
                "Should extract 'sky' literal",
            ),
        ];

        for (field_value, regex_pattern, expected, description) in test_cases {
            let result = engine.quick_regex_precheck(field_value, regex_pattern);
            println!(
                "Test: {} | Field: '{}' | Regex: '{}' | Expected: {} | Got: {}",
                description, field_value, regex_pattern, expected, result
            );

            if result != expected {
                println!("FAILED: {}", description);

                // Debug the new literal extraction logic
                let mut literal_candidates = Vec::new();
                let mut current_literal = String::new();
                let mut chars = regex_pattern.chars().peekable();

                while let Some(ch) = chars.next() {
                    match ch {
                        '.' | '*' | '+' | '?' | '^' | '$' | '[' | ']' | '{' | '}' | '(' | ')'
                        | '|' => {
                            if !current_literal.is_empty() && current_literal.len() >= 2 {
                                literal_candidates.push(current_literal.clone());
                            }
                            current_literal.clear();
                            if ch == '\\' {
                                chars.next();
                            }
                        }
                        c if c.is_alphanumeric() || c.is_whitespace() => {
                            current_literal.push(c);
                        }
                        _ => {
                            current_literal.push(ch);
                        }
                    }
                }

                if !current_literal.is_empty() && current_literal.len() >= 2 {
                    literal_candidates.push(current_literal);
                }

                println!("  Literal candidates: {:?}", literal_candidates);
            }

            assert_eq!(result, expected, "{}", description);
        }
    }

    #[test]
    fn test_improved_precheck_patterns() {
        let engine = DataMappingEngine::new();

        // Test various pattern types
        let test_cases = vec![
            // Simple literal patterns
            ("BBC", "bbc one", true, "Simple literal should match"),
            ("BBC", "itv", false, "Simple literal should not match"),
            // Special character patterns
            (
                "Live@12",
                "CNN Live@12",
                true,
                "Should match @ character and literals",
            ),
            (
                "Live@12",
                "CNN Live HD",
                false,
                "Should not match without @ character",
            ),
            // Mixed patterns with special chars and text
            (
                "BBC.*HD",
                "bbc one hd",
                true,
                "Should extract 'BBC' and 'HD' literals",
            ),
            ("BBC.*HD", "bbc one", false, "Should not match without 'HD'"),
            ("BBC.*HD", "itv hd", false, "Should not match without 'BBC'"),
            // Complex regex patterns that should pass through
            (
                r"(.+?)\s*\+\s*(\d+)",
                "bbc one +1",
                true,
                "Complex pattern should pass through for matching content",
            ),
            (
                r"(.+?)\s*\+\s*(\d+)",
                "bbc one",
                false,
                "Complex pattern should filter out non-matching content",
            ),
            // URL patterns
            (
                "http.*://.*",
                "http://example.com",
                true,
                "Should extract 'http' and '://' patterns",
            ),
            (
                "http.*://.*",
                "https://example.com",
                true,
                "Precheck allows false positives - 'http' substring found in 'https'",
            ),
            // Email patterns
            (
                r"\w+@\w+\.\w+",
                "test@example.com",
                true,
                "Should extract @ character",
            ),
            (
                r"\w+@\w+\.\w+",
                "test-example-com",
                false,
                "Should not match without @ character",
            ),
        ];

        for (pattern, channel, expected, description) in test_cases {
            let result =
                engine.quick_regex_precheck(&channel.to_lowercase(), &pattern.to_lowercase());
            println!(
                "Pattern test: '{}' vs '{}' = {} (expected: {}) - {}",
                pattern, channel, result, expected, description
            );
            assert_eq!(result, expected, "{}", description);
        }
    }

    #[test]
    fn test_real_world_pattern_from_logs() {
        let engine = DataMappingEngine::new();

        // Test the actual pattern from the user's logs
        let pattern = r".*(?:\+([0-9]+)|(-[0-9]+)).*";
        let test_channels = vec![
            (
                "de: sky max 10 4k",
                false,
                "Should not match - no + or - characters",
            ),
            (
                "de: sky max 11 4k",
                false,
                "Should not match - no + or - characters",
            ),
            (
                "de: sky max 12 4k",
                false,
                "Should not match - no + or - characters",
            ),
            (
                "bbc one +1",
                true,
                "Should pass precheck - contains + character",
            ),
            (
                "cnn +24",
                true,
                "Should pass precheck - contains + character",
            ),
            (
                "discovery -2",
                true,
                "Should pass precheck - contains - character",
            ),
            (
                "test + sign",
                true,
                "Should pass precheck - contains + character",
            ),
            (
                "test - sign",
                true,
                "Should pass precheck - contains - character",
            ),
        ];

        for (channel, expected, description) in test_channels {
            let result =
                engine.quick_regex_precheck(&channel.to_lowercase(), &pattern.to_lowercase());
            println!(
                "Real pattern test: '{}' with pattern '{}' = {} (expected: {}) - {}",
                channel, pattern, result, expected, description
            );

            // Always show what the pattern extracts for debugging
            let mut required_chars = Vec::new();
            let mut literal_strings = Vec::new();
            let mut chars = pattern.chars().peekable();
            let mut current_literal = String::new();

            while let Some(ch) = chars.next() {
                match ch {
                    '.' | '*' | '+' | '?' | '^' | '$' | '{' | '}' | '(' | ')' | '|' => {
                        engine.save_current_literal(
                            &mut current_literal,
                            &mut literal_strings,
                            &mut required_chars,
                        );
                    }
                    '[' => {
                        engine.save_current_literal(
                            &mut current_literal,
                            &mut literal_strings,
                            &mut required_chars,
                        );
                        // Skip brackets
                        let mut bracket_depth = 1;
                        while let Some(bracket_ch) = chars.next() {
                            match bracket_ch {
                                '[' => bracket_depth += 1,
                                ']' => {
                                    bracket_depth -= 1;
                                    if bracket_depth == 0 {
                                        break;
                                    }
                                }
                                _ => {}
                            }
                        }
                    }
                    ']' => {
                        engine.save_current_literal(
                            &mut current_literal,
                            &mut literal_strings,
                            &mut required_chars,
                        );
                    }
                    '\\' => {
                        if let Some(&next_char) = chars.peek() {
                            if "dDwWsSbBnrtf".contains(next_char) {
                                chars.next();
                                engine.save_current_literal(
                                    &mut current_literal,
                                    &mut literal_strings,
                                    &mut required_chars,
                                );
                            } else {
                                if let Some(escaped_char) = chars.next() {
                                    current_literal.push(escaped_char);
                                }
                            }
                        } else {
                            engine.save_current_literal(
                                &mut current_literal,
                                &mut literal_strings,
                                &mut required_chars,
                            );
                        }
                    }
                    c => {
                        current_literal.push(c);
                    }
                }
            }
            engine.save_current_literal(
                &mut current_literal,
                &mut literal_strings,
                &mut required_chars,
            );

            println!(
                "  Extracted - Required chars: {:?}, Literal strings: {:?}",
                required_chars, literal_strings
            );

            let has_required_chars = required_chars.is_empty()
                || required_chars
                    .iter()
                    .any(|&ch| channel.to_lowercase().contains(ch));
            let has_required_strings = literal_strings.is_empty()
                || literal_strings
                    .iter()
                    .any(|s| channel.to_lowercase().contains(s));

            println!(
                "  Channel '{}' - Has required chars: {}, Has required strings: {}",
                channel.to_lowercase(),
                has_required_chars,
                has_required_strings
            );

            assert_eq!(result, expected, "{}", description);
        }
    }

    #[test]
    fn test_extended_character_set() {
        let engine = DataMappingEngine::new();

        // Test the new characters we added
        let test_cases = vec![
            (
                "price€100",
                ".*€.*",
                true,
                "Euro symbol should be recognized",
            ),
            (
                "price£50",
                ".*£.*",
                true,
                "Pound symbol should be recognized",
            ),
            (
                "config{value}",
                ".*\\{.*",
                true,
                "Opening brace should be recognized",
            ),
            (
                "config}end",
                ".*\\}.*",
                true,
                "Closing brace should be recognized",
            ),
            (
                "list[0]",
                ".*\\[.*",
                true,
                "Opening bracket should be recognized",
            ),
            (
                "list]end",
                ".*\\].*",
                true,
                "Closing bracket should be recognized",
            ),
        ];

        for (channel, pattern, expected, description) in test_cases {
            let result = engine.quick_regex_precheck(channel, pattern);
            println!(
                "Extended char test: '{}' vs '{}' = {} (expected: {}) - {}",
                channel, pattern, result, expected, description
            );
            assert_eq!(result, expected, "{}", description);
        }
    }

    #[test]
    fn test_remove_channel_action() {
        let mut engine = DataMappingEngine::new();

        // Create a test channel
        let test_channel = crate::models::Channel {
            id: Uuid::new_v4(),
            source_id: Uuid::new_v4(),
            channel_name: "Test Channel".to_string(),
            tvg_id: Some("test".to_string()),
            tvg_name: Some("Test".to_string()),
            tvg_logo: None,
            tvg_shift: None,
            group_title: Some("Test Group".to_string()),
            stream_url: "http://example.com/stream".to_string(),
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        };

        // Create a rule with RemoveChannel action
        let test_rule = DataMappingRuleWithDetails {
            rule: DataMappingRule {
                id: Uuid::new_v4(),
                name: "Remove Test Channels".to_string(),
                description: Some("Remove channels with 'Test' in name".to_string()),
                source_type: DataMappingSourceType::Stream,
                scope: DataMappingRuleScope::Individual,
                is_active: true,
                sort_order: 0,
                created_at: chrono::Utc::now(),
                updated_at: chrono::Utc::now(),
            },
            conditions: vec![DataMappingCondition {
                id: Uuid::new_v4(),
                rule_id: Uuid::new_v4(),
                field_name: "channel_name".to_string(),
                operator: crate::models::FilterOperator::Contains,
                value: "Test".to_string(),
                logical_operator: None,
                sort_order: 0,
                created_at: chrono::Utc::now(),
            }],
            actions: vec![DataMappingAction {
                id: Uuid::new_v4(),
                rule_id: Uuid::new_v4(),
                action_type: DataMappingActionType::RemoveChannel,
                target_field: "channel_name".to_string(),
                value: None,
                logo_asset_id: None,
                timeshift_minutes: None,
                sort_order: 0,
                created_at: chrono::Utc::now(),
            }],
            expression: Some("test expression".to_string()),
        };

        // Apply the rule
        let result = engine.apply_mapping_rules(
            vec![test_channel],
            vec![test_rule],
            std::collections::HashMap::new(),
            Uuid::new_v4(),
            "http://localhost:8080",
        );

        // Verify the channel was removed
        assert!(result.is_ok());
        let mapped_channels = result.unwrap();
        assert_eq!(mapped_channels.len(), 0, "Channel should have been removed");
    }

    #[test]
    fn test_config_based_engine_construction() {
        // Test with custom config
        let custom_config = DataMappingEngineConfig {
            precheck_special_chars: "€£{}[]+-".to_string(),
            minimum_literal_length: 3,
        };

        let engine = DataMappingEngine::with_config(custom_config);

        // Test that it uses the configured characters
        let result = engine.quick_regex_precheck("test€price", ".*€.*");
        assert_eq!(
            result, true,
            "Custom config engine should use configured special chars"
        );

        // Test that it uses the configured minimum length
        assert_eq!(
            engine.minimum_literal_length, 3,
            "Should use configured minimum literal length"
        );
        assert!(
            engine.precheck_special_chars.contains('€'),
            "Should use configured special characters"
        );

        // Test conversion from config::DataMappingEngineConfig
        let config_struct = crate::config::DataMappingEngineConfig {
            precheck_special_chars: Some("@#$%".to_string()),
            minimum_literal_length: Some(4),
        };

        let engine_from_config = DataMappingEngine::with_config(config_struct.into());
        assert_eq!(engine_from_config.minimum_literal_length, 4);
        assert_eq!(engine_from_config.precheck_special_chars, "@#$%");
    }

    #[test]
    fn test_capture_group_substitution_and_tvg_shift() {
        use crate::models::data_mapping::*;
        use std::collections::HashMap;
        use uuid::Uuid;

        let mut engine = DataMappingEngine::new();

        // Create test channel with a name that has numbers for capture groups
        let test_channel = Channel {
            id: Uuid::new_v4(),
            source_id: Uuid::new_v4(),
            channel_name: "◉: beIN Sports 8 HD".to_string(),
            tvg_id: Some("bein8".to_string()),
            tvg_name: Some("beIN Sports 8 HD".to_string()),
            tvg_logo: None,
            tvg_shift: Some("+2".to_string()),
            group_title: Some("Sports".to_string()),
            stream_url: "http://example.com/stream".to_string(),
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        };

        // Create a rule that captures the number and uses it in tvg_shift
        let test_rule = DataMappingRuleWithDetails {
            rule: DataMappingRule {
                id: Uuid::new_v4(),
                name: "Capture Number Test".to_string(),
                description: Some("Test capture group substitution".to_string()),
                source_type: DataMappingSourceType::Stream,
                scope: DataMappingRuleScope::Individual,
                sort_order: 1,
                is_active: true,
                created_at: chrono::Utc::now(),
                updated_at: chrono::Utc::now(),
            },
            conditions: vec![DataMappingCondition {
                id: Uuid::new_v4(),
                rule_id: Uuid::new_v4(),
                field_name: "channel_name".to_string(),
                operator: FilterOperator::Matches,
                value: r".*Sports\s+(\d+)\s+HD".to_string(),
                logical_operator: None,
                sort_order: 1,
                created_at: chrono::Utc::now(),
            }],
            actions: vec![DataMappingAction {
                id: Uuid::new_v4(),
                rule_id: Uuid::new_v4(),
                action_type: DataMappingActionType::SetValue,
                target_field: "tvg_shift".to_string(),
                value: Some("+$1".to_string()), // Use captured number
                logo_asset_id: None,
                timeshift_minutes: None,
                sort_order: 1,
                created_at: chrono::Utc::now(),
            }],
            expression: Some("test expression".to_string()),
        };

        // Apply the rule
        let result = engine.apply_mapping_rules(
            vec![test_channel],
            vec![test_rule],
            HashMap::new(),
            Uuid::new_v4(),
            "http://localhost:8080",
        );

        // Verify the capture group was properly substituted
        assert!(result.is_ok());
        let mapped_channels = result.unwrap();
        assert_eq!(mapped_channels.len(), 1);

        let mapped_channel = &mapped_channels[0];
        assert_eq!(
            mapped_channel.mapped_tvg_shift,
            Some("+8".to_string()),
            "tvg_shift should be +8 (captured from 'Sports 8 HD')"
        );
    }

    #[test]
    fn test_regex_captures_substitution() {
        let mut captures = RegexCaptures::new();
        captures.captures.insert(1, "test1".to_string());
        captures.captures.insert(2, "test2".to_string());
        captures.captures.insert(10, "test10".to_string());

        // Test basic substitution
        let result = captures.substitute_captures("Value: $1");
        assert_eq!(result, "Value: test1");

        // Test multiple substitutions
        let result = captures.substitute_captures("$1 and $2");
        assert_eq!(result, "test1 and test2");

        // Test with higher numbered groups (should not interfere with single digits)
        let result = captures.substitute_captures("$10 vs $1");
        assert_eq!(result, "test10 vs test1");

        // Test with no substitutions needed
        let result = captures.substitute_captures("no substitutions here");
        assert_eq!(result, "no substitutions here");

        // Test unreplaced placeholders are removed
        let result = captures.substitute_captures("$1$3");
        assert_eq!(result, "test1");
    }

    #[test]
    fn test_negative_timeshift_rule() {
        use crate::models::data_mapping::*;
        use std::collections::HashMap;
        use uuid::Uuid;

        let mut engine = DataMappingEngine::new();

        // Test with a negative timeshift channel (space before -1)
        let test_channel = Channel {
            id: Uuid::new_v4(),
            source_id: Uuid::new_v4(),
            channel_name: "BBC One -1 HD".to_string(),
            tvg_id: Some("bbcone-1".to_string()),
            tvg_name: Some("BBC One -1".to_string()),
            tvg_logo: None,
            tvg_shift: None,
            group_title: Some("UK: Entertainment".to_string()),
            stream_url: "http://example.com/stream".to_string(),
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        };

        // Create the default timeshift rule
        let test_rule = DataMappingRuleWithDetails {
            rule: DataMappingRule {
                id: Uuid::new_v4(),
                name: "Default Timeshift Detection (Regex)".to_string(),
                description: Some("Test negative timeshift".to_string()),
                source_type: DataMappingSourceType::Stream,
                scope: DataMappingRuleScope::Individual,
                sort_order: 1,
                is_active: true,
                created_at: chrono::Utc::now(),
                updated_at: chrono::Utc::now(),
            },
            conditions: vec![DataMappingCondition {
                id: Uuid::new_v4(),
                rule_id: Uuid::new_v4(),
                field_name: "channel_name".to_string(),
                operator: FilterOperator::Matches,
                value: r".*(?:\+([0-9]+)|(-[0-9]+)).*".to_string(),
                logical_operator: Some(LogicalOperator::And),
                sort_order: 0,
                created_at: chrono::Utc::now(),
            }],
            actions: vec![DataMappingAction {
                id: Uuid::new_v4(),
                rule_id: Uuid::new_v4(),
                action_type: DataMappingActionType::SetValue,
                target_field: "tvg_shift".to_string(),
                value: Some("$1$2".to_string()),
                logo_asset_id: None,
                timeshift_minutes: None,
                sort_order: 0,
                created_at: chrono::Utc::now(),
            }],
            expression: Some("test expression".to_string()),
        };

        // Apply the rule
        let result = engine.test_mapping_rule(
            vec![test_channel.clone()],
            test_rule.conditions.clone(),
            test_rule.actions.clone(),
            HashMap::new(),
            "http://localhost:8080",
        );

        // Verify the rule was applied
        assert!(result.is_ok(), "Rule should apply successfully");
        let mapped_channels = result.unwrap();
        assert!(!mapped_channels.is_empty(), "Should have mapped channels");

        let mapped_channel = &mapped_channels[0];
        assert_eq!(
            mapped_channel.mapped_tvg_shift,
            Some("-1".to_string()),
            "tvg_shift should be '-1' (captured from '-1' in channel name)"
        );
    }

    #[test]
    fn test_default_timeshift_rule_with_real_data() {
        use crate::models::data_mapping::*;
        use std::collections::HashMap;
        use uuid::Uuid;

        let mut engine = DataMappingEngine::new();

        // Test with the exact channel from the user's example
        let test_channel = Channel {
            id: Uuid::new_v4(),
            source_id: Uuid::new_v4(),
            channel_name: "UK: ITV 4+1 ◉ • strong8k • ITV4.uk".to_string(),
            tvg_id: Some("itv4plus1".to_string()),
            tvg_name: Some("ITV 4+1".to_string()),
            tvg_logo: None,
            tvg_shift: None, // Start with no tvg_shift
            group_title: Some("UK: Entertainment".to_string()),
            stream_url: "http://example.com/stream".to_string(),
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        };

        // Create the exact default rule from the migration
        let test_rule = DataMappingRuleWithDetails {
            rule: DataMappingRule {
                id: Uuid::new_v4(),
                name: "Default Timeshift Detection (Regex)".to_string(),
                description: Some("Automatically detects timeshift channels (+1, +24, etc.) and sets tvg-shift field using regex capture groups.".to_string()),
                source_type: DataMappingSourceType::Stream,
                scope: DataMappingRuleScope::Individual,
                sort_order: 1,
                is_active: true,
                created_at: chrono::Utc::now(),
                updated_at: chrono::Utc::now(),
            },
            conditions: vec![DataMappingCondition {
                id: Uuid::new_v4(),
                rule_id: Uuid::new_v4(),
                field_name: "channel_name".to_string(),
                operator: FilterOperator::Matches,
                value: r".*(?:\+([0-9]+)|(-[0-9]+)).*".to_string(),
                logical_operator: Some(LogicalOperator::And),
                sort_order: 0,
                created_at: chrono::Utc::now(),
            }],
            actions: vec![DataMappingAction {
                id: Uuid::new_v4(),
                rule_id: Uuid::new_v4(),
                action_type: DataMappingActionType::SetValue,
                target_field: "tvg_shift".to_string(),
                value: Some("$1$2".to_string()), // Exact template from migration
                logo_asset_id: None,
                timeshift_minutes: None,
                sort_order: 0,
                created_at: chrono::Utc::now(),
            }],
            expression: Some("test expression".to_string()),
        };

        // Apply the rule using the test function
        let result = engine.test_mapping_rule(
            vec![test_channel.clone()],
            test_rule.conditions.clone(),
            test_rule.actions.clone(),
            HashMap::new(),
            "http://localhost:8080",
        );

        println!("Rule application result: {:?}", result);

        // Verify the rule was applied
        assert!(result.is_ok(), "Rule should apply successfully");
        let mapped_channels = result.unwrap();

        println!("Mapped channels count: {}", mapped_channels.len());

        if !mapped_channels.is_empty() {
            let mapped_channel = &mapped_channels[0];
            println!("Original tvg_shift: {:?}", test_channel.tvg_shift);
            println!("Mapped tvg_shift: {:?}", mapped_channel.mapped_tvg_shift);

            assert_eq!(
                mapped_channel.mapped_tvg_shift,
                Some("1".to_string()),
                "tvg_shift should be '1' (captured from '+1' in channel name)"
            );
        } else {
            panic!(
                "Rule should have matched the channel name 'UK: ITV 4+1 ◉ • strong8k • ITV4.uk'"
            );
        }
    }

    #[test]
    fn test_dual_condition_timeshift_rule() {
        use crate::models::data_mapping::*;
        use std::collections::HashMap;
        use uuid::Uuid;

        let mut engine = DataMappingEngine::new();

        // Test cases with the dual-condition approach
        let test_cases = vec![
            // Should match - legitimate timeshift channels
            ("UK: ITV 4+1 ◉ • strong8k • ITV4.uk", true, "1"),
            ("UK: E4 +1 ◉", true, "1"),
            ("CNN +24", true, "24"),
            ("BBC One +1", true, "1"),
            ("ESPN +2 Sports", true, "2"),
            ("Channel 5 -1", true, "-1"),
            ("Discovery -2 Hours", true, "-2"),
            // Should NOT match - channels with timestamp patterns
            (
                "start:2025-06-28 03:00:00 stop:2025-06-28 05:00:00",
                false,
                "",
            ),
            ("2025-06-28T15:30:00+01:00", false, ""),
            ("Episode aired 2024-12-31 23:59:59", false, ""),
            ("Time: 14:30:00+05:30", false, ""),
            ("Date: 2025-01-01+0000", false, ""),
            // Should NOT match - no timeshift indicators
            ("Normal Channel", false, ""),
            ("BBC One HD", false, ""),
        ];

        for (channel_name, should_match, expected_shift) in test_cases {
            println!(
                "Testing channel: '{}' (length: {})",
                channel_name,
                channel_name.len()
            );
            let test_channel = Channel {
                id: Uuid::new_v4(),
                source_id: Uuid::new_v4(),
                channel_name: channel_name.to_string(),
                tvg_id: Some("test".to_string()),
                tvg_name: Some("Test".to_string()),
                tvg_logo: None,
                tvg_shift: None,
                group_title: Some("Test".to_string()),
                stream_url: "http://example.com/stream".to_string(),
                created_at: chrono::Utc::now(),
                updated_at: chrono::Utc::now(),
            };

            // Create rule with dual conditions
            let test_rule = DataMappingRuleWithDetails {
                rule: DataMappingRule {
                    id: Uuid::new_v4(),
                    name: "Dual Condition Timeshift Test".to_string(),
                    description: Some("Test with exclusion".to_string()),
                    source_type: DataMappingSourceType::Stream,
                    scope: DataMappingRuleScope::Individual,
                    sort_order: 1,
                    is_active: true,
                    created_at: chrono::Utc::now(),
                    updated_at: chrono::Utc::now(),
                },
                conditions: vec![
                    // Condition 1: Match timeshift pattern
                    DataMappingCondition {
                        id: Uuid::new_v4(),
                        rule_id: Uuid::new_v4(),
                        field_name: "channel_name".to_string(),
                        operator: FilterOperator::Matches,
                        value: r".*(?:\+([0-9]+)|(-[0-9]+)).*".to_string(),
                        logical_operator: Some(LogicalOperator::And),
                        sort_order: 0,
                        created_at: chrono::Utc::now(),
                    },
                    // Condition 2: Exclude timestamp patterns
                    DataMappingCondition {
                        id: Uuid::new_v4(),
                        rule_id: Uuid::new_v4(),
                        field_name: "channel_name".to_string(),
                        operator: FilterOperator::NotMatches,
                        value: r".*(?:start:|stop:|\d{4}-\d{2}-\d{2}|\d{2}:\d{2}:\d{2}).*"
                            .to_string(),
                        logical_operator: Some(LogicalOperator::And),
                        sort_order: 1,
                        created_at: chrono::Utc::now(),
                    },
                ],
                actions: vec![DataMappingAction {
                    id: Uuid::new_v4(),
                    rule_id: Uuid::new_v4(),
                    action_type: DataMappingActionType::SetValue,
                    target_field: "tvg_shift".to_string(),
                    value: Some("$1$2".to_string()),
                    logo_asset_id: None,
                    timeshift_minutes: None,
                    sort_order: 0,
                    created_at: chrono::Utc::now(),
                }],
                expression: Some("test expression".to_string()),
            };

            // Apply the rule to the channels
            let result = engine.apply_mapping_rules(
                vec![test_channel.clone()],
                vec![test_rule.clone()],
                HashMap::new(),
                Uuid::new_v4(),
                "http://localhost:8080",
            );

            println!("Result for '{}': {:?}", channel_name, result);

            if should_match {
                assert!(result.is_ok(), "Rule should apply for: {}", channel_name);
                let mapped_channels = result.unwrap();
                println!("Mapped channels count: {}", mapped_channels.len());
                assert!(
                    !mapped_channels.is_empty(),
                    "Should have mapped channels for: {}",
                    channel_name
                );

                let mapped_channel = &mapped_channels[0];
                println!("Mapped tvg_shift: {:?}", mapped_channel.mapped_tvg_shift);
                assert_eq!(
                    mapped_channel.mapped_tvg_shift,
                    Some(expected_shift.to_string()),
                    "Wrong tvg_shift for '{}': expected '{}', got {:?}",
                    channel_name,
                    expected_shift,
                    mapped_channel.mapped_tvg_shift
                );
            } else {
                assert!(
                    result.is_ok(),
                    "Rule evaluation should not error for: {}",
                    channel_name
                );
                let mapped_channels = result.unwrap();
                println!(
                    "Should not match - mapped channels count: {}",
                    mapped_channels.len()
                );
                assert!(
                    mapped_channels.is_empty(),
                    "Should not match: {}",
                    channel_name
                );
            }
        }
    }

    #[test]
    fn test_rule_stats_logging() {
        use crate::models::data_mapping::*;
        use std::collections::HashMap;
        use uuid::Uuid;

        // Initialize tracing for the test to see logs
        let _ = tracing_subscriber::fmt::try_init();

        let mut engine = DataMappingEngine::new();

        // Create multiple test channels to demonstrate rule performance
        let test_channels = vec![
            Channel {
                id: Uuid::new_v4(),
                source_id: Uuid::new_v4(),
                channel_name: "Sky Sports F1 +1 HD".to_string(),
                tvg_id: Some("skysportsf1plus1".to_string()),
                tvg_name: Some("Sky Sports F1 +1".to_string()),
                tvg_logo: None,
                tvg_shift: None,
                group_title: Some("Sports".to_string()),
                stream_url: "http://example.com/stream1".to_string(),
                created_at: chrono::Utc::now(),
                updated_at: chrono::Utc::now(),
            },
            Channel {
                id: Uuid::new_v4(),
                source_id: Uuid::new_v4(),
                channel_name: "BBC One +24 HD".to_string(),
                tvg_id: Some("bbconeplus24".to_string()),
                tvg_name: Some("BBC One +24".to_string()),
                tvg_logo: None,
                tvg_shift: None,
                group_title: Some("Entertainment".to_string()),
                stream_url: "http://example.com/stream2".to_string(),
                created_at: chrono::Utc::now(),
                updated_at: chrono::Utc::now(),
            },
            Channel {
                id: Uuid::new_v4(),
                source_id: Uuid::new_v4(),
                channel_name: "Regular Channel HD".to_string(),
                tvg_id: Some("regular".to_string()),
                tvg_name: Some("Regular Channel".to_string()),
                tvg_logo: None,
                tvg_shift: None,
                group_title: Some("General".to_string()),
                stream_url: "http://example.com/stream3".to_string(),
                created_at: chrono::Utc::now(),
                updated_at: chrono::Utc::now(),
            },
        ];

        // Create the timeshift rule with tvg_id requirement
        let test_rule = DataMappingRuleWithDetails {
            rule: DataMappingRule {
                id: Uuid::new_v4(),
                name: "Timeshift Detection Test".to_string(),
                description: Some("Test rule for demonstrating stats".to_string()),
                source_type: DataMappingSourceType::Stream,
                scope: DataMappingRuleScope::Individual,
                sort_order: 1,
                is_active: true,
                created_at: chrono::Utc::now(),
                updated_at: chrono::Utc::now(),
            },
            conditions: vec![
                DataMappingCondition {
                    id: Uuid::new_v4(),
                    rule_id: Uuid::new_v4(),
                    field_name: "channel_name".to_string(),
                    operator: FilterOperator::Matches,
                    value: r".*(?:\+([0-9]+)|(-[0-9]+)).*".to_string(),
                    logical_operator: Some(LogicalOperator::And),
                    sort_order: 0,
                    created_at: chrono::Utc::now(),
                },
                DataMappingCondition {
                    id: Uuid::new_v4(),
                    rule_id: Uuid::new_v4(),
                    field_name: "tvg_id".to_string(),
                    operator: FilterOperator::Matches,
                    value: r"^.+$".to_string(),
                    logical_operator: Some(LogicalOperator::And),
                    sort_order: 1,
                    created_at: chrono::Utc::now(),
                },
            ],
            actions: vec![DataMappingAction {
                id: Uuid::new_v4(),
                rule_id: Uuid::new_v4(),
                action_type: DataMappingActionType::SetValue,
                target_field: "tvg_shift".to_string(),
                value: Some("$1$2".to_string()),
                logo_asset_id: None,
                timeshift_minutes: None,
                sort_order: 1,
                created_at: chrono::Utc::now(),
            }],
            expression: Some("test expression".to_string()),
        };

        // Apply the rule - this should trigger rule stats logging
        println!("Running data mapping with rule stats...");
        let result = engine.apply_mapping_rules(
            test_channels,
            vec![test_rule],
            HashMap::new(),
            Uuid::new_v4(),
            "http://localhost:8080",
        );

        assert!(result.is_ok());
        let mapped_channels = result.unwrap();

        // Should have 3 channels, 2 with tvg_shift applied (those with +N pattern AND tvg_id)
        assert_eq!(mapped_channels.len(), 3);

        // Check that channels with timeshift patterns and tvg_id got tvg_shift applied
        // The regex captures the number part only, so "+1" becomes "1" and "+24" becomes "24"
        let first_channel = mapped_channels
            .iter()
            .find(|c| c.original.channel_name.contains("+1"))
            .unwrap();
        assert_eq!(first_channel.mapped_tvg_shift, Some("1".to_string()));

        let second_channel = mapped_channels
            .iter()
            .find(|c| c.original.channel_name.contains("+24"))
            .unwrap();
        assert_eq!(second_channel.mapped_tvg_shift, Some("24".to_string()));

        // Regular channel should not have tvg_shift applied
        let regular_channel = mapped_channels
            .iter()
            .find(|c| c.original.channel_name.contains("Regular"))
            .unwrap();
        assert_eq!(regular_channel.mapped_tvg_shift, None);

        println!("Test completed successfully - check logs above for rule performance stats!");
    }

    #[test]
    fn test_preview_with_apply_mapping_rules() {
        use crate::models::data_mapping::*;
        use std::collections::HashMap;
        use uuid::Uuid;

        // Initialize tracing for the test to see logs
        let _ = tracing_subscriber::fmt::try_init();

        let mut engine = DataMappingEngine::new();

        // Create test channels from multiple sources (simulating preview scenario)
        let source1_id = Uuid::new_v4();
        let source2_id = Uuid::new_v4();

        let test_channels = vec![
            // Source 1 channels
            Channel {
                id: Uuid::new_v4(),
                source_id: source1_id,
                channel_name: "Sky Sports F1 +1 HD".to_string(),
                tvg_id: Some("skysportsf1plus1".to_string()),
                tvg_name: Some("Sky Sports F1 +1".to_string()),
                tvg_logo: None,
                tvg_shift: None,
                group_title: Some("Sports".to_string()),
                stream_url: "http://example.com/stream1".to_string(),
                created_at: chrono::Utc::now(),
                updated_at: chrono::Utc::now(),
            },
            Channel {
                id: Uuid::new_v4(),
                source_id: source1_id,
                channel_name: "BBC One +24 HD".to_string(),
                tvg_id: Some("bbconeplus24".to_string()),
                tvg_name: Some("BBC One +24".to_string()),
                tvg_logo: None,
                tvg_shift: None,
                group_title: Some("Entertainment".to_string()),
                stream_url: "http://example.com/stream2".to_string(),
                created_at: chrono::Utc::now(),
                updated_at: chrono::Utc::now(),
            },
            // Source 2 channels
            Channel {
                id: Uuid::new_v4(),
                source_id: source2_id,
                channel_name: "ITV +1 HD".to_string(),
                tvg_id: Some("itvplus1".to_string()),
                tvg_name: Some("ITV +1".to_string()),
                tvg_logo: None,
                tvg_shift: None,
                group_title: Some("Entertainment".to_string()),
                stream_url: "http://example.com/stream3".to_string(),
                created_at: chrono::Utc::now(),
                updated_at: chrono::Utc::now(),
            },
            Channel {
                id: Uuid::new_v4(),
                source_id: source2_id,
                channel_name: "Regular Channel HD".to_string(),
                tvg_id: Some("regular".to_string()),
                tvg_name: Some("Regular Channel".to_string()),
                tvg_logo: None,
                tvg_shift: None,
                group_title: Some("General".to_string()),
                stream_url: "http://example.com/stream4".to_string(),
                created_at: chrono::Utc::now(),
                updated_at: chrono::Utc::now(),
            },
        ];

        // Create the timeshift rule (same as database rule)
        let timeshift_rule = DataMappingRuleWithDetails {
            rule: DataMappingRule {
                id: Uuid::new_v4(),
                name: "Preview Timeshift Detection".to_string(),
                description: Some("Demonstrates preview with apply_mapping_rules".to_string()),
                source_type: DataMappingSourceType::Stream,
                scope: DataMappingRuleScope::Individual,
                sort_order: 1,
                is_active: true,
                created_at: chrono::Utc::now(),
                updated_at: chrono::Utc::now(),
            },
            conditions: vec![
                DataMappingCondition {
                    id: Uuid::new_v4(),
                    rule_id: Uuid::new_v4(),
                    field_name: "channel_name".to_string(),
                    operator: FilterOperator::Matches,
                    value: r".*(?:\+([0-9]+)|(-[0-9]+)).*".to_string(),
                    logical_operator: Some(LogicalOperator::And),
                    sort_order: 0,
                    created_at: chrono::Utc::now(),
                },
                DataMappingCondition {
                    id: Uuid::new_v4(),
                    rule_id: Uuid::new_v4(),
                    field_name: "channel_name".to_string(),
                    operator: FilterOperator::NotMatches,
                    value: r".*(?:start:|stop:|\d{4}-\d{2}-\d{2}|\d{2}:\d{2}:\d{2}).*".to_string(),
                    logical_operator: Some(LogicalOperator::And),
                    sort_order: 1,
                    created_at: chrono::Utc::now(),
                },
                DataMappingCondition {
                    id: Uuid::new_v4(),
                    rule_id: Uuid::new_v4(),
                    field_name: "tvg_id".to_string(),
                    operator: FilterOperator::Matches,
                    value: r"^.+$".to_string(),
                    logical_operator: Some(LogicalOperator::And),
                    sort_order: 2,
                    created_at: chrono::Utc::now(),
                },
            ],
            actions: vec![DataMappingAction {
                id: Uuid::new_v4(),
                rule_id: Uuid::new_v4(),
                action_type: DataMappingActionType::SetValue,
                target_field: "tvg_shift".to_string(),
                value: Some("$1$2".to_string()),
                logo_asset_id: None,
                timeshift_minutes: None,
                sort_order: 1,
                created_at: chrono::Utc::now(),
            }],
            expression: Some("test expression".to_string()),
        };

        println!("=== PREVIEW DEMO: Using apply_mapping_rules ===");
        println!("This shows how preview should work with proper performance stats");

        // Apply mapping rules - this gives us full performance stats like production
        let result = engine.apply_mapping_rules(
            test_channels,
            vec![timeshift_rule],
            HashMap::new(),
            source1_id,
            "http://localhost:8080",
        );

        assert!(result.is_ok());
        let mapped_channels = result.unwrap();

        // Analyze results for preview
        let mut affected_channels = 0;
        let mut preview_results = Vec::new();

        for mapped_channel in &mapped_channels {
            if !mapped_channel.applied_rules.is_empty() {
                affected_channels += 1;

                // This is what preview would show
                let preview_entry = format!(
                    "Channel: '{}' | tvg_id: {:?} | tvg_shift: {:?} -> {:?}",
                    mapped_channel.original.channel_name,
                    mapped_channel.original.tvg_id,
                    mapped_channel.original.tvg_shift,
                    mapped_channel.mapped_tvg_shift
                );
                preview_results.push(preview_entry);
                println!("  {}", preview_results.last().unwrap());
            }
        }

        println!("=== PREVIEW SUMMARY ===");
        println!("Total channels: {}", mapped_channels.len());
        println!("Affected channels: {}", affected_channels);
        println!(
            "Rules with tvg_id requirement working: {}",
            mapped_channels
                .iter()
                .all(|c| c.applied_rules.is_empty() || c.original.tvg_id.is_some())
        );

        // Verify our tvg_id requirement is working
        assert_eq!(affected_channels, 3); // 3 channels have +N pattern AND tvg_id
        assert!(mapped_channels
            .iter()
            .find(|c| c.original.channel_name.contains("Regular"))
            .unwrap()
            .applied_rules
            .is_empty());

        println!("SUCCESS: Preview with apply_mapping_rules provides full performance stats!");
    }

    #[test]
    fn test_timeshift_regex_with_signs() {
        use crate::models::data_mapping::*;

        let mut engine = DataMappingEngine::new();

        let test_channels = vec![
            Channel {
                id: Uuid::new_v4(),
                source_id: Uuid::new_v4(),
                channel_name: "BE: BE 1 +1 4K".to_string(),
                tvg_id: Some("be1plus1.be".to_string()),
                tvg_name: None,
                tvg_logo: None,
                tvg_shift: None,
                group_title: None,
                stream_url: "http://example.com/stream1".to_string(),
                created_at: chrono::Utc::now(),
                updated_at: chrono::Utc::now(),
            },
            Channel {
                id: Uuid::new_v4(),
                source_id: Uuid::new_v4(),
                channel_name: "UK: ITV +3".to_string(),
                tvg_id: Some("itvplus3.uk".to_string()),
                tvg_name: None,
                tvg_logo: None,
                tvg_shift: None,
                group_title: None,
                stream_url: "http://example.com/stream2".to_string(),
                created_at: chrono::Utc::now(),
                updated_at: chrono::Utc::now(),
            },
            Channel {
                id: Uuid::new_v4(),
                source_id: Uuid::new_v4(),
                channel_name: "DE: ARD -2".to_string(),
                tvg_id: Some("ardminus2.de".to_string()),
                tvg_name: None,
                tvg_logo: None,
                tvg_shift: None,
                group_title: None,
                stream_url: "http://example.com/stream3".to_string(),
                created_at: chrono::Utc::now(),
                updated_at: chrono::Utc::now(),
            },
            Channel {
                id: Uuid::new_v4(),
                source_id: Uuid::new_v4(),
                channel_name: "DE: DATELINE 24-7 ᴴᴰ ᵃᵐᶻ".to_string(),
                tvg_id: Some("dateline.de".to_string()),
                tvg_name: None,
                tvg_logo: None,
                tvg_shift: None,
                group_title: None,
                stream_url: "http://example.com/stream4".to_string(),
                created_at: chrono::Utc::now(),
                updated_at: chrono::Utc::now(),
            },
            Channel {
                id: Uuid::new_v4(),
                source_id: Uuid::new_v4(),
                channel_name: "US: CNN +1h".to_string(),
                tvg_id: Some("cnnplus1h.us".to_string()),
                tvg_name: None,
                tvg_logo: None,
                tvg_shift: None,
                group_title: None,
                stream_url: "http://example.com/stream5".to_string(),
                created_at: chrono::Utc::now(),
                updated_at: chrono::Utc::now(),
            },
            Channel {
                id: Uuid::new_v4(),
                source_id: Uuid::new_v4(),
                channel_name: "FR: TF1-2".to_string(),
                tvg_id: Some("tf1dash2.fr".to_string()),
                tvg_name: None,
                tvg_logo: None,
                tvg_shift: None,
                group_title: None,
                stream_url: "http://example.com/stream6".to_string(),
                created_at: chrono::Utc::now(),
                updated_at: chrono::Utc::now(),
            },
        ];

        // Create timeshift rule with updated regex that requires spaces before +/-
        let timeshift_rule = DataMappingRuleWithDetails {
            rule: DataMappingRule {
                id: Uuid::new_v4(),
                name: "Fixed Timeshift Detection".to_string(),
                description: Some("Timeshift detection that requires spaces before +/- to avoid false matches like '24-7'".to_string()),
                source_type: DataMappingSourceType::Stream,
                scope: DataMappingRuleScope::Individual,
                sort_order: 1,
                is_active: true,
                created_at: chrono::Utc::now(),
                updated_at: chrono::Utc::now(),
            },
            conditions: vec![
                DataMappingCondition {
                    id: Uuid::new_v4(),
                    rule_id: Uuid::new_v4(),
                    field_name: "channel_name".to_string(),
                    operator: FilterOperator::Matches,
                    value: r".*(?:(?:\s|^)\+([0-9]+)h?(?:\s|$)|(?:\s|^)(-[0-9]+)h?(?:\s|$)).*"
                        .to_string(),
                    logical_operator: None,
                    sort_order: 0,
                    created_at: chrono::Utc::now(),
                },
                DataMappingCondition {
                    id: Uuid::new_v4(),
                    rule_id: Uuid::new_v4(),
                    field_name: "tvg_id".to_string(),
                    operator: FilterOperator::Matches,
                    value: r"^.+$".to_string(),
                    logical_operator: Some(LogicalOperator::And),
                    sort_order: 1,
                    created_at: chrono::Utc::now(),
                },
            ],
            actions: vec![DataMappingAction {
                id: Uuid::new_v4(),
                rule_id: Uuid::new_v4(),
                action_type: DataMappingActionType::SetValue,
                target_field: "tvg_shift".to_string(),
                value: Some("$1$2".to_string()),
                logo_asset_id: None,
                timeshift_minutes: None,
                sort_order: 0,
                created_at: chrono::Utc::now(),
            }],
            expression: Some("test expression".to_string()),
        };

        println!("=== TESTING FIXED TIMESHIFT REGEX ===");

        let logo_assets = std::collections::HashMap::new();
        let source_id = Uuid::new_v4();
        let result = engine
            .apply_mapping_rules(
                test_channels,
                vec![timeshift_rule],
                logo_assets,
                source_id,
                "http://localhost:8080",
            )
            .unwrap();

        println!("Results:");
        for mapped in &result {
            if !mapped.applied_rules.is_empty() {
                println!(
                    "✅ MATCHED: '{}' -> tvg_shift: {:?}",
                    mapped.original.channel_name, mapped.mapped_tvg_shift
                );
            } else {
                println!("❌ NO MATCH: '{}'", mapped.original.channel_name);
            }
        }

        // Verify expected results - should match 4 channels with proper timeshift indicators
        let matched: Vec<_> = result
            .iter()
            .filter(|m| !m.applied_rules.is_empty())
            .collect();
        assert_eq!(
            matched.len(),
            4,
            "Should match 4 channels with valid timeshift patterns"
        );

        // Check BE: BE 1 +1 4K - should match +1
        let be_result = matched
            .iter()
            .find(|m| m.original.channel_name.contains("BE 1 +1"));
        assert!(be_result.is_some(), "Should find BE 1 +1 channel");
        assert_eq!(be_result.unwrap().mapped_tvg_shift, Some("1".to_string()));

        // Check UK: ITV +3 - should match +3
        let itv_result = matched
            .iter()
            .find(|m| m.original.channel_name.contains("ITV +3"));
        assert!(itv_result.is_some(), "Should find ITV +3 channel");
        assert_eq!(itv_result.unwrap().mapped_tvg_shift, Some("3".to_string()));

        // Check DE: ARD -2 - should match -2
        let ard_result = matched
            .iter()
            .find(|m| m.original.channel_name.contains("ARD -2"));
        assert!(ard_result.is_some(), "Should find ARD -2 channel");
        assert_eq!(ard_result.unwrap().mapped_tvg_shift, Some("-2".to_string()));

        // Check US: CNN +1h - should match +1h
        let cnn_result = matched
            .iter()
            .find(|m| m.original.channel_name.contains("CNN +1h"));
        assert!(cnn_result.is_some(), "Should find CNN +1h channel");
        assert_eq!(cnn_result.unwrap().mapped_tvg_shift, Some("1".to_string()));

        // Verify channels that should NOT match
        let not_matched: Vec<_> = result
            .iter()
            .filter(|m| m.applied_rules.is_empty())
            .collect();
        assert_eq!(
            not_matched.len(),
            2,
            "Should have 2 channels that don't match"
        );

        // Check DATELINE 24-7 should NOT match (false positive avoided)
        let dateline_result = not_matched
            .iter()
            .find(|m| m.original.channel_name.contains("24-7"));
        assert!(
            dateline_result.is_some(),
            "DATELINE 24-7 should NOT match timeshift pattern"
        );
        assert!(
            dateline_result.unwrap().mapped_tvg_shift.is_none(),
            "DATELINE 24-7 should have no tvg_shift"
        );

        // Check TF1-2 should NOT match (dash without space)
        let tf1_result = not_matched
            .iter()
            .find(|m| m.original.channel_name.contains("TF1-2"));
        assert!(
            tf1_result.is_some(),
            "TF1-2 should NOT match timeshift pattern"
        );
        assert!(
            tf1_result.unwrap().mapped_tvg_shift.is_none(),
            "TF1-2 should have no tvg_shift"
        );
    }

    #[test]
    fn test_debug_rule_matching_issue() {
        use crate::models::data_mapping::*;
        use std::collections::HashMap;

        let mut engine = DataMappingEngine::new();

        // Create test channels that should match timeshift pattern
        let test_channels = vec![
            Channel {
                id: Uuid::new_v4(),
                source_id: Uuid::new_v4(),
                channel_name: "UK: ITV 1 +1 ◉".to_string(),
                tvg_id: Some("ITV1.uk".to_string()),
                tvg_name: None,
                tvg_logo: None,
                tvg_shift: None,
                group_title: None,
                stream_url: "http://example.com/stream1".to_string(),
                created_at: chrono::Utc::now(),
                updated_at: chrono::Utc::now(),
            },
            Channel {
                id: Uuid::new_v4(),
                source_id: Uuid::new_v4(),
                channel_name: "UK: E4 +1 ◉".to_string(),
                tvg_id: Some("E4.uk".to_string()),
                tvg_name: None,
                tvg_logo: None,
                tvg_shift: None,
                group_title: None,
                stream_url: "http://example.com/stream2".to_string(),
                created_at: chrono::Utc::now(),
                updated_at: chrono::Utc::now(),
            },
            Channel {
                id: Uuid::new_v4(),
                source_id: Uuid::new_v4(),
                channel_name: "UK: CHANNEL 5 +1 ◉".to_string(),
                tvg_id: Some("Channel5.uk".to_string()),
                tvg_name: None,
                tvg_logo: None,
                tvg_shift: None,
                group_title: None,
                stream_url: "http://example.com/stream3".to_string(),
                created_at: chrono::Utc::now(),
                updated_at: chrono::Utc::now(),
            },
            Channel {
                id: Uuid::new_v4(),
                source_id: Uuid::new_v4(),
                channel_name: "US: Regular Channel".to_string(),
                tvg_id: Some("regular.us".to_string()),
                tvg_name: None,
                tvg_logo: None,
                tvg_shift: None,
                group_title: None,
                stream_url: "http://example.com/stream4".to_string(),
                created_at: chrono::Utc::now(),
                updated_at: chrono::Utc::now(),
            },
        ];

        // Create the exact timeshift rule from database
        let timeshift_rule = DataMappingRuleWithDetails {
            rule: DataMappingRule {
                id: Uuid::parse_str("550e8400-e29b-41d4-a716-446655440001").unwrap(),
                name: "Default Timeshift Detection (Regex)".to_string(),
                description: Some("Automatically detects timeshift channels (+1, +24, etc.) and sets tvg-shift field using regex capture groups.".to_string()),
                source_type: DataMappingSourceType::Stream,
                scope: DataMappingRuleScope::Individual,
                sort_order: 1,
                is_active: true,
                created_at: chrono::Utc::now(),
                updated_at: chrono::Utc::now(),
            },
            conditions: vec![
                DataMappingCondition {
                    id: Uuid::new_v4(),
                    rule_id: Uuid::parse_str("550e8400-e29b-41d4-a716-446655440001").unwrap(),
                    field_name: "channel_name".to_string(),
                    operator: FilterOperator::Matches,
                    value: r".*(?:\+([0-9]+)|(-[0-9]+)).*".to_string(),
                    logical_operator: None,
                    sort_order: 0,
                    created_at: chrono::Utc::now(),
                },
                DataMappingCondition {
                    id: Uuid::new_v4(),
                    rule_id: Uuid::parse_str("550e8400-e29b-41d4-a716-446655440001").unwrap(),
                    field_name: "channel_name".to_string(),
                    operator: FilterOperator::NotMatches,
                    value: r".*(?:start:|stop:|\d{4}-\d{2}-\d{2}|\d{2}:\d{2}:\d{2}).*".to_string(),
                    logical_operator: Some(LogicalOperator::And),
                    sort_order: 1,
                    created_at: chrono::Utc::now(),
                },
                DataMappingCondition {
                    id: Uuid::new_v4(),
                    rule_id: Uuid::parse_str("550e8400-e29b-41d4-a716-446655440001").unwrap(),
                    field_name: "tvg_id".to_string(),
                    operator: FilterOperator::Matches,
                    value: r"^.+$".to_string(),
                    logical_operator: Some(LogicalOperator::And),
                    sort_order: 2,
                    created_at: chrono::Utc::now(),
                },
            ],
            actions: vec![DataMappingAction {
                id: Uuid::new_v4(),
                rule_id: Uuid::parse_str("550e8400-e29b-41d4-a716-446655440001").unwrap(),
                action_type: DataMappingActionType::SetValue,
                target_field: "tvg_shift".to_string(),
                value: Some("$1$2".to_string()),
                logo_asset_id: None,
                timeshift_minutes: None,
                sort_order: 0,
                created_at: chrono::Utc::now(),
            }],
            expression: Some("test expression".to_string()),
        };

        println!("=== DEBUG: Rule Matching Investigation ===");
        println!(
            "Testing {} channels against timeshift rule",
            test_channels.len()
        );

        // Test individual condition evaluation
        for (i, channel) in test_channels.iter().enumerate() {
            println!("\n--- Channel {}: {} ---", i + 1, channel.channel_name);
            println!("tvg_id: {:?}", channel.tvg_id);

            for (j, condition) in timeshift_rule.conditions.iter().enumerate() {
                println!(
                    "  Condition {}: {} {:?} '{}'",
                    j + 1,
                    condition.field_name,
                    condition.operator,
                    condition.value
                );

                let (result, captures) = engine
                    .evaluate_rule_conditions(channel, &[condition.clone()])
                    .unwrap();
                println!(
                    "    Result: {} (captures: {})",
                    result,
                    captures.captures.len()
                );

                if !captures.captures.is_empty() {
                    for (group, value) in &captures.captures {
                        println!("      Capture group {}: '{}'", group, value);
                    }
                }
            }

            // Test all conditions together
            let (all_match, all_captures) = engine
                .evaluate_rule_conditions(channel, &timeshift_rule.conditions)
                .unwrap();
            println!(
                "  ALL CONDITIONS: {} (captures: {})",
                all_match,
                all_captures.captures.len()
            );
        }

        // Apply the rule to all channels
        let logo_assets = HashMap::new();
        let source_id = Uuid::new_v4();
        let result = engine
            .apply_mapping_rules(
                test_channels,
                vec![timeshift_rule],
                logo_assets,
                source_id,
                "http://localhost:8080",
            )
            .unwrap();

        println!("\n=== RESULTS ===");
        println!("Total mapped channels: {}", result.len());

        let mut matched_count = 0;
        for mapped in &result {
            if !mapped.applied_rules.is_empty() {
                matched_count += 1;
                println!(
                    "MATCHED: '{}' -> tvg_shift: {:?}",
                    mapped.original.channel_name, mapped.mapped_tvg_shift
                );
            } else {
                println!("NO MATCH: '{}'", mapped.original.channel_name);
            }
        }

        println!("Channels with rules applied: {}", matched_count);

        // This should match 3 out of 4 channels (all except "US: Regular Channel")
        assert!(
            matched_count >= 3,
            "Expected at least 3 matches, got {}",
            matched_count
        );
    }
}
