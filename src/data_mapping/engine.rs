use crate::models::{
    data_mapping::{
        DataMappingAction, DataMappingActionType, DataMappingCondition, DataMappingRule,
        DataMappingRuleWithDetails, DataMappingSourceType, DataMappingRuleScope, MappedChannel, MappedEpgChannel,
        MappedEpgProgram, EpgDataMappingResult, DataMappingFieldInfo,
    },
    logo_asset::LogoAsset,
    Channel, EpgChannel, EpgProgram, FilterOperator, LogicalOperator,
};
use crate::config::ChannelSimilarityConfig;
use regex::{Regex, RegexBuilder};
use std::collections::HashMap;
use tracing::{debug, info, warn};
use std::time::Instant;
use uuid::Uuid;

/// Default special characters used for regex precheck filtering
/// These characters are considered significant enough to use as first-pass filters
const DEFAULT_PRECHECK_SPECIAL_CHARS: &str = "+-@#$%&*=<>!~`€£{}[]";

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
        let mut result = value.to_string();
        for (group_index, captured_value) in &self.captures {
            let placeholder = format!("${}", group_index);
            result = result.replace(&placeholder, captured_value);
        }
        result
    }
}

pub struct DataMappingEngine {
    regex_cache: HashMap<String, Regex>,
    similarity_config: ChannelSimilarityConfig,
    rule_stats: HashMap<String, (u128, usize)>, // (total_time_micros, channels_processed)
    precheck_special_chars: String,
    minimum_literal_length: usize,
}

impl DataMappingEngine {
    pub fn new() -> Self {
        Self {
            regex_cache: HashMap::new(),
            similarity_config: ChannelSimilarityConfig::default(),
            rule_stats: HashMap::new(),
            precheck_special_chars: DEFAULT_PRECHECK_SPECIAL_CHARS.to_string(),
            minimum_literal_length: 2,
        }
    }

    pub fn new_with_similarity_config(similarity_config: ChannelSimilarityConfig) -> Self {
        Self {
            regex_cache: HashMap::new(),
            rule_stats: HashMap::new(),
            similarity_config,
            precheck_special_chars: DEFAULT_PRECHECK_SPECIAL_CHARS.to_string(),
            minimum_literal_length: 2,
        }
    }

    pub fn new_with_config(
        similarity_config: ChannelSimilarityConfig,
        data_mapping_config: Option<crate::config::DataMappingConfig>
    ) -> Self {
        let (precheck_special_chars, minimum_literal_length) = if let Some(config) = data_mapping_config {
            (
                config.precheck_special_chars.unwrap_or_else(|| DEFAULT_PRECHECK_SPECIAL_CHARS.to_string()),
                config.minimum_literal_length.unwrap_or(2)
            )
        } else {
            (DEFAULT_PRECHECK_SPECIAL_CHARS.to_string(), 2)
        };

        Self {
            regex_cache: HashMap::new(),
            rule_stats: HashMap::new(),
            similarity_config,
            precheck_special_chars,
            minimum_literal_length,
        }
    }

    pub fn new_with_custom_precheck(
        similarity_config: ChannelSimilarityConfig,
        precheck_special_chars: Option<String>,
        minimum_literal_length: Option<usize>
    ) -> Self {
        Self {
            regex_cache: HashMap::new(),
            rule_stats: HashMap::new(),
            similarity_config,
            precheck_special_chars: precheck_special_chars.unwrap_or_else(|| DEFAULT_PRECHECK_SPECIAL_CHARS.to_string()),
            minimum_literal_length: minimum_literal_length.unwrap_or(2),
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

        let total_mutations = 0;
        let mut channels_affected = 0;

        for channel in channels {
            let original_name = channel.channel_name.clone();
            let record_start = Instant::now();
            let mapped =
                self.apply_rules_to_channel_with_filtering(channel, &active_rules, &logo_assets, base_url)?;
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
        let mut sorted_rules: Vec<_> = self.rule_stats.iter().collect();
        sorted_rules.sort_by(|a, b| b.1.0.cmp(&a.1.0)); // Sort by total time descending
        
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
        info!("=== End Performance Summary ===");
        
        // Clear stats for next run
        self.rule_stats.clear();

        Ok(mapped_channels)
    }

    /// Categorize rules into simple (non-regex) and complex (regex) rules
    fn categorize_rules_by_complexity<'a>(&self, rules: &'a [DataMappingRuleWithDetails]) -> (Vec<&'a DataMappingRuleWithDetails>, Vec<&'a DataMappingRuleWithDetails>) {
        let mut simple_rules = Vec::new();
        let mut regex_rules = Vec::new();

        for rule in rules {
            if self.rule_contains_regex_conditions(rule) {
                regex_rules.push(rule);
            } else {
                simple_rules.push(rule);
            }
        }

        (simple_rules, regex_rules)
    }

    /// Check if a rule contains any regex conditions
    fn rule_contains_regex_conditions(&self, rule: &DataMappingRuleWithDetails) -> bool {
        rule.conditions.iter().any(|condition| {
            matches!(condition.operator, FilterOperator::Matches | FilterOperator::NotMatches)
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
                let filter_result = self.first_pass_regex_filter(&mapped.original, &rule.conditions);
                
                let precheck_duration = precheck_start.elapsed().as_micros();
                total_precheck_time += precheck_duration;
                
                if filter_result {
                    precheck_passed += 1;
                    debug!(
                        "PRECHECK PASSED: Rule '{}' on channel '{}' ({}μs)",
                        rule.rule.name, mapped.original.channel_name, precheck_duration
                    );
                } else {
                    precheck_filtered += 1;
                    debug!(
                        "PRECHECK FILTERED: Rule '{}' on channel '{}' ({}μs)",
                        rule.rule.name, mapped.original.channel_name, precheck_duration
                    );
                }
                filter_result
            } else {
                // Always evaluate non-regex rules
                true
            };

            if should_evaluate {
                let regex_start = Instant::now();
                
                let (conditions_match, captures) = self.evaluate_rule_conditions(&mapped.original, &rule.conditions)?;
                
                if is_regex_rule {
                    regex_evaluated += 1;
                    let regex_duration = regex_start.elapsed().as_micros();
                    total_regex_time += regex_duration;
                    
                    debug!(
                        "REGEX EVALUATED: Rule '{}' on channel '{}' -> {} ({}μs)",
                        rule.rule.name, mapped.original.channel_name, conditions_match, regex_duration
                    );
                }
                
                if conditions_match {
                    debug!(
                        "{} rule '{}' conditions matched for channel '{}'",
                        if is_regex_rule { "Regex" } else { "Simple" },
                        rule.rule.name, 
                        mapped.original.channel_name
                    );

                    let mutations =
                        self.apply_rule_actions(&mut mapped, &rule.actions, logo_assets, base_url, &captures)?;
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
            let entry = self.rule_stats.entry(rule.rule.name.clone()).or_insert((0, 0));
            entry.0 += rule_duration;
            entry.1 += 1;
        }
        
        // Log performance summary for this channel if there were regex rules
        if precheck_passed + precheck_filtered > 0 {
            debug!(
                "CHANNEL SUMMARY '{}': Precheck({}μs, pass:{}, filter:{}), Regex({}μs, eval:{})",
                mapped.original.channel_name,
                total_precheck_time,
                precheck_passed,
                precheck_filtered,
                total_regex_time,
                regex_evaluated
            );
        }

        Ok(mapped)
    }

    /// First-pass filter for regex rules using simple string operations
    fn first_pass_regex_filter(&self, channel: &Channel, conditions: &[DataMappingCondition]) -> bool {
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
            if matches!(condition.operator, FilterOperator::Matches | FilterOperator::NotMatches) {
                has_regex_conditions = true;
                let precheck_result = self.quick_regex_precheck(&field_value.to_lowercase(), &condition.value.to_lowercase());
                debug!(
                    "PRECHECK: field='{}' operator={:?} pattern='{}' channel_value='{}' -> {}",
                    condition.field_name, condition.operator, condition.value, field_value, precheck_result
                );
                if precheck_result {
                    any_precheck_passed = true;
                }
            } else {
                // Non-regex condition always passes precheck
                debug!(
                    "PRECHECK: Non-regex condition field='{}' operator={:?} - always passes precheck",
                    condition.field_name, condition.operator
                );
                return true;
            }
        }
        
        let final_result = if has_regex_conditions {
            any_precheck_passed
        } else {
            true
        };
        
        debug!(
            "PRECHECK FINAL: channel='{}' has_regex={} any_passed={} -> {}",
            channel.channel_name, has_regex_conditions, any_precheck_passed, final_result
        );
        
        final_result
    }

    /// Quick regex pre-check using simple string operations
    fn quick_regex_precheck(&self, field_value: &str, regex_pattern: &str) -> bool {
        // For very simple patterns, check if they're present as literals
        if !regex_pattern.chars().any(|c| r".*+?^$[]{}()|\\".contains(c)) {
            // Pure literal string - check if it's contained in the field
            return field_value.contains(regex_pattern);
        }
        
        // Extract meaningful patterns from the regex
        let mut required_chars = Vec::new();      // Single special chars that must be present
        let mut literal_strings = Vec::new();     // Multi-char strings that must be present
        
        // Parse the regex pattern to extract useful precheck patterns
        let mut chars = regex_pattern.chars().peekable();
        let mut current_literal = String::new();
        
        while let Some(ch) = chars.next() {
            match ch {
                // Regex metacharacters that break literal sequences
                '.' | '*' | '+' | '?' | '^' | '$' | '{' | '}' | '(' | ')' | '|' => {
                    self.save_current_literal(&mut current_literal, &mut literal_strings, &mut required_chars);
                }
                // Handle character classes [...]
                '[' => {
                    self.save_current_literal(&mut current_literal, &mut literal_strings, &mut required_chars);
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
                    self.save_current_literal(&mut current_literal, &mut literal_strings, &mut required_chars);
                }
                // Handle escaped characters
                '\\' => {
                    if let Some(&next_char) = chars.peek() {
                        if "dDwWsSbBnrtf".contains(next_char) {
                            // Character class like \d, \w, etc. - end current literal
                            chars.next();
                            self.save_current_literal(&mut current_literal, &mut literal_strings, &mut required_chars);
                        } else {
                            // Escaped literal character - add to current literal
                            if let Some(escaped_char) = chars.next() {
                                current_literal.push(escaped_char);
                            }
                        }
                    } else {
                        self.save_current_literal(&mut current_literal, &mut literal_strings, &mut required_chars);
                    }
                }
                // Regular characters
                c => {
                    current_literal.push(c);
                }
            }
        }
        
        // Handle any remaining literal
        self.save_current_literal(&mut current_literal, &mut literal_strings, &mut required_chars);
        
        // Log what was extracted for debugging
        debug!(
            "PRECHECK EXTRACT: pattern='{}' -> required_chars={:?}, literal_strings={:?}",
            regex_pattern, required_chars, literal_strings
        );
        
        // Check if field contains required patterns
        let has_required_chars = if required_chars.is_empty() {
            true
        } else {
            let found_chars: Vec<char> = required_chars.iter().filter(|&&ch| field_value.contains(ch)).copied().collect();
            let result = !found_chars.is_empty();
            debug!(
                "PRECHECK CHARS: field='{}' required={:?} found={:?} -> {}",
                field_value, required_chars, found_chars, result
            );
            result
        };
        
        let has_required_strings = if literal_strings.is_empty() {
            true
        } else {
            let found_strings: Vec<&String> = literal_strings.iter().filter(|s| field_value.contains(*s)).collect();
            let result = !found_strings.is_empty();
            debug!(
                "PRECHECK STRINGS: field='{}' required={:?} found={:?} -> {}",
                field_value, literal_strings, found_strings, result
            );
            result
        };
        
        let result = if required_chars.is_empty() && literal_strings.is_empty() {
            debug!(
                "PRECHECK PASSTHROUGH: pattern='{}' field='{}' - no extractable patterns, allowing full regex",
                regex_pattern, field_value
            );
            true // No specific requirements - let full regex handle it
        } else {
            has_required_chars && has_required_strings
        };
        
        debug!(
            "PRECHECK RESULT: field='{}' pattern='{}' -> {} (chars:{} strings:{})",
            field_value, regex_pattern, result, has_required_chars, has_required_strings
        );
        
        result
    }
    
    /// Helper function to save the current literal and extract special characters
    pub fn save_current_literal(&self, current_literal: &mut String, literal_strings: &mut Vec<String>, required_chars: &mut Vec<char>) {
        if current_literal.is_empty() {
            return;
        }
        
        // Check if this is a special character that's useful for prefiltering
        if current_literal.len() == 1 {
            let ch = current_literal.chars().next().unwrap();
            if self.precheck_special_chars.contains(ch) {
                required_chars.push(ch);
            }
        } else if current_literal.len() >= self.minimum_literal_length {
            // Multi-character string - check for mixed content
            let has_special = current_literal.chars().any(|c| self.precheck_special_chars.contains(c));
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
                        if alpha_part.len() >= self.minimum_literal_length {
                            literal_strings.push(alpha_part.clone());
                        }
                        alpha_part.clear();
                    } else if ch.is_alphanumeric() || ch.is_whitespace() {
                        alpha_part.push(ch);
                    } else {
                        // Other punctuation
                        if alpha_part.len() >= self.minimum_literal_length {
                            literal_strings.push(alpha_part.clone());
                        }
                        alpha_part.clear();
                    }
                }
                if alpha_part.len() >= self.minimum_literal_length {
                    literal_strings.push(alpha_part);
                }
            } else if has_special {
                // Only special chars - extract each one
                for ch in current_literal.chars() {
                    if self.precheck_special_chars.contains(ch) && !required_chars.contains(&ch) {
                        required_chars.push(ch);
                    }
                }
            } else {
                // Pure text - add as literal string
                let trimmed = current_literal.trim();
                if trimmed.len() >= self.minimum_literal_length {
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
            .filter(|rule| rule.rule.is_active && rule.rule.source_type == DataMappingSourceType::Epg)
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
            let mapped = self.apply_epg_rules_to_channel(channel, &active_rules, &logo_assets, base_url)?;
            
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
            if let Some(mapped_channel) = mapped_channels.iter().find(|ch| ch.original.channel_id == program.channel_id) {
                let mapped_program = self.apply_epg_rules_to_program(
                    program, 
                    mapped_channel, 
                    &active_rules
                )?;
                
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

    fn apply_rules_to_channel(
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
            original: channel,
        };

        for rule in rules.iter() {
            let rule_start = Instant::now();
            let (conditions_match, captures) = self.evaluate_rule_conditions(&mapped.original, &rule.conditions)?;
            if conditions_match {
                debug!(
                    "Rule '{}' conditions matched for channel '{}'",
                    rule.rule.name, mapped.original.channel_name
                );

                let mutations =
                    self.apply_rule_actions(&mut mapped, &rule.actions, logo_assets, base_url, &captures)?;
                mapped.applied_rules.push(rule.rule.id);

                if mutations > 0 {
                    debug!(
                        "Rule '{}' applied {} mutation(s) to channel '{}'",
                        rule.rule.name, mutations, mapped.original.channel_name
                    );
                }
            }
            let rule_duration = rule_start.elapsed().as_micros();
            
            // Track rule performance statistics
            let entry = self.rule_stats.entry(rule.rule.name.clone()).or_insert((0, 0));
            entry.0 += rule_duration;
            entry.1 += 1;
        }

        Ok(mapped)
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
            let (condition_result, condition_captures) = self.evaluate_condition(channel, &conditions[i])?;

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
                    
                    debug!("Regex match found with {} capture groups", captures.captures.len());
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
                        if !DataMappingFieldInfo::is_valid_field_for_source_type(&action.target_field, &DataMappingSourceType::Stream) {
                            warn!("Invalid field '{}' for Stream source type", action.target_field);
                            continue;
                        }
                        // Substitute regex capture groups in the value
                        let substituted_value = captures.substitute_captures(value);
                        debug!(
                            "SetValue action: field='{}' template='{}' captures={:?} substituted='{}'",
                            action.target_field, value, captures.captures, substituted_value
                        );
                        self.set_field_value(mapped, &action.target_field, Some(substituted_value.clone()));
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
                        if !DataMappingFieldInfo::is_valid_field_for_source_type(&action.target_field, &DataMappingSourceType::Stream) {
                            warn!("Invalid field '{}' for Stream source type", action.target_field);
                            continue;
                        }
                        let current = self.get_mapped_field_value(mapped, &action.target_field);
                        if current.is_none() || current.as_ref().map_or(true, |s| s.is_empty()) {
                            // Substitute regex capture groups in the value
                            let substituted_value = captures.substitute_captures(value);
                            self.set_field_value(mapped, &action.target_field, Some(substituted_value.clone()));
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
                DataMappingActionType::DeduplicateClonedChannel => {
                    // This action is primarily for EPG channels, not stream channels
                    debug!(
                        "DeduplicateClonedChannel action is for EPG channels, not stream channels"
                    );
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
                created_at: chrono::Utc::now(),
                updated_at: chrono::Utc::now(),
            },
            conditions,
            actions,
        };

        let mut mapped_channels = Vec::new();
        for channel in channels {
            let (conditions_match, captures) = self.evaluate_rule_conditions(&channel, &test_rule.conditions)?;
            if conditions_match {
                let mut mapped = MappedChannel {
                    mapped_tvg_id: channel.tvg_id.clone(),
                    mapped_tvg_name: channel.tvg_name.clone(),
                    mapped_tvg_logo: channel.tvg_logo.clone(),
                    mapped_tvg_shift: channel.tvg_shift.clone(),
                    mapped_group_title: channel.group_title.clone(),
                    mapped_channel_name: channel.channel_name.clone(),
                    applied_rules: vec![test_rule.rule.id],
                    original: channel,
                };
                self.apply_rule_actions(&mut mapped, &test_rule.actions, &logo_assets, base_url, &captures)?;
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

                let mutations = self.apply_epg_channel_actions(&mut mapped, &rule.actions, logo_assets, base_url)?;
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
                        self.set_epg_channel_field_value(mapped, &action.target_field, Some(value.clone()));
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
                        let current = self.get_mapped_epg_channel_field_value(mapped, &action.target_field);
                        if current.is_none() || current.as_ref().map_or(true, |s| s.is_empty()) {
                            self.set_epg_channel_field_value(mapped, &action.target_field, Some(value.clone()));
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
                DataMappingActionType::DeduplicateClonedChannel => {
                    // Use channel similarity to identify clone groups
                    let clone_group_id = self.generate_clone_group_id(&mapped.original.channel_name);
                    mapped.clone_group_id = Some(clone_group_id.clone());
                    mapped.is_primary_clone = true; // First channel in group is primary
                    action_applied = true;
                    debug!(
                        "EPG DeduplicateClonedChannel: Channel '{}' assigned to clone group '{}'",
                        mapped.original.channel_name, clone_group_id
                    );
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
            }

            if action_applied {
                mutations += 1;
            }
        }

        Ok(mutations)
    }

    fn get_epg_channel_field_value(&self, channel: &EpgChannel, field_name: &str) -> Option<String> {
        match field_name {
            "channel_id" => Some(channel.channel_id.clone()),
            "channel_name" => Some(channel.channel_name.clone()),
            "channel_logo" => channel.channel_logo.clone(),
            "channel_group" => channel.channel_group.clone(),
            "language" => channel.language.clone(),
            _ => None,
        }
    }

    fn get_mapped_epg_channel_field_value(&self, mapped: &MappedEpgChannel, field_name: &str) -> Option<String> {
        match field_name {
            "channel_id" => Some(mapped.mapped_channel_id.clone()),
            "channel_name" => Some(mapped.mapped_channel_name.clone()),
            "channel_logo" => mapped.mapped_channel_logo.clone(),
            "channel_group" => mapped.mapped_channel_group.clone(),
            "language" => mapped.mapped_language.clone(),
            _ => None,
        }
    }

    fn set_epg_channel_field_value(&self, mapped: &mut MappedEpgChannel, field_name: &str, value: Option<String>) {
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

    fn generate_clone_group_id(&self, channel_name: &str) -> String {
        // Simple clone group ID generation based on normalized channel name
        // This should be enhanced to use the similarity engine
        let normalized = channel_name
            .to_lowercase()
            .replace(" hd", "")
            .replace(" 4k", "")
            .replace(" uhd", "")
            .replace(" +1", "")
            .replace(" +24", "")
            .trim()
            .to_string();
        
        format!("clone_{}", normalized.replace(' ', "_"))
    }

    /// Convert mapped EPG channels back to regular EPG channels for database storage
    pub fn mapped_epg_channels_to_channels(mapped_channels: Vec<MappedEpgChannel>) -> Vec<EpgChannel> {
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
    pub fn mapped_epg_programs_to_programs(mapped_programs: Vec<MappedEpgProgram>) -> Vec<EpgProgram> {
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
            ("bbc one +1", r"bbc.*\+1", true, "Should extract 'bbc' as literal part"),
            ("cnn +24", r".*\+(\d+)", true, "Should allow complex regex to pass through"),
            
            // Timeshift regex pattern from default rule
            ("bbc one +1", r"(.+?)\s*\+\s*(\d+)", true, "Complex pattern should pass through"),
            ("itv +24", r"(.+?)\s*\+\s*(\d+)", true, "Complex pattern should pass through"),
            
            // Non-matching cases with clear literals
            ("discovery", "bbc", false, "No literal match should fail"),
            ("channel 5", "bbc", false, "Different literal should fail"),
            
            // Edge cases
            ("", "test", false, "Empty field should not match"),
            ("test", "", true, "Empty regex should pass through"),
            ("test", r".*", true, "Pure wildcard should pass through"),
            ("test", r"\d+", true, "Pure character class should pass through"),
            
            // Test literal extraction from complex patterns
            ("bbc news", r"bbc.*news", true, "Should extract 'bbc' literal"),
            ("sky sports", r"sky.+sports", true, "Should extract 'sky' literal"),
        ];
        
        for (field_value, regex_pattern, expected, description) in test_cases {
            let result = engine.quick_regex_precheck(field_value, regex_pattern);
            println!("Test: {} | Field: '{}' | Regex: '{}' | Expected: {} | Got: {}", 
                description, field_value, regex_pattern, expected, result);
            
            if result != expected {
                println!("FAILED: {}", description);
                
                // Debug the new literal extraction logic
                let mut literal_candidates = Vec::new();
                let mut current_literal = String::new();
                let mut chars = regex_pattern.chars().peekable();
                
                while let Some(ch) = chars.next() {
                    match ch {
                        '.' | '*' | '+' | '?' | '^' | '$' | '[' | ']' | '{' | '}' | '(' | ')' | '|' => {
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
            ("test@email", "user@domain.com", true, "Should match @ character"),
            ("test@email", "user-domain-com", false, "Should not match without @ character"),
            
            // Mixed patterns with special chars and text
            ("BBC.*HD", "bbc one hd", true, "Should extract 'BBC' and 'HD' literals"),
            ("BBC.*HD", "bbc one", false, "Should not match without 'HD'"),
            ("BBC.*HD", "itv hd", false, "Should not match without 'BBC'"),
            
            // Complex regex patterns that should pass through
            (r"(.+?)\s*\+\s*(\d+)", "bbc one +1", true, "Complex pattern should pass through for matching content"),
            (r"(.+?)\s*\+\s*(\d+)", "bbc one", false, "Complex pattern should filter out non-matching content"),
            
            // URL patterns
            ("http.*://.*", "http://example.com", true, "Should extract 'http' and '://' patterns"),
            ("http.*://.*", "https://example.com", false, "Should not match different protocol"),
            
            // Email patterns
            (r"\w+@\w+\.\w+", "test@example.com", true, "Should extract @ character"),
            (r"\w+@\w+\.\w+", "test-example-com", false, "Should not match without @ character"),
        ];
        
        for (pattern, channel, expected, description) in test_cases {
            let result = engine.quick_regex_precheck(&channel.to_lowercase(), &pattern.to_lowercase());
            println!("Pattern test: '{}' vs '{}' = {} (expected: {}) - {}", 
                pattern, channel, result, expected, description);
            assert_eq!(result, expected, "{}", description);
        }
    }
    
    #[test]
    fn test_real_world_pattern_from_logs() {
        let engine = DataMappingEngine::new();
        
        // Test the actual pattern from the user's logs
        let pattern = r".*(?:\+([0-9]+)|(-[0-9]+)).*";
        let test_channels = vec![
            ("de: sky max 10 4k", false, "Should not match - no + or - characters"),
            ("de: sky max 11 4k", false, "Should not match - no + or - characters"), 
            ("de: sky max 12 4k", false, "Should not match - no + or - characters"),
            ("bbc one +1", true, "Should pass precheck - contains + character"),
            ("cnn +24", true, "Should pass precheck - contains + character"),
            ("discovery -2", true, "Should pass precheck - contains - character"),
            ("test + sign", true, "Should pass precheck - contains + character"),
            ("test - sign", true, "Should pass precheck - contains - character"),
        ];
        
        for (channel, expected, description) in test_channels {
            let result = engine.quick_regex_precheck(&channel.to_lowercase(), &pattern.to_lowercase());
            println!("Real pattern test: '{}' with pattern '{}' = {} (expected: {}) - {}", 
                channel, pattern, result, expected, description);
            
            // Always show what the pattern extracts for debugging
            let mut required_chars = Vec::new();
            let mut literal_strings = Vec::new();
            let mut chars = pattern.chars().peekable();
            let mut current_literal = String::new();
            
            while let Some(ch) = chars.next() {
                match ch {
                    '.' | '*' | '+' | '?' | '^' | '$' | '{' | '}' | '(' | ')' | '|' => {
                        engine.save_current_literal(&mut current_literal, &mut literal_strings, &mut required_chars);
                    }
                    '[' => {
                        engine.save_current_literal(&mut current_literal, &mut literal_strings, &mut required_chars);
                        // Skip brackets
                        let mut bracket_depth = 1;
                        while let Some(bracket_ch) = chars.next() {
                            match bracket_ch {
                                '[' => bracket_depth += 1,
                                ']' => {
                                    bracket_depth -= 1;
                                    if bracket_depth == 0 { break; }
                                }
                                _ => {}
                            }
                        }
                    }
                    ']' => {
                        engine.save_current_literal(&mut current_literal, &mut literal_strings, &mut required_chars);
                    }
                    '\\' => {
                        if let Some(&next_char) = chars.peek() {
                            if "dDwWsSbBnrtf".contains(next_char) {
                                chars.next();
                                engine.save_current_literal(&mut current_literal, &mut literal_strings, &mut required_chars);
                            } else {
                                if let Some(escaped_char) = chars.next() {
                                    current_literal.push(escaped_char);
                                }
                            }
                        } else {
                            engine.save_current_literal(&mut current_literal, &mut literal_strings, &mut required_chars);
                        }
                    }
                    c => {
                        current_literal.push(c);
                    }
                }
            }
            engine.save_current_literal(&mut current_literal, &mut literal_strings, &mut required_chars);
            
            println!("  Extracted - Required chars: {:?}, Literal strings: {:?}", required_chars, literal_strings);
            
            let has_required_chars = required_chars.is_empty() || required_chars.iter().any(|&ch| channel.to_lowercase().contains(ch));
            let has_required_strings = literal_strings.is_empty() || literal_strings.iter().any(|s| channel.to_lowercase().contains(s));
            
            println!("  Channel '{}' - Has required chars: {}, Has required strings: {}", 
                channel.to_lowercase(), has_required_chars, has_required_strings);
                
            assert_eq!(result, expected, "{}", description);
        }
    }

    #[test]
    fn test_configurable_special_characters() {
        // Test with default special characters
        let default_engine = DataMappingEngine::new();
        let default_result = default_engine.quick_regex_precheck("test+channel", ".*\\+.*");
        assert_eq!(default_result, true, "Default engine should recognize + character");

        // Test with custom special characters (excluding +)
        let custom_engine = DataMappingEngine::new_with_custom_precheck(
            ChannelSimilarityConfig::default(),
            Some("@#$%&*".to_string()), // No + or - characters
            Some(2)
        );
        let custom_result = custom_engine.quick_regex_precheck("test+channel", ".*\\+.*");
        assert_eq!(custom_result, true, "Custom engine should pass through when + not in special chars list");

        // Test with custom special characters (including €)
        let euro_engine = DataMappingEngine::new_with_custom_precheck(
            ChannelSimilarityConfig::default(),
            Some("€£{}[]".to_string()),
            Some(2)
        );
        let euro_result = euro_engine.quick_regex_precheck("channel€price", ".*€.*");
        assert_eq!(euro_result, true, "Euro engine should recognize € character");

        // Test minimum literal length configuration
        let length_engine = DataMappingEngine::new_with_custom_precheck(
            ChannelSimilarityConfig::default(),
            Some(DEFAULT_PRECHECK_SPECIAL_CHARS.to_string()),
            Some(3) // Require 3+ character literals
        );
        let length_result = length_engine.quick_regex_precheck("ab test", "ab.*");
        assert_eq!(length_result, true, "Should pass through when literal too short for filtering");
    }

    #[test]
    fn test_config_based_construction() {
        use crate::config::DataMappingConfig;

        // Test with config-based construction
        let config = DataMappingConfig {
            precheck_special_chars: Some("€£{}[]+-".to_string()),
            minimum_literal_length: Some(3),
        };

        let engine = DataMappingEngine::new_with_config(
            ChannelSimilarityConfig::default(),
            Some(config)
        );

        // Test that it uses the configured characters
        let result = engine.quick_regex_precheck("test€price", ".*€.*");
        assert_eq!(result, true, "Config-based engine should use configured special chars");

        // Test that it uses the configured minimum length
        // This is harder to test directly, but we can verify the field
        assert_eq!(engine.minimum_literal_length, 3, "Should use configured minimum literal length");
        assert!(engine.precheck_special_chars.contains('€'), "Should use configured special characters");
    }

    #[test]
    fn test_extended_character_set() {
        let engine = DataMappingEngine::new();
        
        // Test the new characters we added
        let test_cases = vec![
            ("price€100", ".*€.*", true, "Euro symbol should be recognized"),
            ("price£50", ".*£.*", true, "Pound symbol should be recognized"),
            ("config{value}", ".*\\{.*", true, "Opening brace should be recognized"),
            ("config}end", ".*\\}.*", true, "Closing brace should be recognized"),
            ("list[0]", ".*\\[.*", true, "Opening bracket should be recognized"),
            ("list]end", ".*\\].*", true, "Closing bracket should be recognized"),
        ];

        for (channel, pattern, expected, description) in test_cases {
            let result = engine.quick_regex_precheck(channel, pattern);
            println!("Extended char test: '{}' vs '{}' = {} (expected: {}) - {}", 
                channel, pattern, result, expected, description);
            assert_eq!(result, expected, "{}", description);
        }
    }
}
