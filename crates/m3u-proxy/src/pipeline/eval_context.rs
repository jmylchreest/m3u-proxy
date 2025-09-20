//! Evaluation Contexts for Runtime Field Resolution
//!
//! This module provides lightweight, _non-persisted_ runtime context
//! structures that enrich core persisted models (e.g. `Channel`) with
//! additional metadata required for expression evaluation, such as
//! `source_name`, `source_type`, and a sanitised `source_url`.
//!
//! Rationale / Design:
//! -------------------
//! - We deliberately avoid adding transient fields to the core domain
//!   structs to keep a clean separation between persisted schema and
//!   computed / injected metadata.
//! - The evaluation context borrows the underlying record and the
//!   associated source metadata so we do not perform redundant cloning.
//! - Field access is canonical-name oriented (the expression parser
//!   should already have normalised legacy / alias names via the
//!   `FieldRegistry` before evaluation).
//! - This layer does NOT perform alias resolution itself to avoid
//!   duplicate logic; callers should canonicalise first.
//!
//! Extension Points:
//! -----------------
//! - Additional runtime-only fields (e.g. enrichment scores, normalised
//!   names) can be added to `SourceMeta` or a future `ChannelRuntime`
//!   struct without modifying the persisted model.
//! - A trait abstraction (`FieldValueAccessor`) is provided to allow
//!   generic evaluation over multiple record types in the future
//!   (e.g. EPG programmes) while keeping the call sites uniform.
//!
//! Usage (High-level):
//! -------------------
//! ```ignore
//! let meta_map: HashMap<Uuid, SourceMeta> = build_meta_map(...);
//! let source_meta = meta_map.get(&channel.source_id);
//! let ctx = ChannelEvalContext::new(&channel, source_meta);
//! if let Some(val) = ctx.get_owned("source_name") {
//!     // use value in condition
//! }
//! ```
//!
//! NOTE: Mutation (write) logic should **never** operate through this
//! context for read-only fields. Enforcement occurs earlier (validation)
//! plus a defensive guard in rule application code.
//!
use crate::models::Channel;
use crate::pipeline::engines::rule_processor::EpgProgram;
use std::borrow::Cow;

/// Metadata about a source (stream or EPG) required at evaluation time.
///
/// All fields are already sanitised / canonical:
/// - `url_sanitised` MUST have credentials & sensitive query params removed.
#[derive(Debug, Clone)]
pub struct SourceMeta {
    pub name: String,
    pub kind: String,
    pub url_sanitised: String,
}

impl SourceMeta {
    pub fn new(
        name: impl Into<String>,
        kind: impl Into<String>,
        url_sanitised: impl Into<String>,
    ) -> Self {
        Self {
            name: name.into(),
            kind: kind.into(),
            url_sanitised: url_sanitised.into(),
        }
    }
}

/// Trait for runtime field value access. Designed so additional
/// record types (e.g. EPG programme contexts) can implement the same
/// interface without duplicating evaluator logic.
pub trait FieldValueAccessor {
    /// Borrowing access (avoids allocation). Returns `None` if the
    /// canonical field is not present or unknown in this context.
    fn get(&self, canonical: &str) -> Option<Cow<'_, str>>;

    /// Owned access helper (default blanket implementation).
    fn get_owned(&self, canonical: &str) -> Option<String> {
        self.get(canonical).map(|c| c.into_owned())
    }
}

/// Runtime evaluation context for a `Channel` + optional `SourceMeta`.
///
/// This wraps a borrowed channel plus an optional borrowed source meta
/// struct. If source metadata is unexpectedly missing, `source_*` fields
/// resolve to `None` (callers may trace-log this anomaly).
pub struct ChannelEvalContext<'a> {
    pub channel: &'a Channel,
    pub source_meta: Option<&'a SourceMeta>,
}

impl<'a> ChannelEvalContext<'a> {
    pub fn new(channel: &'a Channel, source_meta: Option<&'a SourceMeta>) -> Self {
        Self {
            channel,
            source_meta,
        }
    }

    /// Fast-path check useful for rule processors deciding if a field
    /// could ever be mutated (avoids repeating registry lookups).
    pub fn is_mutable_channel_field(canonical: &str) -> bool {
        matches!(
            canonical,
            "tvg_id"
                | "tvg_name"
                | "tvg_chno"
                | "tvg_logo"
                | "tvg_shift"
                | "group_title"
                | "channel_name"
                | "stream_url"
        )
    }
}

impl<'a> FieldValueAccessor for ChannelEvalContext<'a> {
    fn get(&self, canonical: &str) -> Option<Cow<'_, str>> {
        // Channel fields (persisted)
        match canonical {
            // Required (always Some)
            "channel_name" => return Some(Cow::Borrowed(self.channel.channel_name.as_str())),
            "stream_url" => return Some(Cow::Borrowed(self.channel.stream_url.as_str())),
            // Optional
            "tvg_id" => {
                if let Some(v) = &self.channel.tvg_id {
                    return Some(Cow::Borrowed(v.as_str()));
                }
            }
            "tvg_name" => {
                if let Some(v) = &self.channel.tvg_name {
                    return Some(Cow::Borrowed(v.as_str()));
                }
            }
            "tvg_chno" => {
                if let Some(v) = &self.channel.tvg_chno {
                    return Some(Cow::Borrowed(v.as_str()));
                }
            }
            "tvg_logo" => {
                if let Some(v) = &self.channel.tvg_logo {
                    return Some(Cow::Borrowed(v.as_str()));
                }
            }
            "tvg_shift" => {
                if let Some(v) = &self.channel.tvg_shift {
                    return Some(Cow::Borrowed(v.as_str()));
                }
            }
            "group_title" => {
                if let Some(v) = &self.channel.group_title {
                    return Some(Cow::Borrowed(v.as_str()));
                }
            }
            // Source meta (read-only injected)
            "source_name" => {
                if let Some(meta) = self.source_meta {
                    return Some(Cow::Borrowed(meta.name.as_str()));
                }
            }
            "source_type" => {
                if let Some(meta) = self.source_meta {
                    return Some(Cow::Borrowed(meta.kind.as_str()));
                }
            }
            "source_url" => {
                if let Some(meta) = self.source_meta {
                    return Some(Cow::Borrowed(meta.url_sanitised.as_str()));
                }
            }
            _ => {}
        }
        None
    }
}

/// Runtime evaluation context for an `EpgProgram` + optional `SourceMeta`.
///
/// This mirrors the channel context but adapts to canonical programme field
/// naming (British English) resolved by the expression parser beforehand.
pub struct EpgProgramEvalContext<'a> {
    pub program: &'a EpgProgram,
    pub source_meta: Option<&'a SourceMeta>,
}

impl<'a> EpgProgramEvalContext<'a> {
    pub fn new(program: &'a EpgProgram, source_meta: Option<&'a SourceMeta>) -> Self {
        Self {
            program,
            source_meta,
        }
    }
}

impl<'a> FieldValueAccessor for EpgProgramEvalContext<'a> {
    fn get(&self, canonical: &str) -> Option<Cow<'_, str>> {
        match canonical {
            // Core programme / channel linkage
            "channel_id" => return Some(Cow::Borrowed(self.program.channel_id.as_str())),
            "channel_name" => return Some(Cow::Borrowed(self.program.channel_name.as_str())),
            // Programme metadata (canonical British spellings)
            "programme_title" => return Some(Cow::Borrowed(self.program.title.as_str())),
            "programme_description" => {
                if let Some(v) = &self.program.description {
                    return Some(Cow::Borrowed(v.as_str()));
                }
            }
            "programme_category" => {
                if let Some(v) = &self.program.program_category {
                    return Some(Cow::Borrowed(v.as_str()));
                }
            }
            "programme_icon" => {
                if let Some(v) = &self.program.program_icon {
                    return Some(Cow::Borrowed(v.as_str()));
                }
            }
            "programme_subtitle" => {
                if let Some(v) = &self.program.subtitles {
                    return Some(Cow::Borrowed(v.as_str()));
                }
            }
            // Numeric / textual episode info
            "episode_num" => {
                if let Some(v) = &self.program.episode_num {
                    return Some(Cow::Borrowed(v.as_str()));
                }
            }
            "season_num" => {
                if let Some(v) = &self.program.season_num {
                    return Some(Cow::Borrowed(v.as_str()));
                }
            }
            // Additional metadata
            "language" => {
                if let Some(v) = &self.program.language {
                    return Some(Cow::Borrowed(v.as_str()));
                }
            }
            "rating" => {
                if let Some(v) = &self.program.rating {
                    return Some(Cow::Borrowed(v.as_str()));
                }
            }
            "aspect_ratio" => {
                if let Some(v) = &self.program.aspect_ratio {
                    return Some(Cow::Borrowed(v.as_str()));
                }
            }
            // Injected source metadata (read-only)
            "source_name" => {
                if let Some(meta) = self.source_meta {
                    return Some(Cow::Borrowed(meta.name.as_str()));
                }
            }
            "source_type" => {
                if let Some(meta) = self.source_meta {
                    return Some(Cow::Borrowed(meta.kind.as_str()));
                }
            }
            "source_url" => {
                if let Some(meta) = self.source_meta {
                    return Some(Cow::Borrowed(meta.url_sanitised.as_str()));
                }
            }
            _ => {}
        }
        None
    }
}

// -------------------------
// Unit Tests
// -------------------------
#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{TimeZone, Utc};
    use uuid::Uuid;

    fn make_channel() -> Channel {
        Channel {
            id: Uuid::new_v4(),
            source_id: Uuid::new_v4(),
            tvg_id: Some("id123".into()),
            tvg_name: Some("Name123".into()),
            tvg_chno: Some("101".into()),
            tvg_logo: Some("http://logo".into()),
            tvg_shift: None,
            group_title: Some("GroupA".into()),
            channel_name: "Channel HD".into(),
            stream_url: "http://example/stream.m3u8".into(),
            video_codec: None,
            audio_codec: None,
            resolution: None,
            probe_method: None,
            last_probed_at: None,
            created_at: Utc.timestamp_opt(1_700_000_000, 0).single().unwrap(),
            updated_at: Utc::now(),
        }
    }

    #[test]
    fn channel_context_resolves_core_fields() {
        let channel = make_channel();
        let ctx = ChannelEvalContext::new(&channel, None);

        assert_eq!(ctx.get("channel_name").unwrap(), "Channel HD");
        assert_eq!(ctx.get("tvg_id").unwrap(), "id123");
        assert!(ctx.get("tvg_shift").is_none());
    }

    #[test]
    fn channel_context_resolves_source_meta() {
        let channel = make_channel();
        let meta = SourceMeta::new("SourceOne", "m3u", "http://example.com/list.m3u");
        let ctx = ChannelEvalContext::new(&channel, Some(&meta));

        assert_eq!(ctx.get("source_name").unwrap(), "SourceOne");
        assert_eq!(ctx.get("source_type").unwrap(), "m3u");
        assert_eq!(
            ctx.get("source_url").unwrap(),
            "http://example.com/list.m3u"
        );
    }

    #[test]
    fn channel_context_missing_source_meta_is_none() {
        let channel = make_channel();
        let ctx = ChannelEvalContext::new(&channel, None);

        assert!(ctx.get("source_name").is_none());
        assert!(ctx.get("source_type").is_none());
        assert!(ctx.get("source_url").is_none());
    }

    #[test]
    fn mutable_field_check() {
        assert!(ChannelEvalContext::is_mutable_channel_field("tvg_id"));
        assert!(ChannelEvalContext::is_mutable_channel_field("channel_name"));
        assert!(!ChannelEvalContext::is_mutable_channel_field("source_name"));
    }

    // -----------------------------
    // EPG Program Context Utilities
    // -----------------------------
    fn make_program() -> EpgProgram {
        use chrono::{TimeZone, Utc};
        EpgProgram {
            id: "prog1".into(),
            channel_id: "chan123".into(),
            channel_name: "Channel A".into(),
            title: "Programme Title".into(),
            description: Some("A descriptive text".into()),
            program_icon: Some("http://example/icon.png".into()),
            start_time: Utc.timestamp_opt(1_700_000_500, 0).single().unwrap(),
            end_time: Utc.timestamp_opt(1_700_001_000, 0).single().unwrap(),
            episode_num: Some("5".into()),
            season_num: Some("2".into()),
            language: Some("en".into()),
            rating: Some("PG".into()),
            aspect_ratio: Some("16:9".into()),
            subtitles: Some("CC".into()),
            program_category: Some("Drama".into()),
        }
    }

    #[test]
    fn epg_program_context_resolves_core_and_programme_fields() {
        let program = make_program();
        let ctx = EpgProgramEvalContext::new(&program, None);

        assert_eq!(ctx.get("channel_id").unwrap(), "chan123");
        assert_eq!(ctx.get("programme_title").unwrap(), "Programme Title");
        assert_eq!(
            ctx.get("programme_description").unwrap(),
            "A descriptive text"
        );
        assert_eq!(ctx.get("programme_category").unwrap(), "Drama");
        assert_eq!(
            ctx.get("programme_icon").unwrap(),
            "http://example/icon.png"
        );
        assert_eq!(ctx.get("programme_subtitle").unwrap(), "CC");
        assert_eq!(ctx.get("episode_num").unwrap(), "5");
        assert_eq!(ctx.get("season_num").unwrap(), "2");
        assert_eq!(ctx.get("language").unwrap(), "en");
        assert_eq!(ctx.get("rating").unwrap(), "PG");
        assert_eq!(ctx.get("aspect_ratio").unwrap(), "16:9");
    }

    #[test]
    fn epg_program_context_resolves_source_meta() {
        let program = make_program();
        let meta = SourceMeta::new("EPG Source", "xmltv", "http://egp.example/source.xml");
        let ctx = EpgProgramEvalContext::new(&program, Some(&meta));

        assert_eq!(ctx.get("source_name").unwrap(), "EPG Source");
        assert_eq!(ctx.get("source_type").unwrap(), "xmltv");
        assert_eq!(
            ctx.get("source_url").unwrap(),
            "http://egp.example/source.xml"
        );
    }

    #[test]
    fn epg_program_context_missing_source_meta_is_none() {
        let program = make_program();
        let ctx = EpgProgramEvalContext::new(&program, None);

        assert!(ctx.get("source_name").is_none());
        assert!(ctx.get("source_type").is_none());
        assert!(ctx.get("source_url").is_none());
    }
}
