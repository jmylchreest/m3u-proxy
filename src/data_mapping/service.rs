use crate::data_mapping::engine::DataMappingEngine;
use crate::logo_assets::LogoAssetService;
use crate::models::data_mapping::*;
use crate::models::{
    logo_asset::{LogoAsset, LogoAssetListRequest},
    Channel,
};
use crate::utils;
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

        let mut tx = self.pool.begin().await?;

        // Insert rule
        sqlx::query(
            r#"
            INSERT INTO data_mapping_rules (id, name, description, sort_order, is_active, created_at, updated_at)
            VALUES (?, ?, ?, (SELECT COALESCE(MAX(sort_order), 0) + 1 FROM data_mapping_rules), TRUE, ?, ?)
            "#
        )
        .bind(rule_id.to_string())
        .bind(&request.name)
        .bind(&request.description)
        .bind(now.to_rfc3339())
        .bind(now.to_rfc3339())
        .execute(&mut *tx)
        .await?;

        // Insert conditions
        for (index, condition) in request.conditions.iter().enumerate() {
            let condition_id = Uuid::new_v4();
            let operator_str = serde_json::to_value(condition.operator.clone())
                .unwrap()
                .as_str()
                .unwrap()
                .to_string();
            let logical_operator_str = condition.logical_operator.as_ref().map(|op| {
                serde_json::to_value(op.clone())
                    .unwrap()
                    .as_str()
                    .unwrap()
                    .to_string()
            });

            sqlx::query(
                r#"
                INSERT INTO data_mapping_conditions (id, rule_id, field_name, operator, value, logical_operator, sort_order, created_at)
                VALUES (?, ?, ?, ?, ?, ?, ?, ?)
                "#
            )
            .bind(condition_id.to_string())
            .bind(rule_id.to_string())
            .bind(&condition.field_name)
            .bind(operator_str)
            .bind(&condition.value)
            .bind(logical_operator_str)
            .bind(index as i32)
            .bind(now.to_rfc3339())
            .execute(&mut *tx)
            .await?;
        }

        // Insert actions
        for (index, action) in request.actions.iter().enumerate() {
            let action_id = Uuid::new_v4();
            let action_type_str = match action.action_type {
                DataMappingActionType::SetValue => "set_value",
                DataMappingActionType::SetDefaultIfEmpty => "set_default_if_empty",
                DataMappingActionType::SetLogo => "set_logo",

                DataMappingActionType::TimeshiftEpg => "timeshift_epg",
                DataMappingActionType::DeduplicateStreamUrls => "deduplicate_stream_urls",
                DataMappingActionType::RemoveChannel => "remove_channel",
            };

            sqlx::query(
                r#"
                INSERT INTO data_mapping_actions (id, rule_id, action_type, target_field, value, logo_asset_id, timeshift_minutes, sort_order, created_at)
                VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)
                "#
            )
            .bind(action_id.to_string())
            .bind(rule_id.to_string())
            .bind(action_type_str)
            .bind(&action.target_field)
            .bind(&action.value)
            .bind(action.logo_asset_id.map(|id| id.to_string()))
            .bind(action.timeshift_minutes)
            .bind(index as i32)
            .bind(now.to_rfc3339())
            .execute(&mut *tx)
            .await?;
        }

        tx.commit().await?;

        self.get_rule_with_details(rule_id).await
    }

    pub async fn update_rule(
        &self,
        rule_id: Uuid,
        request: DataMappingRuleUpdateRequest,
    ) -> Result<DataMappingRuleWithDetails, sqlx::Error> {
        let mut tx = self.pool.begin().await?;

        // Update rule
        sqlx::query(
            r#"
            UPDATE data_mapping_rules
            SET name = ?, description = ?, is_active = ?, updated_at = ?
            WHERE id = ?
            "#,
        )
        .bind(&request.name)
        .bind(&request.description)
        .bind(request.is_active)
        .bind(Utc::now().to_rfc3339())
        .bind(rule_id.to_string())
        .execute(&mut *tx)
        .await?;

        // Delete existing conditions and actions
        sqlx::query("DELETE FROM data_mapping_conditions WHERE rule_id = ?")
            .bind(rule_id.to_string())
            .execute(&mut *tx)
            .await?;

        sqlx::query("DELETE FROM data_mapping_actions WHERE rule_id = ?")
            .bind(rule_id.to_string())
            .execute(&mut *tx)
            .await?;

        let now = Utc::now();

        // Insert new conditions
        for (index, condition) in request.conditions.iter().enumerate() {
            let condition_id = Uuid::new_v4();
            let operator_str = serde_json::to_value(condition.operator.clone())
                .unwrap()
                .as_str()
                .unwrap()
                .to_string();
            let logical_operator_str = condition.logical_operator.as_ref().map(|op| {
                serde_json::to_value(op.clone())
                    .unwrap()
                    .as_str()
                    .unwrap()
                    .to_string()
            });

            sqlx::query(
                r#"
                INSERT INTO data_mapping_conditions (id, rule_id, field_name, operator, value, logical_operator, sort_order, created_at)
                VALUES (?, ?, ?, ?, ?, ?, ?, ?)
                "#
            )
            .bind(condition_id.to_string())
            .bind(rule_id.to_string())
            .bind(&condition.field_name)
            .bind(operator_str)
            .bind(&condition.value)
            .bind(logical_operator_str)
            .bind(index as i32)
            .bind(now.to_rfc3339())
            .execute(&mut *tx)
            .await?;
        }

        // Insert new actions
        for (index, action) in request.actions.iter().enumerate() {
            let action_id = Uuid::new_v4();
            let action_type_str = match action.action_type {
                DataMappingActionType::SetValue => "set_value",
                DataMappingActionType::SetDefaultIfEmpty => "set_default_if_empty",
                DataMappingActionType::SetLogo => "set_logo",

                DataMappingActionType::TimeshiftEpg => "timeshift_epg",
                DataMappingActionType::DeduplicateStreamUrls => "deduplicate_stream_urls",
                DataMappingActionType::RemoveChannel => "remove_channel",
            };

            sqlx::query(
                r#"
                INSERT INTO data_mapping_actions (id, rule_id, action_type, target_field, value, logo_asset_id, timeshift_minutes, sort_order, created_at)
                VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)
                "#
            )
            .bind(action_id.to_string())
            .bind(rule_id.to_string())
            .bind(action_type_str)
            .bind(&action.target_field)
            .bind(&action.value)
            .bind(action.logo_asset_id.map(|id| id.to_string()))
            .bind(action.timeshift_minutes)
            .bind(index as i32)
            .bind(now.to_rfc3339())
            .execute(&mut *tx)
            .await?;
        }

        tx.commit().await?;

        self.get_rule_with_details(rule_id).await
    }

    pub async fn get_all_rules(&self) -> Result<Vec<DataMappingRuleWithDetails>, sqlx::Error> {
        let rows = sqlx::query(
            r#"
            SELECT id, name, description, sort_order, is_active, created_at, updated_at
            FROM data_mapping_rules
            ORDER BY sort_order
            "#,
        )
        .fetch_all(&self.pool)
        .await?;

        let mut rules_with_details = Vec::new();
        for row in rows {
            let rule_id_str: String = row.get("id");
            let rule_id = Uuid::parse_str(&rule_id_str).map_err(|e| sqlx::Error::ColumnDecode {
                index: "id".to_string(),
                source: Box::new(std::io::Error::new(std::io::ErrorKind::InvalidData, e)),
            })?;

            let rule = DataMappingRule {
                id: rule_id,
                name: row.get("name"),
                description: row.get("description"),
                source_type: DataMappingSourceType::Stream,
                scope: DataMappingRuleScope::Individual,
                sort_order: row.get("sort_order"),
                is_active: row.get("is_active"),
                created_at: utils::parse_datetime(&row.get::<String, _>("created_at"))?,
                updated_at: utils::parse_datetime(&row.get::<String, _>("updated_at"))?,
            };

            let conditions = self.get_rule_conditions(rule_id).await?;
            let actions = self.get_rule_actions(rule_id).await?;

            rules_with_details.push(DataMappingRuleWithDetails {
                rule,
                conditions,
                actions,
            });
        }

        Ok(rules_with_details)
    }

    pub async fn get_rule_with_details(
        &self,
        rule_id: Uuid,
    ) -> Result<DataMappingRuleWithDetails, sqlx::Error> {
        let row = sqlx::query(
            "SELECT id, name, description, sort_order, is_active, created_at, updated_at FROM data_mapping_rules WHERE id = ?"
        )
        .bind(rule_id.to_string())
        .fetch_one(&self.pool)
        .await?;

        let rule = DataMappingRule {
            id: rule_id,
            name: row.get("name"),
            description: row.get("description"),
            source_type: DataMappingSourceType::Stream,
            scope: DataMappingRuleScope::Individual,
            sort_order: row.get("sort_order"),
            is_active: row.get("is_active"),
            created_at: utils::parse_datetime(&row.get::<String, _>("created_at"))?,
            updated_at: utils::parse_datetime(&row.get::<String, _>("updated_at"))?,
        };

        let conditions = self.get_rule_conditions(rule_id).await?;
        let actions = self.get_rule_actions(rule_id).await?;

        Ok(DataMappingRuleWithDetails {
            rule,
            conditions,
            actions,
        })
    }

    pub async fn get_rule_conditions(
        &self,
        rule_id: Uuid,
    ) -> Result<Vec<DataMappingCondition>, sqlx::Error> {
        let rows = sqlx::query(
            r#"
            SELECT id, rule_id, field_name, operator, value, logical_operator, sort_order, created_at
            FROM data_mapping_conditions
            WHERE rule_id = ?
            ORDER BY sort_order
            "#
        )
        .bind(rule_id.to_string())
        .fetch_all(&self.pool)
        .await?;

        let mut conditions = Vec::new();
        for row in rows {
            let operator_str = row.get::<String, _>("operator");
            let operator = serde_json::from_str::<crate::models::FilterOperator>(&format!(
                "\"{}\"",
                operator_str
            ))
            .unwrap_or(crate::models::FilterOperator::Equals);

            let logical_operator = row.get::<Option<String>, _>("logical_operator").map(|op| {
                serde_json::from_str::<crate::models::LogicalOperator>(&format!("\"{}\"", op))
                    .unwrap_or(crate::models::LogicalOperator::And)
            });

            let condition_id_str: String = row.get("id");
            let condition_id =
                Uuid::parse_str(&condition_id_str).map_err(|e| sqlx::Error::ColumnDecode {
                    index: "id".to_string(),
                    source: Box::new(std::io::Error::new(std::io::ErrorKind::InvalidData, e)),
                })?;

            conditions.push(DataMappingCondition {
                id: condition_id,
                rule_id,
                field_name: row.get("field_name"),
                operator,
                value: row.get("value"),
                logical_operator,
                sort_order: row.get("sort_order"),
                created_at: utils::parse_datetime(&row.get::<String, _>("created_at"))?,
            });
        }

        Ok(conditions)
    }

    pub async fn get_rule_actions(
        &self,
        rule_id: Uuid,
    ) -> Result<Vec<DataMappingAction>, sqlx::Error> {
        let rows = sqlx::query(
            r#"
            SELECT id, rule_id, action_type, target_field, value, logo_asset_id, timeshift_minutes, sort_order, created_at
            FROM data_mapping_actions
            WHERE rule_id = ?
            ORDER BY sort_order
            "#
        )
        .bind(rule_id.to_string())
        .fetch_all(&self.pool)
        .await?;

        let mut actions = Vec::new();
        for row in rows {
            let action_type = match row.get::<String, _>("action_type").as_str() {
                "set_value" => DataMappingActionType::SetValue,
                "set_default_if_empty" => DataMappingActionType::SetDefaultIfEmpty,
                "set_logo" => DataMappingActionType::SetLogo,

                "timeshift_epg" => DataMappingActionType::TimeshiftEpg,
                "deduplicate_stream_urls" => DataMappingActionType::DeduplicateStreamUrls,
                "remove_channel" => DataMappingActionType::RemoveChannel,
                _ => DataMappingActionType::SetValue,
            };

            let logo_asset_id = row
                .get::<Option<String>, _>("logo_asset_id")
                .map(|id| Uuid::parse_str(&id))
                .transpose()
                .map_err(|e| sqlx::Error::ColumnDecode {
                    index: "logo_asset_id".to_string(),
                    source: Box::new(std::io::Error::new(std::io::ErrorKind::InvalidData, e)),
                })?;

            let action_id_str: String = row.get("id");
            let action_id =
                Uuid::parse_str(&action_id_str).map_err(|e| sqlx::Error::ColumnDecode {
                    index: "id".to_string(),
                    source: Box::new(std::io::Error::new(std::io::ErrorKind::InvalidData, e)),
                })?;

            actions.push(DataMappingAction {
                id: action_id,
                rule_id,
                action_type,
                target_field: row.get("target_field"),
                value: row.get("value"),
                logo_asset_id,
                timeshift_minutes: row.get("timeshift_minutes"),
                sort_order: row.get("sort_order"),
                created_at: utils::parse_datetime(&row.get::<String, _>("created_at"))?,
            });
        }

        Ok(actions)
    }

    pub async fn delete_rule(&self, rule_id: Uuid) -> Result<(), sqlx::Error> {
        sqlx::query("DELETE FROM data_mapping_rules WHERE id = ?")
            .bind(rule_id.to_string())
            .execute(&self.pool)
            .await?;

        Ok(())
    }

    pub async fn reorder_rules(&self, rule_orders: Vec<(Uuid, i32)>) -> Result<(), sqlx::Error> {
        let mut tx = self.pool.begin().await?;

        for (rule_id, sort_order) in rule_orders {
            sqlx::query(
                "UPDATE data_mapping_rules SET sort_order = ?, updated_at = ? WHERE id = ?",
            )
            .bind(sort_order)
            .bind(Utc::now().to_rfc3339())
            .bind(rule_id.to_string())
            .execute(&mut *tx)
            .await?;
        }

        tx.commit().await?;
        Ok(())
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
    ) -> Result<Vec<crate::models::data_mapping::MappedChannel>, anyhow::Error> {
        info!(
            "Applying data mapping with metadata - source {} with {} channels",
            source_id,
            channels.len()
        );

        // Get all active rules ordered by sort_order
        let rules = match self.get_all_rules().await {
            Ok(rules) => rules,
            Err(e) => {
                error!("Failed to load data mapping rules: {}", e);
                return Err(anyhow::anyhow!("Failed to load data mapping rules: {}", e));
            }
        };

        if rules.is_empty() {
            info!("No data mapping rules found, returning original channels as mapped");
            // Convert original channels to MappedChannel format with no modifications
            return Ok(channels
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
                .collect());
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
                info!(
                    "Data mapping with metadata completed successfully for source {}",
                    source_id
                );
                Ok(mapped_channels)
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
        logo_service: &LogoAssetService,
        base_url: &str,
        engine_config: Option<crate::config::DataMappingEngineConfig>,
    ) -> Result<Vec<Channel>, anyhow::Error> {
        // Use the core mapping function and convert to final channels
        let mapped_channels = self
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
        info!(
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

    /// Filter mapped channels to show final state (removes deleted channels, keeps all others).
    pub fn filter_final_channels(
        mapped_channels: Vec<crate::models::data_mapping::MappedChannel>,
    ) -> Vec<crate::models::data_mapping::MappedChannel> {
        mapped_channels
            .into_iter()
            .filter(|channel| !channel.is_removed)
            .collect()
    }

    /// Load logo assets for data mapping
    async fn load_logo_assets(
        &self,
        logo_service: &LogoAssetService,
        base_url: &str,
    ) -> Result<HashMap<Uuid, LogoAsset>, anyhow::Error> {
        let logo_list = logo_service
            .list_assets(
                LogoAssetListRequest {
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

        info!("Loaded {} logo assets for data mapping", logo_map.len());
        Ok(logo_map)
    }
}
