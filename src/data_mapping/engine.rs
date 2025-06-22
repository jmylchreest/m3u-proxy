use crate::models::{
    data_mapping::{
        DataMappingAction, DataMappingActionType, DataMappingCondition, DataMappingRule,
        DataMappingRuleWithDetails, MappedChannel,
    },
    logo_asset::LogoAsset,
    Channel, FilterOperator, LogicalOperator,
};
use regex::{Regex, RegexBuilder};
use std::collections::HashMap;
use tracing::{debug, info, warn};
use uuid::Uuid;

pub struct DataMappingEngine {
    regex_cache: HashMap<String, Regex>,
}

impl DataMappingEngine {
    pub fn new() -> Self {
        Self {
            regex_cache: HashMap::new(),
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
            let mapped =
                self.apply_rules_to_channel(channel, &active_rules, &logo_assets, base_url)?;

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

        info!(
            "Data mapping completed for source {}: {} channels affected, {} total mutations applied",
            source_id,
            channels_affected,
            total_mutations
        );

        Ok(mapped_channels)
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
            mapped_group_title: channel.group_title.clone(),
            mapped_channel_name: channel.channel_name.clone(),
            labels: std::collections::HashMap::new(),
            applied_rules: Vec::new(),
            original: channel,
        };

        for rule in rules.iter() {
            if self.evaluate_rule_conditions(&mapped.original, &rule.conditions)? {
                debug!(
                    "Rule '{}' conditions matched for channel '{}'",
                    rule.rule.name, mapped.original.channel_name
                );

                let mutations =
                    self.apply_rule_actions(&mut mapped, &rule.actions, logo_assets, base_url)?;
                mapped.applied_rules.push(rule.rule.id);

                if mutations > 0 {
                    info!(
                        "Rule '{}' applied {} mutation(s) to channel '{}'",
                        rule.rule.name, mutations, mapped.original.channel_name
                    );
                }
            }
        }

        Ok(mapped)
    }

    pub fn evaluate_rule_conditions(
        &mut self,
        channel: &Channel,
        conditions: &[DataMappingCondition],
    ) -> Result<bool, Box<dyn std::error::Error>> {
        if conditions.is_empty() {
            return Ok(true);
        }

        let mut result = self.evaluate_condition(channel, &conditions[0])?;

        for i in 1..conditions.len() {
            let condition_result = self.evaluate_condition(channel, &conditions[i])?;

            let logical_op = conditions[i]
                .logical_operator
                .as_ref()
                .unwrap_or(&LogicalOperator::And);

            // Support both old (and/or) and new (all/any) formats
            if logical_op.is_and_like() {
                result = result && condition_result;
            } else {
                result = result || condition_result;
            }
        }

        Ok(result)
    }

    fn evaluate_condition(
        &mut self,
        channel: &Channel,
        condition: &DataMappingCondition,
    ) -> Result<bool, Box<dyn std::error::Error>> {
        let field_value = self.get_field_value(channel, &condition.field_name);
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
                let regex = self.get_or_create_regex(&condition.value, false)?; // case_insensitive by default
                Ok(regex.is_match(&field_value))
            }
            FilterOperator::NotMatches => {
                let regex = self.get_or_create_regex(&condition.value, false)?; // case_insensitive by default
                Ok(!regex.is_match(&field_value))
            }
        }
    }

    fn apply_rule_actions(
        &self,
        mapped: &mut MappedChannel,
        actions: &[DataMappingAction],
        logo_assets: &HashMap<Uuid, LogoAsset>,
        base_url: &str,
    ) -> Result<usize, Box<dyn std::error::Error>> {
        let mut mutations = 0;

        for action in actions {
            let old_value = self.get_mapped_field_value(mapped, &action.target_field);
            let mut action_applied = false;

            match action.action_type {
                DataMappingActionType::SetValue => {
                    if let Some(value) = &action.value {
                        self.set_field_value(mapped, &action.target_field, Some(value.clone()));
                        action_applied = true;
                        debug!(
                            "SetValue: {} '{}' -> '{}'",
                            action.target_field,
                            old_value.unwrap_or_else(|| "null".to_string()),
                            value
                        );
                    }
                }
                DataMappingActionType::SetDefaultIfEmpty => {
                    if let Some(value) = &action.value {
                        let current = self.get_mapped_field_value(mapped, &action.target_field);
                        if current.is_none() || current.as_ref().map_or(true, |s| s.is_empty()) {
                            self.set_field_value(mapped, &action.target_field, Some(value.clone()));
                            action_applied = true;
                            debug!(
                                "SetDefaultIfEmpty: {} '{}' -> '{}'",
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
                DataMappingActionType::SetLabel => {
                    if let (Some(key), Some(value)) = (&action.label_key, &action.label_value) {
                        mapped.labels.insert(key.clone(), value.clone());
                        action_applied = true;
                        debug!("SetLabel: Added label '{}' = '{}'", key, value);
                    }
                }
                DataMappingActionType::TransformValue => {
                    // TODO: Implement transform value action
                    debug!(
                        "TransformValue action not yet implemented for field {}",
                        action.target_field
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
            if self.evaluate_rule_conditions(&channel, &test_rule.conditions)? {
                let mut mapped = MappedChannel {
                    mapped_tvg_id: channel.tvg_id.clone(),
                    mapped_tvg_name: channel.tvg_name.clone(),
                    mapped_tvg_logo: channel.tvg_logo.clone(),
                    mapped_group_title: channel.group_title.clone(),
                    mapped_channel_name: channel.channel_name.clone(),
                    labels: std::collections::HashMap::new(),
                    applied_rules: vec![test_rule.rule.id],
                    original: channel,
                };
                self.apply_rule_actions(&mut mapped, &test_rule.actions, &logo_assets, base_url)?;
                mapped_channels.push(mapped);
            }
        }

        Ok(mapped_channels)
    }

    pub fn get_available_fields() -> Vec<String> {
        vec![
            "channel_name".to_string(),
            "tvg_id".to_string(),
            "tvg_name".to_string(),
            "tvg_logo".to_string(),
            "group_title".to_string(),
            "stream_url".to_string(),
        ]
    }
}
