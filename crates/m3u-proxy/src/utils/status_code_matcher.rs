//! HTTP Status Code Matching Utilities
//!
//! Provides utilities for matching HTTP status codes against patterns,
//! supporting wildcard patterns like "2xx", "4xx", etc.

use reqwest::StatusCode;

/// Check if a status code matches any of the acceptable status code patterns
pub fn is_status_acceptable(status: &StatusCode, acceptable_codes: &[String]) -> bool {
    let status_code = status.as_u16();
    
    for pattern in acceptable_codes {
        if matches_pattern(status_code, pattern) {
            return true;
        }
    }
    
    false
}

/// Check if a status code matches a specific pattern
fn matches_pattern(status_code: u16, pattern: &str) -> bool {
    if pattern.ends_with("xx") {
        // Handle wildcard patterns like "2xx", "4xx"
        if pattern.len() == 3 {
            let prefix = &pattern[0..1];
            if let Ok(prefix_digit) = prefix.parse::<u16>() {
                let status_prefix = status_code / 100;
                return status_prefix == prefix_digit;
            }
        }
    } else {
        // Handle exact matches like "404", "200"
        if let Ok(exact_code) = pattern.parse::<u16>() {
            return status_code == exact_code;
        }
    }
    
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_exact_status_codes() {
        let acceptable = vec!["404".to_string(), "200".to_string()];
        
        assert!(is_status_acceptable(&StatusCode::NOT_FOUND, &acceptable));
        assert!(is_status_acceptable(&StatusCode::OK, &acceptable));
        assert!(!is_status_acceptable(&StatusCode::INTERNAL_SERVER_ERROR, &acceptable));
    }
    
    #[test]
    fn test_wildcard_status_codes() {
        let acceptable = vec!["2xx".to_string(), "404".to_string()];
        
        // 2xx should match all 200-299
        assert!(is_status_acceptable(&StatusCode::OK, &acceptable));
        assert!(is_status_acceptable(&StatusCode::CREATED, &acceptable));
        assert!(is_status_acceptable(&StatusCode::NO_CONTENT, &acceptable));
        
        // 404 should match exactly
        assert!(is_status_acceptable(&StatusCode::NOT_FOUND, &acceptable));
        
        // Other codes should not match
        assert!(!is_status_acceptable(&StatusCode::INTERNAL_SERVER_ERROR, &acceptable));
        assert!(!is_status_acceptable(&StatusCode::BAD_REQUEST, &acceptable));
    }
    
    #[test]
    fn test_mixed_patterns() {
        let acceptable = vec!["2xx".to_string(), "3xx".to_string(), "404".to_string()];
        
        assert!(is_status_acceptable(&StatusCode::OK, &acceptable));
        assert!(is_status_acceptable(&StatusCode::MOVED_PERMANENTLY, &acceptable));
        assert!(is_status_acceptable(&StatusCode::NOT_FOUND, &acceptable));
        assert!(!is_status_acceptable(&StatusCode::BAD_REQUEST, &acceptable));
        assert!(!is_status_acceptable(&StatusCode::INTERNAL_SERVER_ERROR, &acceptable));
    }
    
    #[test]
    fn test_empty_acceptable_codes() {
        let acceptable: Vec<String> = vec![];
        
        assert!(!is_status_acceptable(&StatusCode::OK, &acceptable));
        assert!(!is_status_acceptable(&StatusCode::NOT_FOUND, &acceptable));
    }
}