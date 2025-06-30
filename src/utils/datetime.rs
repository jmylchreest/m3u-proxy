//! Centralized datetime handling utilities
//!
//! This module provides consistent datetime parsing, serialization, and formatting
//! across the entire application. It eliminates the duplication of datetime
//! handling logic found in multiple modules.
//!
//! # Features
//!
//! - Flexible parsing from multiple datetime formats
//! - Consistent UTC timezone handling
//! - Serde integration for JSON serialization
//! - Error types specific to datetime operations
//!
//! # Usage
//!
//! ```rust
//! use crate::utils::datetime::DateTimeParser;
//! use chrono::{DateTime, Utc};
//!
//! // Parse from various formats
//! let dt1 = DateTimeParser::parse_flexible("2023-01-01T12:00:00Z")?;
//! let dt2 = DateTimeParser::parse_flexible("2023-01-01 12:00:00")?;
//!
//! // Format for database storage
//! let formatted = DateTimeParser::format_for_storage(&dt1);
//! ```

use chrono::{DateTime, NaiveDateTime, Utc};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use thiserror::Error;

/// Errors that can occur during datetime operations
#[derive(Error, Debug)]
pub enum DateTimeError {
    /// Invalid datetime format provided
    #[error("Invalid datetime format: '{input}' - expected formats: RFC3339 (2023-01-01T12:00:00Z) or SQLite (2023-01-01 12:00:00)")]
    InvalidFormat { input: String },
    
    /// Timezone parsing failed
    #[error("Failed to parse timezone from: {input}")]
    TimezoneParseError { input: String },
    
    /// Date is outside valid range
    #[error("Date out of range: {input}")]
    OutOfRange { input: String },
}

/// Centralized datetime parsing and formatting utilities
pub struct DateTimeParser;

impl DateTimeParser {
    /// Parse datetime from various common formats used in the application
    ///
    /// Supports:
    /// - RFC3339 format with timezone: "2023-01-01T12:00:00Z"
    /// - RFC3339 format with offset: "2023-01-01T12:00:00+02:00"
    /// - SQLite format (assumes UTC): "2023-01-01 12:00:00"
    /// - ISO 8601 basic format: "20230101T120000Z"
    ///
    /// # Arguments
    ///
    /// * `datetime_str` - The datetime string to parse
    ///
    /// # Returns
    ///
    /// * `Ok(DateTime<Utc>)` - Successfully parsed datetime in UTC
    /// * `Err(DateTimeError)` - Parse error with details
    ///
    /// # Examples
    ///
    /// ```rust
    /// use crate::utils::datetime::DateTimeParser;
    ///
    /// // RFC3339 with timezone
    /// let dt1 = DateTimeParser::parse_flexible("2023-01-01T12:00:00Z")?;
    ///
    /// // SQLite format (assumes UTC)
    /// let dt2 = DateTimeParser::parse_flexible("2023-01-01 12:00:00")?;
    ///
    /// // Both result in UTC datetime
    /// assert_eq!(dt1.timezone(), dt2.timezone());
    /// ```
    pub fn parse_flexible(datetime_str: &str) -> Result<DateTime<Utc>, DateTimeError> {
        let trimmed = datetime_str.trim();
        
        // Try RFC3339 first (most common for APIs)
        if let Ok(dt) = DateTime::parse_from_rfc3339(trimmed) {
            return Ok(dt.with_timezone(&Utc));
        }
        
        // Try alternative RFC3339 formats
        if let Ok(dt) = DateTime::parse_from_str(trimmed, "%Y-%m-%dT%H:%M:%S%.fZ") {
            return Ok(dt.with_timezone(&Utc));
        }
        
        if let Ok(dt) = DateTime::parse_from_str(trimmed, "%Y-%m-%dT%H:%M:%S%z") {
            return Ok(dt.with_timezone(&Utc));
        }
        
        // Try naive datetime formats (assume UTC)
        let naive_formats = [
            "%Y-%m-%d %H:%M:%S",           // SQLite format
            "%Y-%m-%d %H:%M:%S%.f",        // SQLite with microseconds
            "%Y-%m-%dT%H:%M:%S",           // ISO without timezone
            "%Y-%m-%dT%H:%M:%S%.f",        // ISO with microseconds
            "%d/%m/%Y %H:%M:%S",           // European format
            "%m/%d/%Y %H:%M:%S",           // US format
            "%Y%m%dT%H%M%S",               // Basic ISO format
        ];
        
        for format in &naive_formats {
            if let Ok(naive_dt) = NaiveDateTime::parse_from_str(trimmed, format) {
                return Ok(DateTime::from_naive_utc_and_offset(naive_dt, Utc));
            }
        }
        
        Err(DateTimeError::InvalidFormat {
            input: datetime_str.to_string(),
        })
    }
    
    /// Parse datetime specifically for SQLite format
    ///
    /// This is a specialized version for SQLite's default datetime format.
    /// It's more efficient than the flexible parser when you know the format.
    ///
    /// # Arguments
    ///
    /// * `sqlite_str` - SQLite datetime string in format "YYYY-MM-DD HH:MM:SS"
    ///
    /// # Returns
    ///
    /// * `Ok(DateTime<Utc>)` - Successfully parsed datetime
    /// * `Err(DateTimeError)` - Parse error
    pub fn parse_sqlite(sqlite_str: &str) -> Result<DateTime<Utc>, DateTimeError> {
        NaiveDateTime::parse_from_str(sqlite_str.trim(), "%Y-%m-%d %H:%M:%S")
            .map(|naive| DateTime::from_naive_utc_and_offset(naive, Utc))
            .map_err(|_| DateTimeError::InvalidFormat {
                input: sqlite_str.to_string(),
            })
    }
    
    /// Format datetime for storage in SQLite
    ///
    /// Returns the datetime in SQLite's preferred format: "YYYY-MM-DD HH:MM:SS"
    ///
    /// # Arguments
    ///
    /// * `dt` - The datetime to format
    ///
    /// # Returns
    ///
    /// String formatted for SQLite storage
    pub fn format_for_storage(dt: &DateTime<Utc>) -> String {
        dt.format("%Y-%m-%d %H:%M:%S").to_string()
    }
    
    /// Format datetime for API responses (RFC3339)
    ///
    /// Returns the datetime in RFC3339 format suitable for JSON APIs
    ///
    /// # Arguments
    ///
    /// * `dt` - The datetime to format
    ///
    /// # Returns
    ///
    /// String formatted for API responses
    pub fn format_for_api(dt: &DateTime<Utc>) -> String {
        dt.to_rfc3339()
    }
    
    /// Get current UTC datetime
    ///
    /// Convenience method for getting the current time in UTC
    ///
    /// # Returns
    ///
    /// Current datetime in UTC
    pub fn now_utc() -> DateTime<Utc> {
        Utc::now()
    }
    
    /// Validate datetime string without parsing
    ///
    /// Checks if a datetime string is in a valid format without
    /// actually parsing it. Useful for validation in web forms.
    ///
    /// # Arguments
    ///
    /// * `datetime_str` - The datetime string to validate
    ///
    /// # Returns
    ///
    /// `true` if the format is valid, `false` otherwise
    pub fn is_valid_format(datetime_str: &str) -> bool {
        Self::parse_flexible(datetime_str).is_ok()
    }
}

/// Serde serialization helper for datetime fields
///
/// Use this function with `#[serde(serialize_with = "serialize_datetime")]`
/// to ensure consistent datetime serialization across the application.
pub fn serialize_datetime<S>(dt: &DateTime<Utc>, serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    DateTimeParser::format_for_api(dt).serialize(serializer)
}

/// Serde deserialization helper for datetime fields
///
/// Use this function with `#[serde(deserialize_with = "deserialize_datetime")]`
/// to ensure consistent datetime deserialization across the application.
pub fn deserialize_datetime<'de, D>(deserializer: D) -> Result<DateTime<Utc>, D::Error>
where
    D: Deserializer<'de>,
{
    let s = String::deserialize(deserializer)?;
    DateTimeParser::parse_flexible(&s).map_err(serde::de::Error::custom)
}

/// Serde helper for optional datetime fields
///
/// Use with `#[serde(serialize_with = "serialize_optional_datetime")]`
pub fn serialize_optional_datetime<S>(
    dt: &Option<DateTime<Utc>>,
    serializer: S,
) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    match dt {
        Some(dt) => serialize_datetime(dt, serializer),
        None => serializer.serialize_none(),
    }
}

/// Serde helper for deserializing optional datetime fields
///
/// Use with `#[serde(deserialize_with = "deserialize_optional_datetime")]`
pub fn deserialize_optional_datetime<'de, D>(
    deserializer: D,
) -> Result<Option<DateTime<Utc>>, D::Error>
where
    D: Deserializer<'de>,
{
    let opt = Option::<String>::deserialize(deserializer)?;
    match opt {
        Some(s) => DateTimeParser::parse_flexible(&s)
            .map(Some)
            .map_err(serde::de::Error::custom),
        None => Ok(None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    #[test]
    fn test_parse_rfc3339() {
        let dt = DateTimeParser::parse_flexible("2023-01-01T12:00:00Z").unwrap();
        assert_eq!(dt.year(), 2023);
        assert_eq!(dt.month(), 1);
        assert_eq!(dt.day(), 1);
        assert_eq!(dt.hour(), 12);
        assert_eq!(dt.minute(), 0);
        assert_eq!(dt.second(), 0);
    }

    #[test]
    fn test_parse_sqlite_format() {
        let dt = DateTimeParser::parse_flexible("2023-01-01 12:00:00").unwrap();
        assert_eq!(dt.year(), 2023);
        assert_eq!(dt.month(), 1);
        assert_eq!(dt.day(), 1);
        assert_eq!(dt.hour(), 12);
    }

    #[test]
    fn test_parse_with_timezone() {
        let dt = DateTimeParser::parse_flexible("2023-01-01T12:00:00+02:00").unwrap();
        // Should be converted to UTC
        assert_eq!(dt.hour(), 10); // 12 - 2 hours offset
    }

    #[test]
    fn test_invalid_format() {
        let result = DateTimeParser::parse_flexible("invalid-date");
        assert!(result.is_err());
        match result {
            Err(DateTimeError::InvalidFormat { input }) => {
                assert_eq!(input, "invalid-date");
            }
            _ => panic!("Expected InvalidFormat error"),
        }
    }

    #[test]
    fn test_format_for_storage() {
        let dt = Utc.with_ymd_and_hms(2023, 1, 1, 12, 0, 0).unwrap();
        let formatted = DateTimeParser::format_for_storage(&dt);
        assert_eq!(formatted, "2023-01-01 12:00:00");
    }

    #[test]
    fn test_format_for_api() {
        let dt = Utc.with_ymd_and_hms(2023, 1, 1, 12, 0, 0).unwrap();
        let formatted = DateTimeParser::format_for_api(&dt);
        assert_eq!(formatted, "2023-01-01T12:00:00+00:00");
    }

    #[test]
    fn test_is_valid_format() {
        assert!(DateTimeParser::is_valid_format("2023-01-01T12:00:00Z"));
        assert!(DateTimeParser::is_valid_format("2023-01-01 12:00:00"));
        assert!(!DateTimeParser::is_valid_format("invalid-date"));
    }

    #[test]
    fn test_parse_sqlite_specific() {
        let dt = DateTimeParser::parse_sqlite("2023-01-01 12:00:00").unwrap();
        assert_eq!(dt.year(), 2023);
        assert_eq!(dt.month(), 1);
    }
}