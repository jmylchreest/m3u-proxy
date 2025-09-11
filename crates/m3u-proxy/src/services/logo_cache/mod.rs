//! Ultra-compact logo cache service with memory-optimized indexing
//!
//! This module provides an efficient logo caching system designed for handling
//! 100k+ logos with minimal memory usage through:
//! 
//! - Hash-based string matching instead of string storage
//! - Smart 12-bit dimension encoding with variable precision
//! - LRU caching for search result strings only
//! - Maintenance jobs with age and size-based cleanup

pub mod dimension_encoder;
pub mod entry;
pub mod metadata;
pub mod service;

pub use dimension_encoder::DimensionEncoder;
pub use entry::{LogoCacheEntry, LogoCacheQuery};
pub use metadata::CachedLogoMetadata;
pub use service::{LogoCacheService, LogoCacheSearchResult, MaintenanceStats, LogoCacheStats};