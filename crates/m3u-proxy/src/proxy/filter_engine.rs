use anyhow::Result;
use regex::Regex;
use std::collections::HashMap;
use tracing::{debug, info, warn};

use crate::models::*;
use crate::utils::RegexPreprocessor;

#[allow(dead_code)]
pub struct FilterEngine {
    // Cache compiled regexes for performance
    regex_cache: HashMap<String, Regex>,
    // Regex preprocessor for performance optimization
    preprocessor: RegexPreprocessor,
}

impl FilterEngine {
    #[allow(dead_code)]
    pub fn new() -> Self {
        Self {
            regex_cache: HashMap::new(),
            preprocessor: RegexPreprocessor::default(),
        }
    }

    #[allow(dead_code)]
    pub async fn apply_filters(
        &mut self,
        channels: Vec<Channel>,
        filters: Vec<(Filter, ProxyFilter)>,
    ) -> Result<Vec<Channel>> {
        let initial_channel_count = channels.len();
        info!(
            "Starting filter application with {} initial channels",
            initial_channel_count
        );

        // Sort filters by their order
        let mut sorted_filters = filters;
        sorted_filters.sort_by_key(|(_, proxy_filter)| proxy_filter.priority_order);

        // Keep the original full channel set for INCLUDE filters to operate on
        let original_channels = channels.clone();
        let mut result_channels = Vec::new(); // Start with empty set
        let mut _current_channel_number = 1;

        for (filter_index, (filter, proxy_filter)) in sorted_filters.iter().enumerate() {
            if !proxy_filter.is_active {
                info!(
                    "Filter #{} '{}' is inactive, skipping",
                    filter_index + 1,
                    filter.name
                );
                continue;
            }

            if filter.is_inverse {
                // For EXCLUDE filters, operate on current result set
                let channels_before_filter = result_channels.len();
                debug!(
                    "Filter #{} '{}' (EXCLUDE): Evaluating {} channels against filter condition",
                    filter_index + 1,
                    filter.name,
                    channels_before_filter
                );

                let matched_channels = self.apply_single_filter(&result_channels, filter).await?;
                let matched_count = matched_channels.len();

                // Log some sample matches for debugging
                if matched_count > 0 && matched_count != channels_before_filter {
                    debug!(
                        "Filter #{} '{}' sample matches: {}",
                        filter_index + 1,
                        filter.name,
                        matched_channels
                            .iter()
                            .take(5)
                            .map(|ch| format!(
                                "'{}' (group: '{}')",
                                ch.channel_name,
                                ch.group_title.as_deref().unwrap_or("N/A")
                            ))
                            .collect::<Vec<_>>()
                            .join(", ")
                    );
                } else if matched_count == channels_before_filter {
                    warn!(
                        "Filter #{} '{}' (EXCLUDE): ALL {} channels matched - this may indicate a filter logic error!",
                        filter_index + 1,
                        filter.name,
                        matched_count
                    );
                    // Log first few channels to help debug
                    debug!(
                        "First few channels being matched: {}",
                        result_channels
                            .iter()
                            .take(3)
                            .map(|ch| format!(
                                "'{}' (group: '{}')",
                                ch.channel_name,
                                ch.group_title.as_deref().unwrap_or("N/A")
                            ))
                            .collect::<Vec<_>>()
                            .join(", ")
                    );
                }

                // Remove matches from the current result
                result_channels.retain(|channel: &Channel| {
                    !matched_channels
                        .iter()
                        .any(|filtered_channel| filtered_channel.id == channel.id)
                });
                let after_removal = result_channels.len();
                let removed_count = channels_before_filter - after_removal;

                info!(
                    "Filter #{} '{}' (EXCLUDE): {} channels matched filter criteria, {} channels removed, {} channels remaining",
                    filter_index + 1,
                    filter.name,
                    matched_count,
                    removed_count,
                    after_removal
                );
            } else {
                // For INCLUDE filters, operate on the ORIGINAL full channel set
                let matched_channels = self.apply_single_filter(&original_channels, filter).await?;
                let matched_count = matched_channels.len();

                // Add matched channels to result set (union operation)
                let channels_before_add = result_channels.len();
                for matched_channel in matched_channels {
                    // Only add if not already in result set
                    if !result_channels.iter().any(|ch| ch.id == matched_channel.id) {
                        result_channels.push(matched_channel);
                    }
                }
                let channels_after_add = result_channels.len();
                let added_count = channels_after_add - channels_before_add;

                info!(
                    "Filter #{} '{}' (INCLUDE): {} channels matched filter criteria, {} channels added, {} channels now in result",
                    filter_index + 1,
                    filter.name,
                    matched_count,
                    added_count,
                    result_channels.len()
                );
            }

            // Channel numbers are now handled by the proxy itself, not by filters
        }

        let final_channel_count = result_channels.len();
        let total_added = final_channel_count;

        info!(
            "Filter application complete: {} initial channels → {} final channels ({} channels in final result)",
            initial_channel_count, final_channel_count, total_added
        );

        Ok(result_channels)
    }

    #[allow(dead_code)]
    pub async fn apply_single_filter(
        &mut self,
        channels: &[Channel],
        filter: &Filter,
    ) -> Result<Vec<Channel>> {
        let mut matches = Vec::new();
        let mut sample_non_matches = Vec::new();

        for channel in channels {
            let channel_matches = self.evaluate_filter_tree(channel, filter)?;

            if channel_matches {
                matches.push(channel.clone());
            } else if sample_non_matches.len() < 3 {
                sample_non_matches.push(channel);
            }

        }

        // Log debug info for problematic filters
        if filter.name.contains("Adult") {
            debug!(
                "Filter '{}' evaluation: {}/{} channels matched",
                filter.name,
                matches.len(),
                channels.len()
            );

            if !matches.is_empty() {
                debug!(
                    "Sample matches: {}",
                    matches
                        .iter()
                        .take(3)
                        .map(|ch| format!(
                            "'{}' (group: '{}')",
                            ch.channel_name,
                            ch.group_title.as_deref().unwrap_or("N/A")
                        ))
                        .collect::<Vec<_>>()
                        .join(", ")
                );
            }

            if !sample_non_matches.is_empty() {
                debug!(
                    "Sample non-matches: {}",
                    sample_non_matches
                        .iter()
                        .map(|ch| format!(
                            "'{}' (group: '{}')",
                            ch.channel_name,
                            ch.group_title.as_deref().unwrap_or("N/A")
                        ))
                        .collect::<Vec<_>>()
                        .join(", ")
                );
            }
        }

        Ok(matches)
    }

    #[allow(dead_code)]
    fn get_or_compile_regex(&mut self, pattern: &str, case_sensitive: bool) -> Result<&Regex> {
        let cache_key = format!("{}:{}", pattern, case_sensitive);
        if !self.regex_cache.contains_key(&cache_key) {
            let regex = if case_sensitive {
                Regex::new(pattern)?
            } else {
                regex::RegexBuilder::new(pattern)
                    .case_insensitive(true)
                    .build()?
            };
            self.regex_cache.insert(cache_key.clone(), regex);
        }

        Ok(self.regex_cache.get(&cache_key).unwrap())
    }

    // Tree-based filter evaluation methods

    fn evaluate_filter_tree(&mut self, channel: &Channel, filter: &Filter) -> Result<bool> {
        match filter.get_condition_tree() {
            Some(tree) => {
                let result = self.evaluate_condition_node(channel, &tree.root)?;

                // Debug log for adult filter
                if filter.name.contains("Adult") && result {
                    debug!(
                        "Adult filter matched channel '{}' (group: '{}')",
                        channel.channel_name,
                        channel.group_title.as_deref().unwrap_or("N/A")
                    );
                }

                Ok(result)
            }
            None => {
                warn!(
                    "Filter '{}' has no condition tree - parsing failed! Raw condition_tree: '{}'",
                    filter.name, filter.condition_tree
                );
                // For safety, exclude filters should default to false (no matches)
                // and include filters should default to true (match all)
                // This prevents catastrophic failures like excluding all channels
                let default_result = !filter.is_inverse;
                warn!(
                    "Filter '{}' defaulting to {} (is_inverse: {})",
                    filter.name, default_result, filter.is_inverse
                );
                Ok(default_result)
            }
        }
    }

    fn get_channel_field_value(channel: &Channel, field_name: &str) -> String {
        // Dynamically access channel fields
        // If a field doesn't exist, return empty string (graceful degradation)
        match field_name {
            "channel_name" => channel.channel_name.clone(),
            "group_title" => channel.group_title.clone().unwrap_or_default(),
            "tvg_id" => channel.tvg_id.clone().unwrap_or_default(),
            "tvg_name" => channel.tvg_name.clone().unwrap_or_default(),
            "tvg_logo" => channel.tvg_logo.clone().unwrap_or_default(),
            "stream_url" => channel.stream_url.clone(),
            // For unknown fields, return empty string (graceful handling of schema changes)
            _ => String::new(),
        }
    }

    fn evaluate_condition_node(&mut self, channel: &Channel, node: &ConditionNode) -> Result<bool> {
        match node {
            ConditionNode::Condition {
                field,
                operator,
                value,
                case_sensitive,
                negate,
            } => self.evaluate_tree_condition(
                channel,
                field,
                operator,
                value,
                *case_sensitive,
                *negate,
            ),
            ConditionNode::Group { operator, children } => {
                if children.is_empty() {
                    return Ok(true);
                }

                let results: Result<Vec<bool>> = children
                    .iter()
                    .map(|child| self.evaluate_condition_node(channel, child))
                    .collect();

                let results = results?;

                match operator {
                    LogicalOperator::And => Ok(results.iter().all(|&x| x)),
                    LogicalOperator::Or => Ok(results.iter().any(|&x| x)),
                }
            }
        }
    }

    #[allow(dead_code)]
    fn evaluate_tree_condition(
        &mut self,
        channel: &Channel,
        field: &str,
        operator: &FilterOperator,
        value: &str,
        case_sensitive: bool,
        negate: bool,
    ) -> Result<bool> {
        let field_value = Self::get_channel_field_value(channel, field);

        // Debug logging for adult filter
        if value.contains("adult")
            || value.contains("xxx")
            || value.contains("porn")
            || value.contains("18")
        {
            debug!(
                "Evaluating condition: field='{}' ({}) {} '{}' (case_sensitive={}, negate={})",
                field,
                field_value,
                match operator {
                    FilterOperator::Contains => "contains",
                    FilterOperator::Matches => "matches",
                    FilterOperator::Equals => "equals",
                    _ => "other",
                },
                value,
                case_sensitive,
                negate
            );
        }

        let result = match operator {
            FilterOperator::Contains => {
                if case_sensitive {
                    field_value.contains(value)
                } else {
                    field_value.to_lowercase().contains(&value.to_lowercase())
                }
            }
            FilterOperator::Equals => {
                if case_sensitive {
                    field_value == *value
                } else {
                    field_value.to_lowercase() == value.to_lowercase()
                }
            }
            FilterOperator::StartsWith => {
                if case_sensitive {
                    field_value.starts_with(value)
                } else {
                    field_value
                        .to_lowercase()
                        .starts_with(&value.to_lowercase())
                }
            }
            FilterOperator::EndsWith => {
                if case_sensitive {
                    field_value.ends_with(value)
                } else {
                    field_value.to_lowercase().ends_with(&value.to_lowercase())
                }
            }
            FilterOperator::Matches => {
                // Apply first-pass filtering if enabled
                if !self.preprocessor.should_run_regex(&field_value, value, "Filter") {
                    false
                } else {
                    match self.get_or_compile_regex(value, case_sensitive) {
                        Ok(regex) => regex.is_match(&field_value),
                        Err(e) => {
                            warn!(
                                "Regex compilation failed for pattern '{}': {}. Defaulting to false.",
                                value, e
                            );
                            false
                        }
                    }
                }
            },
            FilterOperator::NotContains => {
                if case_sensitive {
                    !field_value.contains(value)
                } else {
                    !field_value.to_lowercase().contains(&value.to_lowercase())
                }
            }
            FilterOperator::NotEquals => {
                if case_sensitive {
                    field_value != *value
                } else {
                    field_value.to_lowercase() != value.to_lowercase()
                }
            }
            FilterOperator::NotMatches => {
                // Apply first-pass filtering if enabled
                if !self.preprocessor.should_run_regex(&field_value, value, "Filter") {
                    true // If preprocessing suggests no match, then "not matches" should be true
                } else {
                    match self.get_or_compile_regex(value, case_sensitive) {
                        Ok(regex) => !regex.is_match(&field_value),
                        Err(e) => {
                            warn!(
                                "Regex compilation failed for pattern '{}': {}. Defaulting to true (not matching).",
                                value, e
                            );
                            true
                        }
                    }
                }
            },
            FilterOperator::NotStartsWith => {
                if case_sensitive {
                    !field_value.starts_with(value)
                } else {
                    !field_value
                        .to_lowercase()
                        .starts_with(&value.to_lowercase())
                }
            }
            FilterOperator::NotEndsWith => {
                if case_sensitive {
                    !field_value.ends_with(value)
                } else {
                    !field_value.to_lowercase().ends_with(&value.to_lowercase())
                }
            }
        };

        let final_result = if negate { !result } else { result };

        // Debug logging for adult filter results
        if (value.contains("adult")
            || value.contains("xxx")
            || value.contains("porn")
            || value.contains("18"))
            && final_result
        {
            debug!(
                "  → MATCH: '{}' matched pattern '{}' (result={}, negate={}, final={})",
                field_value, value, result, negate, final_result
            );
        }

        Ok(final_result)
    }
}
