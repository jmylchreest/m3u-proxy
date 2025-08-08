//! SQLite utility functions for consistent data handling
//!
//! This module provides utilities for handling SQLite-specific data types
//! and conversions, particularly for datetime and UUID fields.

use chrono::{DateTime, Utc};
use sqlx::Row;
use uuid::Uuid;
use crate::utils::datetime::DateTimeParser;

/// Trait for extracting typed values from SQLite rows consistently
pub trait SqliteRowExt {
    /// Get a datetime field from a SQLite row as DateTime<Utc>
    /// SQLx automatically handles parsing from TEXT to DateTime<Utc>
    fn get_datetime(&self, column: &str) -> DateTime<Utc>;
    
    /// Get an optional datetime field from a SQLite row
    fn get_datetime_opt(&self, column: &str) -> Option<DateTime<Utc>>;
    
    /// Get a UUID field from a SQLite row (stored as TEXT)
    fn get_uuid(&self, column: &str) -> Result<Uuid, uuid::Error>;
}

impl SqliteRowExt for sqlx::sqlite::SqliteRow {
    fn get_datetime(&self, column: &str) -> DateTime<Utc> {
        let datetime_str: String = self.get(column);
        DateTimeParser::parse_flexible(&datetime_str)
            .unwrap_or_else(|_| panic!("Failed to parse datetime from column '{}': '{}'", column, datetime_str))
    }
    
    fn get_datetime_opt(&self, column: &str) -> Option<DateTime<Utc>> {
        let datetime_str: Option<String> = self.get(column);
        datetime_str.and_then(|s| DateTimeParser::parse_flexible(&s).ok())
    }
    
    fn get_uuid(&self, column: &str) -> Result<Uuid, uuid::Error> {
        let uuid_str: String = self.get(column);
        Uuid::parse_str(&uuid_str)
    }
}

