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
    async fn apply_single_filter(
        &mut self,
        channels: &[Channel],
        filter: &Filter,
    ) -> Result<Vec<Channel>> {
        let regex = self.get_or_compile_regex(&filter.pattern)?;
        let mut matches = Vec::new();

        for channel in channels {
            if FilterEngine::channel_matches_filter_static(channel, &regex) {
                matches.push(channel.clone());
            }
        }

        Ok(matches)
    }

    #[allow(dead_code)]
    fn channel_matches_filter_static(channel: &Channel, regex: &Regex) -> bool {
        // Check all relevant fields for matches
        let fields_to_check = [
            channel.channel_name.as_str(),
            channel.tvg_id.as_deref().unwrap_or(""),
            channel.tvg_name.as_deref().unwrap_or(""),
            channel.group_title.as_deref().unwrap_or(""),
        ];

        for field in &fields_to_check {
            if regex.is_match(field) {
                return true;
            }
        }

        false
    }

    #[allow(dead_code)]
    fn get_or_compile_regex(&mut self, pattern: &str) -> Result<&Regex> {
        if !self.regex_cache.contains_key(pattern) {
            let regex = Regex::new(pattern)?;
            self.regex_cache.insert(pattern.to_string(), regex);
        }

        Ok(self.regex_cache.get(pattern).unwrap())
    }
}
