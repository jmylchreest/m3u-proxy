// DEPRECATED: This module has been renamed to expression_parser
// This file provides backward compatibility - new code should use expression_parser directly

// Re-export everything from the new expression_parser module
pub use crate::expression_parser::*;

// Note: FilterParser is now an alias for ExpressionParser in expression_parser.rs
// The type alias ensures all existing code continues to work unchanged