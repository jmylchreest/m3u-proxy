use crate::models::*;
use anyhow::Result;
use sqlx::Row;
use crate::utils::uuid_parser::parse_uuid_flexible;
use crate::utils::datetime::DateTimeParser;

use uuid::Uuid;

impl super::Database {
    /// Discover available filter fields from the channels table schema
    pub async fn get_available_filter_fields(&self) -> Result<Vec<FilterFieldInfo>> {
        let schema_info = sqlx::query("PRAGMA table_info(channels)")
            .fetch_all(&self.pool)
            .await?;

        let mut fields = Vec::new();
        for row in schema_info {
            let column_name: String = row.get("name");
            let column_type: String = row.get("type");

            // Skip system fields that shouldn't be user-filterable
            if matches!(
                column_name.as_str(),
                "id" | "source_id" | "created_at" | "updated_at"
            ) {
                continue;
            }

            let field_info = FilterFieldInfo {
                name: column_name.clone(),
                display_name: column_name
                    .replace("_", " ")
                    .split_whitespace()
                    .map(|word| {
                        let mut chars = word.chars();
                        match chars.next() {
                            None => String::new(),
                            Some(first) => {
                                first.to_uppercase().collect::<String>() + chars.as_str()
                            }
                        }
                    })
                    .collect::<Vec<_>>()
                    .join(" "),
                field_type: if column_type.contains("TEXT") || column_type.contains("VARCHAR") {
                    "string".to_string()
                } else {
                    "unknown".to_string()
                },
                nullable: row.get::<i32, _>("notnull") == 0,
                source_type: FilterSourceType::Stream,
            };

            fields.push(field_info);
        }

        Ok(fields)
    }
    #[allow(dead_code)]
    pub async fn list_filters(&self) -> Result<Vec<Filter>> {
        let rows = sqlx::query(
            "SELECT id, name, source_type, is_inverse, is_system_default, condition_tree, created_at, updated_at
             FROM filters
             ORDER BY name",
        )
        .fetch_all(&self.pool)
        .await?;

        let mut filters = Vec::new();
        for row in rows {
            let filter = Filter {
                id: parse_uuid_flexible(&row.try_get::<String, _>("id")?)?,
                name: row.try_get("name")?,
                source_type: match row.try_get::<String, _>("source_type")?.as_str() {
                    "epg" => FilterSourceType::Epg,
                    _ => FilterSourceType::Stream,
                },
                is_inverse: row.try_get("is_inverse")?,
                is_system_default: row.try_get("is_system_default")?,
                condition_tree: row.try_get("condition_tree")?,
                created_at: row.try_get("created_at")?,
                updated_at: row.try_get("updated_at")?,
            };
            filters.push(filter);
        }

        Ok(filters)
    }

    pub async fn create_filter(&self, request: &FilterCreateRequest) -> Result<Filter> {
        let id = Uuid::new_v4();

        // Get available fields for validation
        let available_fields = self.get_available_filter_fields().await?;
        let field_names: Vec<String> = available_fields.into_iter().map(|f| f.name).collect();

        // Parse the filter expression using the proper parser
        let parser = crate::filter_parser::FilterParser::new().with_fields(field_names);
        let condition_tree = parser.parse(&request.filter_expression)?;

        // Start a transaction to insert filter
        let mut tx = self.pool.begin().await?;

        sqlx::query(
            "INSERT INTO filters (id, name, source_type, is_inverse, is_system_default, condition_tree)
             VALUES (?, ?, ?, ?, ?, ?)",
        )
        .bind(id.to_string())
        .bind(&request.name)
        .bind(match request.source_type {
            FilterSourceType::Stream => "stream",
            FilterSourceType::Epg => "epg",
        })
        .bind(request.is_inverse)
        .bind(false) // Always set is_system_default to false for user-created filters
        .bind(serde_json::to_string(&condition_tree)?)
        .execute(&mut *tx)
        .await?;

        tx.commit().await?;

        self.get_filter(id)
            .await?
            .ok_or_else(|| anyhow::anyhow!("Failed to retrieve created filter"))
    }

    pub async fn get_filter(&self, id: Uuid) -> Result<Option<Filter>> {
        let row = sqlx::query(
            "SELECT id, name, source_type, is_inverse, is_system_default, condition_tree, created_at, updated_at
             FROM filters
             WHERE id = ?",
        )
        .bind(id.to_string())
        .fetch_optional(&self.pool)
        .await?;

        if let Some(row) = row {
            let filter = Filter {
                id: parse_uuid_flexible(&row.try_get::<String, _>("id")?)?,
                name: row.try_get("name")?,
                source_type: match row.try_get::<String, _>("source_type")?.as_str() {
                    "epg" => FilterSourceType::Epg,
                    _ => FilterSourceType::Stream,
                },
                is_inverse: row.try_get("is_inverse")?,
                is_system_default: row.try_get("is_system_default")?,
                condition_tree: row.try_get("condition_tree")?,
                created_at: row.try_get("created_at")?,
                updated_at: row.try_get("updated_at")?,
            };
            Ok(Some(filter))
        } else {
            Ok(None)
        }
    }

    pub async fn update_filter(
        &self,
        id: Uuid,
        request: &FilterUpdateRequest,
    ) -> Result<Option<Filter>> {
        // Get available fields for validation
        let available_fields = self.get_available_filter_fields().await?;
        let field_names: Vec<String> = available_fields.into_iter().map(|f| f.name).collect();

        // Parse the filter expression using the proper parser
        let parser = crate::filter_parser::FilterParser::new().with_fields(field_names);
        let condition_tree = parser.parse(&request.filter_expression)?;

        let mut tx = self.pool.begin().await?;

        let result = sqlx::query(
            "UPDATE filters
             SET name = ?, source_type = ?, is_inverse = ?, condition_tree = ?
             WHERE id = ?",
        )
        .bind(&request.name)
        .bind(match request.source_type {
            FilterSourceType::Stream => "stream",
            FilterSourceType::Epg => "epg",
        })
        .bind(request.is_inverse)
        .bind(serde_json::to_string(&condition_tree)?)
        .bind(id.to_string())
        .execute(&mut *tx)
        .await?;

        if result.rows_affected() == 0 {
            tx.rollback().await?;
            return Ok(None);
        }

        tx.commit().await?;
        self.get_filter(id).await
    }

    pub async fn delete_filter(&self, id: Uuid) -> Result<bool> {
        // Check if this is a system default filter
        let is_system_default =
            sqlx::query_scalar::<_, bool>("SELECT is_system_default FROM filters WHERE id = ?")
                .bind(id.to_string())
                .fetch_optional(&self.pool)
                .await?;

        if let Some(true) = is_system_default {
            return Err(anyhow::anyhow!("Cannot delete system default filter"));
        }

        let result = sqlx::query("DELETE FROM filters WHERE id = ?")
            .bind(id.to_string())
            .execute(&self.pool)
            .await?;

        Ok(result.rows_affected() > 0)
    }

    pub async fn get_filters_with_usage(&self) -> Result<Vec<FilterWithUsage>> {
        let filters = sqlx::query(
            "SELECT f.id, f.name, f.source_type, f.is_inverse, f.is_system_default, f.condition_tree,
             f.created_at, f.updated_at, COUNT(pf.filter_id) as usage_count
             FROM filters f
             LEFT JOIN proxy_filters pf ON f.id = pf.filter_id AND pf.is_active = 1
             GROUP BY f.id, f.name, f.source_type, f.is_inverse, f.is_system_default, f.condition_tree,
             f.created_at, f.updated_at
             ORDER BY f.is_system_default DESC, f.name",
        )
        .fetch_all(&self.pool)
        .await?;

        let mut result = Vec::new();
        for row in filters {
            let filter_id: Uuid = parse_uuid_flexible(&row.try_get::<String, _>("id")?)?;
            let filter = Filter {
                id: filter_id,
                name: row.try_get("name")?,
                source_type: match row.try_get::<String, _>("source_type")?.as_str() {
                    "epg" => FilterSourceType::Epg,
                    _ => FilterSourceType::Stream,
                },
                is_inverse: row.try_get("is_inverse")?,
                is_system_default: row.try_get("is_system_default")?,
                condition_tree: row.try_get("condition_tree")?,
                created_at: row.try_get("created_at")?,
                updated_at: row.try_get("updated_at")?,
            };

            let usage_count: i64 = row.try_get("usage_count")?;

            result.push(FilterWithUsage {
                filter,
                usage_count,
            });
        }

        Ok(result)
    }

    pub async fn test_filter_pattern(
        &self,
        source_id: Uuid,
        request: &FilterTestRequest,
    ) -> Result<FilterTestResult> {
        // Get available fields for validation
        let available_fields = self.get_available_filter_fields().await?;
        let field_names: Vec<String> = available_fields.into_iter().map(|f| f.name).collect();

        // Parse the filter expression using the proper parser
        let parser = crate::filter_parser::FilterParser::new().with_fields(field_names);
        let condition_tree = match parser.parse(&request.filter_expression) {
            Ok(tree) => tree,
            Err(e) => {
                return Ok(FilterTestResult {
                    is_valid: false,
                    error: Some(format!("Syntax error: {}", e)),
                    matching_channels: vec![],
                    total_channels: 0,
                    matched_count: 0,
                    expression_tree: None,
                });
            }
        };

        // Create a temporary filter for testing with the parsed tree
        let temp_filter = Filter {
            id: Uuid::new_v4(),
            name: "Test Filter".to_string(),
            source_type: request.source_type.clone(),
            is_inverse: request.is_inverse,
            is_system_default: false,
            condition_tree: serde_json::to_string(&condition_tree)?,
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        };

        // Using condition_tree only

        // Get all channels for the source - using manual row mapping to handle UUID strings
        let rows = sqlx::query(
            "SELECT id, source_id, tvg_id, tvg_name, tvg_logo, tvg_shift, group_title, channel_name, stream_url, created_at, updated_at
             FROM channels
             WHERE source_id = ?
             ORDER BY channel_name"
        )
        .bind(source_id.to_string())
        .fetch_all(&self.pool)
        .await?;

        let mut channels = Vec::new();
        for row in rows {
            let id_str: String = row.get("id");
            let source_id_str: String = row.get("source_id");
            let created_at_str: String = row.get("created_at");
            let updated_at_str: String = row.get("updated_at");

            channels.push(Channel {
                id: parse_uuid_flexible(&id_str)?,
                source_id: parse_uuid_flexible(&source_id_str)?,
                tvg_id: row.get("tvg_id"),
                tvg_name: row.get("tvg_name"),
                tvg_chno: row.try_get("tvg_chno").unwrap_or(None),
                tvg_logo: row.get("tvg_logo"),
                tvg_shift: row.get("tvg_shift"),
                group_title: row.get("group_title"),
                channel_name: row.get("channel_name"),
                stream_url: row.get("stream_url"),
                created_at: DateTimeParser::parse_flexible(&created_at_str)?,
                updated_at: DateTimeParser::parse_flexible(&updated_at_str)?,
            });
        }

        let total_channels = channels.len();
        // Use the filter engine to apply the filter
        let mut filter_engine = crate::proxy::filter_engine::FilterEngine::new();
        let matching_channels: Vec<FilterTestChannel> = match filter_engine
            .apply_single_filter(&channels, &temp_filter)
            .await
        {
            Ok(filtered_channels) => {
                filtered_channels
                    .iter()
                    .map(|channel| FilterTestChannel {
                        channel_name: channel.channel_name.clone(),
                        group_title: channel.group_title.clone(),
                        matched_text: None, // We can expand this later to show which field matched
                    })
                    .collect()
            }
            Err(e) => {
                return Ok(FilterTestResult {
                    is_valid: false,
                    error: Some(format!("Filter evaluation error: {}", e)),
                    matching_channels: vec![],
                    total_channels,
                    matched_count: 0,
                    expression_tree: None,
                });
            }
        };

        let matched_count = matching_channels.len();

        // Generate expression tree for frontend
        let expression_tree = crate::web::api::generate_expression_tree_json(&condition_tree);

        Ok(FilterTestResult {
            is_valid: true,
            error: None,
            matching_channels,
            total_channels,
            matched_count,
            expression_tree: Some(expression_tree),
        })
    }

    #[allow(dead_code)]
    pub async fn get_proxy_filters(&self, proxy_id: Uuid) -> Result<Vec<ProxyFilterWithDetails>> {
        let filters = sqlx::query(
            "SELECT pf.proxy_id, pf.filter_id, pf.priority_order, pf.is_active, pf.created_at,
                    f.name, f.source_type, f.is_inverse, f.is_system_default, f.condition_tree, f.updated_at as filter_updated_at
             FROM proxy_filters pf
             JOIN filters f ON pf.filter_id = f.id
             WHERE pf.proxy_id = ? AND pf.is_active = 1
             ORDER BY pf.priority_order",
        )
        .bind(proxy_id.to_string())
        .fetch_all(&self.pool)
        .await?;

        let mut result = Vec::new();
        for row in filters {
            let proxy_filter = ProxyFilter {
                proxy_id: parse_uuid_flexible(&row.get::<String, _>("proxy_id"))?,
                filter_id: parse_uuid_flexible(&row.get::<String, _>("filter_id"))?,
                priority_order: row.get("priority_order"),
                is_active: row.get("is_active"),
                created_at: row.get("created_at"),
            };

            let filter = Filter {
                id: proxy_filter.filter_id,
                name: row.get("name"),
                source_type: match row.get::<String, _>("source_type").as_str() {
                    "epg" => FilterSourceType::Epg,
                    _ => FilterSourceType::Stream,
                },
                is_inverse: row.get("is_inverse"),
                is_system_default: row.get("is_system_default"),
                condition_tree: row.get("condition_tree"),
                created_at: row.get("created_at"),
                updated_at: row.get("filter_updated_at"),
            };

            result.push(ProxyFilterWithDetails {
                proxy_filter,
                filter,
            });
        }

        Ok(result)
    }

    #[allow(dead_code)]
    pub async fn add_filter_to_proxy(
        &self,
        proxy_id: Uuid,
        filter_id: Uuid,
        sort_order: i32,
    ) -> Result<()> {
        sqlx::query(
            "INSERT INTO proxy_filters (proxy_id, filter_id, priority_order, is_active)
             VALUES (?, ?, ?, 1)",
        )
        .bind(proxy_id.to_string())
        .bind(filter_id.to_string())
        .bind(sort_order)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    #[allow(dead_code)]
    pub async fn remove_filter_from_proxy(&self, proxy_id: Uuid, filter_id: Uuid) -> Result<bool> {
        let result = sqlx::query("DELETE FROM proxy_filters WHERE proxy_id = ? AND filter_id = ?")
            .bind(proxy_id.to_string())
            .bind(filter_id.to_string())
            .execute(&self.pool)
            .await?;

        Ok(result.rows_affected() > 0)
    }

    #[allow(dead_code)]
    pub async fn update_proxy_filter_order(
        &self,
        proxy_id: Uuid,
        filter_orders: &[(Uuid, i32)],
    ) -> Result<()> {
        let mut tx = self.pool.begin().await?;

        for (filter_id, sort_order) in filter_orders {
            sqlx::query(
                "UPDATE proxy_filters SET priority_order = ? WHERE proxy_id = ? AND filter_id = ?",
            )
            .bind(sort_order)
            .bind(proxy_id.to_string())
            .bind(filter_id.to_string())
            .execute(&mut *tx)
            .await?;
        }

        tx.commit().await?;
        Ok(())
    }

    #[allow(dead_code)]
    pub async fn toggle_proxy_filter(
        &self,
        proxy_id: Uuid,
        filter_id: Uuid,
        is_active: bool,
    ) -> Result<bool> {
        let result = sqlx::query(
            "UPDATE proxy_filters SET is_active = ? WHERE proxy_id = ? AND filter_id = ?",
        )
        .bind(is_active)
        .bind(proxy_id.to_string())
        .bind(filter_id.to_string())
        .execute(&self.pool)
        .await?;

        Ok(result.rows_affected() > 0)
    }

    /// Internal method to create system default filters during initialization
    async fn create_system_default_filter(&self, request: &FilterCreateRequest) -> Result<Filter> {
        let id = Uuid::new_v4();

        // Get available fields for validation
        let available_fields = self.get_available_filter_fields().await?;
        let field_names: Vec<String> = available_fields.into_iter().map(|f| f.name).collect();

        // Parse the filter expression using the proper parser
        let parser = crate::filter_parser::FilterParser::new().with_fields(field_names);
        let condition_tree = parser.parse(&request.filter_expression)?;

        // Start a transaction to insert filter
        let mut tx = self.pool.begin().await?;

        sqlx::query(
            "INSERT INTO filters (id, name, source_type, is_inverse, is_system_default, condition_tree)
             VALUES (?, ?, ?, ?, ?, ?)",
        )
        .bind(id.to_string())
        .bind(&request.name)
        .bind(match request.source_type {
            FilterSourceType::Stream => "stream",
            FilterSourceType::Epg => "epg",
        })
        .bind(request.is_inverse)
        .bind(true) // System default filters have is_system_default = true
        .bind(serde_json::to_string(&condition_tree)?)
        .execute(&mut *tx)
        .await?;

        tx.commit().await?;

        self.get_filter(id)
            .await?
            .ok_or_else(|| anyhow::anyhow!("Failed to retrieve created system default filter"))
    }

    /// Ensure default filters exist in the database
    pub async fn ensure_default_filters(&self) -> Result<()> {
        // Check if default filters already exist
        let existing_count = sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM filters WHERE id IN (
                '00000000-0000-0000-0000-000000000001',
                '00000000-0000-0000-0000-000000000002'
            )",
        )
        .fetch_one(&self.pool)
        .await?;

        if existing_count >= 2 {
            return Ok(()); // Default filters already exist
        }

        // Create "Include All Valid Stream URLs" filter
        let valid_urls_filter = FilterCreateRequest {
            name: "Include All Valid Stream URLs".to_string(),
            source_type: FilterSourceType::Stream,
            is_inverse: false,
            filter_expression: "stream_url starts_with \"http\"".to_string(),
        };

        // Create "Exclude Adult Content" filter
        let exclude_adult_filter = FilterCreateRequest {
            name: "Exclude Adult Content".to_string(),
            source_type: FilterSourceType::Stream,
            is_inverse: true, // This makes it an exclude filter
            filter_expression: "(group_title contains \"adult\" OR group_title contains \"xxx\" OR group_title contains \"porn\" OR channel_name contains \"adult\" OR channel_name contains \"xxx\" OR channel_name contains \"porn\")".to_string(),
        };

        // Try to create the filters, but don't fail if they already exist
        if let Err(e) = self.create_system_default_filter(&valid_urls_filter).await {
            tracing::warn!(
                "Could not create default 'Include All Valid Stream URLs' filter: {}",
                e
            );
        } else {
            tracing::info!("Created default filter: Include All Valid Stream URLs");
        }

        if let Err(e) = self.create_system_default_filter(&exclude_adult_filter).await {
            tracing::warn!(
                "Could not create default 'Exclude Adult Content' filter: {}",
                e
            );
        } else {
            tracing::info!("Created default filter: Exclude Adult Content");
        }

        Ok(())
    }
}
