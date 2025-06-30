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
        filters: Vec<(Filter, ProxyFilter)>,
    ) -> Result<Vec<Channel>> {
        // Sort filters by their order
        let mut sorted_filters = filters;
        sorted_filters.sort_by_key(|(_, proxy_filter)| proxy_filter.sort_order);

        let mut result_channels = Vec::new();
        let mut _current_channel_number = 1;

        for (filter, proxy_filter) in sorted_filters {
            if !proxy_filter.is_active {
                continue;
            }

            let filtered = self.apply_single_filter(&channels, &filter).await?;

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
    ) -> Result<Vec<Channel>> {
        let mut matches = Vec::new();

        for channel in channels {
            let channel_matches = self.evaluate_filter_tree(channel, filter)?;

            if channel_matches {
                matches.push(channel.clone());
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
            Some(tree) => self.evaluate_condition_node(channel, &tree.root),
            None => Ok(true), // If no tree, default to true (shouldn't happen)
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
                    // Legacy fallback mapping
                    LogicalOperator::All => Ok(results.iter().all(|&x| x)),
                    LogicalOperator::Any => Ok(results.iter().any(|&x| x)),
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
                let regex = self.get_or_compile_regex(value, case_sensitive)?;
                regex.is_match(&field_value)
            }
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
                let regex = self.get_or_compile_regex(value, case_sensitive)?;
                !regex.is_match(&field_value)
            }
        };

        // Apply negation if specified
        Ok(if negate { !result } else { result })
    }
}
