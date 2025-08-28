//! SeaORM Filter repository implementation
//!
//! This module provides the SeaORM implementation of filter repository
//! that works across SQLite, PostgreSQL, and MySQL databases.

use anyhow::Result;
use sea_orm::{DatabaseConnection, EntityTrait, QueryOrder, ActiveModelTrait, Set, QueryFilter, ColumnTrait, PaginatorTrait};
use std::sync::Arc;
use uuid::Uuid;

use crate::entities::{filters, prelude::*};
use crate::models::{Filter, FilterCreateRequest, FilterUpdateRequest};

/// SeaORM-based Filter repository
#[derive(Clone)]
pub struct FilterSeaOrmRepository {
    connection: Arc<DatabaseConnection>,
}

impl FilterSeaOrmRepository {
    /// Create a new FilterSeaOrmRepository
    pub fn new(connection: Arc<DatabaseConnection>) -> Self {
        Self { connection }
    }

    /// Create a new filter
    pub async fn create(&self, request: FilterCreateRequest) -> Result<Filter> {
        let id = Uuid::new_v4();
        let now = chrono::Utc::now();

        let active_model = filters::ActiveModel {
            id: Set(id),
            name: Set(request.name.clone()),
            source_type: Set(request.source_type),
            is_inverse: Set(request.is_inverse),
            is_system_default: Set(false),
            expression: Set(request.expression.clone()),
            created_at: Set(now),
            updated_at: Set(now),
        };

        let model = active_model.insert(&*self.connection).await?;
        Ok(Filter {
            id: model.id,
            name: model.name,
            source_type: model.source_type,
            is_inverse: model.is_inverse,
            is_system_default: model.is_system_default,
            expression: model.expression,
            created_at: model.created_at,
            updated_at: model.updated_at,
        })
    }

    /// Find filter by ID
    pub async fn find_by_id(&self, id: Uuid) -> Result<Option<Filter>> {
        let model = Filters::find_by_id(id)
            .one(&*self.connection)
            .await?;

        match model {
            Some(m) => Ok(Some(Filter {
                id: m.id,
                name: m.name,
                source_type: m.source_type,
                is_inverse: m.is_inverse,
                is_system_default: m.is_system_default,
                expression: m.expression,
                created_at: m.created_at,
                updated_at: m.updated_at,
            })),
            None => Ok(None)
        }
    }

    /// List all filters
    pub async fn list_all(&self) -> Result<Vec<Filter>> {
        let models = Filters::find()
            .order_by_asc(filters::Column::Name)
            .all(&*self.connection)
            .await?;

        let mut results = Vec::new();
        for m in models {
            results.push(Filter {
                id: m.id,
                name: m.name,
                source_type: m.source_type,
                is_inverse: m.is_inverse,
                is_system_default: m.is_system_default,
                expression: m.expression,
                created_at: m.created_at,
                updated_at: m.updated_at,
            });
        }
        Ok(results)
    }

    /// Update filter
    pub async fn update(&self, id: &Uuid, request: FilterUpdateRequest) -> Result<Filter> {
        let model = Filters::find_by_id(*id)
            .one(&*self.connection)
            .await?
            .ok_or_else(|| anyhow::anyhow!("Filter not found"))?;

        let mut active_model: filters::ActiveModel = model.into();
        
        active_model.name = Set(request.name);
        active_model.expression = Set(request.expression);
        active_model.is_inverse = Set(request.is_inverse);
        active_model.source_type = Set(request.source_type);
        active_model.updated_at = Set(chrono::Utc::now());

        let updated_model = active_model.update(&*self.connection).await?;
        Ok(Filter {
            id: updated_model.id,
            name: updated_model.name,
            source_type: updated_model.source_type,
            is_inverse: updated_model.is_inverse,
            is_system_default: updated_model.is_system_default,
            expression: updated_model.expression,
            created_at: updated_model.created_at,
            updated_at: updated_model.updated_at,
        })
    }

    /// Delete filter
    pub async fn delete(&self, id: &Uuid) -> Result<()> {
        let result = Filters::delete_by_id(*id).exec(&*self.connection).await?;
        if result.rows_affected == 0 {
            return Err(anyhow::anyhow!("Filter not found"));
        }
        Ok(())
    }

    /// Get available filter fields for building filter expressions
    pub async fn get_available_filter_fields(&self) -> Result<Vec<crate::models::FilterFieldInfo>> {
        // Return the available fields that can be used in filter expressions
        Ok(vec![
            crate::models::FilterFieldInfo {
                name: "channel_name".to_string(),
                display_name: "Channel Name".to_string(),
                field_type: "string".to_string(),
                nullable: false,
                source_type: crate::models::FilterSourceType::Stream,
            },
            crate::models::FilterFieldInfo {
                name: "group_title".to_string(),
                display_name: "Channel Group".to_string(),
                field_type: "string".to_string(),
                nullable: true,
                source_type: crate::models::FilterSourceType::Stream,
            },
            crate::models::FilterFieldInfo {
                name: "tvg_id".to_string(),
                display_name: "TV Guide ID".to_string(),
                field_type: "string".to_string(),
                nullable: true,
                source_type: crate::models::FilterSourceType::Stream,
            },
            crate::models::FilterFieldInfo {
                name: "tvg_name".to_string(),
                display_name: "TV Guide Name".to_string(),
                field_type: "string".to_string(),
                nullable: true,
                source_type: crate::models::FilterSourceType::Stream,
            },
            crate::models::FilterFieldInfo {
                name: "tvg_logo".to_string(),
                display_name: "TV Guide Logo".to_string(),
                field_type: "string".to_string(),
                nullable: true,
                source_type: crate::models::FilterSourceType::Stream,
            },
            crate::models::FilterFieldInfo {
                name: "stream_url".to_string(),
                display_name: "Stream URL".to_string(),
                field_type: "string".to_string(),
                nullable: false,
                source_type: crate::models::FilterSourceType::Stream,
            },
            // EPG-specific fields
            crate::models::FilterFieldInfo {
                name: "program_title".to_string(),
                display_name: "Program Title".to_string(),
                field_type: "string".to_string(),
                nullable: false,
                source_type: crate::models::FilterSourceType::Epg,
            },
            crate::models::FilterFieldInfo {
                name: "program_description".to_string(),
                display_name: "Program Description".to_string(),
                field_type: "string".to_string(),
                nullable: true,
                source_type: crate::models::FilterSourceType::Epg,
            },
        ])
    }

    /// Get usage count for a specific filter (how many proxy filters use it)
    pub async fn get_usage_count(&self, filter_id: &Uuid) -> Result<u64> {
        use crate::entities::{prelude::ProxyFilters, proxy_filters};
        
        let count = ProxyFilters::find()
            .filter(proxy_filters::Column::FilterId.eq(*filter_id))
            .count(&*self.connection)
            .await?;

        Ok(count)
    }

    /// Alias for get_usage_count (for backward compatibility)
    pub async fn get_filter_usage_count(&self, filter_id: &Uuid) -> Result<u64> {
        self.get_usage_count(filter_id).await
    }

    /// Get filters with usage information and optional filtering
    pub async fn get_filters_with_usage_filtered(
        &self, 
        source_type: Option<crate::models::FilterSourceType>,
        sort: Option<String>,
        order: Option<String>
    ) -> Result<Vec<crate::models::FilterWithUsage>> {
        let filters = self.list_all().await?;
        let mut filter_usage_list = Vec::new();

        for filter in filters {
            // Filter by source type if specified
            if let Some(ref st) = source_type {
                if &filter.source_type != st {
                    continue;
                }
            }

            let usage_count = self.get_usage_count(&filter.id).await.unwrap_or(0);
            filter_usage_list.push(crate::models::FilterWithUsage {
                filter: filter,
                usage_count: usage_count as i64,
            });
        }

        // Apply sorting
        if let Some(sort_field) = sort {
            let ascending = order.as_deref().unwrap_or("asc") == "asc";
            match sort_field.as_str() {
                "name" => {
                    if ascending {
                        filter_usage_list.sort_by(|a, b| a.filter.name.cmp(&b.filter.name));
                    } else {
                        filter_usage_list.sort_by(|a, b| b.filter.name.cmp(&a.filter.name));
                    }
                }
                "usage_count" => {
                    if ascending {
                        filter_usage_list.sort_by_key(|f| f.usage_count);
                    } else {
                        filter_usage_list.sort_by_key(|f| std::cmp::Reverse(f.usage_count));
                    }
                }
                "created_at" => {
                    if ascending {
                        filter_usage_list.sort_by_key(|f| f.filter.created_at);
                    } else {
                        filter_usage_list.sort_by_key(|f| std::cmp::Reverse(f.filter.created_at));
                    }
                }
                _ => {
                    // Default sort by name
                    filter_usage_list.sort_by(|a, b| a.filter.name.cmp(&b.filter.name));
                }
            }
        }

        Ok(filter_usage_list)
    }

    /// Test a filter pattern against channels from a specific source
    pub async fn test_filter_pattern(
        &self,
        _filter_expression: &str,
        _source_type: crate::models::FilterSourceType,
        _source_id: Option<Uuid>
    ) -> Result<crate::models::FilterTestResult> {
        // For now, return a basic implementation
        // TODO: Implement actual filter pattern testing logic
        Ok(crate::models::FilterTestResult {
            is_valid: true,
            error: None,
            matching_channels: vec![],
            total_channels: 0,
            matched_count: 0,
            expression_tree: None,
        })
    }
}