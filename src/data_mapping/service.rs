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
            let operator_str = format!("{:?}", condition.operator).to_lowercase();
            let logical_operator_str = condition
                .logical_operator
                .as_ref()
                .map(|op| format!("{:?}", op).to_lowercase());

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
                DataMappingActionType::DeduplicateClonedChannel => "deduplicate_cloned_channel",
                DataMappingActionType::TimeshiftEpg => "timeshift_epg",
                DataMappingActionType::DeduplicateStreamUrls => "deduplicate_stream_urls",
            };

            sqlx::query(
                r#"
                INSERT INTO data_mapping_actions (id, rule_id, action_type, target_field, value, logo_asset_id, timeshift_minutes, similarity_threshold, sort_order, created_at)
                VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
                "#
            )
            .bind(action_id.to_string())
            .bind(rule_id.to_string())
            .bind(action_type_str)
            .bind(&action.target_field)
            .bind(&action.value)
            .bind(action.logo_asset_id.map(|id| id.to_string()))
            .bind(action.timeshift_minutes)
            .bind(action.similarity_threshold)
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
            let operator_str = format!("{:?}", condition.operator).to_lowercase();
            let logical_operator_str = condition
                .logical_operator
                .as_ref()
                .map(|op| format!("{:?}", op).to_lowercase());

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
                DataMappingActionType::DeduplicateClonedChannel => "deduplicate_cloned_channel",
                DataMappingActionType::TimeshiftEpg => "timeshift_epg",
                DataMappingActionType::DeduplicateStreamUrls => "deduplicate_stream_urls",
            };

            sqlx::query(
                r#"
                INSERT INTO data_mapping_actions (id, rule_id, action_type, target_field, value, logo_asset_id, timeshift_minutes, similarity_threshold, sort_order, created_at)
                VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
                "#
            )
            .bind(action_id.to_string())
            .bind(rule_id.to_string())
            .bind(action_type_str)
            .bind(&action.target_field)
            .bind(&action.value)
            .bind(action.logo_asset_id.map(|id| id.to_string()))
            .bind(action.timeshift_minutes)
            .bind(action.similarity_threshold)
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
            let operator = match row.get::<String, _>("operator").as_str() {
                "matches" => crate::models::FilterOperator::Matches,
                "equals" => crate::models::FilterOperator::Equals,
                "contains" => crate::models::FilterOperator::Contains,
                "starts_with" => crate::models::FilterOperator::StartsWith,
                "ends_with" => crate::models::FilterOperator::EndsWith,
                "not_matches" => crate::models::FilterOperator::NotMatches,
                "not_equals" => crate::models::FilterOperator::NotEquals,
                "not_contains" => crate::models::FilterOperator::NotContains,
                _ => crate::models::FilterOperator::Equals,
            };

            let logical_operator =
                row.get::<Option<String>, _>("logical_operator")
                    .map(|op| match op.as_str() {
                        "and" => crate::models::LogicalOperator::And,
                        "or" => crate::models::LogicalOperator::Or,
                        _ => crate::models::LogicalOperator::And,
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
            SELECT id, rule_id, action_type, target_field, value, logo_asset_id, timeshift_minutes, similarity_threshold, sort_order, created_at
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
                "deduplicate_cloned_channel" => DataMappingActionType::DeduplicateClonedChannel,
                "timeshift_epg" => DataMappingActionType::TimeshiftEpg,
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
                similarity_threshold: row.get("similarity_threshold"),
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

    /// Apply data mapping rules to channels for proxy generation.
    /// This method creates transformed channels without modifying the original data stored in the database.
    /// The original ingested data remains unchanged, and transformations are only applied during proxy generation.
    pub async fn apply_mapping_for_proxy(
        &self,
        channels: Vec<Channel>,
        source_id: Uuid,
        logo_service: &LogoAssetService,
        base_url: &str,
    ) -> Result<Vec<Channel>, anyhow::Error> {
        info!(
            "Applying data mapping for proxy generation - source {} with {} channels",
            source_id,
            channels.len()
        );

        // Get all active rules ordered by sort_order
        let rules = match self.get_all_rules().await {
            Ok(rules) => rules,
            Err(e) => {
                error!("Failed to load data mapping rules: {}", e);
                warn!(
                    "Skipping data mapping due to rule loading error, returning original channels"
                );
                return Ok(channels);
            }
        };

        if rules.is_empty() {
            info!("No data mapping rules found, returning original channels");
            return Ok(channels);
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

        // Apply mapping rules - this creates transformed channels without affecting originals
        let mut engine = DataMappingEngine::new();
        match engine.apply_mapping_rules(channels.clone(), rules, logo_assets, source_id, base_url)
        {
            Ok(mapped_channels) => {
                let result_channels = DataMappingEngine::mapped_to_channels(mapped_channels);
                info!(
                    "Data mapping for proxy generation completed successfully for source {}",
                    source_id
                );
                Ok(result_channels)
            }
            Err(e) => {
                error!("Data mapping failed for source {}: {}", source_id, e);
                warn!("Returning original channels due to mapping error");
                // Return original channels as fallback
                Ok(channels)
            }
        }
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
