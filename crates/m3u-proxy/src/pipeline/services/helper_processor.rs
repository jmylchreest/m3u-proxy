//! Helper processor system for resolving placeholders in data mapping
//!
//! This module provides an extensible system for processing helper placeholders
//! like @logo:UUID, @time:now(), etc. in data mapping results.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use sea_orm::{DatabaseConnection, EntityTrait, QueryFilter, ColumnTrait, PaginatorTrait};
use std::time::Duration;
use tracing::{debug, error, trace, warn};
use uuid::Uuid;

use crate::pipeline::engines::rule_processor::{FieldModification, ModificationType};
use crate::entities::{logo_assets, prelude::*};

/// Error types for helper processing
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum HelperProcessorError {
    ResolutionFailed(String),
    ServiceUnavailable(String),
    DatabaseError(String),
    CriticalDatabaseError(String), // Unrecoverable database errors that should halt the pipeline
}

impl std::fmt::Display for HelperProcessorError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            HelperProcessorError::ResolutionFailed(msg) => write!(f, "Resolution failed: {msg}"),
            HelperProcessorError::ServiceUnavailable(msg) => write!(f, "Service unavailable: {msg}"),
            HelperProcessorError::DatabaseError(msg) => write!(f, "Database error: {msg}"),
            HelperProcessorError::CriticalDatabaseError(msg) => write!(f, "Critical database error: {msg}"),
        }
    }
}

impl std::error::Error for HelperProcessorError {}

/// Trait for processing specific helper types
#[async_trait]
pub trait HelperProcessor: Send + Sync {
    /// Returns the helper prefix this processor handles (e.g., "@logo:", "@time:")
    fn get_supported_prefix(&self) -> &'static str;
    
    /// Process a field value containing the helper
    /// Returns None if the helper should result in field removal (null value)
    async fn resolve_helper(&self, field_value: &str) -> Result<Option<String>, HelperProcessorError>;
    
    /// Quick check if a field contains this helper (default implementation)
    fn contains_helper(&self, field_value: &str) -> bool {
        field_value.contains(self.get_supported_prefix())
    }
}

/// Trait for logo database operations - allows for easy mocking in tests
#[async_trait]
pub trait LogoRepository: Send + Sync {
    async fn logo_uuid_exists(&self, uuid: &Uuid) -> Result<bool, HelperProcessorError>;
}

/// Production implementation using SeaORM
pub struct SeaOrmLogoRepository {
    db_connection: DatabaseConnection,
}

impl SeaOrmLogoRepository {
    pub fn new(db_connection: DatabaseConnection) -> Self {
        Self { db_connection }
    }
}

#[async_trait]
impl LogoRepository for SeaOrmLogoRepository {
    async fn logo_uuid_exists(&self, uuid: &Uuid) -> Result<bool, HelperProcessorError> {
        const MAX_RETRIES: u32 = 3;
        const BASE_DELAY_MS: u64 = 100;
        
        for attempt in 1..=MAX_RETRIES {
            match LogoAssets::find()
                .filter(logo_assets::Column::Id.eq(*uuid))
                .filter(logo_assets::Column::AssetType.eq("uploaded"))
                .count(&self.db_connection)
                .await
            {
                Ok(result) => {
                    trace!("Logo UUID {} lookup succeeded on attempt {}", uuid, attempt);
                    return Ok(result > 0);
                }
                Err(e) => {
                    if attempt == MAX_RETRIES {
                        error!("Logo UUID lookup failed after {} attempts for UUID {}: {}", 
                            MAX_RETRIES, uuid, e);
                        return Err(HelperProcessorError::CriticalDatabaseError(
                            format!("Failed to check logo UUID {uuid} after {MAX_RETRIES} retries: {e}")
                        ));
                    } else {
                        let delay_ms = BASE_DELAY_MS * (2_u64.pow(attempt - 1));
                        debug!("Logo UUID lookup attempt {} failed for UUID {}: {}. Retrying in {}ms...", 
                            attempt, uuid, e, delay_ms);
                        tokio::time::sleep(Duration::from_millis(delay_ms)).await;
                    }
                }
            }
        }
        
        unreachable!("Loop should always return within MAX_RETRIES attempts")
    }
}

/// Mock implementation for testing
#[cfg(test)]
pub struct MockLogoRepository {
    pub exists_responses: std::collections::HashMap<Uuid, bool>,
    pub should_error: bool,
}

#[cfg(test)]
impl Default for MockLogoRepository {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
impl MockLogoRepository {
    pub fn new() -> Self {
        Self {
            exists_responses: std::collections::HashMap::new(),
            should_error: false,
        }
    }
    
    pub fn with_uuid_exists(mut self, uuid: Uuid, exists: bool) -> Self {
        self.exists_responses.insert(uuid, exists);
        self
    }
    
    pub fn with_error(mut self) -> Self {
        self.should_error = true;
        self
    }
}

#[cfg(test)]
#[async_trait]
impl LogoRepository for MockLogoRepository {
    async fn logo_uuid_exists(&self, uuid: &Uuid) -> Result<bool, HelperProcessorError> {
        if self.should_error {
            return Err(HelperProcessorError::CriticalDatabaseError(
                "Mock database error".to_string()
            ));
        }
        
        Ok(self.exists_responses.get(uuid).copied().unwrap_or(false))
    }
}

/// Field that can be processed by helpers
#[derive(Debug, Clone)]
pub struct HelperField {
    pub name: String,
    pub value: Option<String>,
}

/// Trait for records that can be checked for helpers
pub trait HelperDetectable {
    fn contains_any_helpers(&self, processors: &[Box<dyn HelperProcessor>]) -> bool;
}

/// Trait for records that can have their helper fields processed
pub trait HelperProcessable: HelperDetectable {
    fn get_helper_processable_fields(&self) -> Vec<HelperField>;
    fn update_from_helper_fields(&mut self, fields: Vec<HelperField>);
}

/// Main helper post-processor service
pub struct HelperPostProcessor {
    processors: Vec<Box<dyn HelperProcessor>>,
}

impl Default for HelperPostProcessor {
    fn default() -> Self {
        Self::new()
    }
}

impl HelperPostProcessor {
    pub fn new() -> Self {
        Self {
            processors: Vec::new(),
        }
    }
    
    pub fn register_processor(mut self, processor: Box<dyn HelperProcessor>) -> Self {
        self.processors.push(processor);
        self
    }
    
    /// Check if any field in a record contains helpers
    pub fn record_needs_processing<T>(&self, record: &T) -> bool 
    where 
        T: HelperDetectable,
    {
        record.contains_any_helpers(&self.processors)
    }
    
    /// Process all helper fields in a record
    pub async fn process_record<T>(&self, mut record: T) -> Result<(T, Vec<FieldModification>), HelperProcessorError>
    where 
        T: HelperProcessable,
    {
        let mut modifications = Vec::new();
        let mut fields = record.get_helper_processable_fields();
        
        for field in &mut fields {
            if let Some(field_value) = field.value.clone() {
                let old_value = field_value.clone();
                let mut new_value = None;
                let mut processed = false;
                
                // Try each processor until one matches
                for processor in &self.processors {
                    if processor.contains_helper(&field_value) {
                        trace!("Processing field '{}' with {} processor", field.name, processor.get_supported_prefix());
                        
                        match processor.resolve_helper(&field_value).await {
                            Ok(result_value) => {
                                new_value = result_value;
                                processed = true;
                                break; // First matching processor wins
                            }
                            Err(e) => {
                                warn!("Helper processing failed for field '{}': {}", field.name, e);
                                // Continue with original value on error
                                break;
                            }
                        }
                    }
                }
                
                if processed {
                    // Update field value and track modification
                    field.value = new_value.clone();
                    
                    // Track modification if value actually changed
                    if field.value != Some(old_value.clone()) {
                        modifications.push(FieldModification {
                            field_name: field.name.clone(),
                            old_value: Some(old_value),
                            new_value,
                            modification_type: ModificationType::Set,
                        });
                    }
                } else {
                    trace!("No helper processor matched field '{}' value '{}'", field.name, field_value);
                }
            }
        }
        
        // Update the record with processed fields
        record.update_from_helper_fields(fields);
        
        Ok((record, modifications))
    }
}

/// Logo helper processor that validates UUIDs against the database (generic version)
pub struct LogoHelperProcessor<R: LogoRepository> {
    logo_repo: R,
    base_url: String,
}

impl<R: LogoRepository> LogoHelperProcessor<R> {
    pub fn new(logo_repo: R, base_url: String) -> Self {
        Self {
            logo_repo,
            base_url,
        }
    }
}

#[async_trait]
impl<R: LogoRepository + Send + Sync> HelperProcessor for LogoHelperProcessor<R> {
    fn get_supported_prefix(&self) -> &'static str {
        "@logo:"
    }
    
    async fn resolve_helper(&self, field_value: &str) -> Result<Option<String>, HelperProcessorError> {
        if let Some(uuid_str) = field_value.strip_prefix("@logo:") {
            match uuid_str.parse::<Uuid>() {
                Ok(uuid) => {
                    // Check if the UUID exists in the database
                    match self.logo_repo.logo_uuid_exists(&uuid).await {
                        Ok(exists) => {
                            if exists {
                                // Generate uploaded logo URL
                                let url = format!(
                                    "{}/api/v1/logos/{}", 
                                    self.base_url.trim_end_matches('/'), 
                                    uuid
                                );
                                trace!("Resolved @logo:{} to {}", uuid, url);
                                Ok(Some(url))
                            } else {
                                // UUID doesn't exist in database, remove the field (return None)
                                warn!("Logo UUID {} not found in database, removing field", uuid);
                                Ok(None)
                            }
                        }
                        Err(e @ HelperProcessorError::CriticalDatabaseError(_)) => {
                            // Propagate critical database errors - these should halt the pipeline
                            Err(e)
                        }
                        Err(e) => {
                            // Other database errors - shouldn't happen with current implementation but handle gracefully
                            warn!("Unexpected error checking logo UUID {}: {}", uuid, e);
                            Ok(None) // Remove field on error
                        }
                    }
                }
                Err(_) => {
                    // Malformed UUID - remove the field instead of erroring
                    warn!("Malformed UUID in @logo: helper '{}', removing field", uuid_str);
                    Ok(None)
                }
            }
        } else {
            // Not a @logo: helper, return as-is
            Ok(Some(field_value.to_string()))
        }
    }
}

// Convenience constructor for production use
impl LogoHelperProcessor<SeaOrmLogoRepository> {
    pub fn from_connection(db_connection: DatabaseConnection, base_url: String) -> Self {
        let repo = SeaOrmLogoRepository::new(db_connection);
        Self::new(repo, base_url)
    }
}

/// Time helper processor using existing time resolution logic
pub struct TimeHelperProcessor;

#[async_trait]
impl HelperProcessor for TimeHelperProcessor {
    fn get_supported_prefix(&self) -> &'static str {
        "@time:"
    }
    
    async fn resolve_helper(&self, field_value: &str) -> Result<Option<String>, HelperProcessorError> {
        // Use existing time resolution from utils::time
        match crate::utils::time::resolve_time_functions(field_value) {
            Ok(resolved) => {
                trace!("Resolved time helper: {} -> {}", field_value, resolved);
                Ok(Some(resolved))
            }
            Err(e) => Err(HelperProcessorError::ResolutionFailed(
                format!("Time helper resolution failed: {e}")
            ))
        }
    }
    
    fn contains_helper(&self, field_value: &str) -> bool {
        // More sophisticated detection for complex @time: patterns
        field_value.contains("@time:")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[tokio::test]
    async fn test_logo_helper_processor_valid_uuid_not_found() {
        let uuid = Uuid::parse_str("550e8400-e29b-41d4-a716-446655440000").unwrap();
        
        // Create mock repository that returns false for this UUID
        let mock_repo = MockLogoRepository::new()
            .with_uuid_exists(uuid, false);
            
        let processor = LogoHelperProcessor::new(mock_repo, "https://example.com".to_string());
        
        // Valid UUID format but doesn't exist in database
        let result = processor.resolve_helper("@logo:550e8400-e29b-41d4-a716-446655440000").await;
        assert!(result.is_ok(), "Expected Ok result but got error: {:?}", result.as_ref().err());
        assert_eq!(result.unwrap(), None); // Field should be removed
    }
    
    #[tokio::test]
    async fn test_logo_helper_processor_valid_uuid_found() {
        let uuid = Uuid::parse_str("550e8400-e29b-41d4-a716-446655440000").unwrap();
        
        // Create mock repository that returns true for this UUID
        let mock_repo = MockLogoRepository::new()
            .with_uuid_exists(uuid, true);
            
        let processor = LogoHelperProcessor::new(mock_repo, "https://example.com".to_string());
        
        let test_uuid = "550e8400-e29b-41d4-a716-446655440000";
        
        // Valid UUID that exists in database
        let result = processor.resolve_helper(&format!("@logo:{test_uuid}")).await;
        assert!(result.is_ok(), "Expected Ok result but got error: {:?}", result.as_ref().err());
        assert_eq!(
            result.unwrap(), 
            Some(format!("https://example.com/api/v1/logos/{test_uuid}"))
        );
    }
    
    #[tokio::test]
    async fn test_logo_helper_processor_database_error() {
        // Create mock repository that returns an error
        let mock_repo = MockLogoRepository::new()
            .with_error();
            
        let processor = LogoHelperProcessor::new(mock_repo, "https://example.com".to_string());
        
        // Valid UUID format but database error occurs
        let result = processor.resolve_helper("@logo:550e8400-e29b-41d4-a716-446655440000").await;
        assert!(result.is_err());
        match result.unwrap_err() {
            HelperProcessorError::CriticalDatabaseError(_) => {
                // Expected error type
            }
            _ => panic!("Expected CriticalDatabaseError"),
        }
    }
    
    #[tokio::test]
    async fn test_logo_helper_processor_malformed_uuid() {
        let mock_repo = MockLogoRepository::new();
        let processor = LogoHelperProcessor::new(mock_repo, "https://example.com".to_string());
        
        // Malformed UUID should remove field, not error
        let result = processor.resolve_helper("@logo:not-a-uuid").await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), None); // Field should be removed
    }
    
    #[tokio::test]
    async fn test_logo_helper_processor_non_logo_field() {
        let mock_repo = MockLogoRepository::new();
        let processor = LogoHelperProcessor::new(mock_repo, "https://example.com".to_string());
        
        // Non-logo field should pass through unchanged
        let result = processor.resolve_helper("some other value").await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), Some("some other value".to_string()));
    }
    
    #[tokio::test]
    async fn test_base_url_trimming() {
        let uuid = Uuid::parse_str("550e8400-e29b-41d4-a716-446655440000").unwrap();
        let mock_repo = MockLogoRepository::new()
            .with_uuid_exists(uuid, true);
            
        // Test with trailing slash in base URL
        let processor = LogoHelperProcessor::new(mock_repo, "https://example.com/".to_string());
        
        let result = processor.resolve_helper("@logo:550e8400-e29b-41d4-a716-446655440000").await;
        assert!(result.is_ok());
        assert_eq!(
            result.unwrap(), 
            Some("https://example.com/api/v1/logos/550e8400-e29b-41d4-a716-446655440000".to_string())
        );
    }
    
    #[tokio::test] 
    async fn test_time_helper_processor() {
        let processor = TimeHelperProcessor;
        
        let result = processor.resolve_helper("@time:now()").await;
        assert!(result.is_ok());
        let resolved = result.unwrap();
        assert!(resolved.is_some());
        
        // Should be a numeric timestamp
        let timestamp = resolved.unwrap();
        assert!(timestamp.parse::<i64>().is_ok());
    }
    
    #[tokio::test]
    async fn test_helper_post_processor() {
        let _processor = HelperPostProcessor::new()
            .register_processor(Box::new(TimeHelperProcessor));
            
        // Test with mock record - would need to implement traits on test struct
    }
}