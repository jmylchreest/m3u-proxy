/*!
 Field Registry

 Central authoritative definition of:
  - Canonical field names (British English for programme-related fields)
  - Field metadata (display name, data type, nullability, read-only)
  - Stage / source applicability
  - Alias → canonical mapping (American spellings & legacy variants)
  - Utility accessors for validators, parsers, APIs

 This eliminates the prior duplication of field lists scattered across:
  - Filter repositories
  - Data mapping validators
  - Rule / filter processors
  - Validation endpoints
  - Hard-coded helpers

 Usage pattern (high-level):
   let reg = FieldRegistry::global();
   let canonical = reg.resolve_alias("program_title"); // -> Some("programme_title")
   let fields = reg.field_names_for(SourceKind::Epg, StageKind::Filtering);

 Read-only fields (e.g. source_* meta fields) are enforced upstream by
 checking `is_read_only("source_name")` before allowing assignments.

 NOTE: This module intentionally has no dependency on the expression parser
 to avoid circular references. The parser should *consume* this module,
 not the other way round.
*/

use std::collections::{HashMap, HashSet};
use std::sync::OnceLock;

/// The origin category for a field.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SourceKind {
    Stream,
    Epg,
}

/// The processing stage / context in which a field is usable.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum StageKind {
    Filtering,
    DataMapping,
    Numbering,
    Generation,
}

/// A lightweight enum for data typing – can be extended when stricter
/// operator validation is introduced.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum FieldDataType {
    String,
    Url,
    Integer,
    DateTime,
    Duration,
}

/// Descriptor for a single canonical field.
pub struct FieldDescriptor {
    pub name: &'static str,
    pub display_name: &'static str,
    pub data_type: FieldDataType,
    pub nullable: bool,
    pub read_only: bool,
    pub source_kinds: &'static [SourceKind],
    pub stages: &'static [StageKind],
    pub aliases: &'static [&'static str],
}

/// Convenience macro to define a FieldDescriptor.
macro_rules! fd {
    (
        name: $name:expr,
        display: $display:expr,
        ty: $ty:expr,
        nullable: $nullable:expr,
        read_only: $ro:expr,
        sources: [$($sk:expr),* $(,)?],
        stages:  [$($st:expr),* $(,)?],
        aliases: [$($alias:expr),* $(,)?]
    ) => {
        FieldDescriptor {
            name: $name,
            display_name: $display,
            data_type: $ty,
            nullable: $nullable,
            read_only: $ro,
            source_kinds: &[$($sk),*],
            stages: &[$($st),*],
            aliases: &[$($alias),*],
        }
    };
}

// Stage convenience arrays removed; stages are now inlined per descriptor to avoid unused constant warnings.

/// Static registry of canonical descriptors.
/// NOTE: Keep alphabetical-ish grouping per domain for clarity.
static FIELD_DESCRIPTORS: &[FieldDescriptor] = &[
    // ---------------------------
    // Stream Channel Fields
    // ---------------------------
    fd! {
        name: "channel_name",
        display: "Channel Name",
        ty: FieldDataType::String,
        nullable: false,
        read_only: false,
        sources: [SourceKind::Stream],
        stages: [StageKind::Filtering, StageKind::DataMapping, StageKind::Numbering, StageKind::Generation],
        aliases: []
    },
    fd! {
        name: "group_title",
        display: "Channel Group",
        ty: FieldDataType::String,
        nullable: true,
        read_only: false,
        sources: [SourceKind::Stream],
        stages: [StageKind::Filtering, StageKind::DataMapping, StageKind::Numbering, StageKind::Generation],
        // Add alias so 'channel_group' (EPG naming) resolves to canonical 'group_title'
        aliases: ["channel_group"]
    },
    fd! {
        name: "tvg_id",
        display: "TV Guide ID",
        ty: FieldDataType::String,
        nullable: true,
        read_only: false,
        sources: [SourceKind::Stream],
        stages: [StageKind::Filtering, StageKind::DataMapping, StageKind::Numbering, StageKind::Generation],
        aliases: []
    },
    fd! {
        name: "tvg_name",
        display: "TV Guide Name",
        ty: FieldDataType::String,
        nullable: true,
        read_only: false,
        sources: [SourceKind::Stream],
        stages: [StageKind::Filtering, StageKind::DataMapping, StageKind::Numbering, StageKind::Generation],
        aliases: []
    },
    fd! {
        name: "tvg_logo",
        display: "TV Guide Logo",
        ty: FieldDataType::Url,
        nullable: true,
        read_only: false,
        sources: [SourceKind::Stream],
        stages: [StageKind::Filtering, StageKind::DataMapping, StageKind::Numbering, StageKind::Generation],
        aliases: []
    },
    fd! {
        name: "tvg_shift",
        display: "Timeshift Offset",
        ty: FieldDataType::String,
        nullable: true,
        read_only: false,
        sources: [SourceKind::Stream],
        stages: [StageKind::Filtering, StageKind::DataMapping, StageKind::Numbering, StageKind::Generation],
        aliases: []
    },
    fd! {
        name: "tvg_chno",
        display: "Channel Number",
        ty: FieldDataType::String,
        nullable: true,
        read_only: false,
        sources: [SourceKind::Stream],
        stages: [StageKind::Filtering, StageKind::DataMapping, StageKind::Numbering, StageKind::Generation],
        aliases: ["channel_number"]
    },
    fd! {
        name: "stream_url",
        display: "Stream URL",
        ty: FieldDataType::Url,
        nullable: false,
        read_only: false,
        sources: [SourceKind::Stream],
        stages: [StageKind::Filtering, StageKind::DataMapping, StageKind::Numbering, StageKind::Generation],
        aliases: []
    },
    // ---------------------------
    // New Source Meta (Read-only for both domains)
    // ---------------------------
    fd! {
        name: "source_name",
        display: "Source Name",
        ty: FieldDataType::String,
        nullable: false,
        read_only: true,
        sources: [SourceKind::Stream, SourceKind::Epg],
        stages: [StageKind::Filtering, StageKind::DataMapping, StageKind::Numbering, StageKind::Generation],
        aliases: []
    },
    fd! {
        name: "source_type",
        display: "Source Type",
        ty: FieldDataType::String,
        nullable: false,
        read_only: true,
        sources: [SourceKind::Stream, SourceKind::Epg],
        stages: [StageKind::Filtering, StageKind::DataMapping, StageKind::Numbering, StageKind::Generation],
        aliases: []
    },
    fd! {
        name: "source_url",
        display: "Source URL (Sanitised)",
        ty: FieldDataType::Url,
        nullable: false,
        read_only: true,
        sources: [SourceKind::Stream, SourceKind::Epg],
        stages: [StageKind::Filtering, StageKind::DataMapping, StageKind::Numbering, StageKind::Generation],
        aliases: []
    },
    // ---------------------------
    // EPG Programme / Channel Fields
    // ---------------------------
    fd! {
        name: "channel_id",
        display: "EPG Channel ID",
        ty: FieldDataType::String,
        nullable: false,
        read_only: false,
        sources: [SourceKind::Epg],
        stages: [StageKind::Filtering, StageKind::DataMapping, StageKind::Generation],
        aliases: []
    },
    fd! {
        name: "channel_logo",
        display: "EPG Channel Logo",
        ty: FieldDataType::Url,
        nullable: true,
        read_only: false,
        sources: [SourceKind::Epg],
        stages: [StageKind::Filtering, StageKind::DataMapping, StageKind::Generation],
        aliases: []
    },
    fd! {
        name: "channel_group",
        display: "EPG Channel Group",
        ty: FieldDataType::String,
        nullable: true,
        read_only: false,
        sources: [SourceKind::Epg],
        stages: [StageKind::Filtering, StageKind::DataMapping, StageKind::Generation],
        // Removed reverse alias to prevent mapping 'group_title' -> 'channel_group'
        aliases: ["group_title"] // allow a reused concept
    },
    fd! {
        name: "language",
        display: "Programme Language",
        ty: FieldDataType::String,
        nullable: true,
        read_only: false,
        sources: [SourceKind::Epg],
        stages: [StageKind::Filtering, StageKind::DataMapping, StageKind::Generation],
        aliases: []
    },
    fd! {
        name: "rating",
        display: "Content Rating",
        ty: FieldDataType::String,
        nullable: true,
        read_only: false,
        sources: [SourceKind::Epg],
        stages: [StageKind::Filtering, StageKind::DataMapping, StageKind::Generation],
        aliases: []
    },
    fd! {
        name: "aspect_ratio",
        display: "Aspect Ratio",
        ty: FieldDataType::String,
        nullable: true,
        read_only: false,
        sources: [SourceKind::Epg],
        stages: [StageKind::Filtering, StageKind::DataMapping, StageKind::Generation],
        aliases: []
    },
    fd! {
        name: "episode_num",
        display: "Episode Number",
        ty: FieldDataType::String,
        nullable: true,
        read_only: false,
        sources: [SourceKind::Epg],
        stages: [StageKind::Filtering, StageKind::DataMapping, StageKind::Generation],
        aliases: ["episode_number"]
    },
    fd! {
        name: "season_num",
        display: "Season Number",
        ty: FieldDataType::String,
        nullable: true,
        read_only: false,
        sources: [SourceKind::Epg],
        stages: [StageKind::Filtering, StageKind::DataMapping, StageKind::Generation],
        aliases: ["season_number"]
    },
    fd! {
        name: "programme_title",
        display: "Programme Title",
        ty: FieldDataType::String,
        nullable: false,
        read_only: false,
        sources: [SourceKind::Epg],
        stages: [StageKind::Filtering, StageKind::DataMapping, StageKind::Generation],
        aliases: ["program_title", "title", "prog_title"]
    },
    fd! {
        name: "programme_description",
        display: "Programme Description",
        ty: FieldDataType::String,
        nullable: true,
        read_only: false,
        sources: [SourceKind::Epg],
        stages: [StageKind::Filtering, StageKind::DataMapping, StageKind::Generation],
        aliases: ["program_description", "description", "prog_desc"]
    },
    fd! {
        name: "programme_category",
        display: "Programme Category",
        ty: FieldDataType::String,
        nullable: true,
        read_only: false,
        sources: [SourceKind::Epg],
        stages: [StageKind::Filtering, StageKind::DataMapping, StageKind::Generation],
        aliases: ["program_category"]
    },
    fd! {
        name: "programme_icon",
        display: "Programme Icon",
        ty: FieldDataType::Url,
        nullable: true,
        read_only: false,
        sources: [SourceKind::Epg],
        stages: [StageKind::Filtering, StageKind::DataMapping, StageKind::Generation],
        aliases: ["program_icon"]
    },
    fd! {
        name: "programme_subtitle",
        display: "Programme Subtitle",
        ty: FieldDataType::String,
        nullable: true,
        read_only: false,
        sources: [SourceKind::Epg],
        stages: [StageKind::Filtering, StageKind::DataMapping, StageKind::Generation],
        aliases: ["subtitles", "programme_subtitles"]
    },
    // (Optional) temporal fields: add when expression engine supports date arithmetic.
    // fd! { name: "start_time", ... } etc.
];

/// Central registry object (immutable after init).
pub struct FieldRegistry {
    descriptors: &'static [FieldDescriptor],
    alias_to_canonical: HashMap<&'static str, &'static str>,
    canonical_set: HashSet<&'static str>,
    read_only: HashSet<&'static str>,
}

impl FieldRegistry {
    fn new() -> Self {
        let mut alias_to_canonical = HashMap::new();
        let mut canonical_set = HashSet::new();
        let mut read_only = HashSet::new();

        for d in FIELD_DESCRIPTORS {
            canonical_set.insert(d.name);
            if d.read_only {
                read_only.insert(d.name);
            }
            for &alias in d.aliases {
                // If duplicate alias appears, first one wins (deterministic definition).
                alias_to_canonical.entry(alias).or_insert(d.name);
            }
        }

        Self {
            descriptors: FIELD_DESCRIPTORS,
            alias_to_canonical,
            canonical_set,
            read_only,
        }
    }

    /// Global singleton accessor (no external dependency on once_cell).
    pub fn global() -> &'static Self {
        static REGISTRY: OnceLock<FieldRegistry> = OnceLock::new();
        REGISTRY.get_or_init(FieldRegistry::new)
    }

    /// Resolve an alias (American or legacy) to the canonical British field name.
    /// Returns None if the name is already canonical (or not known as an alias).
    pub fn resolve_alias(&self, candidate: &str) -> Option<&'static str> {
        self.alias_to_canonical.get(candidate).copied()
    }

    /// Return canonical if either canonical or alias; None if unknown.
    pub fn canonical_or_none(&self, field: &str) -> Option<&'static str> {
        if let Some(canon) = self.canonical_set.get(field) {
            Some(*canon)
        } else {
            self.resolve_alias(field)
        }
    }

    /// Is this canonical field read-only?
    pub fn is_read_only(&self, field: &str) -> bool {
        self.read_only.contains(field)
    }

    /// Return descriptors valid for the given source kind & stage.
    pub fn descriptors_for(
        &self,
        source: SourceKind,
        stage: StageKind,
    ) -> Vec<&'static FieldDescriptor> {
        self.descriptors
            .iter()
            .filter(|d| d.source_kinds.contains(&source) && d.stages.contains(&stage))
            .collect()
    }

    /// Return just the canonical field names for a (source, stage).
    pub fn field_names_for(&self, source: SourceKind, stage: StageKind) -> Vec<&'static str> {
        self.descriptors_for(source, stage)
            .into_iter()
            .map(|d| d.name)
            .collect()
    }

    /// Return every canonical field (across all sources & stages).
    pub fn all_canonical_fields(&self) -> Vec<&'static str> {
        let mut v: Vec<&'static str> = self.canonical_set.iter().copied().collect();
        v.sort_unstable();
        v
    }

    /// Produce a complete alias map clone (for parser injection).
    pub fn alias_map(&self) -> HashMap<String, String> {
        self.alias_to_canonical
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    /// Convenience: test if a string is a known canonical field.
    pub fn is_canonical(&self, field: &str) -> bool {
        self.canonical_set.contains(field)
    }

    /// Utility to sanitise a source URL (remove credentials + sensitive query params).
    /// This is intentionally conservative; expand as needed.
    pub fn sanitise_source_url(raw: &str) -> String {
        // Strip userinfo: scheme://user:pass@host -> scheme://host
        let mut sanitized = if let Some(idx) = raw.find("://") {
            let (scheme, rest) = raw.split_at(idx + 3); // include "://"
            if let Some(at_pos) = rest.find('@') {
                // Remove up to '@'
                format!("{}{}", scheme, &rest[at_pos + 1..])
            } else {
                raw.to_string()
            }
        } else {
            raw.to_string()
        };

        // Remove sensitive query parameters
        if let Some(q_idx) = sanitized.find('?') {
            let (base, query) = sanitized.split_at(q_idx);
            let query = &query[1..];
            let mut filtered_pairs = vec![];
            for pair in query.split('&') {
                let key = pair.split('=').next().unwrap_or("");
                let lower = key.to_ascii_lowercase();
                if matches!(
                    lower.as_str(),
                    "username"
                        | "user"
                        | "password"
                        | "pass"
                        | "token"
                        | "auth"
                        | "api_key"
                        | "apikey"
                ) {
                    continue;
                }
                filtered_pairs.push(pair);
            }
            if filtered_pairs.is_empty() {
                sanitized = base.to_string();
            } else {
                sanitized = format!("{}?{}", base, filtered_pairs.join("&"));
            }
        }

        sanitized
    }
}

// ---------------------------
// Tests (kept lightweight – can be expanded in dedicated test module)
// ---------------------------
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn alias_resolution_basic() {
        let reg = FieldRegistry::global();
        assert_eq!(reg.resolve_alias("program_title"), Some("programme_title"));
        assert_eq!(reg.resolve_alias("programme_title"), None);
        assert_eq!(
            reg.canonical_or_none("program_description"),
            Some("programme_description")
        );
        assert!(reg.is_canonical("programme_title"));
    }

    #[test]
    fn read_only_flags() {
        let reg = FieldRegistry::global();
        assert!(reg.is_read_only("source_name"));
        assert!(reg.is_read_only("source_type"));
        assert!(reg.is_read_only("source_url"));
        assert!(!reg.is_read_only("channel_name"));
    }

    #[test]
    fn stream_field_listing_contains_expected() {
        let reg = FieldRegistry::global();
        let mut fields = reg.field_names_for(SourceKind::Stream, StageKind::Filtering);
        fields.sort();
        assert!(fields.contains(&"channel_name"));
        assert!(fields.contains(&"source_name"));
        assert!(fields.contains(&"source_type"));
        assert!(fields.contains(&"source_url"));
        assert!(fields.contains(&"tvg_chno"));
    }

    #[test]
    fn epg_aliases_work() {
        let reg = FieldRegistry::global();
        assert_eq!(
            reg.canonical_or_none("program_category"),
            Some("programme_category")
        );
        assert_eq!(
            reg.canonical_or_none("subtitles"),
            Some("programme_subtitle")
        );
    }

    #[test]
    fn url_sanitisation() {
        let raw = "http://user:secret@example.com/path?token=XYZ&keep=1&password=abc";
        let sanitized = FieldRegistry::sanitise_source_url(raw);
        assert!(!sanitized.contains("user:secret"));
        assert!(!sanitized.contains("token=XYZ"));
        assert!(!sanitized.contains("password=abc"));
        assert!(sanitized.contains("keep=1"));
    }

    // ---------------------------
    // Parity / Consistency Tests
    // ---------------------------

    #[test]
    fn parity_datamapping_stream_fields() {
        use crate::models::data_mapping::{DataMappingFieldInfo, DataMappingSourceType};
        let reg = FieldRegistry::global();

        let registry: std::collections::HashSet<&'static str> = reg
            .field_names_for(SourceKind::Stream, StageKind::DataMapping)
            .into_iter()
            .collect();

        let helper: std::collections::HashSet<String> =
            DataMappingFieldInfo::available_for_source_type(&DataMappingSourceType::Stream)
                .into_iter()
                .map(|f| f.canonical_name)
                .collect();

        assert_eq!(
            registry,
            helper.iter().map(|s| s.as_str()).collect(),
            "Stream DataMapping fields in registry and DataMappingFieldInfo diverge"
        );
    }

    #[test]
    fn parity_datamapping_epg_fields() {
        use crate::models::data_mapping::{DataMappingFieldInfo, DataMappingSourceType};
        let reg = FieldRegistry::global();

        let registry: std::collections::HashSet<&'static str> = reg
            .field_names_for(SourceKind::Epg, StageKind::DataMapping)
            .into_iter()
            .collect();

        let helper: std::collections::HashSet<String> =
            DataMappingFieldInfo::available_for_source_type(&DataMappingSourceType::Epg)
                .into_iter()
                .map(|f| f.canonical_name)
                .collect();

        assert_eq!(
            registry,
            helper.iter().map(|s| s.as_str()).collect(),
            "EPG DataMapping fields in registry and DataMappingFieldInfo diverge"
        );
    }

    #[test]
    fn parity_filter_stream_fields() {
        use crate::models::{FilterFieldInfo, FilterSourceType};
        let reg = FieldRegistry::global();

        let registry: std::collections::HashSet<&'static str> = reg
            .field_names_for(SourceKind::Stream, StageKind::Filtering)
            .into_iter()
            .collect();

        let helper: std::collections::HashSet<String> =
            FilterFieldInfo::available_for_source_type(&FilterSourceType::Stream)
                .into_iter()
                .map(|f| f.canonical_name)
                .collect();

        assert_eq!(
            registry,
            helper.iter().map(|s| s.as_str()).collect(),
            "Stream Filtering fields in registry and FilterFieldInfo diverge"
        );
    }

    #[test]
    fn parity_filter_epg_fields() {
        use crate::models::{FilterFieldInfo, FilterSourceType};
        let reg = FieldRegistry::global();

        let registry: std::collections::HashSet<&'static str> = reg
            .field_names_for(SourceKind::Epg, StageKind::Filtering)
            .into_iter()
            .collect();

        let helper: std::collections::HashSet<String> =
            FilterFieldInfo::available_for_source_type(&FilterSourceType::Epg)
                .into_iter()
                .map(|f| f.canonical_name)
                .collect();

        assert_eq!(
            registry,
            helper.iter().map(|s| s.as_str()).collect(),
            "EPG Filtering fields in registry and FilterFieldInfo diverge"
        );
    }

    // ----------------------------------------------------
    // Alias acceptance tests (expression parser integration)
    // Ensures program_* (American) aliases parse correctly
    // and are accepted via alias -> canonical resolution.
    // ----------------------------------------------------
    #[test]
    fn alias_expression_parsing_programme_fields() {
        use crate::expression_parser::ExpressionParser;
        let reg = FieldRegistry::global();
        // Use EPG DataMapping stage (programme fields present)
        let fields: Vec<String> = reg
            .field_names_for(SourceKind::Epg, StageKind::DataMapping)
            .into_iter()
            .map(|s| s.to_string())
            .collect();
        let parser = ExpressionParser::new()
            .with_fields(fields)
            .with_aliases(reg.alias_map());

        // American spellings only in expression (symbolic operators, preprocessed for normalization):
        let expr = r#"program_title == "Match" AND program_description =~ "Live" AND program_category == "Sports""#;
        let pre = crate::expression::preprocess_expression(expr).expect("preprocess ok");
        parser
            .parse_extended(&pre)
            .expect("Alias-based programme expression should parse using canonical mapping");
    }

    #[test]
    fn alias_expression_parsing_subtitles_and_icon() {
        use crate::expression_parser::ExpressionParser;
        let reg = FieldRegistry::global();
        let fields: Vec<String> = reg
            .field_names_for(SourceKind::Epg, StageKind::DataMapping)
            .into_iter()
            .map(|s| s.to_string())
            .collect();
        let parser = ExpressionParser::new()
            .with_fields(fields)
            .with_aliases(reg.alias_map());

        // Mix of aliases: subtitles (-> programme_subtitle), program_icon (-> programme_icon) using symbolic operators
        let expr = r#"subtitles =~ "CC" OR program_icon != "" "#;
        let pre = crate::expression::preprocess_expression(expr).expect("preprocess ok");
        parser
            .parse_extended(&pre)
            .expect("Alias subtitles/program_icon expression should parse");
    }

    #[test]
    fn alias_expression_parsing_mixed_canonical_and_alias() {
        use crate::expression_parser::ExpressionParser;
        let reg = FieldRegistry::global();
        let fields: Vec<String> = reg
            .field_names_for(SourceKind::Epg, StageKind::DataMapping)
            .into_iter()
            .map(|s| s.to_string())
            .collect();
        let parser = ExpressionParser::new()
            .with_fields(fields)
            .with_aliases(reg.alias_map());

        // Mix canonical (programme_title) with alias (program_description) using symbolic operators
        let expr = r#"programme_title == "News" AND program_description =~ "Breaking""#;
        let pre = crate::expression::preprocess_expression(expr).expect("preprocess ok");
        parser
            .parse_extended(&pre)
            .expect("Mixed canonical + alias expression should parse");
    }
}
