use anyhow::Result;
use regex::Regex;
use std::collections::HashMap;

use crate::models::*;

#[allow(dead_code)]
pub struct FilterEngine {
    // Cache compiled regexes for performance
    regex_cache: HashMap<String, Regex>,
}

impl FilterEngine {
    #[allow(dead_code)]
    pub fn new() -> Self {
        Self {
            regex_cache: HashMap::new(),
        }
    }

    #[allow(dead_code)]
    pub async fn apply_filters(
        &mut self,
        channels: Vec<Channel>,
        filters: Vec<(Filter, ProxyFilter, Vec<FilterCondition>)>,
    ) -> Result<Vec<Channel>> {
        // Sort filters by their order
        let mut sorted_filters = filters;
        sorted_filters.sort_by_key(|(_, proxy_filter, _)| proxy_filter.sort_order);

        let mut result_channels = Vec::new();
        let mut _current_channel_number = 1;

        for (filter, proxy_filter, conditions) in sorted_filters {
            if !proxy_filter.is_active {
                continue;
            }

            let filtered = self
                .apply_single_filter(&channels, &filter, &conditions)
                .await?;

            if filter.is_inverse {
                // For inverse filters, remove matches from the current result
                result_channels.retain(|channel: &Channel| {
                    !filtered
                        .iter()
                        .any(|filtered_channel| filtered_channel.id == channel.id)
                });
            } else {
                // For normal filters, add matches to the result
                let mut numbered_channels = filtered;

                // Apply starting channel number
                for (_index, _channel) in numbered_channels.iter_mut().enumerate() {
                    // Note: We don't store channel numbers in the Channel struct
                    // This would be handled during M3U generation
                }

                result_channels.extend(numbered_channels);
            }

            // Update current channel number for next filter
            _current_channel_number = filter.starting_channel_number + result_channels.len() as i32;
        }

        Ok(result_channels)
    }

    #[allow(dead_code)]
    pub async fn apply_single_filter(
        &mut self,
        channels: &[Channel],
        filter: &Filter,
        conditions: &[FilterCondition],
    ) -> Result<Vec<Channel>> {
        let mut matches = Vec::new();

        for channel in channels {
            let channel_matches = self.evaluate_filter_conditions(channel, filter, conditions)?;

            if channel_matches {
                matches.push(channel.clone());
            }
        }

        Ok(matches)
    }

    #[allow(dead_code)]
    fn get_or_compile_regex(&mut self, pattern: &str) -> Result<&Regex> {
        if !self.regex_cache.contains_key(pattern) {
            let regex = Regex::new(pattern)?;
            self.regex_cache.insert(pattern.to_string(), regex);
        }

        Ok(self.regex_cache.get(pattern).unwrap())
    }

    #[allow(dead_code)]
    fn evaluate_filter_conditions(
        &mut self,
        channel: &Channel,
        filter: &Filter,
        conditions: &[FilterCondition],
    ) -> Result<bool> {
        if conditions.is_empty() {
            return Ok(true);
        }

        let mut condition_results = Vec::new();

        // Evaluate all conditions
        for condition in conditions {
            let result = self.evaluate_condition(channel, condition)?;
            condition_results.push(result);
        }

        // Apply logical operator
        match filter.logical_operator {
            LogicalOperator::And => Ok(condition_results.iter().all(|&x| x)),
            LogicalOperator::Or => Ok(condition_results.iter().any(|&x| x)),
        }
    }

    #[allow(dead_code)]
    fn evaluate_condition(
        &mut self,
        channel: &Channel,
        condition: &FilterCondition,
    ) -> Result<bool> {
        // Get field value from channel using dynamic field access
        let field_value = self.get_channel_field_value(channel, &condition.field_name);

        match condition.operator {
            FilterOperator::Contains => Ok(field_value
                .to_lowercase()
                .contains(&condition.value.to_lowercase())),
            FilterOperator::Equals => Ok(field_value == condition.value),
            FilterOperator::StartsWith => Ok(field_value
                .to_lowercase()
                .starts_with(&condition.value.to_lowercase())),
            FilterOperator::EndsWith => Ok(field_value
                .to_lowercase()
                .ends_with(&condition.value.to_lowercase())),
            FilterOperator::Matches => {
                let regex = self.get_or_compile_regex(&condition.value)?;
                Ok(regex.is_match(&field_value))
            }
            FilterOperator::NotContains => Ok(!field_value
                .to_lowercase()
                .contains(&condition.value.to_lowercase())),
            FilterOperator::NotEquals => Ok(field_value != condition.value),
            FilterOperator::NotMatches => {
                let regex = self.get_or_compile_regex(&condition.value)?;
                Ok(!regex.is_match(&field_value))
            }
        }
    }

    #[allow(dead_code)]
    fn get_channel_field_value(&self, channel: &Channel, field_name: &str) -> String {
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
}
