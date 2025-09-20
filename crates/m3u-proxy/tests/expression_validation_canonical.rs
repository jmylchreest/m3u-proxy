/*!
 * Integration-style canonical expression tests.
 *
 * These tests simulate the unified validation endpoint behavior by constructing
 * parsers over one or more ExpressionDomain values, reproducing the alias +
 * canonical field filtering logic used in the API layer. They assert that:
 *
 * 1. Aliases (American spellings) are accepted as valid input fields.
 * 2. Canonicalization rewrites aliases to their canonical (British) forms.
 * 3. Multi-domain (union) validation accepts fields from all included domains.
 * 4. Canonicalization is idempotent when the input is already canonical.
 * 5. Actions are preserved while their field identifiers are canonicalized.
 *
 * NOTE: These tests deliberately do not spin up the HTTP layer; instead they
 * mirror the endpoint’s domain/alias preparation logic. This keeps them fast
 * while still providing end‑to‑end confidence in parser + alias + canonicalization
 * behavior.
 */

#![cfg(test)]

use m3u_proxy::expression::ExpressionDomain;
use m3u_proxy::field_registry::{FieldRegistry, SourceKind, StageKind};

/// Build a parser mimicking the unified endpoint's logic for the given domains:
/// - Union canonical field names across all domains.
/// - Filter alias map so only aliases whose canonical target is included survive.
fn build_union_parser(
    domains: &[ExpressionDomain],
) -> m3u_proxy::expression_parser::ExpressionParser {
    let registry = FieldRegistry::global();

    fn domain_pair(d: ExpressionDomain) -> (SourceKind, StageKind) {
        match d {
            ExpressionDomain::StreamFilter => (SourceKind::Stream, StageKind::Filtering),
            ExpressionDomain::EpgFilter => (SourceKind::Epg, StageKind::Filtering),
            ExpressionDomain::StreamDataMapping | ExpressionDomain::StreamRule => {
                (SourceKind::Stream, StageKind::DataMapping)
            }
            ExpressionDomain::EpgDataMapping | ExpressionDomain::EpgRule => {
                (SourceKind::Epg, StageKind::DataMapping)
            }
        }
    }

    use std::collections::{HashMap, HashSet};

    // Collect canonical fields (union).
    let mut canon_union: HashSet<String> = HashSet::new();
    for d in domains {
        let (sk, st) = domain_pair(*d);
        for name in registry.field_names_for(sk, st) {
            canon_union.insert(name.to_string());
        }
    }

    let mut canonical_vec: Vec<String> = canon_union.into_iter().collect();
    canonical_vec.sort();

    let allowed: HashSet<&str> = canonical_vec.iter().map(|s| s.as_str()).collect();
    let filtered_aliases: HashMap<String, String> = registry
        .alias_map()
        .into_iter()
        .filter(|(_alias, canon)| allowed.contains(canon.as_str()))
        .collect();

    m3u_proxy::expression_parser::ExpressionParser::new()
        .with_fields(canonical_vec)
        .with_aliases(filtered_aliases)
}

/// Test: Single EPG mapping domain – alias program_category must validate and canonicalize.
#[test]
fn test_canonical_expression_epg_mapping_single_alias() {
    let parser = build_union_parser(&[ExpressionDomain::EpgDataMapping]);
    let raw = r#"program_category contains "Drama""#;
    let result = parser.validate(raw);
    assert!(
        result.is_valid,
        "Validation should succeed for alias field; errors: {:?}",
        result.errors
    );
    let canonical = parser.canonicalize_expression_lossy(raw);
    assert!(
        canonical.contains("programme_category"),
        "Canonical form expected to contain programme_category, got: {canonical}"
    );
    assert!(
        !canonical.contains("program_category"),
        "Alias should have been replaced in canonical form: {canonical}"
    );
}

/// Test: Multi-domain union (stream + epg filtering) with alias + canonical mix.
#[test]
fn test_canonical_expression_union_stream_epg_filter() {
    let parser = build_union_parser(&[ExpressionDomain::StreamFilter, ExpressionDomain::EpgFilter]);

    // channel_name (stream), program_title (alias), programme_description (canonical)
    let raw = r#"channel_name contains "News" AND program_title contains "Update" AND programme_description contains "World""#;
    let result = parser.validate(raw);
    assert!(
        result.is_valid,
        "Union domain validation failed; errors: {:?}",
        result.errors
    );
    let canonical = parser.canonicalize_expression_lossy(raw);
    assert!(
        canonical.contains("programme_title"),
        "Alias program_title should canonicalize to programme_title, got: {canonical}"
    );
    assert!(
        !canonical.contains("program_title "),
        "Residual alias program_title present in canonical: {canonical}"
    );
}

/// Test: Union stream+epg data mapping with actions – ensure action LHS field canonicalizes.
#[test]
fn test_canonical_expression_union_mapping_with_action() {
    let parser = build_union_parser(&[
        ExpressionDomain::StreamDataMapping,
        ExpressionDomain::EpgDataMapping,
    ]);

    // title (alias -> programme_title), program_category (alias), action sets channel_name (canonical)
    let raw = r#"title contains "Match" AND program_category equals "Sports" SET channel_name ?= "Sports Channel""#;
    let result = parser.validate(raw);
    assert!(
        result.is_valid,
        "Expected valid expression with action; errors: {:?}",
        result.errors
    );
    let canonical = parser.canonicalize_expression_lossy(raw);
    assert!(
        canonical.contains("programme_title"),
        "Expected programme_title in canonical: {canonical}"
    );
    assert!(
        canonical.contains("programme_category"),
        "Expected programme_category in canonical: {canonical}"
    );
    assert!(
        !canonical.contains("program_category"),
        "Alias program_category should be replaced: {canonical}"
    );
    assert!(
        canonical.contains("SET channel_name ?="),
        "Action syntax / channel_name field lost or altered unexpectedly: {canonical}"
    );
}

/// Test: Canonical input remains stable (idempotent canonicalization).
#[test]
fn test_canonical_expression_idempotent() {
    let parser = build_union_parser(&[ExpressionDomain::EpgFilter]);
    let raw = r#"programme_title contains "Report" AND programme_category equals "News""#;
    let canonical_once = parser.canonicalize_expression_lossy(raw);
    let canonical_twice = parser.canonicalize_expression_lossy(&canonical_once);
    assert_eq!(
        canonical_once, canonical_twice,
        "Canonicalization should be idempotent"
    );
    assert!(
        !canonical_once.contains("program_title"),
        "Alias program_title appeared unexpectedly in canonical form"
    );
}

/// Test: Unknown field in union still generates structured error.
#[test]
fn test_canonical_expression_unknown_field_error() {
    let parser = build_union_parser(&[ExpressionDomain::StreamFilter, ExpressionDomain::EpgFilter]);
    let raw = r#"unknown_field equals "X""#;
    let result = parser.validate(raw);
    assert!(!result.is_valid, "Should fail validation for unknown_field");
    assert!(
        result
            .errors
            .iter()
            .any(|e| e.error_type == "unknown_field"),
        "Expected unknown_field error, got: {:?}",
        result.errors
    );
}
