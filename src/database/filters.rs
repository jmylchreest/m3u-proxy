use crate::models::*;
use anyhow::Result;
use sqlx::Row;

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
            };

            fields.push(field_info);
        }

        Ok(fields)
    }
    #[allow(dead_code)]
    pub async fn list_filters(&self) -> Result<Vec<Filter>> {
        let rows = sqlx::query(
            "SELECT id, name, starting_channel_number, is_inverse, logical_operator, condition_tree, created_at, updated_at
             FROM filters
             ORDER BY name",
        )
        .fetch_all(&self.pool)
        .await?;

        let mut filters = Vec::new();
        for row in rows {
            let filter = Filter {
                id: row.try_get::<String, _>("id")?.parse()?,
                name: row.try_get("name")?,
                starting_channel_number: row.try_get("starting_channel_number")?,
                is_inverse: row.try_get("is_inverse")?,
                logical_operator: row.try_get("logical_operator")?,
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

        // Start a transaction to insert both filter and conditions
        let mut tx = self.pool.begin().await?;

        sqlx::query(
            "INSERT INTO filters (id, name, starting_channel_number, is_inverse, logical_operator, condition_tree)
             VALUES (?, ?, ?, ?, ?, ?)",
        )
        .bind(id.to_string())
        .bind(&request.name)
        .bind(request.starting_channel_number)
        .bind(request.is_inverse)
        .bind(&request.logical_operator)
        .bind(&request.condition_tree)
        .execute(&mut *tx)
        .await?;

        // Insert filter conditions
        for (index, condition) in request.conditions.iter().enumerate() {
            let condition_id = Uuid::new_v4();
            sqlx::query(
                "INSERT INTO filter_conditions (id, filter_id, field_name, operator, value, sort_order)
                 VALUES (?, ?, ?, ?, ?, ?)",
            )
            .bind(condition_id.to_string())
            .bind(id.to_string())
            .bind(&condition.field_name)
            .bind(&condition.operator)
            .bind(&condition.value)
            .bind(index as i32)
            .execute(&mut *tx)
            .await?;
        }

        tx.commit().await?;

        self.get_filter(id)
            .await?
            .ok_or_else(|| anyhow::anyhow!("Failed to retrieve created filter"))
    }

    pub async fn get_filter(&self, id: Uuid) -> Result<Option<Filter>> {
        let row = sqlx::query(
            "SELECT id, name, starting_channel_number, is_inverse, logical_operator, condition_tree, created_at, updated_at
             FROM filters
             WHERE id = ?",
        )
        .bind(id.to_string())
        .fetch_optional(&self.pool)
        .await?;

        if let Some(row) = row {
            let filter = Filter {
                id: row.try_get::<String, _>("id")?.parse()?,
                name: row.try_get("name")?,
                starting_channel_number: row.try_get("starting_channel_number")?,
                is_inverse: row.try_get("is_inverse")?,
                logical_operator: row.try_get("logical_operator")?,
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
        let mut tx = self.pool.begin().await?;

        let result = sqlx::query(
            "UPDATE filters
             SET name = ?, starting_channel_number = ?, is_inverse = ?, logical_operator = ?, condition_tree = ?
             WHERE id = ?",
        )
        .bind(&request.name)
        .bind(request.starting_channel_number)
        .bind(request.is_inverse)
        .bind(&request.logical_operator)
        .bind(&request.condition_tree)
        .bind(id.to_string())
        .execute(&mut *tx)
        .await?;

        if result.rows_affected() == 0 {
            tx.rollback().await?;
            return Ok(None);
        }

        // Delete existing conditions
        sqlx::query("DELETE FROM filter_conditions WHERE filter_id = ?")
            .bind(id.to_string())
            .execute(&mut *tx)
            .await?;

        // Insert new conditions
        for (index, condition) in request.conditions.iter().enumerate() {
            let condition_id = Uuid::new_v4();
            sqlx::query(
                "INSERT INTO filter_conditions (id, filter_id, field_name, operator, value, sort_order)
                 VALUES (?, ?, ?, ?, ?, ?)",
            )
            .bind(condition_id.to_string())
            .bind(id.to_string())
            .bind(&condition.field_name)
            .bind(&condition.operator)
            .bind(&condition.value)
            .bind(index as i32)
            .execute(&mut *tx)
            .await?;
        }

        tx.commit().await?;
        self.get_filter(id).await
    }

    pub async fn delete_filter(&self, id: Uuid) -> Result<bool> {
        let result = sqlx::query("DELETE FROM filters WHERE id = ?")
            .bind(id.to_string())
            .execute(&self.pool)
            .await?;

        Ok(result.rows_affected() > 0)
    }

    pub async fn get_filters_with_usage(&self) -> Result<Vec<FilterWithUsage>> {
        let filters = sqlx::query(
            "SELECT f.id, f.name, f.starting_channel_number, f.is_inverse, f.logical_operator, f.condition_tree,
             f.created_at, f.updated_at, COUNT(pf.filter_id) as usage_count
             FROM filters f
             LEFT JOIN proxy_filters pf ON f.id = pf.filter_id AND pf.is_active = 1
             GROUP BY f.id, f.name, f.starting_channel_number, f.is_inverse, f.logical_operator, f.condition_tree,
             f.created_at, f.updated_at
             ORDER BY f.name",
        )
        .fetch_all(&self.pool)
        .await?;

        let mut result = Vec::new();
        for row in filters {
            let filter_id: Uuid = row.try_get::<String, _>("id")?.parse()?;
            let filter = Filter {
                id: filter_id,
                name: row.try_get("name")?,
                starting_channel_number: row.try_get("starting_channel_number")?,
                is_inverse: row.try_get("is_inverse")?,
                logical_operator: row.try_get("logical_operator")?,
                condition_tree: row.try_get("condition_tree")?,
                created_at: row.try_get("created_at")?,
                updated_at: row.try_get("updated_at")?,
            };

            // Get conditions for this filter
            let condition_rows = sqlx::query(
                "SELECT id, filter_id, field_name, operator, value, sort_order, created_at
                 FROM filter_conditions
                 WHERE filter_id = ?
                 ORDER BY sort_order",
            )
            .bind(filter_id.to_string())
            .fetch_all(&self.pool)
            .await?;

            let mut conditions = Vec::new();
            for condition_row in condition_rows {
                let condition = FilterCondition {
                    id: condition_row.try_get::<String, _>("id")?.parse()?,
                    filter_id: condition_row.try_get::<String, _>("filter_id")?.parse()?,
                    field_name: condition_row.try_get("field_name")?,
                    operator: condition_row.try_get("operator")?,
                    value: condition_row.try_get("value")?,
                    sort_order: condition_row.try_get("sort_order")?,
                    created_at: condition_row.try_get("created_at")?,
                };
                conditions.push(condition);
            }

            let usage_count: i64 = row.try_get("usage_count")?;

            result.push(FilterWithUsage {
                filter,
                conditions,
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
        // Create a temporary filter for testing
        let temp_filter = Filter {
            id: Uuid::new_v4(),
            name: "Test Filter".to_string(),
            starting_channel_number: 1,
            is_inverse: request.is_inverse,
            logical_operator: request.logical_operator.clone(),
            condition_tree: request.condition_tree.clone(),
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        };

        // Create temporary conditions
        let temp_conditions: Vec<FilterCondition> = request
            .conditions
            .iter()
            .enumerate()
            .map(|(index, condition)| FilterCondition {
                id: Uuid::new_v4(),
                filter_id: temp_filter.id,
                field_name: condition.field_name.clone(),
                operator: condition.operator.clone(),
                value: condition.value.clone(),
                sort_order: index as i32,
                created_at: chrono::Utc::now(),
            })
            .collect();

        // Get all channels for the source - using manual row mapping to handle UUID strings
        let rows = sqlx::query(
            "SELECT id, source_id, tvg_id, tvg_name, tvg_logo, group_title, channel_name, stream_url, created_at, updated_at
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
                id: Uuid::parse_str(&id_str).map_err(|e| anyhow::anyhow!("Invalid UUID: {}", e))?,
                source_id: Uuid::parse_str(&source_id_str)
                    .map_err(|e| anyhow::anyhow!("Invalid UUID: {}", e))?,
                tvg_id: row.get("tvg_id"),
                tvg_name: row.get("tvg_name"),
                tvg_logo: row.get("tvg_logo"),
                group_title: row.get("group_title"),
                channel_name: row.get("channel_name"),
                stream_url: row.get("stream_url"),
                created_at: chrono::DateTime::parse_from_rfc3339(&created_at_str)
                    .map_err(|e| anyhow::anyhow!("Invalid datetime: {}", e))?
                    .with_timezone(&chrono::Utc),
                updated_at: chrono::DateTime::parse_from_rfc3339(&updated_at_str)
                    .map_err(|e| anyhow::anyhow!("Invalid datetime: {}", e))?
                    .with_timezone(&chrono::Utc),
            });
        }

        let total_channels = channels.len();
        // Use the filter engine to apply the filter
        let mut filter_engine = crate::proxy::filter_engine::FilterEngine::new();
        let matching_channels: Vec<FilterTestChannel> = match filter_engine
            .apply_single_filter(&channels, &temp_filter, &temp_conditions)
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
                });
            }
        };

        let matched_count = matching_channels.len();

        Ok(FilterTestResult {
            is_valid: true,
            error: None,
            matching_channels,
            total_channels,
            matched_count,
        })
    }

    #[allow(dead_code)]
    pub async fn get_proxy_filters(&self, proxy_id: Uuid) -> Result<Vec<ProxyFilterWithDetails>> {
        let filters = sqlx::query(
            "SELECT pf.proxy_id, pf.filter_id, pf.sort_order, pf.is_active, pf.created_at,
                    f.name, f.starting_channel_number, f.is_inverse, f.logical_operator, f.condition_tree, f.updated_at as filter_updated_at
             FROM proxy_filters pf
             JOIN filters f ON pf.filter_id = f.id
             WHERE pf.proxy_id = ?
             ORDER BY pf.sort_order",
        )
        .bind(proxy_id.to_string())
        .fetch_all(&self.pool)
        .await?;

        let mut result = Vec::new();
        for row in filters {
            let proxy_filter = ProxyFilter {
                proxy_id: Uuid::parse_str(&row.get::<String, _>("proxy_id"))?,
                filter_id: Uuid::parse_str(&row.get::<String, _>("filter_id"))?,
                sort_order: row.get("sort_order"),
                is_active: row.get("is_active"),
                created_at: row.get("created_at"),
            };

            let filter = Filter {
                id: proxy_filter.filter_id,
                name: row.get("name"),
                starting_channel_number: row.get("starting_channel_number"),
                is_inverse: row.get("is_inverse"),
                logical_operator: row.get("logical_operator"),
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
            "INSERT INTO proxy_filters (proxy_id, filter_id, sort_order, is_active)
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
                "UPDATE proxy_filters SET sort_order = ? WHERE proxy_id = ? AND filter_id = ?",
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

    pub async fn get_filter_conditions(&self, filter_id: Uuid) -> Result<Vec<FilterCondition>> {
        let rows = sqlx::query(
            "SELECT id, filter_id, field_name, operator, value, sort_order, created_at
             FROM filter_conditions
             WHERE filter_id = ?
             ORDER BY sort_order",
        )
        .bind(filter_id.to_string())
        .fetch_all(&self.pool)
        .await?;

        let mut conditions = Vec::new();
        for row in rows {
            let condition = FilterCondition {
                id: Uuid::parse_str(&row.get::<String, _>("id"))?,
                filter_id: Uuid::parse_str(&row.get::<String, _>("filter_id"))?,
                field_name: row.get("field_name"),
                operator: row.get("operator"),
                value: row.get("value"),
                sort_order: row.get("sort_order"),
                created_at: row.get("created_at"),
            };
            conditions.push(condition);
        }

        Ok(conditions)
    }
}
