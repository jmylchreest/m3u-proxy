//! SeaORM-based DataMappingService implementation
//!
//! This module provides a clean SeaORM-based implementation of the DataMappingService
//! with the essential methods needed by the web API.

use crate::database::repositories::DataMappingRuleSeaOrmRepository;
use crate::field_registry::FieldRegistry;
use crate::models::data_mapping::*;
use crate::pipeline::engines::DataMappingValidator;
use anyhow::Result;
use regex::Regex;
use sea_orm::DatabaseConnection;
use tracing::error;
use uuid::Uuid;

/// SeaORM-based data mapping service
#[derive(Clone)]
pub struct SeaOrmDataMappingService {
    repository: DataMappingRuleSeaOrmRepository,
}

// Canonicalize field aliases (e.g. program_* -> programme_*) for storage.
// This operates purely on token word boundaries to avoid touching string literal contents.
pub(crate) fn canonicalize_expression(expr: &str) -> String {
    let registry = FieldRegistry::global();
    let alias_map = registry.alias_map();

    // Collect alias->canonical pairs (owning Strings) and sort by descending alias length
    let mut pairs: Vec<(String, String)> = alias_map
        .iter()
        .map(|(a, c)| (a.to_string(), c.to_string()))
        .collect();
    pairs.sort_by(|(a, _), (b, _)| b.len().cmp(&a.len()));

    let mut result = expr.to_string();
    for (alias, canonical) in pairs {
        if alias == canonical {
            continue;
        }
        // Word boundary replacement so we only replace standalone field tokens.
        let pattern = format!(r"\b{}\b", regex::escape(&alias));
        if let Ok(re) = Regex::new(&pattern) {
            result = re.replace_all(&result, canonical.as_str()).into_owned();
        }
    }
    result
}

impl SeaOrmDataMappingService {
    pub fn new(connection: std::sync::Arc<DatabaseConnection>) -> Self {
        let repository = DataMappingRuleSeaOrmRepository::new(connection);
        Self { repository }
    }

    /// Create a new data mapping rule
    pub async fn create_rule(
        &self,
        mut request: DataMappingRuleCreateRequest,
    ) -> Result<DataMappingRule> {
        // Validate expression if provided
        if let Some(ref expression) = request.expression {
            let validation =
                DataMappingValidator::validate_expression(expression, &request.source_type);
            if !validation.is_valid {
                error!(
                    "Invalid expression for rule '{}': {:?}",
                    request.name, validation.error
                );
                return Err(anyhow::anyhow!(
                    "Invalid expression: {:?}",
                    validation.error.unwrap_or_default()
                ));
            }
        }

        // Canonicalize aliases (program_* -> programme_*, etc.) after validation
        if let Some(ref mut expression) = request.expression {
            *expression = canonicalize_expression(expression);
        }

        self.repository.create(request).await
    }

    /// Get all data mapping rules
    pub async fn get_all_rules(&self) -> Result<Vec<DataMappingRule>> {
        self.repository.list_all().await
    }

    /// Get a specific rule by ID
    pub async fn get_rule_with_details(&self, rule_id: Uuid) -> Result<Option<DataMappingRule>> {
        self.repository.find_by_id(&rule_id).await
    }

    /// Update a data mapping rule
    pub async fn update_rule(
        &self,
        rule_id: Uuid,
        mut request: DataMappingRuleUpdateRequest,
    ) -> Result<DataMappingRule> {
        // Validate expression if provided (needs source_type context if changed)
        if let Some(ref expression) = request.expression
            && let Some(ref source_type) = request.source_type
        {
            let validation = DataMappingValidator::validate_expression(expression, source_type);
            if !validation.is_valid {
                error!("Invalid expression for rule update: {:?}", validation.error);
                return Err(anyhow::anyhow!(
                    "Invalid expression: {:?}",
                    validation.error.unwrap_or_default()
                ));
            }
        }

        // Canonicalize aliases if an expression is provided
        if let Some(ref mut expression) = request.expression {
            *expression = canonicalize_expression(expression);
        }

        self.repository.update(&rule_id, request).await
    }

    /// Delete a data mapping rule
    pub async fn delete_rule(&self, rule_id: Uuid) -> Result<()> {
        self.repository.delete(&rule_id).await
    }

    /// Reorder rules (simplified implementation)
    pub async fn reorder_rules(&self, _rule_orders: Vec<(Uuid, i32)>) -> Result<()> {
        // TODO: Implement reordering logic if needed
        // For now, just return Ok to satisfy the API
        Ok(())
    }

    /// Apply mapping with metadata (simplified implementation)
    pub async fn apply_mapping_with_metadata(
        &self,
        channels: Vec<crate::models::Channel>,
        _source_uuid: uuid::Uuid,
        _logo_asset_service: &crate::logo_assets::service::LogoAssetService,
        _base_url: &str,
        _data_mapping_engine: Option<String>,
    ) -> Result<(
        Vec<crate::models::Channel>,
        std::collections::HashMap<String, u64>,
    )> {
        // For now, return channels as-is since the current data model
        // doesn't have the action/target_field/value structure needed
        // for complex mapping operations.
        // TODO: Implement when data model is extended with mapping fields
        Ok((channels, std::collections::HashMap::new()))
    }

    /// Filter channels that were modified by mapping
    pub fn filter_modified_channels(
        channels: Vec<crate::models::Channel>,
    ) -> Vec<crate::models::Channel> {
        // For simplicity, return all channels as potentially modified
        // In a real implementation, you would track which channels were actually changed
        channels
    }
}
