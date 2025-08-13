//! Service layer trait definitions
//!
//! This module defines the core traits that services implement to provide
//! consistent interfaces for business operations across the application.

use async_trait::async_trait;
use crate::errors::AppResult;

/// Core service trait for entity management
///
/// This trait defines the standard business operations that all entity services
/// should support. It focuses on business logic rather than simple CRUD operations.
///
/// # Type Parameters
///
/// * `T` - The entity type (e.g., StreamSource, Channel)
/// * `ID` - The identifier type (usually Uuid)
///
/// # Examples
///
/// ```rust
/// use m3u_proxy::services::traits::Service;
/// use m3u_proxy::models::StreamSource;
/// use m3u_proxy::errors::AppResult;
/// use uuid::Uuid;
///
/// async fn example<S: Service<StreamSource, Uuid>>(service: S) -> AppResult<()> {
///     let id = Uuid::new_v4();
///     let source = service.get_by_id(id).await?;
///     // Example would need actual update request data
///     Ok(())
/// }
/// ```
#[async_trait]
pub trait Service<T, ID: Send + 'static>: Send + Sync {
    /// Request type for creating new entities
    type CreateRequest;
    /// Request type for updating existing entities
    type UpdateRequest;
    /// Query type for filtering and searching
    type Query;
    /// List response type (may include metadata)
    type ListResponse;

    /// Get an entity by its ID
    ///
    /// This method includes business logic such as access control,
    /// data enrichment, and logging.
    ///
    /// # Arguments
    ///
    /// * `id` - The unique identifier of the entity
    ///
    /// # Returns
    ///
    /// * `Ok(Some(T))` - Entity found and accessible
    /// * `Ok(None)` - Entity not found
    /// * `Err(AppError)` - Business logic error or access denied
    async fn get_by_id(&self, id: ID) -> AppResult<Option<T>>;

    /// List entities based on query parameters
    ///
    /// This method applies business logic for filtering, sorting,
    /// and access control before delegating to the repository.
    ///
    /// # Arguments
    ///
    /// * `query` - Query parameters for filtering and pagination
    ///
    /// # Returns
    ///
    /// * `Ok(ListResponse)` - List of entities with metadata
    /// * `Err(AppError)` - Business logic error
    async fn list(&self, query: Self::Query) -> AppResult<Self::ListResponse>;

    /// Create a new entity
    ///
    /// This method validates the request, applies business rules,
    /// and may trigger side effects (notifications, logging, etc.).
    ///
    /// # Arguments
    ///
    /// * `request` - Data for creating the entity
    ///
    /// # Returns
    ///
    /// * `Ok(T)` - Created entity
    /// * `Err(AppError)` - Validation, business rule, or persistence error
    async fn create(&self, request: Self::CreateRequest) -> AppResult<T>;

    /// Update an existing entity
    ///
    /// This method validates the request, checks business rules,
    /// and may trigger side effects.
    ///
    /// # Arguments
    ///
    /// * `id` - The ID of the entity to update
    /// * `request` - Updated data for the entity
    ///
    /// # Returns
    ///
    /// * `Ok(T)` - Updated entity
    /// * `Err(AppError)` - Validation, business rule, or persistence error
    async fn update(&self, id: ID, request: Self::UpdateRequest) -> AppResult<T>;

    /// Delete an entity by ID
    ///
    /// This method checks business rules (e.g., cascading effects),
    /// performs cleanup, and may trigger side effects.
    ///
    /// # Arguments
    ///
    /// * `id` - The ID of the entity to delete
    ///
    /// # Returns
    ///
    /// * `Ok(())` - Entity deleted successfully
    /// * `Err(AppError)` - Business rule violation or persistence error
    async fn delete(&self, id: ID) -> AppResult<()>;

    /// Check if an entity exists and is accessible
    ///
    /// This method applies access control and business rules
    /// to determine if an entity exists from the caller's perspective.
    ///
    /// # Arguments
    ///
    /// * `id` - The ID to check
    ///
    /// # Returns
    ///
    /// * `Ok(true)` - Entity exists and is accessible
    /// * `Ok(false)` - Entity does not exist or is not accessible
    /// * `Err(AppError)` - Business logic error
    async fn exists(&self, id: ID) -> AppResult<bool> {
        match self.get_by_id(id).await? {
            Some(_) => Ok(true),
            None => Ok(false),
        }
    }
}

/// Extended service trait for validation operations
///
/// This trait provides methods for validating entities and business rules
/// without actually persisting changes.
#[async_trait]
pub trait ValidationService<T, ID: Send + 'static>: Service<T, ID> {
    /// Validation result type
    type ValidationResult;

    /// Validate a create request without persisting
    ///
    /// This method runs all validation logic and business rules
    /// without actually creating the entity.
    ///
    /// # Arguments
    ///
    /// * `request` - The create request to validate
    ///
    /// # Returns
    ///
    /// * `Ok(ValidationResult)` - Validation results
    /// * `Err(AppError)` - Validation error
    async fn validate_create(&self, request: &Self::CreateRequest) -> AppResult<Self::ValidationResult>;

    /// Validate an update request without persisting
    ///
    /// # Arguments
    ///
    /// * `id` - The ID of the entity to update
    /// * `request` - The update request to validate
    ///
    /// # Returns
    ///
    /// * `Ok(ValidationResult)` - Validation results
    /// * `Err(AppError)` - Validation error
    async fn validate_update(&self, id: ID, request: &Self::UpdateRequest) -> AppResult<Self::ValidationResult>;

    /// Validate a delete operation without persisting
    ///
    /// # Arguments
    ///
    /// * `id` - The ID of the entity to delete
    ///
    /// # Returns
    ///
    /// * `Ok(ValidationResult)` - Validation results (e.g., dependencies)
    /// * `Err(AppError)` - Validation error
    async fn validate_delete(&self, id: ID) -> AppResult<Self::ValidationResult>;
}

/// Service trait for bulk operations
///
/// This trait extends the basic Service with bulk operations that can be
/// more efficient and maintain consistency across multiple entities.
#[async_trait]
pub trait BulkService<T, ID: Send + 'static>: Service<T, ID> {
    /// Bulk operation result type
    type BulkResult;

    /// Create multiple entities in a single transaction
    ///
    /// This method validates all requests, applies business rules,
    /// and creates all entities atomically.
    ///
    /// # Arguments
    ///
    /// * `requests` - Vector of create requests
    ///
    /// # Returns
    ///
    /// * `Ok(BulkResult)` - Results of bulk operation
    /// * `Err(AppError)` - Validation or business rule error (all operations rolled back)
    async fn create_bulk(&self, requests: Vec<Self::CreateRequest>) -> AppResult<Self::BulkResult>;

    /// Update multiple entities in a single transaction
    ///
    /// # Arguments
    ///
    /// * `updates` - Map of ID to update request
    ///
    /// # Returns
    ///
    /// * `Ok(BulkResult)` - Results of bulk operation
    /// * `Err(AppError)` - Validation or business rule error (all operations rolled back)
    async fn update_bulk(&self, updates: std::collections::HashMap<ID, Self::UpdateRequest>) -> AppResult<Self::BulkResult>;

    /// Delete multiple entities in a single transaction
    ///
    /// # Arguments
    ///
    /// * `ids` - Vector of IDs to delete
    ///
    /// # Returns
    ///
    /// * `Ok(BulkResult)` - Results of bulk operation
    /// * `Err(AppError)` - Business rule violation (all operations rolled back)
    async fn delete_bulk(&self, ids: Vec<ID>) -> AppResult<Self::BulkResult>;
}

/// Service trait for entities that support import/export operations
///
/// This trait provides methods for importing data from external sources
/// and exporting data in various formats.
#[async_trait]
pub trait ImportExportService<T, ID: Send + 'static>: Service<T, ID> {
    /// Import data type
    type ImportData;
    /// Import result type
    type ImportResult;
    /// Export format type
    type ExportFormat;
    /// Export result type
    type ExportResult;

    /// Import entities from external data
    ///
    /// This method processes external data, validates it according to
    /// business rules, and creates/updates entities as needed.
    ///
    /// # Arguments
    ///
    /// * `data` - External data to import
    /// * `options` - Import options (merge strategy, validation level, etc.)
    ///
    /// # Returns
    ///
    /// * `Ok(ImportResult)` - Import results with statistics
    /// * `Err(AppError)` - Import validation or processing error
    async fn import(&self, data: Self::ImportData, options: ImportOptions) -> AppResult<Self::ImportResult>;

    /// Export entities in specified format
    ///
    /// This method retrieves entities based on query parameters
    /// and formats them for export.
    ///
    /// # Arguments
    ///
    /// * `query` - Query to determine which entities to export
    /// * `format` - Export format specification
    ///
    /// # Returns
    ///
    /// * `Ok(ExportResult)` - Exported data
    /// * `Err(AppError)` - Export processing error
    async fn export(&self, query: Self::Query, format: Self::ExportFormat) -> AppResult<Self::ExportResult>;
}

/// Options for import operations
#[derive(Debug, Clone)]
#[derive(Default)]
pub struct ImportOptions {
    /// Whether to update existing entities
    pub update_existing: bool,
    /// Whether to skip validation errors
    pub skip_validation_errors: bool,
    /// Maximum number of entities to process
    pub max_entities: Option<usize>,
    /// Whether to run in dry-run mode (validate only)
    pub dry_run: bool,
}


/// Standard service response for list operations
#[derive(Debug, Clone)]
pub struct ServiceListResponse<T> {
    /// The items for this response
    pub items: Vec<T>,
    /// Total number of items (for pagination)
    pub total_count: u64,
    /// Whether there are more items available
    pub has_more: bool,
    /// Next page token (if applicable)
    pub next_page_token: Option<String>,
}

impl<T> ServiceListResponse<T> {
    /// Create a new service list response
    pub fn new(items: Vec<T>, total_count: u64) -> Self {
        Self {
            has_more: items.len() as u64 >= total_count,
            items,
            total_count,
            next_page_token: None,
        }
    }

    /// Create a paginated response
    pub fn paginated(items: Vec<T>, total_count: u64, page: u32, limit: u32) -> Self {
        let has_more = (page * limit) < total_count as u32;
        let next_page_token = if has_more {
            Some((page + 1).to_string())
        } else {
            None
        };

        Self {
            items,
            total_count,
            has_more,
            next_page_token,
        }
    }
}

/// Standard service response for bulk operations
#[derive(Debug, Clone)]
pub struct ServiceBulkResponse<T> {
    /// Successfully processed items
    pub successful: Vec<T>,
    /// Failed items with error details
    pub failed: Vec<BulkOperationError>,
    /// Total number of items processed
    pub total_processed: usize,
    /// Total number of successful operations
    pub success_count: usize,
    /// Total number of failed operations
    pub failure_count: usize,
}

impl<T> ServiceBulkResponse<T> {
    /// Create a new bulk response
    pub fn new(successful: Vec<T>, failed: Vec<BulkOperationError>) -> Self {
        let success_count = successful.len();
        let failure_count = failed.len();
        let total_processed = success_count + failure_count;

        Self {
            successful,
            failed,
            total_processed,
            success_count,
            failure_count,
        }
    }
}

/// Error information for failed bulk operations
#[derive(Debug, Clone)]
pub struct BulkOperationError {
    /// Index of the failed item in the original request
    pub index: usize,
    /// Error that occurred
    pub error: String,
    /// Optional context about the failure
    pub context: Option<String>,
}

impl BulkOperationError {
    /// Create a new bulk operation error
    pub fn new<S: Into<String>>(index: usize, error: S) -> Self {
        Self {
            index,
            error: error.into(),
            context: None,
        }
    }

    /// Create a bulk operation error with context
    pub fn with_context<S: Into<String>, C: Into<String>>(index: usize, error: S, context: C) -> Self {
        Self {
            index,
            error: error.into(),
            context: Some(context.into()),
        }
    }
}