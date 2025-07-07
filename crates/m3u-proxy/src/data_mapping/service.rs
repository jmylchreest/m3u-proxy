use crate::data_mapping::engine::DataMappingEngine;
use crate::filter_parser::FilterParser;
use crate::logo_assets::LogoAssetService;
use crate::models::data_mapping::*;
use crate::models::{logo_asset::LogoAsset, Channel};
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
    ) -> Result<DataMappingRule, sqlx::Error> {
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

        // Return the created rule
        self.get_rule_with_details(rule_id).await
    }

    pub async fn update_rule(
        &self,
        rule_id: Uuid,
        request: DataMappingRuleUpdateRequest,
    ) -> Result<DataMappingRule, sqlx::Error> {
        let now = Utc::now();

        // Validate expression before saving
        if let Err(e) = request.validate_expression() {
            error!("Invalid expression for rule update: {}", e);
            return Err(sqlx::Error::Decode(
                format!("Invalid expression: {}", e).into(),
            ));
        }

        let mut tx = self.pool.begin().await?;

        // Update rule
        let result = sqlx::query(
            r#"
            UPDATE data_mapping_rules
            SET name = COALESCE(?, name),
                description = COALESCE(?, description),
                source_type = COALESCE(?, source_type),
                expression = COALESCE(?, expression),
                is_active = COALESCE(?, is_active),
                updated_at = ?
            WHERE id = ?
            "#,
        )
        .bind(&request.name)
        .bind(&request.description)
        .bind(request.source_type.as_ref().map(|s| s.to_string()))
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

        // Return the updated rule
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
    ) -> Result<DataMappingRule, sqlx::Error> {
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

        self.row_to_rule(&row)
    }

    pub async fn get_all_rules(&self) -> Result<Vec<DataMappingRule>, sqlx::Error> {
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
            rules.push(self.row_to_rule(&row)?);
        }

        Ok(rules)
    }

    pub async fn get_active_rules(&self) -> Result<Vec<DataMappingRule>, sqlx::Error> {
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
            rules.push(self.row_to_rule(&row)?);
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
        // Validate the expression syntax
        if let Err(e) = self.validate_expression(&request.expression, &request.source_type) {
            return Ok(DataMappingTestResult {
                is_valid: false,
                error: Some(e),
                matching_channels: vec![],
                total_channels: channels.len() as i32,
                matched_count: 0,
            });
        }

        // Create a temporary rule for testing
        let test_rule = DataMappingRule {
            id: Uuid::new_v4(),
            name: "Test Rule".to_string(),
            description: Some("Temporary rule for testing".to_string()),
            source_type: request.source_type,
            scope: DataMappingRuleScope::Individual,
            sort_order: 0,
            is_active: true,
            expression: Some(request.expression.clone()),
            created_at: Utc::now(),
            updated_at: Utc::now(),
        };

        // Use the engine to test the rule
        let mut engine = DataMappingEngine::new();
        let total_channels = channels.len();
        let (mapped_channels, _rule_performance) = engine.apply_mapping_rules(
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
                // Create simplified original and mapped values
                let original_values = serde_json::json!({
                    "channel_name": mc.original.channel_name,
                    "tvg_id": mc.original.tvg_id,
                    "tvg_name": mc.original.tvg_name,
                    "tvg_logo": mc.original.tvg_logo,
                    "group_title": mc.original.group_title,
                });

                let mapped_values = serde_json::json!({
                    "channel_name": mc.mapped_channel_name,
                    "tvg_id": mc.mapped_tvg_id,
                    "tvg_name": mc.mapped_tvg_name,
                    "tvg_logo": mc.mapped_tvg_logo,
                    "group_title": mc.mapped_group_title,
                });

                DataMappingTestChannel {
                    channel_name: mc.mapped_channel_name,
                    group_title: mc.mapped_group_title,
                    original_values,
                    mapped_values,
                    applied_actions: mc.applied_rules,
                }
            })
            .collect();

        let matched_count = matching_channels.len();

        Ok(DataMappingTestResult {
            is_valid: true,
            error: None,
            matching_channels,
            total_channels: total_channels as i32,
            matched_count: matched_count as i32,
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
            expression: row.try_get("expression").ok(),
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

    fn validate_expression(
        &self,
        expression: &str,
        source_type: &DataMappingSourceType,
    ) -> Result<(), String> {
        if expression.trim().is_empty() {
            return Err("Expression cannot be empty".to_string());
        }

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

        // Parse and validate the expression
        match parser.parse_extended(expression) {
            Ok(parsed) => {
                parser
                    .validate_extended(&parsed)
                    .map_err(|e| e.to_string())?;
                Ok(())
            }
            Err(e) => Err(format!("Expression syntax error: {}", e)),
        }
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
            std::collections::HashMap<Uuid, (u128, u128, usize)>,
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
                    capture_group_values: std::collections::HashMap::new(),
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
            Ok((mapped_channels, rule_performance)) => {
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
        let result_channels = mapped_channels
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
            .collect();
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
                    include_cached: Some(true),
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
