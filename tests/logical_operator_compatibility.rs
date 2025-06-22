//! Test to verify backward compatibility for logical operators (and/or vs all/any)

use m3u_proxy::models::LogicalOperator;
use serde_json;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_logical_operator_deserialization_old_format() {
        // Test that old format (and/or) still deserializes correctly
        let and_json = r#""and""#;
        let or_json = r#""or""#;

        let and_op: LogicalOperator = serde_json::from_str(and_json).expect("Failed to deserialize 'and'");
        let or_op: LogicalOperator = serde_json::from_str(or_json).expect("Failed to deserialize 'or'");

        assert!(matches!(and_op, LogicalOperator::And));
        assert!(matches!(or_op, LogicalOperator::Or));
    }

    #[test]
    fn test_logical_operator_deserialization_new_format() {
        // Test that new format (all/any) deserializes correctly
        let all_json = r#""all""#;
        let any_json = r#""any""#;

        let all_op: LogicalOperator = serde_json::from_str(all_json).expect("Failed to deserialize 'all'");
        let any_op: LogicalOperator = serde_json::from_str(any_json).expect("Failed to deserialize 'any'");

        assert!(matches!(all_op, LogicalOperator::All));
        assert!(matches!(any_op, LogicalOperator::Any));
    }

    #[test]
    fn test_logical_operator_behavior_compatibility() {
        // Test that old and new formats behave the same way
        let and_op = LogicalOperator::And;
        let all_op = LogicalOperator::All;
        let or_op = LogicalOperator::Or;
        let any_op = LogicalOperator::Any;

        // Test is_and_like
        assert!(and_op.is_and_like());
        assert!(all_op.is_and_like());
        assert!(!or_op.is_and_like());
        assert!(!any_op.is_and_like());

        // Test is_or_like  
        assert!(!and_op.is_or_like());
        assert!(!all_op.is_or_like());
        assert!(or_op.is_or_like());
        assert!(any_op.is_or_like());
    }

    #[test]
    fn test_logical_operator_serialization() {
        // Test that all variants serialize correctly
        let and_op = LogicalOperator::And;
        let or_op = LogicalOperator::Or;
        let all_op = LogicalOperator::All;
        let any_op = LogicalOperator::Any;

        assert_eq!(serde_json::to_string(&and_op).unwrap(), r#""and""#);
        assert_eq!(serde_json::to_string(&or_op).unwrap(), r#""or""#);
        assert_eq!(serde_json::to_string(&all_op).unwrap(), r#""all""#);
        assert_eq!(serde_json::to_string(&any_op).unwrap(), r#""any""#);
    }

    #[test]
    fn test_normalize_function() {
        // Test normalize function converts old to new format
        assert!(matches!(LogicalOperator::And.normalize(), LogicalOperator::All));
        assert!(matches!(LogicalOperator::Or.normalize(), LogicalOperator::Any));
        assert!(matches!(LogicalOperator::All.normalize(), LogicalOperator::All));
        assert!(matches!(LogicalOperator::Any.normalize(), LogicalOperator::Any));
    }
}