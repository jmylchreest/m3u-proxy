//! Input validation utilities
//!
//! This module provides common validation functions used throughout the
//! application for validating user input, URLs, and other data.
//!
//! # Features
//!
//! - URL validation with protocol checking
//! - String validation (length, format, etc.)
//! - UUID validation
//! - Custom validation rule composition
//!
//! # Usage
//!
//! ```rust
//! use m3u_proxy::utils::validation::{Validator, ValidationRule};
//! use std::collections::HashMap;
//!
//! let validator = Validator::new()
//!     .rule(ValidationRule::required("name"))
//!     .rule(ValidationRule::url("url"))
//!     .rule(ValidationRule::max_length("description", 500));
//!
//! let mut data = HashMap::new();
//! data.insert("name".to_string(), Some("Test".to_string()));
//! data.insert("url".to_string(), Some("http://example.com".to_string()));
//! data.insert("description".to_string(), Some("Short description".to_string()));
//! let result = validator.validate(&data);
//! ```

use std::collections::HashMap;
use thiserror::Error;
use url::Url;
use uuid::Uuid;

/// Validation errors that can occur during input validation
#[derive(Error, Debug, Clone, PartialEq)]
pub enum ValidationError {
    /// Field is required but missing or empty
    #[error("Field '{field}' is required")]
    Required { field: String },

    /// Field value is too short
    #[error("Field '{field}' must be at least {min} characters long (got {actual})")]
    TooShort {
        field: String,
        min: usize,
        actual: usize,
    },

    /// Field value is too long
    #[error("Field '{field}' must be at most {max} characters long (got {actual})")]
    TooLong {
        field: String,
        max: usize,
        actual: usize,
    },

    /// Field value is not a valid URL
    #[error("Field '{field}' must be a valid URL")]
    InvalidUrl { field: String },

    /// Field value is not a valid UUID
    #[error("Field '{field}' must be a valid UUID")]
    InvalidUuid { field: String },

    /// Field value doesn't match required pattern
    #[error("Field '{field}' has invalid format")]
    InvalidFormat { field: String },

    /// Field value is not in allowed list
    #[error("Field '{field}' must be one of: {allowed:?}")]
    InvalidChoice { field: String, allowed: Vec<String> },

    /// Custom validation error
    #[error("Field '{field}': {message}")]
    Custom { field: String, message: String },
}

/// Result type for validation operations
pub type ValidationResult<T> = Result<T, Vec<ValidationError>>;

/// A validation rule that can be applied to a field
#[derive(Debug, Clone)]
pub enum ValidationRule {
    /// Field is required (not None, not empty string)
    Required(String),

    /// Field must be at least min characters long
    MinLength { field: String, min: usize },

    /// Field must be at most max characters long
    MaxLength { field: String, max: usize },

    /// Field must be a valid URL
    Url(String),

    /// Field must be a valid UUID
    Uuid(String),

    /// Field must match a regex pattern
    Regex { field: String, pattern: String },

    /// Field must be one of the allowed values
    Choice { field: String, allowed: Vec<String> },

    /// Custom validation function
    Custom {
        field: String,
        validator: fn(&str) -> Result<(), String>,
    },
}

impl ValidationRule {
    /// Create a required field rule
    pub fn required<S: Into<String>>(field: S) -> Self {
        Self::Required(field.into())
    }

    /// Create a minimum length rule
    pub fn min_length<S: Into<String>>(field: S, min: usize) -> Self {
        Self::MinLength {
            field: field.into(),
            min,
        }
    }

    /// Create a maximum length rule
    pub fn max_length<S: Into<String>>(field: S, max: usize) -> Self {
        Self::MaxLength {
            field: field.into(),
            max,
        }
    }

    /// Create a URL validation rule
    pub fn url<S: Into<String>>(field: S) -> Self {
        Self::Url(field.into())
    }

    /// Create a UUID validation rule
    pub fn uuid<S: Into<String>>(field: S) -> Self {
        Self::Uuid(field.into())
    }

    /// Create a regex pattern rule
    pub fn regex<S: Into<String>>(field: S, pattern: S) -> Self {
        Self::Regex {
            field: field.into(),
            pattern: pattern.into(),
        }
    }

    /// Create a choice validation rule
    pub fn choice<S: Into<String>>(field: S, allowed: Vec<S>) -> Self {
        Self::Choice {
            field: field.into(),
            allowed: allowed.into_iter().map(|s| s.into()).collect(),
        }
    }

    /// Create a custom validation rule
    pub fn custom<S: Into<String>>(field: S, validator: fn(&str) -> Result<(), String>) -> Self {
        Self::Custom {
            field: field.into(),
            validator,
        }
    }
}

/// Validator that applies multiple validation rules
#[derive(Debug)]
pub struct Validator {
    rules: Vec<ValidationRule>,
}

impl Validator {
    /// Create a new empty validator
    pub fn new() -> Self {
        Self { rules: Vec::new() }
    }

    /// Add a validation rule
    pub fn rule(mut self, rule: ValidationRule) -> Self {
        self.rules.push(rule);
        self
    }

    /// Add multiple validation rules
    pub fn rules(mut self, rules: Vec<ValidationRule>) -> Self {
        self.rules.extend(rules);
        self
    }

    /// Validate a map of field values
    pub fn validate(&self, data: &HashMap<String, Option<String>>) -> ValidationResult<()> {
        let mut errors = Vec::new();

        for rule in &self.rules {
            match self.apply_rule(rule, data) {
                Ok(_) => continue,
                Err(error) => errors.push(error),
            }
        }

        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors)
        }
    }

    /// Apply a single validation rule
    fn apply_rule(
        &self,
        rule: &ValidationRule,
        data: &HashMap<String, Option<String>>,
    ) -> Result<(), ValidationError> {
        match rule {
            ValidationRule::Required(field) => {
                let value = data.get(field).and_then(|v| v.as_ref());
                match value {
                    Some(v) if !v.trim().is_empty() => Ok(()),
                    _ => Err(ValidationError::Required {
                        field: field.clone(),
                    }),
                }
            }

            ValidationRule::MinLength { field, min } => {
                if let Some(Some(value)) = data.get(field) {
                    if value.len() < *min {
                        Err(ValidationError::TooShort {
                            field: field.clone(),
                            min: *min,
                            actual: value.len(),
                        })
                    } else {
                        Ok(())
                    }
                } else {
                    Ok(()) // Skip validation if field is missing
                }
            }

            ValidationRule::MaxLength { field, max } => {
                if let Some(Some(value)) = data.get(field) {
                    if value.len() > *max {
                        Err(ValidationError::TooLong {
                            field: field.clone(),
                            max: *max,
                            actual: value.len(),
                        })
                    } else {
                        Ok(())
                    }
                } else {
                    Ok(()) // Skip validation if field is missing
                }
            }

            ValidationRule::Url(field) => {
                if let Some(Some(value)) = data.get(field) {
                    if Url::parse(value).is_ok() {
                        Ok(())
                    } else {
                        Err(ValidationError::InvalidUrl {
                            field: field.clone(),
                        })
                    }
                } else {
                    Ok(()) // Skip validation if field is missing
                }
            }

            ValidationRule::Uuid(field) => {
                if let Some(Some(value)) = data.get(field) {
                    if Uuid::parse_str(value).is_ok() {
                        Ok(())
                    } else {
                        Err(ValidationError::InvalidUuid {
                            field: field.clone(),
                        })
                    }
                } else {
                    Ok(()) // Skip validation if field is missing
                }
            }

            ValidationRule::Regex { field, pattern } => {
                if let Some(Some(value)) = data.get(field) {
                    if let Ok(regex) = regex::Regex::new(pattern) {
                        if regex.is_match(value) {
                            Ok(())
                        } else {
                            Err(ValidationError::InvalidFormat {
                                field: field.clone(),
                            })
                        }
                    } else {
                        Err(ValidationError::Custom {
                            field: field.clone(),
                            message: "Invalid regex pattern".to_string(),
                        })
                    }
                } else {
                    Ok(()) // Skip validation if field is missing
                }
            }

            ValidationRule::Choice { field, allowed } => {
                if let Some(Some(value)) = data.get(field) {
                    if allowed.contains(value) {
                        Ok(())
                    } else {
                        Err(ValidationError::InvalidChoice {
                            field: field.clone(),
                            allowed: allowed.clone(),
                        })
                    }
                } else {
                    Ok(()) // Skip validation if field is missing
                }
            }

            ValidationRule::Custom { field, validator } => {
                if let Some(Some(value)) = data.get(field) {
                    match validator(value) {
                        Ok(()) => Ok(()),
                        Err(message) => Err(ValidationError::Custom {
                            field: field.clone(),
                            message,
                        }),
                    }
                } else {
                    Ok(()) // Skip validation if field is missing
                }
            }
        }
    }
}

impl Default for Validator {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_required_validation() {
        let mut data = HashMap::new();
        data.insert("name".to_string(), Some("test".to_string()));
        data.insert("empty".to_string(), Some("".to_string()));
        data.insert("missing".to_string(), None);

        let validator = Validator::new()
            .rule(ValidationRule::required("name"))
            .rule(ValidationRule::required("empty"))
            .rule(ValidationRule::required("missing"));

        let result = validator.validate(&data);
        assert!(result.is_err());

        let errors = result.unwrap_err();
        assert_eq!(errors.len(), 2); // empty and missing should fail
    }

    #[test]
    fn test_length_validation() {
        let mut data = HashMap::new();
        data.insert("short".to_string(), Some("hi".to_string()));
        data.insert(
            "long".to_string(),
            Some("this is a very long string".to_string()),
        );

        let validator = Validator::new()
            .rule(ValidationRule::min_length("short", 5))
            .rule(ValidationRule::max_length("long", 10));

        let result = validator.validate(&data);
        assert!(result.is_err());

        let errors = result.unwrap_err();
        assert_eq!(errors.len(), 2);
    }

    #[test]
    fn test_url_validation() {
        let mut data = HashMap::new();
        data.insert(
            "valid_url".to_string(),
            Some("https://example.com".to_string()),
        );
        data.insert("invalid_url".to_string(), Some("not-a-url".to_string()));

        let validator = Validator::new()
            .rule(ValidationRule::url("valid_url"))
            .rule(ValidationRule::url("invalid_url"));

        let result = validator.validate(&data);
        assert!(result.is_err());

        let errors = result.unwrap_err();
        assert_eq!(errors.len(), 1); // Only invalid_url should fail
    }

    #[test]
    fn test_uuid_validation() {
        let mut data = HashMap::new();
        data.insert(
            "valid_uuid".to_string(),
            Some("12345678-1234-5678-9abc-123456789abc".to_string()),
        );
        data.insert("invalid_uuid".to_string(), Some("not-a-uuid".to_string()));

        let validator = Validator::new()
            .rule(ValidationRule::uuid("valid_uuid"))
            .rule(ValidationRule::uuid("invalid_uuid"));

        let result = validator.validate(&data);
        assert!(result.is_err());

        let errors = result.unwrap_err();
        assert_eq!(errors.len(), 1); // Only invalid_uuid should fail
    }

    #[test]
    fn test_choice_validation() {
        let mut data = HashMap::new();
        data.insert("valid_choice".to_string(), Some("option1".to_string()));
        data.insert("invalid_choice".to_string(), Some("option3".to_string()));

        let validator = Validator::new()
            .rule(ValidationRule::choice(
                "valid_choice",
                vec!["option1", "option2"],
            ))
            .rule(ValidationRule::choice(
                "invalid_choice",
                vec!["option1", "option2"],
            ));

        let result = validator.validate(&data);
        assert!(result.is_err());

        let errors = result.unwrap_err();
        assert_eq!(errors.len(), 1); // Only invalid_choice should fail
    }
}
