//! Filter repository implementation
//!
//! This module provides the repository implementation for filter entities,
//! handling the persistence and querying of stream filters.

use async_trait::async_trait;
use sqlx::{Pool, Row, Sqlite};
use std::collections::HashMap;
use uuid::Uuid;
use crate::utils::uuid_parser::parse_uuid_flexible;

use super::traits::{
    BulkRepository, PaginatedRepository, PaginatedResult, QueryParams, Repository,
};
use crate::errors::{RepositoryError, RepositoryResult};
use crate::models::{Filter, FilterCreateRequest, FilterSourceType, FilterUpdateRequest};
use crate::utils::sqlite::SqliteRowExt;

/// Query parameters specific to filters
#[derive(Debug, Clone, Default)]
pub struct FilterQuery {
    /// Base query parameters
    pub base: QueryParams,
    /// Filter by source type
    pub source_type: Option<FilterSourceType>,
    /// Filter by enabled status
    pub enabled: Option<bool>,
    /// Filter by applied to sources
    pub applied_to_source: Option<Uuid>,
}

impl FilterQuery {
    /// Create new empty query
    pub fn new() -> Self {
        Self::default()
    }

    /// Filter by source type
    pub fn source_type(mut self, source_type: FilterSourceType) -> Self {
        self.source_type = Some(source_type);
        self
    }

    /// Filter by enabled status
    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = Some(enabled);
        self
    }

    /// Filter by applied to source
    pub fn applied_to_source(mut self, source_id: Uuid) -> Self {
        self.applied_to_source = Some(source_id);
        self
    }

    /// Set base query parameters
    pub fn with_base(mut self, base: QueryParams) -> Self {
        self.base = base;
        self
    }
}

/// Repository implementation for filters
#[derive(Clone)]
pub struct FilterRepository {
    pool: Pool<Sqlite>,
}

impl FilterRepository {
    /// Create a new filter repository
    pub fn new(pool: Pool<Sqlite>) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl Repository<Filter, Uuid> for FilterRepository {
    type CreateRequest = FilterCreateRequest;
    type UpdateRequest = FilterUpdateRequest;
    type Query = FilterQuery;

    async fn find_by_id(&self, id: Uuid) -> RepositoryResult<Option<Filter>> {
        let id_str = id.to_string();
        let row = sqlx::query(
            r#"
            SELECT id, name, source_type, is_inverse, is_system_default, expression, created_at, updated_at
            FROM filters
            WHERE id = ?
            "#
        )
        .bind(id_str)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| RepositoryError::QueryFailed {
            query: "find_filter_by_id".to_string(),
            message: e.to_string(),
        })?;

        match row {
            Some(row) => {
                let filter = Filter {
                    id: parse_uuid_flexible(&row.get::<String, _>("id")).map_err(|e| {
                        RepositoryError::QueryFailed {
                            query: "parse_filter_id".to_string(),
                            message: e.to_string(),
                        }
                    })?,
                    name: row.get("name"),
                    source_type: match row.get::<String, _>("source_type").as_str() {
                        "stream" => FilterSourceType::Stream,
                        "epg" => FilterSourceType::Epg,
                        _ => FilterSourceType::Stream, // Default fallback
                    },
                    is_inverse: row.get("is_inverse"),
                    is_system_default: row.get("is_system_default"),
                    expression: row.get("expression"),
                    created_at: row.get_datetime("created_at"),
                    updated_at: row.get_datetime("updated_at"),
                };
                Ok(Some(filter))
            }
            None => Ok(None),
        }
    }

    async fn find_all(&self, query: Self::Query) -> RepositoryResult<Vec<Filter>> {
        let mut sql = "SELECT id, name, source_type, is_inverse, is_system_default, expression, created_at, updated_at FROM filters WHERE 1=1".to_string();
        let mut params: Vec<String> = Vec::new();

        if let Some(source_type) = &query.source_type {
            sql.push_str(" AND source_type = ?");
            params.push(match source_type {
                FilterSourceType::Stream => "stream".to_string(),
                FilterSourceType::Epg => "epg".to_string(),
            });
        }

        if let Some(enabled) = query.enabled {
            // All filters in the database are enabled by default
            // Disabling is handled at the proxy level via proxy_filters table
            if !enabled {
                return Ok(Vec::new());
            }
        }

        sql.push_str(" ORDER BY name");

        let mut query_builder = sqlx::query(&sql);
        for param in params {
            query_builder = query_builder.bind(param);
        }

        let rows = query_builder.fetch_all(&self.pool).await.map_err(|e| {
            RepositoryError::QueryFailed {
                query: "find_all_filters".to_string(),
                message: e.to_string(),
            }
        })?;

        let mut filters = Vec::new();
        for row in rows {
            let id_str: String = row.get("id");
            let filter = Filter {
                id: parse_uuid_flexible(&id_str).map_err(|e| RepositoryError::QueryFailed {
                    query: "parse_filter_id".to_string(),
                    message: e.to_string(),
                })?,
                name: row.get("name"),
                source_type: match row.get::<String, _>("source_type").as_str() {
                    "stream" => FilterSourceType::Stream,
                    "epg" => FilterSourceType::Epg,
                    _ => FilterSourceType::Stream,
                },
                is_inverse: row.get("is_inverse"),
                is_system_default: row.get("is_system_default"),
                expression: row.get("expression"),
                created_at: row.get_datetime("created_at"),
                updated_at: row.get_datetime("updated_at"),
            };
            filters.push(filter);
        }

        Ok(filters)
    }

    async fn create(&self, request: Self::CreateRequest) -> RepositoryResult<Filter> {
        let id = Uuid::new_v4();
        let id_str = id.to_string();
        let now = chrono::Utc::now();
        let now_str = now.to_rfc3339();
        let source_type_str = match request.source_type {
            FilterSourceType::Stream => "stream",
            FilterSourceType::Epg => "epg",
        };

        // Parse the filter expression into a proper ConditionTree
        let parser = crate::expression_parser::ExpressionParser::new();
        parser.parse(&request.expression)
            .map_err(|e| RepositoryError::QueryFailed { 
                query: "filter expression parsing".to_string(), 
                message: format!("Invalid filter expression: {e}") 
            })?;
        // Store the expression as-is

        sqlx::query(
            r#"
            INSERT INTO filters (id, name, source_type, is_inverse, expression, created_at, updated_at)
            VALUES (?, ?, ?, ?, ?, ?, ?)
            "#
        )
        .bind(id_str)
        .bind(request.name.clone())
        .bind(source_type_str)
        .bind(request.is_inverse)
        .bind(&request.expression)
        .bind(now_str.clone())
        .bind(now_str)
        .execute(&self.pool)
        .await
        .map_err(|e| RepositoryError::QueryFailed {
            query: "create_filter".to_string(),
            message: e.to_string(),
        })?;

        Ok(Filter {
            id,
            name: request.name,
            source_type: request.source_type,
            is_inverse: request.is_inverse,
            is_system_default: false, // Always false for user-created filters
            expression: request.expression,
            created_at: now,
            updated_at: now,
        })
    }

    async fn update(&self, id: Uuid, request: Self::UpdateRequest) -> RepositoryResult<Filter> {
        let id_str = id.to_string();
        let now = chrono::Utc::now();
        let now_str = now.to_rfc3339();
        let source_type_str = match request.source_type {
            FilterSourceType::Stream => "stream",
            FilterSourceType::Epg => "epg",
        };

        // Parse the filter expression into a proper ConditionTree
        let parser = crate::expression_parser::ExpressionParser::new();
        parser.parse(&request.expression)
            .map_err(|e| RepositoryError::QueryFailed { 
                query: "filter expression parsing".to_string(), 
                message: format!("Invalid filter expression: {e}") 
            })?;
        // Store the expression as-is

        sqlx::query(
            r#"
            UPDATE filters
            SET name = ?, source_type = ?, is_inverse = ?, expression = ?, updated_at = ?
            WHERE id = ?
            "#
        )
        .bind(request.name.clone())
        .bind(source_type_str)
        .bind(request.is_inverse)
        .bind(&request.expression)
        .bind(now_str)
        .bind(id_str.clone())
        .execute(&self.pool)
        .await
        .map_err(|e| RepositoryError::QueryFailed {
            query: "update_filter".to_string(),
            message: e.to_string(),
        })?;

        // Return the updated filter
        self.find_by_id(id)
            .await?
            .ok_or_else(|| RepositoryError::QueryFailed {
                query: "find_updated_filter".to_string(),
                message: format!("Filter with id {id_str} not found after update"),
            })
    }

    async fn delete(&self, id: Uuid) -> RepositoryResult<()> {
        let id_str = id.to_string();
        sqlx::query("DELETE FROM filters WHERE id = ?")
            .bind(id_str)
            .execute(&self.pool)
            .await
            .map_err(|e| RepositoryError::QueryFailed {
                query: "delete_filter".to_string(),
                message: e.to_string(),
            })?;

        Ok(())
    }

    async fn count(&self, query: Self::Query) -> RepositoryResult<u64> {
        let mut sql = "SELECT COUNT(*) as count FROM filters WHERE 1=1".to_string();
        let mut params: Vec<String> = Vec::new();

        if let Some(source_type) = &query.source_type {
            sql.push_str(" AND source_type = ?");
            params.push(match source_type {
                FilterSourceType::Stream => "stream".to_string(),
                FilterSourceType::Epg => "epg".to_string(),
            });
        }

        let mut query_builder = sqlx::query(&sql);
        for param in params {
            query_builder = query_builder.bind(param);
        }

        let row = query_builder.fetch_one(&self.pool).await.map_err(|e| {
            RepositoryError::QueryFailed {
                query: "count_filters".to_string(),
                message: e.to_string(),
            }
        })?;

        let count: i64 = row.get("count");
        Ok(count as u64)
    }
}

#[async_trait]
impl BulkRepository<Filter, Uuid> for FilterRepository {
    async fn create_bulk(
        &self,
        _requests: Vec<Self::CreateRequest>,
    ) -> RepositoryResult<Vec<Filter>> {
        // TODO: Implement bulk filter creation
        todo!("Filter bulk repository implementation")
    }

    async fn update_bulk(
        &self,
        _updates: HashMap<Uuid, Self::UpdateRequest>,
    ) -> RepositoryResult<Vec<Filter>> {
        // TODO: Implement bulk filter updates
        todo!("Filter bulk repository implementation")
    }

    async fn delete_bulk(&self, _ids: Vec<Uuid>) -> RepositoryResult<u64> {
        // TODO: Implement bulk filter deletion
        todo!("Filter bulk repository implementation")
    }

    async fn find_by_ids(&self, _ids: Vec<Uuid>) -> RepositoryResult<Vec<Filter>> {
        // TODO: Implement finding multiple filters by IDs
        todo!("Filter bulk repository implementation")
    }
}

#[async_trait]
impl PaginatedRepository<Filter, Uuid> for FilterRepository {
    type PaginatedResult = PaginatedResult<Filter>;

    async fn find_paginated(
        &self,
        _query: Self::Query,
        _page: u32,
        _limit: u32,
    ) -> RepositoryResult<Self::PaginatedResult> {
        // TODO: Implement paginated filter queries
        todo!("Filter paginated repository implementation")
    }
}

impl FilterRepository {
    /// Additional domain-specific methods for filter operations
    /// Get available filter fields for building filter expressions
    pub async fn get_available_filter_fields(&self) -> RepositoryResult<Vec<crate::models::FilterFieldInfo>> {
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
                name: "stream_url".to_string(),
                display_name: "Stream URL".to_string(),
                field_type: "string".to_string(),
                nullable: false,
                source_type: crate::models::FilterSourceType::Stream,
            },
        ])
    }

    /// Get filters with usage statistics, optionally filtered and sorted
    pub async fn get_filters_with_usage_filtered(
        &self,
        source_type: Option<crate::models::FilterSourceType>,
        sort: Option<String>,
        order: Option<String>,
    ) -> RepositoryResult<Vec<crate::models::FilterWithUsage>> {
        let mut sql = r#"
            SELECT f.id, f.name, f.source_type, f.is_inverse, f.is_system_default, f.expression, 
                   f.created_at, f.updated_at, COUNT(pf.filter_id) as usage_count
            FROM filters f
            LEFT JOIN proxy_filters pf ON f.id = pf.filter_id
        "#.to_string();

        let mut params = Vec::new();
        
        if let Some(source_type) = source_type {
            sql.push_str(" WHERE f.source_type = ?");
            params.push(match source_type {
                crate::models::FilterSourceType::Stream => "stream".to_string(),
                crate::models::FilterSourceType::Epg => "epg".to_string(),
            });
        }

        sql.push_str(" GROUP BY f.id");

        // Add sorting
        if let Some(sort_field) = sort {
            let order_direction = order.unwrap_or_else(|| "ASC".to_string()).to_uppercase();
            let valid_order = if order_direction == "DESC" { "DESC" } else { "ASC" };
            
            match sort_field.as_str() {
                "name" => sql.push_str(&format!(" ORDER BY f.name {valid_order}")),
                "usage_count" => sql.push_str(&format!(" ORDER BY usage_count {valid_order}")),
                "created_at" => sql.push_str(&format!(" ORDER BY f.created_at {valid_order}")),
                _ => sql.push_str(" ORDER BY f.name ASC"), // Default sort
            }
        } else {
            sql.push_str(" ORDER BY f.name ASC");
        }

        let mut query_builder = sqlx::query(&sql);
        for param in params {
            query_builder = query_builder.bind(param);
        }

        let rows = query_builder.fetch_all(&self.pool).await?;

        let mut results = Vec::new();
        for row in rows {
            let filter = crate::models::Filter {
                id: parse_uuid_flexible(&row.try_get::<String, _>("id")?)?,
                name: row.try_get("name")?,
                source_type: match row.try_get::<String, _>("source_type")?.as_str() {
                    "stream" => crate::models::FilterSourceType::Stream,
                    "epg" => crate::models::FilterSourceType::Epg,
                    _ => crate::models::FilterSourceType::Stream,
                },
                is_inverse: row.try_get("is_inverse")?,
                is_system_default: row.try_get("is_system_default")?,
                expression: row.try_get("expression")?,
                created_at: row.get_datetime("created_at"),
                updated_at: row.get_datetime("updated_at"),
            };

            let usage_count: i64 = row.try_get("usage_count")?;

            results.push(crate::models::FilterWithUsage {
                filter,
                usage_count,
            });
        }

        Ok(results)
    }

    /// Get usage count for a specific filter
    pub async fn get_filter_usage_count(&self, filter_id: uuid::Uuid) -> RepositoryResult<i64> {
        use crate::repositories::traits::RepositoryHelpers;
        RepositoryHelpers::get_usage_count(&self.pool, "proxy_filters", "filter_id", filter_id).await
    }

    /// Test a filter pattern against available fields
    pub async fn test_filter_pattern(
        &self,
        pattern: &str,
        source_type: crate::models::FilterSourceType,
        source_id: Option<uuid::Uuid>,
    ) -> RepositoryResult<crate::models::FilterTestResult> {
        use crate::models::FilterTestChannel;
        use crate::pipeline::engines::filter_processor::{StreamFilterProcessor, FilterProcessor, RegexEvaluator};
        use crate::utils::regex_preprocessor::{RegexPreprocessor, RegexPreprocessorConfig};
        
        // Parse the expression first to check if it's valid
        let parser = crate::expression_parser::ExpressionParser::new()
            .with_fields(vec![
                "tvg_id".to_string(),
                "tvg_name".to_string(),
                "tvg_logo".to_string(),
                "tvg_shift".to_string(),
                "group_title".to_string(),
                "channel_name".to_string(),
                "stream_url".to_string(),
            ]);
        
        match parser.parse(pattern) {
            Err(e) => {
                Ok(crate::models::FilterTestResult {
                    is_valid: false,
                    error: Some(format!("Invalid filter expression: {e}")),
                    matching_channels: Vec::new(),
                    total_channels: 0,
                    matched_count: 0,
                    expression_tree: None,
                })
            }
            Ok(condition_tree) => {
                // Get channels from database for testing
                let channels = self.get_channels_for_testing(source_type, source_id).await?;
                let total_channels = channels.len();
                
                // Create regex evaluator with default config
                let regex_preprocessor = RegexPreprocessor::new(RegexPreprocessorConfig::default());
                let regex_evaluator = RegexEvaluator::new(regex_preprocessor);
                
                // Create filter processor
                let mut filter_processor = StreamFilterProcessor::new(
                    uuid::Uuid::new_v4().to_string(),
                    "Test Filter".to_string(),
                    false, // not inverse for testing
                    pattern,
                    regex_evaluator,
                ).map_err(|e| RepositoryError::QueryFailed {
                    query: "create_filter_processor".to_string(),
                    message: format!("Failed to create filter processor: {e}"),
                })?;
                
                let mut matching_channels = Vec::new();
                
                for channel in &channels {
                    match filter_processor.process_record(channel) {
                        Ok(result) => {
                            if result.include_match {
                                matching_channels.push(FilterTestChannel {
                                    channel_name: channel.channel_name.clone(),
                                    group_title: channel.group_title.clone(),
                                    matched_text: None,
                                });
                            }
                        }
                        Err(e) => {
                            return Ok(crate::models::FilterTestResult {
                                is_valid: false,
                                error: Some(format!("Filter processing error: {e}")),
                                matching_channels: Vec::new(),
                                total_channels,
                                matched_count: 0,
                                expression_tree: None,
                            });
                        }
                    }
                }
                
                let matched_count = matching_channels.len();
                
                // Convert condition tree to JSON for debugging
                let expression_tree = serde_json::to_value(&condition_tree).ok();
                
                Ok(crate::models::FilterTestResult {
                    is_valid: true,
                    error: None,
                    matching_channels,
                    total_channels,
                    matched_count,
                    expression_tree,
                })
            }
        }
    }
    
    /// Get channels for filter testing with source validation
    async fn get_channels_for_testing(
        &self,
        source_type: crate::models::FilterSourceType,
        source_id: Option<uuid::Uuid>,
    ) -> RepositoryResult<Vec<crate::models::Channel>> {
        // Validate source_id matches the expected source_type if provided
        if let Some(source_id) = source_id {
            let source_table = match source_type {
                crate::models::FilterSourceType::Stream => "stream_sources",
                crate::models::FilterSourceType::Epg => "epg_sources", 
            };
            
            // Verify the source exists and is of the correct type
            let source_exists = sqlx::query_scalar::<_, i64>(&format!(
                "SELECT COUNT(*) FROM {source_table} WHERE id = ?"
            ))
            .bind(source_id.to_string())
            .fetch_one(&self.pool)
            .await?;
            
            if source_exists == 0 {
                return Err(RepositoryError::QueryFailed {
                    query: "validate_source_id".to_string(),
                    message: format!("Source ID {source_id} not found in {source_table} table"),
                });
            }
        }

        let table = match source_type {
            crate::models::FilterSourceType::Stream => "channels",
            crate::models::FilterSourceType::Epg => "epg_channels",
        };
        
        let (query, params) = if let Some(source_id) = source_id {
            (
                format!(
                    "SELECT 
                        id, source_id, tvg_id, tvg_name, tvg_chno, tvg_logo, tvg_shift,
                        group_title, channel_name, stream_url,
                        created_at, updated_at
                     FROM {table} 
                     WHERE source_id = ?
                     ORDER BY channel_name"
                ),
                Some(source_id.to_string())
            )
        } else {
            (
                format!(
                    "SELECT 
                        id, source_id, tvg_id, tvg_name, tvg_chno, tvg_logo, tvg_shift,
                        group_title, channel_name, stream_url,
                        created_at, updated_at
                     FROM {table} 
                     ORDER BY channel_name"
                ),
                None
            )
        };
        
        let rows = if let Some(param) = params {
            sqlx::query(&query).bind(param).fetch_all(&self.pool).await?
        } else {
            sqlx::query(&query).fetch_all(&self.pool).await?
        };
        
        let mut channels = Vec::new();
        for row in rows {
            // Parse UUIDs from database strings
            let id_str: String = row.get("id");
            let source_id_str: String = row.get("source_id");
            
            let channel = crate::models::Channel {
                id: uuid::Uuid::parse_str(&id_str)
                    .map_err(|e| RepositoryError::query_failed("parse_channel_id", e.to_string()))?,
                source_id: uuid::Uuid::parse_str(&source_id_str)
                    .map_err(|e| RepositoryError::query_failed("parse_source_id", e.to_string()))?,
                tvg_id: row.get("tvg_id"),
                tvg_name: row.get("tvg_name"),
                tvg_chno: row.get("tvg_chno"),
                tvg_logo: row.get("tvg_logo"),
                tvg_shift: row.get("tvg_shift"),
                group_title: row.get("group_title"),
                channel_name: row.get("channel_name"),
                stream_url: row.get("stream_url"),
                created_at: row.get_datetime("created_at"),
                updated_at: row.get_datetime("updated_at"),
            };
            channels.push(channel);
        }
        
        Ok(channels)
    }

    /// Ensure default filters exist in the database
    pub async fn ensure_default_filters(&self) -> RepositoryResult<()> {
        // Check if default filters already exist
        let existing_defaults = sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM filters WHERE is_system_default = 1"
        )
        .fetch_one(&self.pool)
        .await?;

        if existing_defaults > 0 {
            return Ok(()); // Default filters already exist
        }

        // Create default filters
        let default_filters = vec![
            crate::models::FilterCreateRequest {
                name: "Allow All Channels".to_string(),
                source_type: crate::models::FilterSourceType::Stream,
                is_inverse: false,
                expression: "1 == 1".to_string(), // Always true
            },
            crate::models::FilterCreateRequest {
                name: "HD Channels Only".to_string(),
                source_type: crate::models::FilterSourceType::Stream,
                is_inverse: false,
                expression: "channel_name contains 'HD'".to_string(),
            },
        ];

        for filter_request in default_filters {
            let id = uuid::Uuid::new_v4();
            let now = chrono::Utc::now().to_rfc3339();
            
            sqlx::query(
                r#"
                INSERT INTO filters (id, name, source_type, is_inverse, is_system_default, expression, created_at, updated_at)
                VALUES (?, ?, ?, ?, 1, ?, ?, ?)
                "#
            )
            .bind(id.to_string())
            .bind(&filter_request.name)
            .bind(match filter_request.source_type {
                crate::models::FilterSourceType::Stream => "stream",
                crate::models::FilterSourceType::Epg => "epg",
            })
            .bind(filter_request.is_inverse)
            .bind(&filter_request.expression)
            .bind(&now)
            .bind(&now)
            .execute(&self.pool)
            .await?;
        }

        Ok(())
    }
}
