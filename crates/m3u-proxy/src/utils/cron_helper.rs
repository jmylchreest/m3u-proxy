//! Cron utility functions for calculating next scheduled times
//!
//! This module provides utilities for working with cron expressions
//! to calculate next scheduled update times.

use chrono::{DateTime, Utc};
use cron::Schedule;
use std::str::FromStr;

/// Calculate the next scheduled update time from a cron expression
/// 
/// # Arguments
/// * `cron_expression` - A valid cron expression string
/// 
/// # Returns
/// * `Some(DateTime<Utc>)` - The next scheduled time if the expression is valid
/// * `None` - If the cron expression is invalid or has no future schedules
pub fn calculate_next_scheduled_time(cron_expression: &str) -> Option<DateTime<Utc>> {
    match Schedule::from_str(cron_expression) {
        Ok(schedule) => {
            schedule.upcoming(Utc).next()
        }
        Err(_) => {
            // Invalid cron expression
            None
        }
    }
}

/// Calculate the next scheduled update time from a cron expression with validation
/// 
/// # Arguments
/// * `cron_expression` - A valid cron expression string
/// 
/// # Returns
/// * `Ok(Some(DateTime<Utc>))` - The next scheduled time
/// * `Ok(None)` - Valid cron but no future schedules
/// * `Err(String)` - Invalid cron expression with error message
pub fn calculate_next_scheduled_time_validated(cron_expression: &str) -> Result<Option<DateTime<Utc>>, String> {
    match Schedule::from_str(cron_expression) {
        Ok(schedule) => {
            Ok(schedule.upcoming(Utc).next())
        }
        Err(e) => {
            Err(format!("Invalid cron expression '{cron_expression}': {e}"))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    #[test]
    fn test_valid_cron_expression() {
        // Test every 6 hours
        let result = calculate_next_scheduled_time("0 0 0 */6 * * * *");
        assert!(result.is_some());
        
        // Verify the result is in the future
        let next_time = result.unwrap();
        assert!(next_time > Utc::now());
    }

    #[test]
    fn test_invalid_cron_expression() {
        let result = calculate_next_scheduled_time("invalid");
        assert!(result.is_none());
    }

    #[test]
    fn test_validated_cron_expression() {
        // Valid cron
        let result = calculate_next_scheduled_time_validated("0 0 */12 * * *");
        assert!(result.is_ok());
        assert!(result.unwrap().is_some());
        
        // Invalid cron
        let result = calculate_next_scheduled_time_validated("invalid");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Invalid cron expression"));
    }
}