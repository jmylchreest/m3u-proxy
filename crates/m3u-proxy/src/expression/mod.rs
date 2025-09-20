use std::time::Instant;

use tracing::trace;

use crate::expression_parser::ExpressionParser;
use crate::field_registry::{FieldRegistry, SourceKind, StageKind};
use crate::models::ExtendedExpression;

/// Logical “domain” in which an expression is authored / evaluated.
///
/// This lets us:
/// * Select the correct source kind (Stream vs EPG)
/// * Select the correct processing stage (Filtering vs DataMapping)
/// * Derive the canonical field set (and their aliases) from a single registry
/// * Keep future additions (e.g. Generation phase) centralized
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ExpressionDomain {
    // Filtering (selection) contexts
    StreamFilter,
    EpgFilter,

    // Data mapping / transformation (rules applied to entity fields)
    StreamDataMapping,
    EpgDataMapping,

    // “Rule” processors (internally still data‑mapping semantics, but may diverge later)
    StreamRule,
    EpgRule,
}

/// A fully parsed expression, preserving both the original text and the
/// extended AST (which may contain actions, groups, etc).
///
/// Storing the full `ExtendedExpression` instead of only the `ConditionTree`
/// allows later features (e.g. conditional action groups, expression
/// normalization display, partial evaluation, metrics) without reparsing.
pub struct ParsedExpression {
    pub original_text: String,
    pub extended: ExtendedExpression,
    pub parsed_at: Instant,
}

impl ParsedExpression {
    /// Convenience accessor for the underlying condition tree
    pub fn condition_tree(&self) -> &crate::models::ConditionTree {
        self.extended.condition_tree()
    }
}

/// Trait implemented by processors that wrap a parsed expression object.
pub trait HasParsedExpression {
    fn parsed_expression(&self) -> Option<&ParsedExpression>;

    fn condition_tree(&self) -> Option<&crate::models::ConditionTree> {
        self.parsed_expression().map(|p| p.condition_tree())
    }
}

/// Pre-process an expression string (e.g. resolve @time: helpers).
///
/// Any additional pre-parse rewrites (whitespace canonicalization, macro
/// expansion, feature flags) should funnel through here to keep the
/// processors themselves minimal.
pub fn preprocess_expression(raw: &str) -> anyhow::Result<String> {
    // 1. Empty fast‑path
    if raw.trim().is_empty() {
        return Ok(String::new());
    }

    // 2. Normalize symbolic operators, canonicalize any legacy fused negations, & relocate any legacy pre-field modifiers
    let mut rewritten = canonicalize_legacy_fused_negations(&normalize_symbolic_operators(raw));
    if let Some((relocated, changed)) = relocate_pre_field_modifiers(&rewritten) {
        if changed {
            tracing::debug!(
                "[EXPR_REWRITE] kind=pre_field_modifiers original='{}' rewritten='{}'",
                truncate_for_log(raw, 160),
                truncate_for_log(&relocated, 160)
            );
        }
        rewritten = relocated;
    }

    // 3. Collapse excess whitespace introduced by normalization
    rewritten = collapse_whitespace(&rewritten);

    // 4. Resolve time helpers (@time:now(), @time:parse(...))
    let resolved =
        crate::utils::time::resolve_time_functions(&rewritten).map_err(|e| anyhow::anyhow!(e))?;

    Ok(resolved)
}

/// Build an `ExpressionParser` configured for the specified domain.
///
/// This:
/// 1. Obtains the global field registry
/// 2. Filters descriptors by (source kind, stage kind)
/// 3. Collects canonical field names
/// 4. Attaches the alias map
pub fn build_parser_for(domain: ExpressionDomain) -> ExpressionParser {
    // Temporary compatibility wrapper: delegate to ParserFactory (new unified path).
    ParserFactory::global()
        .parser_for_domain(domain)
        .as_ref()
        .clone()
}

/// Public snapshot struct for expression metrics (exposed for debugging endpoints / UI cards).
#[derive(Debug, Clone, serde::Serialize)]
pub struct ExpressionMetricsSnapshot {
    pub total_parses: u64,
    pub cache_hits: u64,
    pub cache_misses: u64,
    pub cache_size: usize,
    pub average_parse_time_ns: u64,
    // New: total validation calls recorded
    pub total_validations: u64,
    // New: duration (ns) of the most recent validation aggregation
    pub validation_last_duration_ns: u64,
    // New: per-domain validation call counts (keys match unified domain query values)
    pub per_domain_validations: std::collections::HashMap<&'static str, u64>,
}

/// Public accessor returning a cheap snapshot of current metrics (parsing + validation).
pub fn expression_metrics_snapshot() -> ExpressionMetricsSnapshot {
    let m = metrics();
    use std::sync::atomic::Ordering;

    let per_domain = {
        if let Ok(map) = m.per_domain_validations.lock() {
            map.clone()
        } else {
            std::collections::HashMap::new()
        }
    };

    ExpressionMetricsSnapshot {
        total_parses: m.total_parses.load(Ordering::Relaxed),
        cache_hits: m.cache_hits.load(Ordering::Relaxed),
        cache_misses: m.cache_misses.load(Ordering::Relaxed),
        cache_size: ParserFactory::global().cache_size(),
        average_parse_time_ns: m.average_parse_time_ns(),
        total_validations: m.total_validations.load(Ordering::Relaxed),
        validation_last_duration_ns: m.validation_last_duration_ns.load(Ordering::Relaxed),
        per_domain_validations: per_domain,
    }
}

/// External helper for the web layer to record validation events (union-domain validations).
/// Call this from the unified validation endpoint with the domains involved and elapsed duration.
pub fn record_validation_metrics(domains: &[ExpressionDomain], duration: std::time::Duration) {
    metrics().record_validation(domains, duration);
}

/// Expression parsing / validation metrics (aggregated).
#[derive(Default)]
struct ExpressionMetrics {
    // Parsing-related
    total_parses: std::sync::atomic::AtomicU64,
    cache_hits: std::sync::atomic::AtomicU64,
    cache_misses: std::sync::atomic::AtomicU64,
    total_parse_time_ns: std::sync::atomic::AtomicU64,

    // Validation-related (added)
    total_validations: std::sync::atomic::AtomicU64,
    validation_last_duration_ns: std::sync::atomic::AtomicU64,
    per_domain_validations: std::sync::Mutex<std::collections::HashMap<&'static str, u64>>,
}

impl ExpressionMetrics {
    fn record_parse(&self, duration: std::time::Duration, cache_hit: bool) {
        self.total_parses
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        if cache_hit {
            self.cache_hits
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        } else {
            self.cache_misses
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        }
        self.total_parse_time_ns.fetch_add(
            duration.as_nanos() as u64,
            std::sync::atomic::Ordering::Relaxed,
        );
    }

    fn record_validation(
        &self,
        domains: &[crate::expression::ExpressionDomain],
        duration: std::time::Duration,
    ) {
        self.total_validations
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        self.validation_last_duration_ns.store(
            duration.as_nanos() as u64,
            std::sync::atomic::Ordering::Relaxed,
        );
        if let Ok(mut map) = self.per_domain_validations.lock() {
            for d in domains {
                let key: &'static str = match d {
                    crate::expression::ExpressionDomain::StreamFilter => "stream_filter",
                    crate::expression::ExpressionDomain::EpgFilter => "epg_filter",
                    crate::expression::ExpressionDomain::StreamDataMapping => "stream_mapping",
                    crate::expression::ExpressionDomain::EpgDataMapping => "epg_mapping",
                    crate::expression::ExpressionDomain::StreamRule => "stream_rule",
                    crate::expression::ExpressionDomain::EpgRule => "epg_rule",
                };
                *map.entry(key).or_insert(0) += 1;
            }
        }
    }

    fn average_parse_time_ns(&self) -> u64 {
        let parses = self.total_parses.load(std::sync::atomic::Ordering::Relaxed);
        if parses == 0 {
            0
        } else {
            self.total_parse_time_ns
                .load(std::sync::atomic::Ordering::Relaxed)
                / parses
        }
    }
}

fn metrics() -> &'static ExpressionMetrics {
    use std::sync::OnceLock;
    static M: OnceLock<ExpressionMetrics> = OnceLock::new();
    M.get_or_init(ExpressionMetrics::default)
}

/// Internal cache key (mirrors ExpressionDomain).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct DomainKey(ExpressionDomain);

impl From<ExpressionDomain> for DomainKey {
    fn from(d: ExpressionDomain) -> Self {
        Self(d)
    }
}

/// ParserFactory caches parsers per domain with filtered aliases.
pub struct ParserFactory {
    cache:
        std::sync::RwLock<std::collections::HashMap<DomainKey, std::sync::Arc<ExpressionParser>>>,
}

impl ParserFactory {
    fn new() -> Self {
        Self {
            cache: std::sync::RwLock::new(std::collections::HashMap::new()),
        }
    }

    pub fn global() -> &'static Self {
        use std::sync::OnceLock;
        static INSTANCE: OnceLock<ParserFactory> = OnceLock::new();
        INSTANCE.get_or_init(Self::new)
    }

    pub fn parser_for_domain(&self, domain: ExpressionDomain) -> std::sync::Arc<ExpressionParser> {
        let key = DomainKey::from(domain);
        // Fast path: read cache
        if let Some(p) = self.cache.read().unwrap().get(&key) {
            return p.clone();
        }

        // Build parser
        let start = std::time::Instant::now();
        let registry = FieldRegistry::global();
        let full_alias_map = registry.alias_map();
        let (source_kind, stage_kind) = domain_to_source_and_stage(domain);

        let mut fields: Vec<String> = registry
            .descriptors_for(source_kind, stage_kind)
            .into_iter()
            .map(|d| d.name.to_string())
            .collect();
        fields.sort();
        fields.dedup();

        let field_set: std::collections::HashSet<&str> =
            fields.iter().map(|s| s.as_str()).collect();
        let filtered_aliases: std::collections::HashMap<String, String> = full_alias_map
            .into_iter()
            .filter(|(_alias, canonical)| field_set.contains(canonical.as_str()))
            .collect();

        let parser = ExpressionParser::new()
            .with_fields(fields)
            .with_aliases(filtered_aliases);
        let arc = std::sync::Arc::new(parser);

        {
            let mut w = self.cache.write().unwrap();
            w.insert(key, arc.clone());
        }

        metrics().record_parse(start.elapsed(), false);
        arc
    }

    pub fn cache_size(&self) -> usize {
        self.cache.read().unwrap().len()
    }
}

/// High-level engine coupling preprocessing + parsing.
pub struct ExpressionEngine {
    parser: std::sync::Arc<ExpressionParser>,
}

impl ExpressionEngine {
    pub fn for_domain(domain: ExpressionDomain) -> Self {
        let parser = ParserFactory::global().parser_for_domain(domain);
        Self { parser }
    }

    pub fn parser(&self) -> &ExpressionParser {
        &self.parser
    }

    pub fn parse_extended(&self, raw_expression: &str) -> anyhow::Result<Option<ParsedExpression>> {
        if raw_expression.trim().is_empty() {
            return Ok(None);
        }
        let preprocessed = preprocess_expression(raw_expression)?;
        if preprocessed.trim().is_empty() {
            return Ok(None);
        }
        let start = std::time::Instant::now();
        let extended = self.parser.parse_extended(&preprocessed)?;
        metrics().record_parse(start.elapsed(), true);
        Ok(Some(ParsedExpression {
            original_text: raw_expression.to_string(),
            extended,
            parsed_at: start,
        }))
    }

    pub fn canonicalize_lossy(&self, raw: &str) -> String {
        self.parser.canonicalize_expression_lossy(raw)
    }
}

/// Helper: map domain to (SourceKind, StageKind)
fn domain_to_source_and_stage(domain: ExpressionDomain) -> (SourceKind, StageKind) {
    match domain {
        ExpressionDomain::StreamFilter => (SourceKind::Stream, StageKind::Filtering),
        ExpressionDomain::EpgFilter => (SourceKind::Epg, StageKind::Filtering),
        ExpressionDomain::StreamDataMapping => (SourceKind::Stream, StageKind::DataMapping),
        ExpressionDomain::EpgDataMapping => (SourceKind::Epg, StageKind::DataMapping),
        ExpressionDomain::StreamRule => (SourceKind::Stream, StageKind::DataMapping),
        ExpressionDomain::EpgRule => (SourceKind::Epg, StageKind::DataMapping),
    }
}

/// Parse (extended) and wrap inside `ParsedExpression`.
///
/// Returns `Ok(None)` if the (already trimmed) expression is empty.
pub fn parse_expression_extended(
    domain: ExpressionDomain,
    raw_expression: &str,
) -> anyhow::Result<Option<ParsedExpression>> {
    if raw_expression.trim().is_empty() {
        return Ok(None);
    }

    let preprocessed = preprocess_expression(raw_expression)?;
    if preprocessed.trim().is_empty() {
        return Ok(None);
    }

    let parser = build_parser_for(domain);

    let started = Instant::now();
    let extended = parser.parse_extended(&preprocessed)?;
    let parsed = ParsedExpression {
        original_text: raw_expression.to_string(),
        extended,
        parsed_at: started,
    };

    trace!(
        "[EXPR_PARSE] domain={:?} len={} raw='{}'",
        domain,
        parsed.original_text.len(),
        truncate_for_log(&parsed.original_text, 240)
    );

    // 5. Validate that every referenced field is legal for this domain
    validate_parsed_fields(domain, parsed.condition_tree())?;

    Ok(Some(parsed))
}

/// Validate that all fields in a parsed condition tree belong to the domain's canonical field set.
/// Returns an error identifying the first offending field with an optional suggestion.
fn validate_parsed_fields(
    domain: ExpressionDomain,
    tree: &crate::models::ConditionTree,
) -> anyhow::Result<()> {
    use crate::models::ConditionNode;

    // Build the domain field set (canonical names only)
    let registry = FieldRegistry::global();
    let (source_kind, stage_kind) = domain_to_source_and_stage(domain);
    let field_set: std::collections::HashSet<&'static str> = registry
        .descriptors_for(source_kind, stage_kind)
        .into_iter()
        .map(|d| d.name)
        .collect();

    // Gather canonical list for suggestion scoring
    let canonical: Vec<&'static str> = field_set.iter().copied().collect();

    // Simple similarity (character overlap + length penalty) – lightweight and good enough here
    fn similarity(a: &str, b: &str) -> u32 {
        if a == b {
            return 100;
        }
        let a_low = a.to_lowercase();
        let b_low = b.to_lowercase();
        let a_chars: std::collections::HashSet<char> = a_low.chars().collect();
        let b_chars: std::collections::HashSet<char> = b_low.chars().collect();
        let common = a_chars.intersection(&b_chars).count();
        // Weighted heuristic
        let max_len = a_low.len().max(b_low.len()).max(1);
        (common * 100) as u32 / max_len as u32
    }

    fn walk(
        node: &ConditionNode,
        invalid: &mut Option<(String, Option<String>)>,
        field_set: &std::collections::HashSet<&'static str>,
        canonical: &[&'static str],
    ) {
        if invalid.is_some() {
            return;
        }
        match node {
            ConditionNode::Condition { field, .. } => {
                if !field_set.contains(field.as_str()) {
                    // Find best suggestion
                    let mut best: Option<(&str, u32)> = None;
                    for cand in canonical {
                        let score = similarity(field, cand);
                        if score >= 55 {
                            match best {
                                Some((_b, s)) if score <= s => {}
                                _ => best = Some((cand, score)),
                            }
                        }
                    }
                    *invalid = Some((field.clone(), best.map(|(s, _)| s.to_string())));
                }
            }
            ConditionNode::Group { children, .. } => {
                for c in children {
                    walk(c, invalid, field_set, canonical);
                    if invalid.is_some() {
                        break;
                    }
                }
            }
        }
    }

    let mut invalid: Option<(String, Option<String>)> = None;
    walk(&tree.root, &mut invalid, &field_set, &canonical);

    if let Some((bad, suggestion)) = invalid {
        let mut msg = format!("Unknown field '{}'", bad);
        if let Some(s) = suggestion {
            msg.push_str(&format!(". Did you mean '{}'? ", s));
        } else {
            msg.push_str(". ");
        }
        // Provide available fields summary (truncated if large)
        let mut all: Vec<&str> = canonical.to_vec();
        all.sort();
        let preview = all.join(", ");
        msg.push_str(&format!("Available fields: {}", preview));
        return Err(anyhow::anyhow!(msg));
    }

    Ok(())
}

/// Utility: safe log truncation to avoid flooding trace logs with huge expressions.
fn truncate_for_log(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        let mut out = s[..max].to_string();
        out.push('…');
        out
    }
}

/// Extension trait (bridge) for `ExtendedExpression` to expose a uniform accessor
/// without leaking internal enum variants everywhere.
///
/// If `ExtendedExpression` already has a method returning a reference to an inner
/// `ConditionTree`, this adapter becomes a thin wrapper and can be removed.
trait ExtendedExpressionExt {
    fn condition_tree(&self) -> &crate::models::ConditionTree;
}

impl ExtendedExpressionExt for ExtendedExpression {
    fn condition_tree(&self) -> &crate::models::ConditionTree {
        match self {
            ExtendedExpression::ConditionOnly(tree) => tree,
            ExtendedExpression::ConditionWithActions { condition, .. } => condition,
            ExtendedExpression::ConditionalActionGroups(groups) => {
                if let Some(first) = groups.first() {
                    &first.conditions
                } else {
                    panic!(
                        "ExtendedExpression::ConditionalActionGroups is empty – no root condition available"
                    );
                }
            }
        }
    }
}

// --- Expected external enum variants imported from existing parser ---
// This documentation is here to clarify intent; actual variants live in expression_parser.rs:
//
// pub enum ExtendedExpression {
//     ConditionOnly { condition: ConditionTree },
//     ConditionWithActions { condition: ConditionTree, actions: Vec<ActionNode> },
//     ConditionalActionGroups(Vec<ConditionalActionGroup>),
// }
//
// Any change to ExtendedExpression that alters access to the underlying
// ConditionTree will require updating ExtendedExpressionExt above.

// -------------------------------------------------------------------------------------------------
// Normalization Helpers
// -------------------------------------------------------------------------------------------------

/// Normalize symbolic operators to canonical snake_case operator tokens plus an optional
/// mid‑field modifier (`not`) for negations. Also normalizes logical symbols/variants:
///   && / and  -> AND
///   || / or   -> OR
///
/// Symbol to token mappings (negations expressed via a separate `not` modifier; we do NOT
/// canonicalize to fused snake_case negated operator tokens):
///   ==  -> equals
///   !=  -> not equals
///   =~  -> matches
///   !~  -> not matches
///   >=  -> greater_than_or_equal
/// > <=  -> less_than_or_equal
///   >   -> greater_than
/// > <   -> less_than
///
/// NOTE: Multi-word operators are emitted in snake_case (greater_than_or_equal, etc.) and
/// negations are expressed via a separate 'not' modifier so the tokenizer can treat
/// 'not' uniformly rather than requiring composite operator variants.
/// We emit surrounding spaces so later whitespace collapsing produces clean single-space boundaries.
fn normalize_symbolic_operators(input: &str) -> String {
    let mut s = input.to_string();

    // Order matters: longer first for comparison/match operators.
    // Emit snake_case for multi-word positive operators; negations are represented via the separate 'not <operator>' modifier form (e.g. 'not equals', 'not matches').
    let replacements = [
        ("!~", " not matches "),
        ("=~", " matches "),
        ("!=", " not equals "),
        ("==", " equals "),
        (">=", " greater_than_or_equal "),
        ("<=", " less_than_or_equal "),
        (">", " greater_than "),
        ("<", " less_than "),
    ];

    for (pat, rep) in replacements {
        // Basic replacement. If false positives appear (inside literals), we could
        // refine with a boundary-aware regex, but string literals are already tokenized
        // after quotes, so interior occurrences won't hit here.
        s = s.replace(pat, rep);
    }

    // Logical operator normalization (symbolic and lowercase textual forms)
    // Replace symbolic operators first.
    s = s.replace("&&", " AND ");
    s = s.replace("||", " OR ");

    // Normalize common lowercase textual variants surrounded by spaces.
    // (We rely on later whitespace collapsing; this is a simple heuristic
    // and may over-replace inside unquoted words containing 'and'/'or'.)
    // To reduce false positives slightly, add leading/trailing space in patterns.
    for (pat, rep) in [(" and ", " AND "), (" or ", " OR ")] {
        s = s.replace(pat, rep);
    }

    s
}

/// Canonicalize any legacy fused negated operator tokens (e.g. `not_equals`)
/// back into the preferred modifier + operator form (`not equals`).
/// This is tolerant and idempotent; if no fused forms are present the input
/// is returned unchanged.
fn canonicalize_legacy_fused_negations(input: &str) -> String {
    let mut out = input.to_string();
    // Surround with spaces to reduce accidental replacements inside values;
    // earlier normalization has already padded operators with spaces.
    let mappings = [
        (" not_equals ", " not equals "),
        (" not_matches ", " not matches "),
        (" not_contains ", " not contains "),
        (" not_starts_with ", " not starts_with "),
        (" not_ends_with ", " not ends_with "),
    ];
    for (from, to) in mappings {
        out = out.replace(from, to);
    }
    out
}

/// Relocate legacy pre-field modifiers ("not field contains", "case_sensitive field equals") to
/// mid-field form ("field not contains"). Returns (rewritten, changed_flag).
fn relocate_pre_field_modifiers(input: &str) -> Option<(String, bool)> {
    // Quick scan: if it does not start with a modifier keyword sequence, skip.
    let trimmed = input.trim_start();
    let pre_mod_starters = ["not ", "case_sensitive "];
    if !pre_mod_starters.iter().any(|p| trimmed.starts_with(p)) {
        return Some((input.to_string(), false));
    }

    // Simple heuristic:
    // Capture leading modifiers, then a field token, then rest.
    // We only rewrite the FIRST leading modifier block; subsequent conditions (after AND/OR) will be
    // processed during a second parse invocation if needed (keeping implementation simple & safe).
    let mut parts = trimmed.split_whitespace();
    let mut modifiers = Vec::new();
    let mut field = None;
    let mut consumed = 0usize;

    for token in parts.by_ref() {
        consumed += token.len() + 1; // +1 space or approximate
        match token {
            "not" | "case_sensitive" => modifiers.push(token),
            _ => {
                field = Some(token.to_string());
                break;
            }
        }
    }

    if field.is_none() || modifiers.is_empty() {
        return Some((input.to_string(), false));
    }

    let field = field.unwrap();

    // Remaining expression (approximate slice)
    let rest = &trimmed[consumed..];

    // Rebuild: field <mods> rest_of_expression
    let mut rebuilt = String::new();
    rebuilt.push_str(&field);
    rebuilt.push(' ');
    rebuilt.push_str(&modifiers.join(" "));
    if !rest.is_empty() {
        rebuilt.push(' ');
        rebuilt.push_str(rest);
    }

    // Prepend any original leading whitespace we trimmed
    let leading_ws_len = input.len() - trimmed.len();
    let rewritten = if leading_ws_len > 0 {
        format!("{}{}", &input[..leading_ws_len], rebuilt)
    } else {
        rebuilt
    };

    let changed = rewritten != input;
    Some((rewritten, changed))
}

/// Collapse redundant internal whitespace to single spaces while preserving quoted literals intact.
fn collapse_whitespace(input: &str) -> String {
    // Fast path: if no double spaces, return.
    if !input.contains("  ") {
        return input.to_string();
    }
    let mut out = String::with_capacity(input.len());
    let mut last_was_space = false;
    for ch in input.chars() {
        if ch.is_whitespace() {
            if !last_was_space {
                out.push(' ');
                last_was_space = true;
            }
        } else {
            out.push(ch);
            last_was_space = false;
        }
    }
    out.trim().to_string()
}
