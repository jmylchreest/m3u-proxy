//! Integration tests for extended expressions examples from documentation
//!
//! This module tests all the examples provided in docs/extended_expressions.md
//! to ensure they parse correctly and produce the expected results.

use m3u_proxy::filter_parser::FilterParser;
use m3u_proxy::models::{
    ActionOperator, ActionValue, ConditionNode, ExtendedExpression, FilterOperator, LogicalOperator,
};

/// Test helper to create a parser with common field names
fn create_test_parser() -> FilterParser {
    let fields = vec![
        "channel_name".to_string(),
        "tvg_id".to_string(),
        "tvg_name".to_string(),
        "tvg_logo".to_string(),
        "tvg_shift".to_string(),
        "group_title".to_string(),
        "stream_url".to_string(),
        "channel_id".to_string(),
        "channel_logo".to_string(),
        "channel_group".to_string(),
        "language".to_string(),
    ];
    FilterParser::new().with_fields(fields)
}

/// Simplified test helper to validate action field and value
fn assert_action(actions: &[m3u_proxy::models::Action], index: usize, field: &str, value: &str) {
    assert!(
        index < actions.len(),
        "Action index {} out of bounds",
        index
    );
    assert_eq!(actions[index].field, field);
    match &actions[index].value {
        ActionValue::Literal(v) => assert_eq!(v, value),
        _ => panic!("Expected literal value for action {}", index),
    }
}

#[test]
fn test_simple_examples_from_docs() {
    let parser = create_test_parser();

    // Example 1: Basic group assignment
    let result = parser
        .parse_extended("channel_name contains \"sport\" SET group_title = \"Sports\"")
        .unwrap();
    match result {
        ExtendedExpression::ConditionWithActions { actions, .. } => {
            assert_eq!(actions.len(), 1);
            assert_action(&actions, 0, "group_title", "Sports");
        }
        _ => panic!("Expected condition with actions"),
    }

    // Example 2: Default logo assignment
    let result = parser
        .parse_extended("tvg_logo equals \"\" SET tvg_logo = \"https://example.com/default.png\"")
        .unwrap();
    match result {
        ExtendedExpression::ConditionWithActions { condition, actions } => {
            match &condition.root {
                ConditionNode::Condition {
                    field, operator, ..
                } => {
                    assert_eq!(field, "tvg_logo");
                    assert!(matches!(operator, FilterOperator::Equals));
                }
                _ => panic!("Expected simple condition"),
            }
            assert_eq!(actions.len(), 1);
            assert_action(&actions, 0, "tvg_logo", "https://example.com/default.png");
        }
        _ => panic!("Expected condition with actions"),
    }

    // Example 4: Multiple actions
    let result = parser.parse_extended("channel_name contains \"HD\" SET group_title = \"HD Channels\", tvg_logo = \"https://example.com/hd.png\"").unwrap();
    match result {
        ExtendedExpression::ConditionWithActions { actions, .. } => {
            assert_eq!(actions.len(), 2);
            assert_action(&actions, 0, "group_title", "HD Channels");
            assert_action(&actions, 1, "tvg_logo", "https://example.com/hd.png");
        }
        _ => panic!("Expected condition with multiple actions"),
    }
}

#[test]
fn test_intermediate_examples_from_docs() {
    let parser = create_test_parser();

    // Example 6: Country-based grouping
    let result = parser.parse_extended("tvg_id starts_with \"uk.\" SET group_title = \"UK Channels\", tvg_logo = \"https://logos.com/uk.png\"").unwrap();
    match result {
        ExtendedExpression::ConditionWithActions { condition, actions } => {
            match &condition.root {
                ConditionNode::Condition {
                    field, operator, ..
                } => {
                    assert_eq!(field, "tvg_id");
                    assert!(matches!(operator, FilterOperator::StartsWith));
                }
                _ => panic!("Expected simple condition"),
            }
            assert_eq!(actions.len(), 2);
            assert_action(&actions, 0, "group_title", "UK Channels");
            assert_action(&actions, 1, "tvg_logo", "https://logos.com/uk.png");
        }
        _ => panic!("Expected condition with actions"),
    }

    // Example 7: Multiple condition matching
    let result = parser.parse_extended("channel_name contains \"BBC\" AND channel_name contains \"HD\" SET group_title = \"BBC HD\"").unwrap();
    match result {
        ExtendedExpression::ConditionWithActions { condition, actions } => {
            match &condition.root {
                ConditionNode::Group { operator, children } => {
                    assert!(matches!(operator, LogicalOperator::And));
                    assert_eq!(children.len(), 2);
                }
                _ => panic!("Expected AND group"),
            }
            assert_eq!(actions.len(), 1);
            assert_action(&actions, 0, "group_title", "BBC HD");
        }
        _ => panic!("Expected condition with actions"),
    }

    // Example 8: Exclusion logic
    let result = parser.parse_extended("channel_name contains \"sport\" AND channel_name not_contains \"news\" SET group_title = \"Sports\"").unwrap();
    match result {
        ExtendedExpression::ConditionWithActions { condition, actions } => {
            match &condition.root {
                ConditionNode::Group { operator, children } => {
                    assert!(matches!(operator, LogicalOperator::And));
                    assert_eq!(children.len(), 2);

                    // Second condition should be not_contains
                    match &children[1] {
                        ConditionNode::Condition { operator, .. } => {
                            assert!(matches!(operator, FilterOperator::NotContains));
                        }
                        _ => panic!("Expected not_contains condition"),
                    }
                }
                _ => panic!("Expected AND group"),
            }
            assert_eq!(actions.len(), 1);
            assert_action(&actions, 0, "group_title", "Sports");
        }
        _ => panic!("Expected condition with actions"),
    }

    // Example 9: Regex pattern matching
    let result = parser
        .parse_extended("channel_name matches \"^([A-Z]+) .*\" SET tvg_id = \"$1\"")
        .unwrap();
    match result {
        ExtendedExpression::ConditionWithActions { condition, actions } => {
            match &condition.root {
                ConditionNode::Condition {
                    field, operator, ..
                } => {
                    assert_eq!(field, "channel_name");
                    assert!(matches!(operator, FilterOperator::Matches));
                }
                _ => panic!("Expected regex condition"),
            }
            assert_eq!(actions.len(), 1);
            assert_action(&actions, 0, "tvg_id", "$1");
        }
        _ => panic!("Expected condition with actions"),
    }

    // Example 10: Timeshift extraction
    let result = parser.parse_extended("channel_name matches \"(.+) \\\\+([0-9]+)\" SET channel_name = \"$1\", tvg_shift = \"$2\"").unwrap();
    match result {
        ExtendedExpression::ConditionWithActions { actions, .. } => {
            assert_eq!(actions.len(), 2);
            assert_action(&actions, 0, "channel_name", "$1");
            assert_action(&actions, 1, "tvg_shift", "$2");
        }
        _ => panic!("Expected condition with actions"),
    }
}

#[test]
fn test_complex_conditional_groups_from_docs() {
    let parser = create_test_parser();

    // Example 12: Advanced regional grouping
    let expr = "(tvg_id matches \"^(uk|gb)\\.\" SET group_title = \"United Kingdom\") OR (tvg_id matches \"^us\\.\" SET group_title = \"United States\") OR (tvg_id matches \"^ca\\.\" SET group_title = \"Canada\")";
    let result = parser.parse_extended(expr).unwrap();

    match result {
        ExtendedExpression::ConditionalActionGroups(groups) => {
            assert_eq!(groups.len(), 3);

            // Check logical operators
            assert_eq!(groups[0].logical_operator, Some(LogicalOperator::Or));
            assert_eq!(groups[1].logical_operator, Some(LogicalOperator::Or));
            assert_eq!(groups[2].logical_operator, None);

            // Check actions
            assert_eq!(groups[0].actions.len(), 1);
            assert_action(&groups[0].actions, 0, "group_title", "United Kingdom");

            assert_eq!(groups[1].actions.len(), 1);
            assert_action(&groups[1].actions, 0, "group_title", "United States");

            assert_eq!(groups[2].actions.len(), 1);
            assert_action(&groups[2].actions, 0, "group_title", "Canada");
        }
        _ => panic!("Expected conditional action groups"),
    }
}

#[test]
fn test_nested_conditional_logic_from_docs() {
    let parser = create_test_parser();

    // Example 16: Nested conditional logic
    let expr = "((channel_name matches \"^(BBC|ITV|Channel [45])\" AND tvg_id not_equals \"\") OR (channel_name matches \"Sky (Sports|Movies|News)\" AND group_title equals \"\")) SET group_title = \"Premium UK\"";
    let result = parser.parse_extended(expr).unwrap();

    match result {
        ExtendedExpression::ConditionWithActions { condition, actions } => {
            // Verify deeply nested structure
            match &condition.root {
                ConditionNode::Group { operator, children } => {
                    assert!(matches!(operator, LogicalOperator::Or)); // Top-level OR
                    assert_eq!(children.len(), 2);

                    // Both children should be AND groups
                    for child in children {
                        match child {
                            ConditionNode::Group { operator, children } => {
                                assert!(matches!(operator, LogicalOperator::And)); // AND groups
                                assert_eq!(children.len(), 2);
                            }
                            _ => panic!("Expected nested AND groups"),
                        }
                    }
                }
                _ => panic!("Expected top-level OR group"),
            }

            assert_eq!(actions.len(), 1);
            assert_action(&actions, 0, "group_title", "Premium UK");
        }
        _ => panic!("Expected condition with actions"),
    }
}

#[test]
fn test_multi_stage_processing_from_docs() {
    let parser = create_test_parser();

    // Example 17: Multi-stage processing
    let expr = "(channel_name matches \"^\\\\[([A-Z]{2,3})\\\\] (.+)\" SET tvg_id = \"$1\", channel_name = \"$2\") AND (tvg_id matches \"^(BBC|ITV|C4|C5)$\" SET group_title = \"UK Terrestrial\")";
    let result = parser.parse_extended(expr).unwrap();

    match result {
        ExtendedExpression::ConditionalActionGroups(groups) => {
            assert_eq!(groups.len(), 2);

            // First stage: Extract and set multiple fields
            assert_eq!(groups[0].actions.len(), 2);
            assert_action(&groups[0].actions, 0, "tvg_id", "$1");
            assert_action(&groups[0].actions, 1, "channel_name", "$2");

            // Second stage: Categorize based on extracted TVG ID
            assert_eq!(groups[1].actions.len(), 1);
            assert_action(&groups[1].actions, 0, "group_title", "UK Terrestrial");

            assert_eq!(groups[0].logical_operator, Some(LogicalOperator::And));
        }
        _ => panic!("Expected conditional action groups"),
    }
}

#[test]
fn test_dynamic_logo_assignment_from_docs() {
    let parser = create_test_parser();

    // Example 18: Dynamic logo assignment
    let expr = "(channel_name matches \"^(BBC One|BBC Two|BBC Three)\" SET tvg_logo = \"@logo:bbc-$1\") AND (channel_name matches \"^(ITV|ITV2|ITV3)\" SET tvg_logo = \"@logo:itv-$1\")";
    let result = parser.parse_extended(expr).unwrap();

    match result {
        ExtendedExpression::ConditionalActionGroups(groups) => {
            assert_eq!(groups.len(), 2);

            for (i, group) in groups.iter().enumerate() {
                assert_eq!(group.actions.len(), 1);
                assert_eq!(group.actions[0].field, "tvg_logo");
                match &group.actions[0].value {
                    ActionValue::Literal(v) => {
                        assert!(v.starts_with("@logo:"));
                        assert!(v.contains("$1"));
                        if i == 0 {
                            assert!(v.contains("bbc"));
                        } else {
                            assert!(v.contains("itv"));
                        }
                    }
                    _ => panic!("Expected logo reference with capture group"),
                }
            }
        }
        _ => panic!("Expected conditional action groups"),
    }
}

#[test]
fn test_comprehensive_normalization_from_docs() {
    let parser = create_test_parser();

    // Example 19: Comprehensive channel normalization (simplified for testing)
    let expr = "(channel_name matches \"^(.+?) *(?:\\\\|| - |: ).*(?:HD|FHD|4K|UHD)\" SET channel_name = \"$1\", group_title = \"High Definition\") AND (channel_name matches \"^(.+?) *\\\\+([0-9]+)h?$\" SET channel_name = \"$1\", tvg_shift = \"$2\")";
    let result = parser.parse_extended(expr).unwrap();

    match result {
        ExtendedExpression::ConditionalActionGroups(groups) => {
            assert_eq!(groups.len(), 2);

            // First stage: HD cleanup
            assert_eq!(groups[0].actions.len(), 2);
            assert_action(&groups[0].actions, 0, "channel_name", "$1");
            assert_action(&groups[0].actions, 1, "group_title", "High Definition");

            // Second stage: Timeshift extraction
            assert_eq!(groups[1].actions.len(), 2);
            assert_action(&groups[1].actions, 0, "channel_name", "$1");
            assert_action(&groups[1].actions, 1, "tvg_shift", "$2");
        }
        _ => panic!("Expected conditional action groups"),
    }
}

#[test]
fn test_real_world_provider_examples_from_docs() {
    let parser = create_test_parser();

    // Sky UK Channel Normalization
    let expr = "(channel_name matches \"Sky (Sports|Movies|News) (.+)\" SET channel_name = \"Sky $1 $2\", group_title = \"Sky\") AND (channel_name starts_with \"Sky Sports\" SET group_title = \"Sky Sports\")";
    let result = parser.parse_extended(expr).unwrap();

    match result {
        ExtendedExpression::ConditionalActionGroups(groups) => {
            assert_eq!(groups.len(), 2);

            // First group has multiple actions with capture groups
            assert_eq!(groups[0].actions.len(), 2);
            assert_action(&groups[0].actions, 0, "channel_name", "Sky $1 $2");
            assert_action(&groups[0].actions, 1, "group_title", "Sky");

            // Second group is more specific categorization
            assert_eq!(groups[1].actions.len(), 1);
            assert_action(&groups[1].actions, 0, "group_title", "Sky Sports");
        }
        _ => panic!("Expected conditional action groups"),
    }

    // US Cable Provider with timeshift
    let expr = "(channel_name matches \"^([A-Z]+) East \\\\+([0-9]+)\" SET channel_name = \"$1 East\", tvg_shift = \"$2\") AND (channel_name contains \"ESPN\" SET group_title = \"ESPN Family\")";
    let result = parser.parse_extended(expr).unwrap();

    match result {
        ExtendedExpression::ConditionalActionGroups(groups) => {
            assert_eq!(groups.len(), 2);

            // Timeshift extraction with capture groups
            assert_eq!(groups[0].actions.len(), 2);
            assert_action(&groups[0].actions, 0, "channel_name", "$1 East");
            assert_action(&groups[0].actions, 1, "tvg_shift", "$2");

            // ESPN grouping
            assert_eq!(groups[1].actions.len(), 1);
            assert_action(&groups[1].actions, 0, "group_title", "ESPN Family");
        }
        _ => panic!("Expected conditional action groups"),
    }
}

#[test]
fn test_quality_based_grouping_from_docs() {
    let parser = create_test_parser();

    // Example 14: Quality-based grouping with logo assignment
    let expr = "(channel_name matches \".*\\\\b(4K|UHD)\\\\b.*\" SET group_title = \"4K Ultra HD\", tvg_logo = \"@logo:4k-badge\") AND (channel_name matches \".*\\\\b(HD|1080p)\\\\b.*\" AND channel_name not_matches \".*\\\\b(4K|UHD)\\\\b.*\" SET group_title = \"HD Channels\")";
    let result = parser.parse_extended(expr).unwrap();

    match result {
        ExtendedExpression::ConditionalActionGroups(groups) => {
            assert_eq!(groups.len(), 2);

            // 4K group with logo
            assert_eq!(groups[0].actions.len(), 2);
            assert_action(&groups[0].actions, 0, "group_title", "4K Ultra HD");
            assert_action(&groups[0].actions, 1, "tvg_logo", "@logo:4k-badge");

            // HD group with exclusion logic
            match &groups[1].conditions.root {
                ConditionNode::Group { operator, children } => {
                    assert!(matches!(operator, LogicalOperator::And));
                    assert_eq!(children.len(), 2);
                }
                _ => panic!("Expected AND group for HD conditions"),
            }
        }
        _ => panic!("Expected conditional action groups"),
    }
}

#[test]
fn test_validation_and_error_cases() {
    let parser = create_test_parser();

    // Test successful parsing and validation
    let expr = "channel_name contains \"test\" SET group_title = \"Test\"";
    let result = parser.parse_extended(expr).unwrap();
    assert!(parser.validate_extended(&result).is_ok());

    // Test malformed expressions that should fail parsing
    let malformed_expressions = vec![
        "channel_name contains \"test\" group_title = \"Test\"", // Missing SET
        "(channel_name contains \"test\" SET group_title = \"Test\"", // Unbalanced parens
        "channel_name matches \"[unclosed\" SET group_title = \"Test\"", // Invalid regex
        "SET group_title = \"Test\"",                            // Missing condition
        "channel_name contains \"test\" SET",                    // Missing action
        "(channel_name contains \"test\") AND",                  // Incomplete logical expression
    ];

    for expr in malformed_expressions {
        assert!(
            parser.parse_extended(expr).is_err(),
            "Expression should have failed to parse: {}",
            expr
        );
    }

    // Test expressions that parse but might fail validation
    let expr_with_empty_value = "channel_name contains \"test\" SET group_title = \"\"";
    let result = parser.parse_extended(expr_with_empty_value).unwrap();
    assert!(parser.validate_extended(&result).is_ok()); // Empty values are allowed

    // Test complex valid expression
    let complex_expr = "(channel_name matches \"^(BBC|ITV)\" SET group_title = \"UK TV\") OR (tvg_id starts_with \"us.\" SET group_title = \"US TV\")";
    let result = parser.parse_extended(complex_expr).unwrap();
    assert!(parser.validate_extended(&result).is_ok());
}

#[test]
fn test_performance_with_complex_expressions() {
    let parser = create_test_parser();

    // Test parsing performance with very complex expression
    let complex_expr = "((channel_name matches \"^(BBC|ITV|Channel [45]|Sky [A-Z]+)\" AND tvg_id not_equals \"\" AND group_title not_contains \"test\") OR (channel_name matches \"(ESPN|Fox Sports|NBC Sports)\" AND language equals \"en\" AND stream_url contains \"https\")) SET group_title = \"Premium\", tvg_logo = \"@logo:premium\", tvg_shift = \"0\"";

    let start = std::time::Instant::now();
    let result = parser.parse_extended(complex_expr).unwrap();
    let parse_duration = start.elapsed();

    // Should parse in reasonable time (less than 1ms for most expressions)
    assert!(
        parse_duration.as_millis() < 100,
        "Parsing took too long: {:?}",
        parse_duration
    );

    // Validate the result
    let start = std::time::Instant::now();
    assert!(parser.validate_extended(&result).is_ok());
    let validate_duration = start.elapsed();

    assert!(
        validate_duration.as_millis() < 50,
        "Validation took too long: {:?}",
        validate_duration
    );

    // Verify structure
    match result {
        ExtendedExpression::ConditionWithActions { condition, actions } => {
            // Should have complex nested structure
            match &condition.root {
                ConditionNode::Group { .. } => {
                    // Complex nested conditions are expected
                }
                _ => panic!("Expected complex group condition"),
            }
            assert_eq!(actions.len(), 3); // Three actions as specified
        }
        _ => panic!("Expected condition with actions"),
    }
}

#[test]
fn test_edge_cases_and_corner_cases() {
    let parser = create_test_parser();

    // Very long field values
    let long_value = "a".repeat(1000);
    let expr = format!(
        "channel_name equals \"{}\" SET group_title = \"Test\"",
        long_value
    );
    assert!(parser.parse_extended(&expr).is_ok());

    // Unicode characters
    let unicode_expr = "channel_name contains \"中文\" SET group_title = \"Chinese\"";
    assert!(parser.parse_extended(unicode_expr).is_ok());

    // Special regex characters
    let special_chars_expr =
        "channel_name matches \".*\\\\[\\\\]\\\\(\\\\)\\\\{\\\\}\" SET group_title = \"Special\"";
    assert!(parser.parse_extended(special_chars_expr).is_ok());

    // Deeply nested parentheses (within reason)
    let nested_expr = "(((channel_name contains \"test\"))) SET group_title = \"Nested\"";
    assert!(parser.parse_extended(nested_expr).is_ok());

    // Many capture groups
    let many_captures_expr =
        "channel_name matches \"([A-Z])([A-Z])([A-Z])([A-Z])\" SET tvg_id = \"$1$2$3$4\"";
    assert!(parser.parse_extended(many_captures_expr).is_ok());

    // Very long action list
    let many_actions_expr = "channel_name contains \"test\" SET group_title = \"Test\", tvg_logo = \"logo\", tvg_shift = \"0\", tvg_name = \"name\", channel_name = \"new_name\"";
    let result = parser.parse_extended(many_actions_expr).unwrap();
    match result {
        ExtendedExpression::ConditionWithActions { actions, .. } => {
            assert_eq!(actions.len(), 5);
        }
        _ => panic!("Expected multiple actions"),
    }
}
