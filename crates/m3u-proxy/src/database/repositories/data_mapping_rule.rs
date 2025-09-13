//! SeaORM DataMappingRule repository implementation
//!
//! This module provides the SeaORM implementation of data mapping rule repository
//! that works across SQLite, PostgreSQL, and MySQL databases.

use anyhow::Result;
use sea_orm::{ActiveModelTrait, DatabaseConnection, EntityTrait, QueryOrder, Set};
use std::sync::Arc;
use uuid::Uuid;

use crate::entities::{data_mapping_rules, prelude::*};
use crate::models::data_mapping::{
    DataMappingRule, DataMappingRuleCreateRequest, DataMappingRuleUpdateRequest,
};

/// SeaORM-based DataMappingRule repository
#[derive(Clone)]
pub struct DataMappingRuleSeaOrmRepository {
    connection: Arc<DatabaseConnection>,
}

impl DataMappingRuleSeaOrmRepository {
    /// Create a new DataMappingRuleSeaOrmRepository
    pub fn new(connection: Arc<DatabaseConnection>) -> Self {
        Self { connection }
    }

    /// Create a new data mapping rule
    pub async fn create(&self, request: DataMappingRuleCreateRequest) -> Result<DataMappingRule> {
        let id = Uuid::new_v4();
        let now = chrono::Utc::now();

        let active_model = data_mapping_rules::ActiveModel {
            id: Set(id),
            name: Set(request.name.clone()),
            description: Set(request.description.clone()),
            source_type: Set(request.source_type),
            expression: Set(request.expression.clone()),
            sort_order: Set(0), // Default sort order for new rules
            is_active: Set(true),
            created_at: Set(now),
            updated_at: Set(now),
        };

        let model = active_model.insert(&*self.connection).await?;
        Ok(DataMappingRule {
            id: model.id,
            name: model.name,
            description: model.description,
            source_type: model.source_type,
            sort_order: model.sort_order,
            is_active: model.is_active,
            expression: model.expression,
            created_at: model.created_at,
            updated_at: model.updated_at,
        })
    }

    /// Find data mapping rule by ID
    pub async fn find_by_id(&self, id: &Uuid) -> Result<Option<DataMappingRule>> {
        let model = DataMappingRules::find_by_id(*id)
            .one(&*self.connection)
            .await?;
        match model {
            Some(m) => Ok(Some(DataMappingRule {
                id: m.id,
                name: m.name,
                description: m.description,
                source_type: m.source_type,
                sort_order: m.sort_order,
                is_active: m.is_active,
                expression: m.expression,
                created_at: m.created_at,
                updated_at: m.updated_at,
            })),
            None => Ok(None),
        }
    }

    /// List all data mapping rules
    pub async fn list_all(&self) -> Result<Vec<DataMappingRule>> {
        let models = DataMappingRules::find()
            .order_by_asc(data_mapping_rules::Column::SortOrder)
            .all(&*self.connection)
            .await?;

        let mut results = Vec::new();
        for m in models {
            results.push(DataMappingRule {
                id: m.id,
                name: m.name,
                description: m.description,
                source_type: m.source_type,
                sort_order: m.sort_order,
                is_active: m.is_active,
                expression: m.expression,
                created_at: m.created_at,
                updated_at: m.updated_at,
            });
        }
        Ok(results)
    }

    /// Update data mapping rule
    pub async fn update(
        &self,
        id: &Uuid,
        request: DataMappingRuleUpdateRequest,
    ) -> Result<DataMappingRule> {
        let model = DataMappingRules::find_by_id(*id)
            .one(&*self.connection)
            .await?
            .ok_or_else(|| anyhow::anyhow!("Data mapping rule not found"))?;

        let mut active_model: data_mapping_rules::ActiveModel = model.into();

        if let Some(name) = request.name {
            active_model.name = Set(name);
        }
        if let Some(description) = request.description {
            active_model.description = Set(Some(description));
        }
        if let Some(source_type) = request.source_type {
            active_model.source_type = Set(source_type);
        }
        if let Some(expression) = request.expression {
            active_model.expression = Set(Some(expression));
        }
        if let Some(is_active) = request.is_active {
            active_model.is_active = Set(is_active);
        }

        active_model.updated_at = Set(chrono::Utc::now());

        let updated_model = active_model.update(&*self.connection).await?;
        Ok(DataMappingRule {
            id: updated_model.id,
            name: updated_model.name,
            description: updated_model.description,
            source_type: updated_model.source_type,
            sort_order: updated_model.sort_order,
            is_active: updated_model.is_active,
            expression: updated_model.expression,
            created_at: updated_model.created_at,
            updated_at: updated_model.updated_at,
        })
    }

    /// Delete data mapping rule
    pub async fn delete(&self, id: &Uuid) -> Result<()> {
        let result = DataMappingRules::delete_by_id(*id)
            .exec(&*self.connection)
            .await?;
        if result.rows_affected == 0 {
            return Err(anyhow::anyhow!("Data mapping rule not found"));
        }
        Ok(())
    }
}
