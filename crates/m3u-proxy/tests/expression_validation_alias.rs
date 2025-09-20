#![cfg(test)]

use m3u_proxy::expression::{ExpressionDomain, build_parser_for};
use m3u_proxy::field_registry::{FieldRegistry, SourceKind, StageKind};

/// Helper to build a parser that mimics the unified validation endpoint's
/// union-of-domains behavior (collect canonical field names for each domain
/// then filter aliases so they only map to canonical names present).
fn build_union_parser(
    domains: &[ExpressionDomain],
) -> m3u_proxy::expression_parser::ExpressionParser {
    let registry = FieldRegistry::global();

    // Map ExpressionDomain -> (SourceKind, StageKind)
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

    // Collect canonical fields (union)
    let mut canonical: HashSet<String> = HashSet::new();
    for d in domains {
        let (sk, st) = domain_pair(*d);
        for name in registry.field_names_for(sk, st) {
            canonical.insert(name.to_string());
        }
    }

    let mut canonical_vec: Vec<String> = canonical.into_iter().collect();
    canonical_vec.sort();

    // Filter alias map to only those whose canonical target is in the union
    let allowed_set: HashSet<&str> = canonical_vec.iter().map(|s| s.as_str()).collect();
    let filtered_aliases: HashMap<String, String> = registry
        .alias_map()
        .into_iter()
        .filter(|(_alias, canon)| allowed_set.contains(canon.as_str()))
        .collect();

    m3u_proxy::expression_parser::ExpressionParser::new()
        .with_fields(canonical_vec)
        .with_aliases(filtered_aliases)
}

/// Test 1: Alias resolution for program_category -> programme_category using
/// the EPG Data Mapping domain.
/// This mirrors the unified endpoint behavior for a single domain.
#[test]
fn test_alias_resolution_program_category_epg_mapping() {
    // Build canonical parser for EPG Data Mapping domain (through existing builder).
    let parser = build_parser_for(ExpressionDomain::EpgDataMapping);

    let expr = r#"program_category contains "Sports""#;
    let result = parser.validate(expr);

    assert!(
        result.is_valid,
        "Alias field should validate as OK, errors: {:?}",
        result.errors
    );

    // Canonicalize and ensure the British spelling is used
    let canonical = parser.canonicalize_expression_lossy(expr);
    assert!(
        canonical.contains("programme_category"),
        "Expected canonical form to contain 'programme_category', got: {canonical}"
    );
    assert!(
        !canonical.contains("program_category"),
        "Alias should have been replaced; canonical form still has 'program_category'"
    );
}

/// Test 2: Union parser that includes both StreamFilter + EpgFilter domains.
/// Ensures a mixed expression using fields from both domains validates.
#[test]
fn test_union_domain_stream_and_epg_filter() {
    let parser = build_union_parser(&[ExpressionDomain::StreamFilter, ExpressionDomain::EpgFilter]);

    // channel_name (stream) + programme_title (epg) with alias program_title to test alias + canonical mix
    let expr = r#"channel_name contains "HD" AND program_title contains "News""#;

    let result = parser.validate(expr);
    assert!(
        result.is_valid,
        "Union domain validation failed; errors: {:?}",
        result.errors
    );

    // Check canonicalization replaced program_title -> programme_title
    let canonical = parser.canonicalize_expression_lossy(expr);
    assert!(
        canonical.contains("programme_title"),
        "Expected programme_title in canonical form, got: {canonical}"
    );
    assert!(
        !canonical.contains("program_title "),
        "Alias 'program_title' should not remain in canonical form: {canonical}"
    );
}

/// Test 3: Combined data-mapping union (stream + epg) allows both channel + programme field aliases.
/// This simulates unified endpoint with domains=stream_mapping,epg_mapping.
#[test]
fn test_union_domain_stream_and_epg_mapping_alias_mix() {
    let parser = build_union_parser(&[
        ExpressionDomain::StreamDataMapping,
        ExpressionDomain::EpgDataMapping,
    ]);

    // Use multiple aliases: title (program_title), group_title, program_category
    // Include an action to ensure it's still accepted syntactically.
    let expr = r#"title contains "Match" AND program_category equals "Sports" SET channel_name ?= "Sports Channel""#;
    let result = parser.validate(expr);

    assert!(
        result.is_valid,
        "Union mapping domain with aliases failed; errors: {:?}",
        result.errors
    );

    let canonical = parser.canonicalize_expression_lossy(expr);
    // title -> programme_title, program_category -> programme_category
    assert!(
        canonical.contains("programme_title"),
        "Expected programme_title in canonical form, got: {canonical}"
    );
    assert!(
        canonical.contains("programme_category"),
        "Expected programme_category in canonical form, got: {canonical}"
    );
    assert!(
        !canonical.contains("program_category"),
        "program_category alias should have been replaced, canonical: {canonical}"
    );
}

/// Test 4: Ensure a bogus field still produces an unknown_field error in union parser,
/// verifying field validation remains active with aliases present.
#[test]
fn test_union_domain_unknown_field_rejected() {
    let parser = build_union_parser(&[ExpressionDomain::StreamFilter, ExpressionDomain::EpgFilter]);
    let expr = r#"channel_name contains "News" AND unknown_field equals "X""#;
    let result = parser.validate(expr);
    assert!(
        !result.is_valid,
        "Expected validation failure for unknown field"
    );
    assert!(
        result
            .errors
            .iter()
            .any(|e| e.error_type == "unknown_field"),
        "Expected at least one unknown_field error; got: {:?}",
        result.errors
    );
}
