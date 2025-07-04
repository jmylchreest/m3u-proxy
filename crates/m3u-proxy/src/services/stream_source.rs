//! Stream source service implementation
//!
//! This module provides the business logic service for managing stream sources.
//! It orchestrates between the repository layer and implements business rules,
//! validation, and cross-cutting concerns.

use async_trait::async_trait;
use tracing::{debug, error, info, warn};
use uuid::Uuid;
use std::collections::HashMap;

use crate::errors::{AppError, AppResult};
use crate::models::{StreamSource, StreamSourceCreateRequest, StreamSourceUpdateRequest, StreamSourceType};
use crate::repositories::{Repository, BulkRepository};
use crate::repositories::stream_source::StreamSourceQuery;
use crate::utils::validation::{Validator, ValidationRule};
use super::traits::{Service, ValidationService, BulkService, ServiceListResponse, ServiceBulkResponse, BulkOperationError};

/// Query parameters for stream source service operations
#[derive(Debug, Clone, Default)]
pub struct StreamSourceServiceQuery {
    /// Search term for name/description
    pub search: Option<String>,
    /// Filter by source type
    pub source_type: Option<StreamSourceType>,
    /// Filter by enabled status
    pub enabled: Option<bool>,
    /// Sort field
    pub sort_by: Option<String>,
    /// Sort direction (true = ascending)
    pub sort_ascending: bool,
    /// Page number (1-based)
    pub page: Option<u32>,
    /// Items per page
    pub limit: Option<u32>,
}

impl StreamSourceServiceQuery {
    /// Create new empty query
    pub fn new() -> Self {
        Self::default()
    }

    /// Set search term
    pub fn search<S: Into<String>>(mut self, term: S) -> Self {
        self.search = Some(term.into());
        self
    }

    /// Filter by source type
    pub fn source_type(mut self, source_type: StreamSourceType) -> Self {
        self.source_type = Some(source_type);
        self
    }

    /// Filter by enabled status
    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = Some(enabled);
        self
    }

    /// Set sorting
    pub fn sort_by<S: Into<String>>(mut self, field: S, ascending: bool) -> Self {
        self.sort_by = Some(field.into());
        self.sort_ascending = ascending;
        self
    }

    /// Set pagination
    pub fn paginate(mut self, page: u32, limit: u32) -> Self {
        self.page = Some(page);
        self.limit = Some(limit);
        self
    }
}

/// Validation result for stream source operations
#[derive(Debug, Clone)]
pub struct StreamSourceValidationResult {
    /// Whether the validation passed
    pub is_valid: bool,
    /// Validation errors if any
    pub errors: Vec<String>,
    /// Warnings that don't prevent the operation
    pub warnings: Vec<String>,
    /// Additional context information
    pub context: HashMap<String, String>,
}

impl StreamSourceValidationResult {
    /// Create a successful validation result
    pub fn success() -> Self {
        Self {
            is_valid: true,
            errors: Vec::new(),
            warnings: Vec::new(),
            context: HashMap::new(),
        }
    }

    /// Create a failed validation result
    pub fn failure(errors: Vec<String>) -> Self {
        Self {
            is_valid: false,
            errors,
            warnings: Vec::new(),
            context: HashMap::new(),
        }
    }

    /// Add a warning to the result
    pub fn with_warning<S: Into<String>>(mut self, warning: S) -> Self {
        self.warnings.push(warning.into());
        self
    }

    /// Add context information
    pub fn with_context<K: Into<String>, V: Into<String>>(mut self, key: K, value: V) -> Self {
        self.context.insert(key.into(), value.into());
        self
    }
}

/// Service for managing stream sources
///
/// This service provides business logic for stream source operations including
/// validation, business rules, and orchestration of repository operations.
///
/// # Examples
///
/// ```rust
/// use crate::services::StreamSourceService;
/// use crate::repositories::StreamSourceRepository;
///
/// async fn example() -> AppResult<()> {
///     let repository = StreamSourceRepository::new(pool);
///     let service = StreamSourceService::new(repository);
///     
///     let request = StreamSourceCreateRequest {
///         name: "My Stream".to_string(),
///         source_type: StreamSourceType::M3u,
///         url: "http://example.com/playlist.m3u".to_string(),
///         // ... other fields
///     };
///     
///     let source = service.create(request).await?;
///     info!("Created stream source: {}", source.id);
///     
///     Ok(())
/// }
/// ```
pub struct StreamSourceService<R> 
where 
    R: Repository<StreamSource, Uuid, CreateRequest = StreamSourceCreateRequest, UpdateRequest = StreamSourceUpdateRequest, Query = StreamSourceQuery> 
        + BulkRepository<StreamSource, Uuid> + Send + Sync,
{
    repository: R,
}

impl<R> StreamSourceService<R>
where
    R: Repository<StreamSource, Uuid, CreateRequest = StreamSourceCreateRequest, UpdateRequest = StreamSourceUpdateRequest, Query = StreamSourceQuery> 
        + BulkRepository<StreamSource, Uuid> + Send + Sync,
{
    /// Create a new stream source service
    pub fn new(repository: R) -> Self {
        Self { repository }
    }

    /// Validate a stream source URL based on its type
    async fn validate_source_url(&self, url: &str, source_type: &StreamSourceType) -> AppResult<()> {
        debug!("Validating source URL: {} for type: {:?}", url, source_type);

        // Basic URL validation
        if !url.starts_with("http://") && !url.starts_with("https://") {
            return Err(AppError::validation("URL must use HTTP or HTTPS protocol"));
        }

        // Type-specific validation
        match source_type {
            StreamSourceType::M3u => {
                if !url.ends_with(".m3u") && !url.ends_with(".m3u8") && !url.contains("playlist") {
                    warn!("M3U URL '{}' doesn't have typical M3U extension or pattern", url);
                }
            }
            StreamSourceType::Xtream => {
                // Xtream URLs typically have specific patterns
                if !url.contains("/get.php") && !url.contains("/xmltv.php") && !url.contains("player_api") {
                    warn!("Xtream URL '{}' doesn't match typical Xtream patterns", url);
                }
            }
        }

        Ok(())
    }

    /// Validate stream source creation request
    async fn validate_create_request(&self, request: &StreamSourceCreateRequest) -> AppResult<StreamSourceValidationResult> {
        debug!("Validating stream source create request: {}", request.name);

        let mut errors = Vec::new();
        let mut warnings = Vec::new();

        // Basic field validation
        let mut validator_data = HashMap::new();
        validator_data.insert("name".to_string(), Some(request.name.clone()));
        validator_data.insert("url".to_string(), Some(request.url.clone()));
        validator_data.insert("update_cron".to_string(), Some(request.update_cron.clone()));

        let validator = Validator::new()
            .rule(ValidationRule::required("name"))
            .rule(ValidationRule::min_length("name", 1))
            .rule(ValidationRule::max_length("name", 255))
            .rule(ValidationRule::required("url"))
            .rule(ValidationRule::url("url"))
            .rule(ValidationRule::required("update_cron"));

        if let Err(validation_errors) = validator.validate(&validator_data) {
            errors.extend(validation_errors.into_iter().map(|e| e.to_string()));
        }

        // URL validation
        if let Err(e) = self.validate_source_url(&request.url, &request.source_type).await {
            errors.push(e.to_string());
        }

        // Business rule validation
        if request.max_concurrent_streams < 1 {
            errors.push("Max concurrent streams must be at least 1".to_string());
        }

        if request.max_concurrent_streams > 1000 {
            warnings.push("Max concurrent streams is very high (>1000), this may impact performance".to_string());
        }

        // Cron validation (basic check)
        if request.update_cron.split_whitespace().count() != 5 {
            errors.push("Update cron must be in standard 5-field format (minute hour day month weekday)".to_string());
        }

        // Xtream-specific validation
        if request.source_type == StreamSourceType::Xtream {
            if request.username.is_none() || request.password.is_none() {
                errors.push("Username and password are required for Xtream sources".to_string());
            }
        }

        let result = if errors.is_empty() {
            StreamSourceValidationResult::success()
        } else {
            StreamSourceValidationResult::failure(errors)
        };

        Ok(result.with_warning(warnings.join("; ")))
    }

    /// Convert service query to repository query
    fn convert_query(&self, query: StreamSourceServiceQuery) -> StreamSourceQuery {
        use crate::repositories::QueryParams;

        let mut base_params = QueryParams::new()
            .search(query.search.unwrap_or_default())
            .limit(query.limit.unwrap_or(50), query.page.unwrap_or(1).saturating_sub(1) * query.limit.unwrap_or(50));

        if let Some(sort_by) = query.sort_by {
            base_params = base_params.sort_by(sort_by, query.sort_ascending);
        }

        if let Some(enabled) = query.enabled {
            // Map enabled to is_active for repository
            // Note: This assumes enabled maps to is_active in the domain model
            base_params = base_params.filter("is_active", enabled.to_string());
        }

        let mut repo_query = StreamSourceQuery::new().with_base(base_params);

        if let Some(source_type) = query.source_type {
            repo_query = repo_query.source_type(source_type);
        }

        repo_query
    }

    /// Check for duplicate names
    async fn check_duplicate_name(&self, name: &str, exclude_id: Option<Uuid>) -> AppResult<bool> {
        let query = StreamSourceServiceQuery::new().search(name.to_string());
        let response = self.list(query).await?;
        
        let duplicates: Vec<_> = response.items.into_iter()
            .filter(|source| {
                source.name.eq_ignore_ascii_case(name) && 
                exclude_id.map_or(true, |id| source.id != id)
            })
            .collect();

        Ok(!duplicates.is_empty())
    }
}

#[async_trait]
impl<R> Service<StreamSource, Uuid> for StreamSourceService<R>
where
    R: Repository<StreamSource, Uuid, CreateRequest = StreamSourceCreateRequest, UpdateRequest = StreamSourceUpdateRequest, Query = StreamSourceQuery> 
        + BulkRepository<StreamSource, Uuid> + Send + Sync,
{
    type CreateRequest = StreamSourceCreateRequest;
    type UpdateRequest = StreamSourceUpdateRequest;
    type Query = StreamSourceServiceQuery;
    type ListResponse = ServiceListResponse<StreamSource>;

    async fn get_by_id(&self, id: Uuid) -> AppResult<Option<StreamSource>> {
        debug!("Getting stream source by ID: {}", id);
        
        match self.repository.find_by_id(id).await {
            Ok(source) => {
                if let Some(ref s) = source {
                    debug!("Found stream source: {} ({})", s.name, s.id);
                } else {
                    debug!("Stream source not found: {}", id);
                }
                Ok(source)
            }
            Err(e) => {
                error!("Failed to get stream source {}: {}", id, e);
                Err(AppError::from(e))
            }
        }
    }

    async fn list(&self, query: Self::Query) -> AppResult<Self::ListResponse> {
        debug!("Listing stream sources with query: {:?}", query);
        
        let repo_query = self.convert_query(query.clone());
        
        match self.repository.find_all(repo_query.clone()).await {
            Ok(sources) => {
                let total_count = self.repository.count(repo_query).await
                    .map_err(AppError::from)?;
                
                debug!("Found {} stream sources (total: {})", sources.len(), total_count);
                
                let response = if let (Some(page), Some(limit)) = (query.page, query.limit) {
                    ServiceListResponse::paginated(sources, total_count, page, limit)
                } else {
                    ServiceListResponse::new(sources, total_count)
                };
                
                Ok(response)
            }
            Err(e) => {
                error!("Failed to list stream sources: {}", e);
                Err(AppError::from(e))
            }
        }
    }

    async fn create(&self, request: Self::CreateRequest) -> AppResult<StreamSource> {
        info!("Creating stream source: {}", request.name);
        
        // Validate the request
        let validation = self.validate_create_request(&request).await?;
        if !validation.is_valid {
            error!("Validation failed for stream source '{}': {:?}", request.name, validation.errors);
            return Err(AppError::validation(validation.errors.join("; ")));
        }

        // Log warnings if any
        if !validation.warnings.is_empty() {
            warn!("Warnings for stream source '{}': {}", request.name, validation.warnings.join("; "));
        }

        // Check for duplicate names
        if self.check_duplicate_name(&request.name, None).await? {
            return Err(AppError::validation("A stream source with this name already exists"));
        }

        // Create the source
        match self.repository.create(request.clone()).await {
            Ok(source) => {
                info!("Successfully created stream source: {} ({})", source.name, source.id);
                Ok(source)
            }
            Err(e) => {
                error!("Failed to create stream source '{}': {}", request.name, e);
                Err(AppError::from(e))
            }
        }
    }

    async fn update(&self, id: Uuid, request: Self::UpdateRequest) -> AppResult<StreamSource> {
        info!("Updating stream source: {}", id);

        // Check if the source exists
        if !self.repository.exists(id).await.map_err(AppError::from)? {
            return Err(AppError::not_found("stream_source", id.to_string()));
        }

        // Validate the update request (convert to create request for validation)
        let create_request = StreamSourceCreateRequest {
            name: request.name.clone(),
            source_type: request.source_type.clone(),
            url: request.url.clone(),
            max_concurrent_streams: request.max_concurrent_streams,
            update_cron: request.update_cron.clone(),
            username: request.username.clone(),
            password: request.password.clone(),
            field_map: request.field_map.clone(),
        };

        let validation = self.validate_create_request(&create_request).await?;
        if !validation.is_valid {
            error!("Validation failed for stream source update '{}': {:?}", request.name, validation.errors);
            return Err(AppError::validation(validation.errors.join("; ")));
        }

        // Check for duplicate names (excluding current source)
        if self.check_duplicate_name(&request.name, Some(id)).await? {
            return Err(AppError::validation("A stream source with this name already exists"));
        }

        // Update the source
        match self.repository.update(id, request.clone()).await {
            Ok(source) => {
                info!("Successfully updated stream source: {} ({})", source.name, source.id);
                Ok(source)
            }
            Err(e) => {
                error!("Failed to update stream source {}: {}", id, e);
                Err(AppError::from(e))
            }
        }
    }

    async fn delete(&self, id: Uuid) -> AppResult<()> {
        info!("Deleting stream source: {}", id);

        // Check if the source exists and get its info for logging
        let source = self.get_by_id(id).await?
            .ok_or_else(|| AppError::not_found("stream_source", id.to_string()))?;

        // TODO: Check for dependencies (channels, proxies, etc.)
        // This would involve checking related entities before deletion

        // Delete the source
        match self.repository.delete(id).await {
            Ok(_) => {
                info!("Successfully deleted stream source: {} ({})", source.name, id);
                Ok(())
            }
            Err(e) => {
                error!("Failed to delete stream source {}: {}", id, e);
                Err(AppError::from(e))
            }
        }
    }
}

#[async_trait]
impl<R> ValidationService<StreamSource, Uuid> for StreamSourceService<R>
where
    R: Repository<StreamSource, Uuid, CreateRequest = StreamSourceCreateRequest, UpdateRequest = StreamSourceUpdateRequest, Query = StreamSourceQuery> 
        + BulkRepository<StreamSource, Uuid> + Send + Sync,
{
    type ValidationResult = StreamSourceValidationResult;

    async fn validate_create(&self, request: &Self::CreateRequest) -> AppResult<Self::ValidationResult> {
        self.validate_create_request(request).await
    }

    async fn validate_update(&self, id: Uuid, request: &Self::UpdateRequest) -> AppResult<Self::ValidationResult> {
        // Convert update request to create request for validation
        let create_request = StreamSourceCreateRequest {
            name: request.name.clone(),
            source_type: request.source_type.clone(),
            url: request.url.clone(),
            max_concurrent_streams: request.max_concurrent_streams,
            update_cron: request.update_cron.clone(),
            username: request.username.clone(),
            password: request.password.clone(),
            field_map: request.field_map.clone(),
        };

        let mut result = self.validate_create_request(&create_request).await?;

        // Additional validation for updates
        if !self.repository.exists(id).await.map_err(AppError::from)? {
            result.errors.push("Stream source does not exist".to_string());
            result.is_valid = false;
        }

        Ok(result)
    }

    async fn validate_delete(&self, id: Uuid) -> AppResult<Self::ValidationResult> {
        let mut result = StreamSourceValidationResult::success();

        // Check if source exists
        if !self.repository.exists(id).await.map_err(AppError::from)? {
            result.errors.push("Stream source does not exist".to_string());
            result.is_valid = false;
        }

        // TODO: Check for dependencies (channels, proxies, etc.)
        // This would add warnings or errors based on what depends on this source

        Ok(result)
    }
}

#[async_trait]
impl<R> BulkService<StreamSource, Uuid> for StreamSourceService<R>
where
    R: Repository<StreamSource, Uuid, CreateRequest = StreamSourceCreateRequest, UpdateRequest = StreamSourceUpdateRequest, Query = StreamSourceQuery> 
        + BulkRepository<StreamSource, Uuid> + Send + Sync,
{
    type BulkResult = ServiceBulkResponse<StreamSource>;

    async fn create_bulk(&self, requests: Vec<Self::CreateRequest>) -> AppResult<Self::BulkResult> {
        info!("Creating {} stream sources in bulk", requests.len());

        let mut successful = Vec::new();
        let mut failed = Vec::new();

        // Validate all requests first
        for (index, request) in requests.iter().enumerate() {
            match self.validate_create_request(request).await {
                Ok(validation) if !validation.is_valid => {
                    failed.push(BulkOperationError::new(index, validation.errors.join("; ")));
                }
                Err(e) => {
                    failed.push(BulkOperationError::new(index, e.to_string()));
                }
                _ => {} // Validation passed
            }
        }

        // If any validations failed, return early
        if !failed.is_empty() {
            warn!("Bulk create validation failed for {} items", failed.len());
            return Ok(ServiceBulkResponse::new(successful, failed));
        }

        // Store the count before moving
        let request_count = requests.len();

        // Create all sources using repository bulk operation
        match self.repository.create_bulk(requests).await {
            Ok(sources) => {
                info!("Successfully created {} stream sources in bulk", sources.len());
                successful = sources;
            }
            Err(e) => {
                error!("Bulk create failed: {}", e);
                // Mark all as failed if bulk operation fails
                for index in 0..request_count {
                    failed.push(BulkOperationError::new(index, e.to_string()));
                }
            }
        }

        Ok(ServiceBulkResponse::new(successful, failed))
    }

    async fn update_bulk(&self, updates: HashMap<Uuid, Self::UpdateRequest>) -> AppResult<Self::BulkResult> {
        info!("Updating {} stream sources in bulk", updates.len());

        let mut successful = Vec::new();
        let mut failed = Vec::new();

        // For simplicity, we'll process updates one by one
        // In a production system, you might want to optimize this further
        for (index, (id, request)) in updates.into_iter().enumerate() {
            match self.update(id, request).await {
                Ok(source) => successful.push(source),
                Err(e) => failed.push(BulkOperationError::new(index, e.to_string())),
            }
        }

        info!("Bulk update completed: {} successful, {} failed", successful.len(), failed.len());
        Ok(ServiceBulkResponse::new(successful, failed))
    }

    async fn delete_bulk(&self, ids: Vec<Uuid>) -> AppResult<Self::BulkResult> {
        info!("Deleting {} stream sources in bulk", ids.len());

        let successful = Vec::new();
        let mut failed = Vec::new();

        // Process deletions one by one
        for (index, id) in ids.into_iter().enumerate() {
            match self.delete(id).await {
                Ok(_) => {
                    // Create a placeholder StreamSource for the response
                    // In a real implementation, you might want to fetch the source before deletion
                    // or change the bulk response type to not require the full entity
                }
                Err(e) => failed.push(BulkOperationError::new(index, e.to_string())),
            }
        }

        info!("Bulk delete completed: {} successful, {} failed", successful.len(), failed.len());
        Ok(ServiceBulkResponse::new(successful, failed))
    }
}