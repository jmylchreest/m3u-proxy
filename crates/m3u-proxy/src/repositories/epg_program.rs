//! EPG program repository implementation
//!
//! This module provides the concrete implementation of the repository pattern
//! for EPG programs, handling program data access including time-based queries.

use async_trait::async_trait;
use sqlx::{Pool, Sqlite, Row};
use uuid::Uuid;
use std::collections::HashMap;
use chrono::{DateTime, Utc};

use crate::errors::{RepositoryError, RepositoryResult};
use crate::models::{EpgProgram};
use crate::utils::uuid_parser::parse_uuid_flexible;
use super::traits::{Repository, BulkRepository, PaginatedRepository, QueryParams, PaginatedResult};

/// Query parameters specific to EPG programs
#[derive(Debug, Clone, Default)]
pub struct EpgProgramQuery {
    /// Base query parameters
    pub base: QueryParams,
    /// Filter by source ID
    pub source_id: Option<Uuid>,
    /// Filter by channel ID
    pub channel_id: Option<Uuid>,
    /// Filter by time range - start time
    pub start_time: Option<DateTime<Utc>>,
    /// Filter by time range - end time
    pub end_time: Option<DateTime<Utc>>,
    /// Filter by program category
    pub category: Option<String>,
}

impl EpgProgramQuery {
    /// Create new empty query
    pub fn new() -> Self {
        Self::default()
    }

    /// Filter by source
    pub fn source_id(mut self, source_id: Uuid) -> Self {
        self.source_id = Some(source_id);
        self
    }

    /// Filter by channel
    pub fn channel_id(mut self, channel_id: Uuid) -> Self {
        self.channel_id = Some(channel_id);
        self
    }

    /// Filter by time range
    pub fn time_range(mut self, start: DateTime<Utc>, end: DateTime<Utc>) -> Self {
        self.start_time = Some(start);
        self.end_time = Some(end);
        self
    }

    /// Filter by category
    pub fn category<S: Into<String>>(mut self, category: S) -> Self {
        self.category = Some(category.into());
        self
    }
}

/// EPG program repository implementation
pub struct EpgProgramRepository {
    pool: Pool<Sqlite>,
}

impl EpgProgramRepository {
    /// Create a new EPG program repository
    pub fn new(pool: Pool<Sqlite>) -> Self {
        Self { pool }
    }

    /// Build WHERE clause from query parameters
    fn build_where_clause(&self, query: &EpgProgramQuery) -> (String, Vec<String>) {
        let mut conditions = Vec::new();
        let mut params = Vec::new();

        if let Some(source_id) = query.source_id {
            conditions.push("source_id = ?".to_string());
            params.push(source_id.to_string());
        }

        if let Some(channel_id) = query.channel_id {
            conditions.push("channel_id = ?".to_string());
            params.push(channel_id.to_string());
        }

        if let Some(start_time) = query.start_time {
            conditions.push("end_time >= ?".to_string());
            params.push(start_time.to_rfc3339());
        }

        if let Some(end_time) = query.end_time {
            conditions.push("start_time <= ?".to_string());
            params.push(end_time.to_rfc3339());
        }

        if let Some(category) = &query.category {
            conditions.push("program_category LIKE ?".to_string());
            params.push(format!("%{category}%"));
        }

        if let Some(search) = &query.base.search {
            conditions.push("(program_title LIKE ? OR program_description LIKE ?)".to_string());
            let search_param = format!("%{search}%");
            params.push(search_param.clone());
            params.push(search_param);
        }

        let where_clause = if conditions.is_empty() {
            String::new()
        } else {
            format!(" WHERE {}", conditions.join(" AND "))
        };

        (where_clause, params)
    }

    /// Build ORDER BY clause from query parameters
    fn build_order_clause(&self, query: &EpgProgramQuery) -> String {
        let sort_field = query.base.sort_by.as_deref().unwrap_or("start_time");
        let direction = if query.base.sort_ascending { "ASC" } else { "DESC" };
        
        match sort_field {
            "start_time" => format!(" ORDER BY start_time {direction}"),
            "end_time" => format!(" ORDER BY end_time {direction}"),
            "program_title" => format!(" ORDER BY program_title {direction}"),
            "channel_name" => format!(" ORDER BY channel_name {direction}, start_time ASC"),
            _ => " ORDER BY start_time ASC".to_string(),
        }
    }

    /// Convert database row to EpgProgram model
    fn row_to_model(&self, row: &sqlx::sqlite::SqliteRow) -> RepositoryResult<EpgProgram> {
        Ok(EpgProgram {
            id: parse_uuid_flexible(&row.try_get::<String, _>("id")?)?,
            source_id: parse_uuid_flexible(&row.try_get::<String, _>("source_id")?)?,
            channel_id: row.try_get("channel_id")?,
            channel_name: row.try_get("channel_name")?,
            program_title: row.try_get("program_title")?,
            program_description: row.try_get("program_description")?,
            program_category: row.try_get("program_category")?,
            start_time: row.try_get("start_time")?,
            end_time: row.try_get("end_time")?,
            episode_num: row.try_get("episode_num")?,
            season_num: row.try_get("season_num")?,
            rating: row.try_get("rating")?,
            language: row.try_get("language")?,
            subtitles: row.try_get("subtitles")?,
            aspect_ratio: row.try_get("aspect_ratio")?,
            program_icon: row.try_get("program_icon")?,
            created_at: row.try_get("created_at")?,
            updated_at: row.try_get("updated_at")?,
        })
    }

    /// Get EPG programs for a specific channel within a time range
    pub async fn get_programs_for_channel_in_timerange(
        &self,
        channel_id: Uuid,
        start_time: DateTime<Utc>,
        end_time: DateTime<Utc>,
    ) -> RepositoryResult<Vec<EpgProgram>> {
        let rows = sqlx::query(
            "SELECT ep.id, ep.source_id, ep.channel_id, ep.channel_name, ep.program_title, ep.program_description,
             ep.program_category, ep.start_time, ep.end_time, ep.episode_num, ep.season_num, ep.rating,
             ep.language, ep.subtitles, ep.aspect_ratio, ep.program_icon, ep.created_at, ep.updated_at
             FROM epg_programs ep
             JOIN epg_channels ec ON ep.channel_id = ec.channel_id AND ep.source_id = ec.source_id
             WHERE ec.id = ? AND ep.start_time >= ? AND ep.end_time <= ?
             ORDER BY ep.start_time",
        )
        .bind(channel_id.to_string())
        .bind(start_time)
        .bind(end_time)
        .fetch_all(&self.pool)
        .await?;

        let mut programs = Vec::new();
        for row in rows {
            programs.push(self.row_to_model(&row)?);
        }

        Ok(programs)
    }

    /// Get all EPG programs for a specific channel
    pub async fn get_programs_for_channel(&self, channel_id: Uuid) -> RepositoryResult<Vec<EpgProgram>> {
        let rows = sqlx::query(
            "SELECT ep.id, ep.source_id, ep.channel_id, ep.channel_name, ep.program_title, ep.program_description,
             ep.program_category, ep.start_time, ep.end_time, ep.episode_num, ep.season_num, ep.rating,
             ep.language, ep.subtitles, ep.aspect_ratio, ep.program_icon, ep.created_at, ep.updated_at
             FROM epg_programs ep
             JOIN epg_channels ec ON ep.channel_id = ec.channel_id AND ep.source_id = ec.source_id
             WHERE ec.id = ?
             ORDER BY ep.start_time",
        )
        .bind(channel_id.to_string())
        .fetch_all(&self.pool)
        .await?;

        let mut programs = Vec::new();
        for row in rows {
            programs.push(self.row_to_model(&row)?);
        }

        Ok(programs)
    }

    /// Get programs by source and time range (for bulk operations)
    pub async fn get_programs_by_source_and_timerange(
        &self,
        source_id: Uuid,
        start_time: DateTime<Utc>,
        end_time: DateTime<Utc>,
    ) -> RepositoryResult<Vec<EpgProgram>> {
        let query = EpgProgramQuery::new()
            .source_id(source_id)
            .time_range(start_time, end_time);
        
        self.find_all(query).await
    }

    /// Clean up old programs before a specific date (for storage management)
    pub async fn cleanup_old_programs(&self, before_date: DateTime<Utc>) -> RepositoryResult<u64> {
        let result = sqlx::query("DELETE FROM epg_programs WHERE end_time < ?")
            .bind(before_date)
            .execute(&self.pool)
            .await?;

        Ok(result.rows_affected())
    }

    /// Get program counts by source for monitoring EPG ingestion
    pub async fn get_program_counts_by_source(&self) -> RepositoryResult<HashMap<Uuid, u64>> {
        let rows = sqlx::query("SELECT source_id, COUNT(*) as count FROM epg_programs GROUP BY source_id")
            .fetch_all(&self.pool)
            .await?;

        let mut result = HashMap::new();
        for row in rows {
            let source_id = parse_uuid_flexible(&row.try_get::<String, _>("source_id")?)?;
            let count = row.try_get::<i64, _>("count")? as u64;
            result.insert(source_id, count);
        }

        Ok(result)
    }

    /// Replace all programs for a source (atomic operation for EPG ingestion)
    pub async fn replace_programs_for_source(
        &self,
        source_id: Uuid,
        programs: Vec<EpgProgram>,
    ) -> RepositoryResult<usize> {
        let mut tx = self.pool.begin().await?;

        // Delete existing programs for this source
        sqlx::query("DELETE FROM epg_programs WHERE source_id = ?")
            .bind(source_id.to_string())
            .execute(&mut *tx)
            .await?;

        // Insert new programs in batches for performance
        let mut inserted_count = 0;
        const BATCH_SIZE: usize = 100;

        for chunk in programs.chunks(BATCH_SIZE) {
            let mut query_parts = Vec::new();
            let mut params = Vec::new();

            for program in chunk {
                query_parts.push("(?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)");
                params.extend([
                    program.id.to_string(),
                    program.source_id.to_string(),
                    program.channel_id.clone(),
                    program.channel_name.clone(),
                    program.program_title.clone(),
                    program.program_description.clone().unwrap_or_default(),
                    program.program_category.clone().unwrap_or_default(),
                    program.start_time.to_rfc3339(),
                    program.end_time.to_rfc3339(),
                    program.episode_num.clone().map(|n| n.to_string()).unwrap_or_default(),
                    program.season_num.clone().map(|n| n.to_string()).unwrap_or_default(),
                    program.rating.clone().unwrap_or_default(),
                    program.language.clone().unwrap_or_default(),
                    program.subtitles.clone().unwrap_or_default(),
                    program.aspect_ratio.clone().unwrap_or_default(),
                    program.program_icon.clone().unwrap_or_default(),
                    program.created_at.to_rfc3339(),
                    program.updated_at.to_rfc3339(),
                ]);
            }

            if !query_parts.is_empty() {
                let insert_sql = format!(
                    "INSERT INTO epg_programs (
                        id, source_id, channel_id, channel_name, program_title, program_description,
                        program_category, start_time, end_time, episode_num, season_num, rating,
                        language, subtitles, aspect_ratio, program_icon, created_at, updated_at
                    ) VALUES {}",
                    query_parts.join(", ")
                );

                let mut query = sqlx::query(&insert_sql);
                for param in params {
                    query = query.bind(param);
                }

                let result = query.execute(&mut *tx).await?;
                inserted_count += result.rows_affected() as usize;
            }
        }

        tx.commit().await?;
        Ok(inserted_count)
    }

}

// For EPG programs, we don't typically need create/update operations since they're
// ingested from external sources, so we'll implement minimal repository traits
#[async_trait]
impl Repository<EpgProgram, Uuid> for EpgProgramRepository {
    type CreateRequest = (); // Not used for EPG programs
    type UpdateRequest = (); // Not used for EPG programs  
    type Query = EpgProgramQuery;

    async fn find_by_id(&self, id: Uuid) -> RepositoryResult<Option<EpgProgram>> {
        let sql = "SELECT id, source_id, channel_id, channel_name, program_title, program_description,
                   program_category, start_time, end_time, episode_num, season_num, rating,
                   language, subtitles, aspect_ratio, program_icon, created_at, updated_at
                   FROM epg_programs WHERE id = ?";

        let row = sqlx::query(sql)
            .bind(id.to_string())
            .fetch_optional(&self.pool)
            .await?;

        match row {
            Some(row) => Ok(Some(self.row_to_model(&row)?)),
            None => Ok(None),
        }
    }

    async fn find_all(&self, query: Self::Query) -> RepositoryResult<Vec<EpgProgram>> {
        let (where_clause, params) = self.build_where_clause(&query);
        let order_clause = self.build_order_clause(&query);
        
        let limit_clause = if let Some(limit) = query.base.limit {
            let offset = query.base.offset.unwrap_or(0);
            format!(" LIMIT {limit} OFFSET {offset}")
        } else {
            String::new()
        };

        let sql = format!(
            "SELECT id, source_id, channel_id, channel_name, program_title, program_description,
             program_category, start_time, end_time, episode_num, season_num, rating,
             language, subtitles, aspect_ratio, program_icon, created_at, updated_at
             FROM epg_programs{where_clause}{order_clause}{limit_clause}"
        );

        let mut query_builder = sqlx::query(&sql);
        for param in params {
            query_builder = query_builder.bind(param);
        }

        let rows = query_builder.fetch_all(&self.pool).await?;

        let mut results = Vec::new();
        for row in rows {
            results.push(self.row_to_model(&row)?);
        }

        Ok(results)
    }

    async fn create(&self, _request: Self::CreateRequest) -> RepositoryResult<EpgProgram> {
        Err(RepositoryError::query_failed(
            "create_epg_program", 
            "EPG programs are read-only and ingested from external sources"
        ))
    }

    async fn update(&self, _id: Uuid, _request: Self::UpdateRequest) -> RepositoryResult<EpgProgram> {
        Err(RepositoryError::query_failed(
            "update_epg_program", 
            "EPG programs are read-only and ingested from external sources"
        ))
    }

    async fn delete(&self, id: Uuid) -> RepositoryResult<()> {
        let sql = "DELETE FROM epg_programs WHERE id = ?";

        let result = sqlx::query(sql)
            .bind(id.to_string())
            .execute(&self.pool)
            .await?;

        if result.rows_affected() == 0 {
            return Err(RepositoryError::record_not_found("epg_programs", "id", id.to_string()));
        }

        Ok(())
    }

    async fn count(&self, query: Self::Query) -> RepositoryResult<u64> {
        let (where_clause, params) = self.build_where_clause(&query);
        let sql = format!("SELECT COUNT(*) FROM epg_programs{where_clause}");

        let mut query_builder = sqlx::query_scalar(&sql);
        for param in params {
            query_builder = query_builder.bind(param);
        }

        let count: i64 = query_builder.fetch_one(&self.pool).await?;
        Ok(count as u64)
    }
}

#[async_trait]
impl BulkRepository<EpgProgram, Uuid> for EpgProgramRepository {
    async fn create_bulk(&self, _requests: Vec<Self::CreateRequest>) -> RepositoryResult<Vec<EpgProgram>> {
        Err(RepositoryError::query_failed(
            "create_bulk_epg_programs", 
            "EPG programs are read-only and ingested from external sources"
        ))
    }

    async fn update_bulk(&self, _updates: HashMap<Uuid, Self::UpdateRequest>) -> RepositoryResult<Vec<EpgProgram>> {
        Err(RepositoryError::query_failed(
            "update_bulk_epg_programs", 
            "EPG programs are read-only and ingested from external sources"
        ))
    }

    async fn delete_bulk(&self, ids: Vec<Uuid>) -> RepositoryResult<u64> {
        if ids.is_empty() {
            return Ok(0);
        }

        let placeholders = ids.iter().map(|_| "?").collect::<Vec<_>>().join(",");
        let sql = format!("DELETE FROM epg_programs WHERE id IN ({placeholders})");

        let mut query_builder = sqlx::query(&sql);
        for id in ids {
            query_builder = query_builder.bind(id.to_string());
        }

        let result = query_builder.execute(&self.pool).await?;
        Ok(result.rows_affected())
    }

    async fn find_by_ids(&self, ids: Vec<Uuid>) -> RepositoryResult<Vec<EpgProgram>> {
        if ids.is_empty() {
            return Ok(Vec::new());
        }

        let placeholders = ids.iter().map(|_| "?").collect::<Vec<_>>().join(",");
        let sql = format!(
            "SELECT id, source_id, channel_id, channel_name, program_title, program_description,
             program_category, start_time, end_time, episode_num, season_num, rating,
             language, subtitles, aspect_ratio, program_icon, created_at, updated_at
             FROM epg_programs WHERE id IN ({placeholders}) ORDER BY start_time"
        );

        let mut query_builder = sqlx::query(&sql);
        for id in ids {
            query_builder = query_builder.bind(id.to_string());
        }

        let rows = query_builder.fetch_all(&self.pool).await?;

        let mut results = Vec::new();
        for row in rows {
            results.push(self.row_to_model(&row)?);
        }

        Ok(results)
    }
}

#[async_trait]
impl PaginatedRepository<EpgProgram, Uuid> for EpgProgramRepository {
    type PaginatedResult = PaginatedResult<EpgProgram>;

    async fn find_paginated(
        &self,
        query: Self::Query,
        page: u32,
        limit: u32,
    ) -> RepositoryResult<Self::PaginatedResult> {
        let offset = (page.saturating_sub(1)) * limit;
        
        // Get total count
        let total_count = self.count(query.clone()).await?;
        
        // Get items for this page
        let mut page_query = query;
        page_query.base.limit = Some(limit);
        page_query.base.offset = Some(offset);
        
        let items = self.find_all(page_query).await?;
        
        Ok(PaginatedResult::new(items, page, limit, total_count))
    }
}