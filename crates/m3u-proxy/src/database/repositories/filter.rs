//! SeaORM Filter repository implementation
//!
//! This module provides the SeaORM implementation of filter repository
//! that works across SQLite, PostgreSQL, and MySQL databases.
//
// NOTE: get_available_filter_fields will be refactored to be registry-driven.
// Please re-run with tooling enabled so I can capture the exact lines for the
// current hard‑coded implementation and replace them minimally.

use anyhow::Result;
use sea_orm::{
    ActiveModelTrait, ColumnTrait, DatabaseConnection, DbErr, EntityTrait, PaginatorTrait,
    QueryFilter, QueryOrder, Set,
};
use std::sync::Arc;
use tracing::warn;
use uuid::Uuid;

/// Detect whether a database error corresponds to the legacy single-column
/// UNIQUE(name) constraint (`filters_name_key`) that should have been removed
/// by the normalization migrations. We fallback to a string match on the
/// formatted error to avoid depending on internal variant shapes.
fn is_legacy_filter_name_unique_violation(err: &DbErr) -> bool {
    let msg = err.to_string();
    let m = msg.to_lowercase();

    // Direct legacy constraint name OR generic duplicate/unique violation mentioning filters + name
    m.contains("filters_name_key")
        || ((m.contains("duplicate") || m.contains("unique"))
            && m.contains("filters")
            && m.contains("name")
            && !m.contains("source_type"))
}

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
    ///
    /// Adds defensive handling for legacy single-column UNIQUE(name) constraint
    /// that should have been removed by migrations. If we detect that constraint
    /// we return a helpful error instructing the operator to run migrations.
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

        let insert_result = active_model.insert(&*self.connection).await;
        let model = match insert_result {
            Ok(m) => m,
            Err(e) => {
                if is_legacy_filter_name_unique_violation(&e) {
                    warn!(
                        "Legacy filters_name_key unique constraint still present; migrations not fully applied"
                    );
                    return Err(anyhow::anyhow!(
                        "Duplicate filter name detected under legacy single-column uniqueness. \
                         The database still has the old UNIQUE(name) constraint. \
                         Run the latest migrations (including normalization) to allow \
                         duplicate names across source types. Original error: {e}"
                    ));
                }
                return Err(e.into());
            }
        };

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
    ///
    /// Defensive handling for legacy UNIQUE(name) constraint still lingering.
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

        let update_result = active_model.update(&*self.connection).await;
        let updated_model = match update_result {
            Ok(m) => m,
            Err(e) => {
                if is_legacy_filter_name_unique_violation(&e) {
                    warn!(
                        "Legacy filters_name_key unique constraint still present during update; migrations not fully applied"
                    );
                    return Err(anyhow::anyhow!(
                        "Cannot rename/update filter due to legacy UNIQUE(name) constraint. \
                         Apply latest migrations to enable composite (name, source_type) uniqueness. \
                         Original error: {e}"
                    ));
                }
                return Err(e.into());
            }
        };

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

    /// Get available filter fields for building filter expressions (registry-driven)
    pub async fn get_available_filter_fields(&self) -> Result<Vec<crate::models::FilterFieldInfo>> {
        use crate::field_registry::{
            FieldDataType, FieldDescriptor, FieldRegistry, SourceKind, StageKind,
        };
        use std::collections::BTreeMap;

        let registry = FieldRegistry::global();

        // Collect Filtering stage descriptors for both source kinds, dedupe by canonical name
        let mut map: BTreeMap<&'static str, &'static FieldDescriptor> = BTreeMap::new();
        for sk in [SourceKind::Stream, SourceKind::Epg] {
            for d in registry.descriptors_for(sk, StageKind::Filtering) {
                map.entry(d.name).or_insert(d);
            }
        }

        let fields = map
            .values()
            .map(|d| {
                let field_type = match d.data_type {
                    FieldDataType::Url => "url",
                    FieldDataType::Integer => "integer",
                    FieldDataType::DateTime => "datetime",
                    FieldDataType::Duration => "duration",
                    FieldDataType::String => "string",
                };
                crate::models::FilterFieldInfo {
                    name: d.name.to_string(),
                    canonical_name: d.name.to_string(),
                    display_name: d.display_name.to_string(),
                    field_type: field_type.to_string(),
                    nullable: d.nullable,
                    // Heuristic: prefer Stream when a field applies to both (UI can still inspect sources array separately if added later)
                    source_type: if d.source_kinds.contains(&SourceKind::Stream) {
                        crate::models::FilterSourceType::Stream
                    } else {
                        crate::models::FilterSourceType::Epg
                    },
                    read_only: d.read_only,
                    aliases: d.aliases.iter().map(|a| a.to_string()).collect(),
                }
            })
            .collect();

        Ok(fields)
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

    /// Test a filter pattern against channels or EPG programs from a specific source (paginated, sampled)
    pub async fn test_filter_pattern(
        &self,
        pattern: &str,
        source_type: crate::models::FilterSourceType,
        source_id: Option<Uuid>,
    ) -> Result<crate::models::FilterTestResult> {
        use crate::models::FilterTestChannel;
        use crate::pipeline::engines::filter_processor::{
            EpgFilterProcessor, FilterProcessor, RegexEvaluator, StreamFilterProcessor,
        };
        use crate::utils::regex_preprocessor::{RegexPreprocessor, RegexPreprocessorConfig};
        use sea_orm::{EntityTrait, PaginatorTrait, QueryFilter};

        // Configure field set based on source type
        let fields = match source_type {
            crate::models::FilterSourceType::Stream => vec![
                "tvg_id",
                "tvg_name",
                "tvg_logo",
                "tvg_shift",
                "group_title",
                "channel_name",
                "stream_url",
            ]
            .into_iter()
            .map(String::from)
            .collect(),
            crate::models::FilterSourceType::Epg => vec![
                "channel_id",
                "program_title",
                "program_description",
                "program_category",
                "start_time",
                "end_time",
                "language",
                "rating",
                "episode_num",
                "season_num",
            ]
            .into_iter()
            .map(String::from)
            .collect(),
        };

        // Parse (syntactic + alias) first
        let registry = crate::field_registry::FieldRegistry::global();
        let parser = crate::expression_parser::ExpressionParser::new()
            .with_fields(fields)
            .with_aliases(registry.alias_map());

        let parsed_tree = match parser.parse(pattern) {
            Err(e) => {
                return Ok(crate::models::FilterTestResult {
                    is_valid: false,
                    error: Some(format!("Invalid filter expression: {e}")),
                    matching_channels: Vec::new(),
                    total_channels: 0,
                    matched_count: 0,
                    expression_tree: None,
                    scanned_records: Some(0),
                    truncated: Some(false),
                });
            }
            Ok(ct) => ct,
        };

        // Common regex evaluator
        let regex_evaluator =
            RegexEvaluator::new(RegexPreprocessor::new(RegexPreprocessorConfig::default()));

        // Pagination + sampling parameters
        let page_size: u64 = 1000;
        // Removed sampling limit: return all matches

        match source_type {
            crate::models::FilterSourceType::Stream => {
                use crate::entities::{channels, prelude::Channels};
                // Build processor
                let mut proc = StreamFilterProcessor::new(
                    uuid::Uuid::new_v4().to_string(),
                    "Test Stream Filter".into(),
                    false,
                    pattern,
                    regex_evaluator,
                )
                .map_err(|e| anyhow::anyhow!("Failed to create stream filter processor: {e}"))?;

                // Base query
                let base = if let Some(src) = source_id {
                    Channels::find().filter(channels::Column::SourceId.eq(src))
                } else {
                    Channels::find()
                };

                let paginator = base.paginate(&*self.connection, page_size);
                let total = paginator.num_items().await? as usize;

                let mut page_index = 0;
                let mut scanned = 0usize;
                let mut matched = 0usize;
                let mut samples = Vec::new();

                while let Ok(models) = paginator.fetch_page(page_index).await {
                    if models.is_empty() {
                        break;
                    }
                    page_index += 1;
                    for model in models {
                        // Map entity model -> Channel DTO expected by processor
                        let channel_dto = crate::models::Channel {
                            id: model.id,
                            source_id: model.source_id,
                            tvg_id: model.tvg_id.clone(),
                            tvg_name: model.tvg_name.clone(),
                            tvg_chno: model.tvg_chno.clone(),
                            tvg_logo: model.tvg_logo.clone(),
                            tvg_shift: model.tvg_shift.clone(),
                            group_title: model.group_title.clone(),
                            channel_name: model.channel_name.clone(),
                            stream_url: model.stream_url.clone(),
                            video_codec: None,
                            audio_codec: None,
                            resolution: None,
                            probe_method: None,
                            last_probed_at: None,
                            created_at: model.created_at,
                            updated_at: model.updated_at,
                        };
                        scanned += 1;
                        match proc.process_record(&channel_dto) {
                            Ok(fr) => {
                                if fr.include_match {
                                    matched += 1;
                                    // Unconditional push (no sampling)
                                    samples.push(FilterTestChannel {
                                        channel_name: model.channel_name.clone(),
                                        group_title: model.group_title.clone(),
                                        matched_text: None,
                                    });
                                }
                            }
                            Err(e) => {
                                return Ok(crate::models::FilterTestResult {
                                    is_valid: false,
                                    error: Some(format!("Filter processing error: {e}")),
                                    matching_channels: Vec::new(),
                                    total_channels: total,
                                    matched_count: 0,
                                    expression_tree: None,
                                    scanned_records: Some(scanned),
                                    truncated: Some(false),
                                });
                            }
                        }
                    }
                }

                Ok(crate::models::FilterTestResult {
                    is_valid: true,
                    error: None,
                    matching_channels: samples,
                    total_channels: total,
                    matched_count: matched,
                    expression_tree: serde_json::to_value(&parsed_tree).ok(),
                    scanned_records: Some(scanned),
                    truncated: Some(false),
                })
            }
            crate::models::FilterSourceType::Epg => {
                use crate::entities::{epg_programs, prelude::EpgPrograms};
                // Build processor
                let mut proc = EpgFilterProcessor::new(
                    uuid::Uuid::new_v4().to_string(),
                    "Test EPG Filter".into(),
                    false,
                    pattern,
                    regex_evaluator,
                )
                .map_err(|e| anyhow::anyhow!("Failed to create EPG filter processor: {e}"))?;

                let base = if let Some(src) = source_id {
                    EpgPrograms::find().filter(epg_programs::Column::SourceId.eq(src))
                } else {
                    EpgPrograms::find()
                };

                let paginator = base.paginate(&*self.connection, page_size);
                let total = paginator.num_items().await? as usize;

                let mut page_index = 0;
                let mut scanned = 0usize;
                let mut matched = 0usize;
                let mut samples = Vec::new();

                while let Ok(models) = paginator.fetch_page(page_index).await {
                    if models.is_empty() {
                        break;
                    }
                    page_index += 1;
                    for model in models {
                        scanned += 1;
                        let program = crate::pipeline::engines::rule_processor::EpgProgram {
                            id: model.id.to_string(),
                            channel_id: model.channel_id.clone(),
                            channel_name: model.channel_name.clone(),
                            title: model.program_title.clone(),
                            description: model.program_description.clone(),
                            program_icon: model.program_icon.clone(),
                            start_time: model.start_time,
                            end_time: model.end_time,
                            program_category: model.program_category.clone(),
                            subtitles: model.subtitles.clone(),
                            episode_num: model.episode_num.clone(),
                            season_num: model.season_num.clone(),
                            language: model.language.clone(),
                            rating: model.rating.clone(),
                            aspect_ratio: model.aspect_ratio.clone(),
                        };
                        match proc.process_record(&program) {
                            Ok(fr) => {
                                if fr.include_match {
                                    matched += 1;
                                    // Unconditional push (no sampling)
                                    samples.push(FilterTestChannel {
                                        channel_name: program.title.clone(),
                                        group_title: program.program_category.clone(),
                                        matched_text: None,
                                    });
                                }
                            }
                            Err(e) => {
                                return Ok(crate::models::FilterTestResult {
                                    is_valid: false,
                                    error: Some(format!("EPG filter processing error: {e}")),
                                    matching_channels: Vec::new(),
                                    total_channels: total,
                                    matched_count: 0,
                                    expression_tree: None,
                                    scanned_records: Some(scanned),
                                    truncated: Some(false),
                                });
                            }
                        }
                    }
                }

                Ok(crate::models::FilterTestResult {
                    is_valid: true,
                    error: None,
                    matching_channels: samples,
                    total_channels: total,
                    matched_count: matched,
                    expression_tree: serde_json::to_value(&parsed_tree).ok(),
                    scanned_records: Some(scanned),
                    truncated: Some(false),
                })
            }
        }
    }
}

#[cfg(test)]
mod epg_filter_test_endpoint_tests {

    use crate::models::FilterSourceType;

    // NOTE:
    // This test focuses on ensuring that the repository `test_filter_pattern`
    // path for EPG filters no longer rejects a valid EPG-only field (`channel_id`)
    // with the pattern: channel_id contains "sport".
    //
    // It uses an in-memory SQLite database, runs migrations, inserts a minimal
    // epg_programs row whose channel_id includes 'sport', and then invokes
    // `test_filter_pattern`. A successful result (is_valid=true and matched_count > 0)
    // demonstrates that the EPG branch (EpgFilterProcessor) is engaged instead of
    // the StreamFilterProcessor (which previously caused the unknown-field error).
    //
    // If additional required columns are added to the epg_programs schema in the future,
    // extend the ActiveModel population below accordingly.

    #[tokio::test]
    async fn test_epg_filter_test_pattern_channel_id_contains_sport() {
        // In-memory SQLite (unique URI to avoid migration version collisions)
        let db_url = format!(
            "sqlite::memory:?cache=shared&mode=memory&filename={}",
            uuid::Uuid::new_v4()
        );
        let db = sea_orm::Database::connect(&db_url)
            .await
            .expect("memory db");
        {
            use crate::database::migrations::Migrator;
            use sea_orm_migration::MigratorTrait;
            Migrator::up(&db, None).await.expect("migrations");
        }

        // Insert a minimal EPG source (needed for FK if enforced)
        let epg_source_id = uuid::Uuid::new_v4();
        {
            use crate::entities::epg_sources;
            use chrono::Utc;
            use sea_orm::{ActiveModelTrait, Set};
            let now = Utc::now();
            let src = epg_sources::ActiveModel {
                id: Set(epg_source_id),
                name: Set("Test EPG Source".to_string()),
                source_type: Set("xmltv".to_string()),
                url: Set("http://example.com/epg.xml".to_string()),
                update_cron: Set("@daily".to_string()),
                username: Set(None),
                password: Set(None),
                original_timezone: Set(None),
                time_offset: Set(None),
                created_at: Set(now),
                updated_at: Set(now),
                last_ingested_at: Set(None),
                is_active: Set(true),
            };
            let _ = src.insert(&db).await.expect("insert epg source");
        }

        // Insert an EPG program whose channel_id includes 'sport'
        let program_id = uuid::Uuid::new_v4();
        {
            use crate::entities::epg_programs;
            use chrono::{Duration, Utc};
            use sea_orm::{ActiveModelTrait, Set};

            let now = Utc::now();
            let prog = epg_programs::ActiveModel {
                id: Set(program_id),
                source_id: Set(epg_source_id),
                channel_id: Set("BeInSports1.fr".to_string()),
                channel_name: Set("BeIn Sports".to_string()),
                program_title: Set("Weekend Match".to_string()),
                program_description: Set(Some("Live sports event".to_string())),
                program_icon: Set(None),
                start_time: Set(now),
                end_time: Set(now + Duration::minutes(90)),
                program_category: Set(Some("Sports".to_string())),
                subtitles: Set(None),
                episode_num: Set(None),
                season_num: Set(None),
                language: Set(Some("en".to_string())),
                rating: Set(None),
                aspect_ratio: Set(None),
                created_at: Set(now),
                updated_at: Set(now),
            };
            let _ = prog.insert(&db).await.expect("insert epg program");
        }

        // Build repository
        let repo = super::FilterSeaOrmRepository::new(db.into());

        // Execute test filter pattern with the EPG source type
        let result = repo
            .test_filter_pattern(
                r#"channel_id contains "sport""#,
                FilterSourceType::Epg,
                None,
            )
            .await
            .expect("repo call");

        assert!(
            result.is_valid,
            "EPG filter pattern should be valid: {:?}",
            result.error
        );
        assert!(
            result.matched_count > 0,
            "Expected at least one matched EPG program, got {}",
            result.matched_count
        );
    }

    #[tokio::test]
    async fn test_duplicate_filter_names_different_source_types() {
        use crate::models::{FilterCreateRequest, FilterSourceType};

        // In-memory SQLite (after rebuild migration ensures composite uniqueness)
        let db_url = format!(
            "sqlite::memory:?cache=shared&mode=memory&filename={}",
            uuid::Uuid::new_v4()
        );
        let db = sea_orm::Database::connect(&db_url)
            .await
            .expect("memory db");

        {
            use crate::database::migrations::Migrator;
            use sea_orm_migration::MigratorTrait;
            Migrator::up(&db, None).await.expect("migrations");
        }

        let repo = super::FilterSeaOrmRepository::new(db.into());

        // Create a STREAM filter named "Sports"
        let f1 = repo
            .create(FilterCreateRequest {
                name: "Sports".into(),
                source_type: FilterSourceType::Stream,
                is_inverse: false,
                expression: r#"channel_name contains "Sport""#.into(),
            })
            .await
            .expect("create stream filter");

        // Create an EPG filter with the same name "Sports"
        let f2 = repo
            .create(FilterCreateRequest {
                name: "Sports".into(),
                source_type: FilterSourceType::Epg,
                is_inverse: false,
                expression: r#"channel_id contains "sport""#.into(),
            })
            .await
            .expect("create epg filter");

        // Assertions: allowed same name across source types, different IDs & types
        assert_ne!(f1.id, f2.id, "Filters should have distinct IDs");
        assert_eq!(
            f1.name, f2.name,
            "Names should match for duplicate-name test"
        );
        assert_ne!(f1.source_type, f2.source_type, "Source types must differ");
    }

    #[tokio::test]
    async fn test_duplicate_filter_same_name_same_source_rejected() {
        use crate::models::{FilterCreateRequest, FilterSourceType};

        let db_url = format!(
            "sqlite::memory:?cache=shared&mode=memory&filename={}",
            uuid::Uuid::new_v4()
        );
        let db = sea_orm::Database::connect(&db_url)
            .await
            .expect("memory db");

        {
            use crate::database::migrations::Migrator;
            use sea_orm_migration::MigratorTrait;
            Migrator::up(&db, None).await.expect("migrations");
        }

        let repo = super::FilterSeaOrmRepository::new(db.into());

        // First creation should succeed
        repo.create(FilterCreateRequest {
            name: "DupCheck".into(),
            source_type: FilterSourceType::Stream,
            is_inverse: false,
            expression: r#"channel_name contains "News""#.into(),
        })
        .await
        .expect("first filter create should succeed");

        // Second creation with same name & same source_type should fail
        let second = repo
            .create(FilterCreateRequest {
                name: "DupCheck".into(),
                source_type: FilterSourceType::Stream,
                is_inverse: false,
                expression: r#"channel_name contains "Sports""#.into(),
            })
            .await;

        assert!(
            second.is_err(),
            "Expected duplicate same-source filter name to be rejected"
        );
    }
}
