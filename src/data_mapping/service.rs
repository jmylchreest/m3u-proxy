use crate::data_mapping::engine::DataMappingEngine;
use crate::filter_parser::FilterParser;
use crate::logo_assets::LogoAssetService;
use crate::models::data_mapping::DataMappingActionType;
use crate::models::data_mapping::*;
use crate::models::{logo_asset::LogoAsset, Channel, ExtendedExpression};
use chrono::Utc;
use sqlx::{Pool, Row, Sqlite};
use std::collections::HashMap;
use tracing::{error, info, warn};
use uuid::Uuid;

#[derive(Clone)]
pub struct DataMappingService {
    pool: Pool<Sqlite>,
}

impl DataMappingService {
    pub fn new(pool: Pool<Sqlite>) -> Self {
        Self { pool }
    }

    pub async fn create_rule(
        &self,
        request: DataMappingRuleCreateRequest,
    ) -> Result<DataMappingRuleWithDetails, sqlx::Error> {
        let rule_id = Uuid::new_v4();
        let now = Utc::now();

        // Validate expression before saving
        if let Err(e) = request.validate_expression() {
            error!("Invalid expression for rule '{}': {}", request.name, e);
            return Err(sqlx::Error::Decode(
                format!("Invalid expression: {}", e).into(),
            ));
        }

        let mut tx = self.pool.begin().await?;

        // Insert rule with expression
        sqlx::query(
            r#"
            INSERT INTO data_mapping_rules (id, name, description, source_type, scope, expression, sort_order, is_active, created_at, updated_at)
            VALUES (?, ?, ?, ?, ?, ?, (SELECT COALESCE(MAX(sort_order), 0) + 1 FROM data_mapping_rules), TRUE, ?, ?)
            "#
        )
        .bind(rule_id.to_string())
        .bind(&request.name)
        .bind(&request.description)
        .bind(request.source_type.to_string())
        .bind("individual") // Default scope for expression-based rules
        .bind(&request.expression)
        .bind(now.to_rfc3339())
        .bind(now.to_rfc3339())
        .execute(&mut *tx)
        .await?;

        tx.commit().await?;

        // Return the created rule with parsed details
        self.get_rule_with_details(rule_id).await
    }

    pub async fn update_rule(
        &self,
        rule_id: Uuid,
        request: DataMappingRuleUpdateRequest,
    ) -> Result<DataMappingRuleWithDetails, sqlx::Error> {
        let now = Utc::now();

        // Validate expression before saving
        if let Err(e) = request.validate_expression() {
            error!(
                "Invalid expression for rule update '{}': {}",
                request.name, e
            );
            return Err(sqlx::Error::Decode(
                format!("Invalid expression: {}", e).into(),
            ));
        }

        let mut tx = self.pool.begin().await?;

        // Update rule
        let result = sqlx::query(
            r#"
            UPDATE data_mapping_rules
            SET name = ?, description = ?, source_type = ?, expression = ?, is_active = ?, updated_at = ?
            WHERE id = ?
            "#
        )
        .bind(&request.name)
        .bind(&request.description)
        .bind(request.source_type.to_string())
        .bind(&request.expression)
        .bind(request.is_active)
        .bind(now.to_rfc3339())
        .bind(rule_id.to_string())
        .execute(&mut *tx)
        .await?;

        if result.rows_affected() == 0 {
            return Err(sqlx::Error::RowNotFound);
        }

        tx.commit().await?;

        // Return the updated rule with parsed details
        self.get_rule_with_details(rule_id).await
    }

    pub async fn delete_rule(&self, rule_id: Uuid) -> Result<(), sqlx::Error> {
        let mut tx = self.pool.begin().await?;

        let result = sqlx::query("DELETE FROM data_mapping_rules WHERE id = ?")
            .bind(rule_id.to_string())
            .execute(&mut *tx)
            .await?;

        if result.rows_affected() == 0 {
            return Err(sqlx::Error::RowNotFound);
        }

        tx.commit().await?;
        Ok(())
    }

    pub async fn get_rule_with_details(
        &self,
        rule_id: Uuid,
    ) -> Result<DataMappingRuleWithDetails, sqlx::Error> {
        let row = sqlx::query(
            r#"
            SELECT id, name, description, source_type, scope, expression, sort_order, is_active, created_at, updated_at
            FROM data_mapping_rules
            WHERE id = ?
            "#
        )
        .bind(rule_id.to_string())
        .fetch_one(&self.pool)
        .await?;

        let rule = self.row_to_rule(&row)?;
        let expression = row.get::<String, _>("expression");

        // Parse expression to extract conditions and actions for compatibility
        // If parsing fails, disable the rule and use empty conditions/actions
        let (conditions, actions, final_rule) =
            match self.parse_expression_for_compatibility(&expression, &rule.source_type) {
                Ok((conditions, actions)) => (conditions, actions, rule),
                Err(e) => {
                    warn!(
                        "Failed to parse expression for rule '{}': {}. Rule will be disabled.",
                        rule.name, e
                    );
                    let mut disabled_rule = rule;
                    disabled_rule.is_active = false;
                    (vec![], vec![], disabled_rule)
                }
            };

        Ok(DataMappingRuleWithDetails {
            rule: final_rule,
            conditions,
            actions,
            expression: Some(expression),
        })
    }

    pub async fn get_all_rules(&self) -> Result<Vec<DataMappingRuleWithDetails>, sqlx::Error> {
        let rows = sqlx::query(
            r#"
            SELECT id, name, description, source_type, scope, expression, sort_order, is_active, created_at, updated_at
            FROM data_mapping_rules
            ORDER BY sort_order ASC
            "#
        )
        .fetch_all(&self.pool)
        .await?;

        let mut rules = Vec::new();
        for row in rows {
            let rule = self.row_to_rule(&row)?;
            let expression = row.get::<String, _>("expression");

            // Parse expression to extract conditions and actions for compatibility
            // If parsing fails, disable the rule and use empty conditions/actions
            let (conditions, actions, final_rule) =
                match self.parse_expression_for_compatibility(&expression, &rule.source_type) {
                    Ok((conditions, actions)) => (conditions, actions, rule),
                    Err(e) => {
                        warn!(
                            "Failed to parse expression for rule '{}': {}. Rule will be disabled.",
                            rule.name, e
                        );
                        let mut disabled_rule = rule;
                        disabled_rule.is_active = false;
                        (vec![], vec![], disabled_rule)
                    }
                };

            rules.push(DataMappingRuleWithDetails {
                rule: final_rule,
                conditions,
                actions,
                expression: Some(expression),
            });
        }

        Ok(rules)
    }

    pub async fn get_active_rules(&self) -> Result<Vec<DataMappingRuleWithDetails>, sqlx::Error> {
        let rows = sqlx::query(
            r#"
            SELECT id, name, description, source_type, scope, expression, sort_order, is_active, created_at, updated_at
            FROM data_mapping_rules
            WHERE is_active = TRUE
            ORDER BY sort_order ASC
            "#
        )
        .fetch_all(&self.pool)
        .await?;

        let mut rules = Vec::new();
        for row in rows {
            let rule = self.row_to_rule(&row)?;
            let expression = row.get::<String, _>("expression");

            // Parse expression to extract conditions and actions for compatibility
            // If parsing fails, disable the rule and use empty conditions/actions
            let (conditions, actions, final_rule) =
                match self.parse_expression_for_compatibility(&expression, &rule.source_type) {
                    Ok((conditions, actions)) => (conditions, actions, rule),
                    Err(e) => {
                        warn!(
                            "Failed to parse expression for rule '{}': {}. Rule will be disabled.",
                            rule.name, e
                        );
                        let mut disabled_rule = rule;
                        disabled_rule.is_active = false;
                        (vec![], vec![], disabled_rule)
                    }
                };

            rules.push(DataMappingRuleWithDetails {
                rule: final_rule,
                conditions,
                actions,
                expression: Some(expression),
            });
        }

        Ok(rules)
    }

    pub async fn reorder_rules(&self, rule_orders: Vec<(Uuid, i32)>) -> Result<(), sqlx::Error> {
        let mut tx = self.pool.begin().await?;

        for (rule_id, sort_order) in rule_orders {
            sqlx::query("UPDATE data_mapping_rules SET sort_order = ? WHERE id = ?")
                .bind(sort_order)
                .bind(rule_id.to_string())
                .execute(&mut *tx)
                .await?;
        }

        tx.commit().await?;
        Ok(())
    }

    pub async fn test_rule(
        &self,
        request: DataMappingTestRequest,
        channels: Vec<Channel>,
        logo_assets: HashMap<Uuid, LogoAsset>,
        base_url: &str,
    ) -> Result<DataMappingTestResult, Box<dyn std::error::Error>> {
        // Parse the expression
        let available_fields = match request.source_type {
            DataMappingSourceType::Stream => StreamMappingFields::available_fields()
                .into_iter()
                .map(|s| s.to_string())
                .collect(),
            DataMappingSourceType::Epg => EpgMappingFields::available_fields()
                .into_iter()
                .map(|s| s.to_string())
                .collect(),
        };

        let parser = FilterParser::new().with_fields(available_fields);
        let parsed_expression = parser.parse_extended(&request.expression)?;

        // Validate expression
        parser.validate_extended(&parsed_expression)?;

        // Create a temporary rule for testing
        let test_rule = DataMappingRuleWithDetails {
            rule: DataMappingRule {
                id: Uuid::new_v4(),
                name: "Test Rule".to_string(),
                description: Some("Temporary rule for testing".to_string()),
                source_type: request.source_type,
                scope: DataMappingRuleScope::Individual,
                sort_order: 0,
                is_active: true,
                created_at: Utc::now(),
                updated_at: Utc::now(),
            },
            conditions: vec![], // Will be filled by expression parsing
            actions: vec![],    // Will be filled by expression parsing
            expression: Some(request.expression.clone()),
        };

        // Use the engine to test the rule
        let mut engine = DataMappingEngine::new();
        let total_channels = channels.len();
        let test_rule_clone = test_rule.clone();
        let mapped_channels = engine.apply_mapping_rules(
            channels,
            vec![test_rule],
            logo_assets,
            Uuid::new_v4(), // Dummy source ID for testing
            base_url,
        )?;

        // Convert to test result format
        let matching_channels: Vec<DataMappingTestChannel> = mapped_channels
            .into_iter()
            .filter(|mc| !mc.applied_rules.is_empty()) // Only include channels that were affected
            .map(|mc| {
                let mut original_values = HashMap::new();
                let mut mapped_values = HashMap::new();

                // Compare original vs mapped values
                original_values.insert(
                    "channel_name".to_string(),
                    Some(mc.original.channel_name.clone()),
                );
                mapped_values.insert(
                    "channel_name".to_string(),
                    Some(mc.mapped_channel_name.clone()),
                );

                if let Some(ref orig_tvg_id) = mc.original.tvg_id {
                    original_values.insert("tvg_id".to_string(), Some(orig_tvg_id.clone()));
                }
                if let Some(ref mapped_tvg_id) = mc.mapped_tvg_id {
                    mapped_values.insert("tvg_id".to_string(), Some(mapped_tvg_id.clone()));
                }

                if let Some(ref orig_group) = mc.original.group_title {
                    original_values.insert("group_title".to_string(), Some(orig_group.clone()));
                }
                if let Some(ref mapped_group) = mc.mapped_group_title {
                    mapped_values.insert("group_title".to_string(), Some(mapped_group.clone()));
                }

                // Helper function to get resolved value from mapped channel
                let get_resolved_value = |field: &str| -> Option<String> {
                    match field {
                        "channel_name" => Some(mc.mapped_channel_name.clone()),
                        "tvg_id" => mc.mapped_tvg_id.clone(),
                        "tvg_name" => mc.mapped_tvg_name.clone(),
                        "tvg_logo" => mc.mapped_tvg_logo.clone(),
                        "tvg_shift" => mc.mapped_tvg_shift.clone(),
                        "group_title" => mc.mapped_group_title.clone(),
                        _ => None,
                    }
                };

                // Create meaningful action descriptions with resolved values
                let applied_actions = if mc.applied_rules.is_empty() {
                    vec![]
                } else {
                    test_rule_clone
                        .actions
                        .iter()
                        .map(|action| match action.action_type {
                            DataMappingActionType::SetValue => {
                                let template = action.value.as_deref().unwrap_or("");
                                let resolved_value = get_resolved_value(&action.target_field);
                                if template.contains('$') && resolved_value.is_some() {
                                    format!(
                                        "Set {} = {} ('{}')",
                                        action.target_field,
                                        template,
                                        resolved_value.unwrap()
                                    )
                                } else {
                                    format!("Set {} = {}", action.target_field, template)
                                }
                            }
                            DataMappingActionType::SetDefaultIfEmpty => {
                                let template = action.value.as_deref().unwrap_or("");
                                let resolved_value = get_resolved_value(&action.target_field);
                                if template.contains('$') && resolved_value.is_some() {
                                    format!(
                                        "Set default {} = {} ('{}')",
                                        action.target_field,
                                        template,
                                        resolved_value.unwrap()
                                    )
                                } else {
                                    format!("Set default {} = {}", action.target_field, template)
                                }
                            }
                            DataMappingActionType::SetLogo => {
                                format!("Set logo for {}", action.target_field)
                            }
                            DataMappingActionType::TimeshiftEpg => {
                                format!(
                                    "Timeshift EPG by {} minutes",
                                    action.timeshift_minutes.unwrap_or(0)
                                )
                            }
                            DataMappingActionType::DeduplicateStreamUrls => {
                                "Deduplicate stream URLs".to_string()
                            }
                            DataMappingActionType::RemoveChannel => "Remove channel".to_string(),
                        })
                        .collect()
                };

                DataMappingTestChannel {
                    channel_name: mc.mapped_channel_name,
                    group_title: mc.mapped_group_title,
                    original_values,
                    mapped_values,
                    applied_actions,
                }
            })
            .collect();

        Ok(DataMappingTestResult {
            is_valid: true,
            error: None,
            total_channels,
            matched_count: matching_channels.len(),
            matching_channels,
        })
    }

    // Helper methods

    fn row_to_rule(&self, row: &sqlx::sqlite::SqliteRow) -> Result<DataMappingRule, sqlx::Error> {
        let source_type_str = row.get::<String, _>("source_type");
        let source_type = match source_type_str.as_str() {
            "stream" => DataMappingSourceType::Stream,
            "epg" => DataMappingSourceType::Epg,
            _ => DataMappingSourceType::Stream, // Default fallback
        };

        let scope_str = row.get::<String, _>("scope");
        let scope = match scope_str.as_str() {
            "individual" => DataMappingRuleScope::Individual,
            "streamwide" => DataMappingRuleScope::StreamWide,
            "epgwide" => DataMappingRuleScope::EpgWide,
            _ => DataMappingRuleScope::Individual, // Default fallback
        };

        Ok(DataMappingRule {
            id: Uuid::parse_str(&row.get::<String, _>("id"))
                .map_err(|_| sqlx::Error::Decode("Invalid UUID".into()))?,
            name: row.get("name"),
            description: row.try_get("description").ok(),
            source_type,
            scope,
            sort_order: row.get("sort_order"),
            is_active: row.get("is_active"),
            created_at: {
                let dt_str = row.get::<String, _>("created_at");
                // Try RFC3339 first, then SQLite format
                chrono::DateTime::parse_from_rfc3339(&dt_str)
                    .or_else(|_| {
                        chrono::NaiveDateTime::parse_from_str(&dt_str, "%Y-%m-%d %H:%M:%S")
                            .map(|ndt| ndt.and_utc().fixed_offset())
                    })
                    .map_err(|_| sqlx::Error::Decode("Invalid datetime".into()))?
                    .with_timezone(&chrono::Utc)
            },
            updated_at: {
                let dt_str = row.get::<String, _>("updated_at");
                // Try RFC3339 first, then SQLite format
                chrono::DateTime::parse_from_rfc3339(&dt_str)
                    .or_else(|_| {
                        chrono::NaiveDateTime::parse_from_str(&dt_str, "%Y-%m-%d %H:%M:%S")
                            .map(|ndt| ndt.and_utc().fixed_offset())
                    })
                    .map_err(|_| sqlx::Error::Decode("Invalid datetime".into()))?
                    .with_timezone(&chrono::Utc)
            },
        })
    }

    fn parse_expression_for_compatibility(
        &self,
        expression: &str,
        source_type: &DataMappingSourceType,
    ) -> Result<(Vec<DataMappingCondition>, Vec<DataMappingAction>), String> {
        // Get available fields for this source type
        let available_fields = match source_type {
            DataMappingSourceType::Stream => StreamMappingFields::available_fields()
                .into_iter()
                .map(|s| s.to_string())
                .collect(),
            DataMappingSourceType::Epg => EpgMappingFields::available_fields()
                .into_iter()
                .map(|s| s.to_string())
                .collect(),
        };

        let parser = FilterParser::new().with_fields(available_fields);

        match parser.parse_extended(expression) {
            Ok(parsed) => {
                // Convert parsed expression back to conditions and actions for compatibility
                let (conditions, actions) = self
                    .extract_conditions_and_actions_from_expression(parsed)
                    .map_err(|e| e.to_string())?;
                Ok((conditions, actions))
            }
            Err(e) => {
                warn!("Failed to parse expression '{}': {}", expression, e);
                Err(e.to_string())
            }
        }
    }

    fn extract_conditions_and_actions_from_expression(
        &self,
        expression: ExtendedExpression,
    ) -> Result<(Vec<DataMappingCondition>, Vec<DataMappingAction>), sqlx::Error> {
        let now = Utc::now();

        match expression {
            ExtendedExpression::ConditionOnly(_) => {
                // No actions, only conditions (not common for data mapping)
                Ok((vec![], vec![]))
            }
            ExtendedExpression::ConditionWithActions { condition, actions } => {
                // Simple case: one set of conditions with actions
                let flat_conditions =
                    self.flatten_condition_tree_for_compatibility(&condition, &now)?;
                let converted_actions = self.convert_actions_for_compatibility(&actions, &now)?;
                Ok((flat_conditions, converted_actions))
            }
            ExtendedExpression::ConditionalActionGroups(groups) => {
                // Complex case: multiple condition-action groups
                // For compatibility, we'll flatten to the first group's conditions and all actions
                let mut all_conditions = vec![];
                let mut all_actions = vec![];

                for (group_idx, group) in groups.iter().enumerate() {
                    let mut group_conditions =
                        self.flatten_condition_tree_for_compatibility(&group.conditions, &now)?;
                    let group_actions =
                        self.convert_actions_for_compatibility(&group.actions, &now)?;

                    // Adjust sort order for conditions to maintain group separation
                    for condition in &mut group_conditions {
                        condition.sort_order += (group_idx * 100) as i32;
                    }

                    all_conditions.extend(group_conditions);
                    all_actions.extend(group_actions);
                }

                Ok((all_conditions, all_actions))
            }
        }
    }

    fn flatten_condition_tree_for_compatibility(
        &self,
        condition_tree: &crate::models::ConditionTree,
        now: &chrono::DateTime<chrono::Utc>,
    ) -> Result<Vec<DataMappingCondition>, sqlx::Error> {
        let mut conditions = vec![];
        self.flatten_condition_node_for_compatibility(
            &condition_tree.root,
            &mut conditions,
            None,
            now,
        );
        Ok(conditions)
    }

    fn flatten_condition_node_for_compatibility(
        &self,
        node: &crate::models::ConditionNode,
        conditions: &mut Vec<DataMappingCondition>,
        logical_op: Option<crate::models::LogicalOperator>,
        now: &chrono::DateTime<chrono::Utc>,
    ) {
        match node {
            crate::models::ConditionNode::Condition {
                field,
                operator,
                value,
                case_sensitive: _,
                negate,
            } => {
                let final_operator = if *negate {
                    match operator {
                        crate::models::FilterOperator::Equals => {
                            crate::models::FilterOperator::NotEquals
                        }
                        crate::models::FilterOperator::Contains => {
                            crate::models::FilterOperator::NotContains
                        }
                        crate::models::FilterOperator::Matches => {
                            crate::models::FilterOperator::NotMatches
                        }
                        crate::models::FilterOperator::NotEquals => {
                            crate::models::FilterOperator::Equals
                        }
                        crate::models::FilterOperator::NotContains => {
                            crate::models::FilterOperator::Contains
                        }
                        crate::models::FilterOperator::NotMatches => {
                            crate::models::FilterOperator::Matches
                        }
                        _ => operator.clone(),
                    }
                } else {
                    operator.clone()
                };

                conditions.push(DataMappingCondition {
                    id: Uuid::new_v4(),
                    rule_id: Uuid::new_v4(), // Will be overridden when actually used
                    field_name: field.clone(),
                    operator: final_operator,
                    value: value.clone(),
                    logical_operator: logical_op,
                    sort_order: conditions.len() as i32,
                    created_at: *now,
                });
            }
            crate::models::ConditionNode::Group { operator, children } => {
                for (i, child) in children.iter().enumerate() {
                    let child_logical_op = if i == 0 {
                        logical_op.clone()
                    } else {
                        Some(operator.clone())
                    };
                    self.flatten_condition_node_for_compatibility(
                        child,
                        conditions,
                        child_logical_op,
                        now,
                    );
                }
            }
        }
    }

    fn convert_actions_for_compatibility(
        &self,
        actions: &[crate::models::Action],
        now: &chrono::DateTime<chrono::Utc>,
    ) -> Result<Vec<DataMappingAction>, sqlx::Error> {
        Ok(actions
            .iter()
            .enumerate()
            .filter_map(|(i, action)| {
                let action_type = match action.operator {
                    crate::models::ActionOperator::Set => DataMappingActionType::SetValue,
                    crate::models::ActionOperator::SetIfEmpty => {
                        DataMappingActionType::SetDefaultIfEmpty
                    }
                    _ => return None, // Skip unsupported operators
                };

                let value = match &action.value {
                    crate::models::ActionValue::Literal(v) => Some(v.clone()),
                    _ => None, // Skip complex values
                };

                Some(DataMappingAction {
                    id: Uuid::new_v4(),
                    rule_id: Uuid::new_v4(), // Will be overridden when actually used
                    action_type,
                    target_field: action.field.clone(),
                    value,
                    logo_asset_id: None, // TODO: Extract from special syntax like @logo:name
                    timeshift_minutes: None,
                    sort_order: i as i32,
                    created_at: *now,
                })
            })
            .collect())
    }

    /// Apply data mapping rules and return mapped channels with metadata for preview/testing.
    /// This is the core mapping function that returns full MappedChannel objects.
    pub async fn apply_mapping_with_metadata(
        &self,
        channels: Vec<Channel>,
        source_id: Uuid,
        logo_service: &LogoAssetService,
        base_url: &str,
        engine_config: Option<crate::config::DataMappingEngineConfig>,
    ) -> Result<
        (
            Vec<crate::models::data_mapping::MappedChannel>,
            std::collections::HashMap<String, (u128, u128, usize)>,
        ),
        anyhow::Error,
    > {
        use std::collections::HashMap;

        tracing::info!(
            "Applying data mapping with metadata - source {} with {} channels",
            source_id,
            channels.len()
        );

        // Get all active rules ordered by sort_order
        let rules = match self.get_active_rules().await {
            Ok(rules) => rules,
            Err(e) => {
                error!("Failed to load data mapping rules: {}", e);
                return Err(anyhow::anyhow!("Failed to load data mapping rules: {}", e));
            }
        };

        if rules.is_empty() {
            tracing::info!("No data mapping rules found, returning original channels as mapped");
            // Convert original channels to MappedChannel format with no modifications
            let mapped_channels: Vec<crate::models::data_mapping::MappedChannel> = channels
                .into_iter()
                .map(|channel| crate::models::data_mapping::MappedChannel {
                    mapped_tvg_id: channel.tvg_id.clone(),
                    mapped_tvg_name: channel.tvg_name.clone(),
                    mapped_tvg_logo: channel.tvg_logo.clone(),
                    mapped_tvg_shift: channel.tvg_shift.clone(),
                    mapped_group_title: channel.group_title.clone(),
                    mapped_channel_name: channel.channel_name.clone(),
                    applied_rules: Vec::new(),
                    is_removed: false,
                    original: channel,
                })
                .collect();

            // Return empty performance stats since no rules were executed
            let empty_stats = HashMap::new();
            return Ok((mapped_channels, empty_stats));
        }

        // Load logo assets
        let logo_assets = match self.load_logo_assets(logo_service, base_url).await {
            Ok(assets) => assets,
            Err(e) => {
                warn!(
                    "Failed to load logo assets: {}, continuing without logos",
                    e
                );
                HashMap::new()
            }
        };

        // Apply mapping rules
        let mut engine = if let Some(config) = engine_config {
            DataMappingEngine::with_config(config.into())
        } else {
            DataMappingEngine::new()
        };

        match engine.apply_mapping_rules(channels.clone(), rules, logo_assets, source_id, base_url)
        {
            Ok(mapped_channels) => {
                // Capture stats after processing but before they're cleared
                let rule_performance = engine.get_rule_performance_summary();
                info!(
                    "Data mapping completed for source {}: {} channels processed",
                    source_id,
                    mapped_channels.len()
                );
                Ok((mapped_channels, rule_performance))
            }
            Err(e) => {
                error!("Data mapping failed for source {}: {}", source_id, e);
                Err(anyhow::anyhow!("Data mapping failed: {}", e))
            }
        }
    }

    /// Apply data mapping rules to channels for proxy generation.
    /// This method creates transformed channels without modifying the original data stored in the database.
    /// The original ingested data remains unchanged, and transformations are only applied during proxy generation.
    pub async fn apply_mapping_for_proxy(
        &self,
        channels: Vec<Channel>,
        source_id: Uuid,
        logo_service: &crate::logo_assets::LogoAssetService,
        base_url: &str,
        engine_config: Option<crate::config::DataMappingEngineConfig>,
    ) -> Result<Vec<Channel>, anyhow::Error> {
        // Use the core mapping function and convert to final channels
        let (mapped_channels, _stats) = self
            .apply_mapping_with_metadata(
                channels.clone(),
                source_id,
                logo_service,
                base_url,
                engine_config,
            )
            .await?;

        // Convert to final channels (for proxy generation)
        let result_channels = DataMappingEngine::mapped_to_channels(mapped_channels);
        tracing::info!(
            "Data mapping for proxy generation completed successfully for source {}",
            source_id
        );
        Ok(result_channels)
    }

    /// Filter mapped channels to show only those modified by rules (for preview).
    pub fn filter_modified_channels(
        mapped_channels: Vec<crate::models::data_mapping::MappedChannel>,
    ) -> Vec<crate::models::data_mapping::MappedChannel> {
        mapped_channels
            .into_iter()
            .filter(|channel| !channel.applied_rules.is_empty())
            .collect()
    }

    /// Load logo assets for data mapping
    async fn load_logo_assets(
        &self,
        logo_service: &crate::logo_assets::LogoAssetService,
        base_url: &str,
    ) -> Result<HashMap<Uuid, LogoAsset>, anyhow::Error> {
        use std::collections::HashMap;

        let logo_list = logo_service
            .list_assets(
                crate::models::logo_asset::LogoAssetListRequest {
                    search: None,
                    asset_type: None,
                    page: Some(1),
                    limit: Some(1000), // Get a reasonable number of logos
                },
                base_url,
            )
            .await?;

        let mut logo_map = HashMap::new();
        for logo_with_url in logo_list.assets {
            logo_map.insert(logo_with_url.asset.id, logo_with_url.asset);
        }

        tracing::info!("Loaded {} logo assets for data mapping", logo_map.len());
        Ok(logo_map)
    }
}
