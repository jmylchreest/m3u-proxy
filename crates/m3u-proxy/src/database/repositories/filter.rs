//! SeaORM Filter repository implementation
//!
//! This module provides the SeaORM implementation of filter repository
//! that works across SQLite, PostgreSQL, and MySQL databases.

use anyhow::Result;
use sea_orm::{
    ActiveModelTrait, ColumnTrait, DatabaseConnection, EntityTrait, PaginatorTrait, QueryFilter,
    QueryOrder, QuerySelect, Set,
};
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
        let model = Filters::find_by_id(id).one(&*self.connection).await?;

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
            None => Ok(None),
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
        order: Option<String>,
    ) -> Result<Vec<crate::models::FilterWithUsage>> {
        let filters = self.list_all().await?;
        let mut filter_usage_list = Vec::new();

        for filter in filters {
            // Filter by source type if specified
            if let Some(ref st) = source_type
                && &filter.source_type != st
            {
                continue;
            }

            let usage_count = self.get_usage_count(&filter.id).await.unwrap_or(0);
            filter_usage_list.push(crate::models::FilterWithUsage {
                filter,
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
        pattern: &str,
        source_type: crate::models::FilterSourceType,
        source_id: Option<Uuid>,
    ) -> Result<crate::models::FilterTestResult> {
        use crate::models::FilterTestChannel;
        use crate::pipeline::engines::filter_processor::{
            FilterProcessor, RegexEvaluator, StreamFilterProcessor,
        };
        use crate::utils::regex_preprocessor::{RegexPreprocessor, RegexPreprocessorConfig};

        // Parse the expression first to check if it's valid
        let fields = match source_type {
            crate::models::FilterSourceType::Stream => vec![
                "tvg_id".to_string(),
                "tvg_name".to_string(),
                "tvg_logo".to_string(),
                "tvg_shift".to_string(),
                "group_title".to_string(),
                "channel_name".to_string(),
                "stream_url".to_string(),
            ],
            crate::models::FilterSourceType::Epg => vec![
                "channel_id".to_string(),
                "program_title".to_string(),
                "program_description".to_string(),
                "program_category".to_string(),
                "start_time".to_string(),
                "end_time".to_string(),
                "language".to_string(),
                "rating".to_string(),
                "episode_num".to_string(),
                "season_num".to_string(),
            ],
        };

        let parser = crate::expression_parser::ExpressionParser::new().with_fields(fields);

        match parser.parse(pattern) {
            Err(e) => Ok(crate::models::FilterTestResult {
                is_valid: false,
                error: Some(format!("Invalid filter expression: {e}")),
                matching_channels: Vec::new(),
                total_channels: 0,
                matched_count: 0,
                expression_tree: None,
            }),
            Ok(condition_tree) => {
                // Get channels from database for testing
                let channels = self
                    .get_channels_for_testing(source_type, source_id)
                    .await?;
                let total_channels = channels.len();

                // Create regex evaluator with default config
                let regex_preprocessor = RegexPreprocessor::new(RegexPreprocessorConfig::default());
                let regex_evaluator = RegexEvaluator::new(regex_preprocessor);

                // Create filter processor
                let mut filter_processor = StreamFilterProcessor::new(
                    Uuid::new_v4().to_string(),
                    "Test Filter".to_string(),
                    false, // not inverse for testing
                    pattern,
                    regex_evaluator,
                )
                .map_err(|e| anyhow::anyhow!("Failed to create filter processor: {e}"))?;

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

    /// Get channels for filter testing with source validation (adapted for SeaORM)
    async fn get_channels_for_testing(
        &self,
        source_type: crate::models::FilterSourceType,
        source_id: Option<Uuid>,
    ) -> Result<Vec<crate::models::Channel>> {
        use crate::entities::{channels, prelude::Channels};

        // Validate source_id exists if provided
        if let Some(source_id) = source_id {
            match source_type {
                crate::models::FilterSourceType::Stream => {
                    use crate::entities::prelude::StreamSources;
                    let exists = StreamSources::find_by_id(source_id)
                        .one(&*self.connection)
                        .await?
                        .is_some();
                    if !exists {
                        return Err(anyhow::anyhow!("Stream source ID {} not found", source_id));
                    }
                }
                crate::models::FilterSourceType::Epg => {
                    use crate::entities::prelude::EpgSources;
                    let exists = EpgSources::find_by_id(source_id)
                        .one(&*self.connection)
                        .await?
                        .is_some();
                    if !exists {
                        return Err(anyhow::anyhow!("EPG source ID {} not found", source_id));
                    }
                }
            }
        }

        // For now, only handle stream channels (EPG channel filtering would need epg_programs table)
        match source_type {
            crate::models::FilterSourceType::Stream => {
                let channels_query = if let Some(source_id) = source_id {
                    Channels::find().filter(channels::Column::SourceId.eq(source_id))
                } else {
                    Channels::find()
                };

                let channel_models = channels_query.all(&*self.connection).await?;
                let mut channels = Vec::new();

                for model in channel_models {
                    channels.push(crate::models::Channel {
                        id: model.id,
                        source_id: model.source_id,
                        tvg_id: model.tvg_id,
                        tvg_name: model.tvg_name,
                        tvg_chno: model.tvg_chno,
                        tvg_logo: model.tvg_logo,
                        tvg_shift: model.tvg_shift,
                        group_title: model.group_title,
                        channel_name: model.channel_name,
                        stream_url: model.stream_url,
                        video_codec: None,
                        audio_codec: None,
                        resolution: None,
                        probe_method: None,
                        last_probed_at: None,
                        created_at: model.created_at,
                        updated_at: model.updated_at,
                    });
                }

                Ok(channels)
            }
            crate::models::FilterSourceType::Epg => {
                // Get EPG programs from database for testing
                self.get_epg_programs_for_testing(source_id).await
            }
        }
    }

    /// Get EPG programs for filter testing with source validation
    async fn get_epg_programs_for_testing(
        &self,
        source_id: Option<Uuid>,
    ) -> Result<Vec<crate::models::Channel>> {
        use crate::entities::{epg_programs, prelude::EpgPrograms};

        // EPG programs need to be converted to a compatible format for filter testing
        // For now, we'll create mock Channel objects from EPG programs
        let epg_programs_query = if let Some(source_id) = source_id {
            EpgPrograms::find().filter(epg_programs::Column::SourceId.eq(source_id))
        } else {
            EpgPrograms::find()
        };

        let epg_program_models = epg_programs_query
            .limit(100) // Limit for testing performance
            .all(&*self.connection)
            .await?;

        let mut channels = Vec::new();

        // Convert EPG programs to Channel format for filter testing
        // This is a temporary approach - ideally we'd have separate EPG filter testing
        for model in epg_program_models {
            channels.push(crate::models::Channel {
                id: model.id,
                source_id: model.source_id,
                tvg_id: Some(model.channel_id.clone()),
                tvg_name: None,
                tvg_chno: None,
                tvg_logo: model.program_icon,
                tvg_shift: None,
                group_title: model.program_category,
                channel_name: model.program_title,
                stream_url: format!("epg://program/{}", model.id), // Mock URL
                video_codec: None,
                audio_codec: None,
                resolution: None,
                probe_method: None,
                last_probed_at: None,
                created_at: model.created_at,
                updated_at: model.updated_at,
            });
        }

        Ok(channels)
    }
}
