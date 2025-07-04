//! SQLite utility functions for consistent data handling
//!
//! This module provides utilities for handling SQLite-specific data types
//! and conversions, particularly for datetime and UUID fields.

use chrono::{DateTime, Utc};
use sqlx::Row;
use uuid::Uuid;

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
        parse_sqlite_datetime(&datetime_str)
            .unwrap_or_else(|_| panic!("Failed to parse datetime from column '{}': '{}'", column, datetime_str))
    }
    
    fn get_datetime_opt(&self, column: &str) -> Option<DateTime<Utc>> {
        let datetime_str: Option<String> = self.get(column);
        datetime_str.and_then(|s| parse_sqlite_datetime(&s).ok())
    }
    
    fn get_uuid(&self, column: &str) -> Result<Uuid, uuid::Error> {
        let uuid_str: String = self.get(column);
        Uuid::parse_str(&uuid_str)
    }
}

/// Parse a datetime string from SQLite in various formats
fn parse_sqlite_datetime(datetime_str: &str) -> Result<DateTime<Utc>, String> {
    // Try RFC3339 format first (our preferred format)
    if let Ok(dt) = DateTime::parse_from_rfc3339(datetime_str) {
        return Ok(dt.with_timezone(&Utc));
    }
    
    // Try SQLite's default format: "YYYY-MM-DD HH:MM:SS"
    if let Ok(naive_dt) = chrono::NaiveDateTime::parse_from_str(datetime_str, "%Y-%m-%d %H:%M:%S") {
        return Ok(DateTime::from_naive_utc_and_offset(naive_dt, Utc));
    }
    
    // Try ISO 8601 with 'T' separator: "YYYY-MM-DDTHH:MM:SS"
    if let Ok(naive_dt) = chrono::NaiveDateTime::parse_from_str(datetime_str, "%Y-%m-%dT%H:%M:%S") {
        return Ok(DateTime::from_naive_utc_and_offset(naive_dt, Utc));
    }
    
    Err(format!("Unable to parse datetime: '{}'", datetime_str))
}

/// Format a DateTime<Utc> for SQLite storage (RFC3339 format)
pub fn format_datetime_for_sqlite(dt: DateTime<Utc>) -> String {
    dt.to_rfc3339()
}