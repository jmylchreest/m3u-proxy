use chrono::{DateTime, Utc, NaiveDateTime};
use sqlx;

/// Parse datetime from SQLite format or RFC3339 format
pub fn parse_datetime(datetime_str: &str) -> Result<DateTime<Utc>, sqlx::Error> {
    // Try parsing as RFC3339 first (with timezone info)
    if let Ok(dt) = DateTime::parse_from_rfc3339(datetime_str) {
        return Ok(dt.with_timezone(&Utc));
    }
    
    // Try parsing as naive datetime and assume UTC
    if let Ok(naive_dt) = NaiveDateTime::parse_from_str(datetime_str, "%Y-%m-%d %H:%M:%S") {
        return Ok(DateTime::from_naive_utc_and_offset(naive_dt, Utc));
    }
    
    // If both fail, return a decode error
    Err(sqlx::Error::Decode(
        format!("Unable to parse datetime: {}", datetime_str).into()
    ))
}