/*!
 * Streaming Classification Module
 * ===============================
 *
 * Purpose:
 *   Provide a *single* async entry point to classify an upstream channel URL into one of the
 *   hybrid streaming modes described in `TODO-HybridStream.md`:
 *
 *     - PassthroughRawTs
 *     - CollapsedSingleVariantTs
 *     - TransparentHls
 *     - TransparentUnknown
 *
 * Strategy (Auto Mode):
 *   1. Heuristic (extension / content hint) to detect obvious raw TS (.ts) or HLS (.m3u8/.m3u).
 *   2. For probable HLS: fetch (bounded) playlist text and inspect:
 *        * Master playlist? (#EXT-X-STREAM-INF)
 *            - >1 variants => TransparentHls
 *            - ==1 variant => Optionally inspect its media playlist (NOT IMPLEMENTED YET, TODO)
 *        * Media playlist:
 *            - If encrypted (#EXT-X-KEY) or fMP4 (#EXT-X-MAP) or non-.ts segments => TransparentHls
 *            - Else => CollapsedSingleVariantTs
 *   3. Non-matching / failures => TransparentUnknown
 *
 * Raw passthrough override (?format=raw):
 *   - If heuristic identifies Raw TS => PassthroughRawTs
 *   - Else: degrade to auto classification (adds a reason note). The caller can decide whether to
 *           reject with 406 or proceed.
 *
 * NOTE:
 *   - This module is intentionally **pure classification** (no streaming logic).
 *   - No caching / DB hints yet (format_hint integration TODO).
 *   - Master playlist with exactly one variant: we currently treat as TransparentHls (simpler),
 *     but mark in reasons so we can later optionally chase the variant to attempt collapsing.
 *
 * Bounded Playlist Fetch:
 *   To avoid excessive memory, we only pull up to `MAX_PLAYLIST_BYTES` (default 256 KiB).
 *
 * Non-Goals (Phase 1):
 *   - Decryption, fMP4 transmux, discontinuity normalization, multi-audio merging.
 *
 * Public API Surface:
 *   - classify_stream()
 *   - StreamModeDecision
 *   - ClassificationResult
 *   - ClassificationError
 *
 * Tests:
 *   - Provide baseline unit tests with embedded sample playlists (master & media).
 *
 * Future TODOs:
 *   - Integrate channel.format_hint (DB stored).
 *   - Secondary fetch for single-variant master playlists.
 *   - Expose more granular stats (segment count, average duration).
 */

use std::time::Duration;

use reqwest::Client;
use thiserror::Error;
use tracing::debug;

/// Maximum bytes to read from a playlist body (defensive upper bound).
pub const MAX_PLAYLIST_BYTES: usize = 256 * 1024;

/// The final decision about how the server should *serve* this channel.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StreamModeDecision {
    /// Upstream is already a continuous (or single-file) raw MPEG-TS stream.
    PassthroughRawTs,
    /// Upstream is a single-variant HLS TS media playlist that we can poll & concatenate.
    CollapsedSingleVariantTs,
    /// Upstream requires transparent HLS (master with >1 variants, fMP4, encryption, etc.).
    TransparentHls { variant_count: usize },
    /// Catch-all fallback (unknown / failed classification). Safe to just proxy original.
    TransparentUnknown,
}

/// Classification output with diagnostic context.
#[derive(Debug, Clone)]
pub struct ClassificationResult {
    pub decision: StreamModeDecision,
    pub variant_count: Option<usize>,
    pub target_duration: Option<f32>,
    pub reasons: Vec<String>,
    pub is_encrypted: bool,
    pub uses_fmp4: bool,
    pub eligible_for_collapse: bool,
    pub forced_raw_rejected: bool,
    // NEW: Selected media playlist URL when collapsing a master playlist
    pub selected_media_playlist_url: Option<String>,
    // NEW: Variant bandwidth (from #EXT-X-STREAM-INF BANDWIDTH=) if master collapsed
    pub selected_variant_bandwidth: Option<u64>,
    // NEW: Variant resolution (width,height) if available
    pub selected_variant_resolution: Option<(u32, u32)>,
}

/// Error domain for classification. Most errors are *softened* into TransparentUnknown, but
/// callers may want to inspect root causes.
#[derive(Debug, Error)]
pub enum ClassificationError {
    #[error("Invalid URL: {0}")]
    InvalidUrl(String),
    #[error("HTTP request failed: {0}")]
    Http(String),
    #[error("Playlist fetch exceeded size limit ({limit} bytes)")]
    SizeLimit { limit: usize },
    #[error("I/O error: {0}")]
    Io(String),
    #[error("Timeout while fetching playlist")]
    Timeout,
    #[error("Unexpected content while parsing playlist")]
    Parse,
}

/// Parameters controlling classification behavior.
#[derive(Debug, Clone)]
pub struct ClassificationParams<'a> {
    /// Desired output format override: `raw` or `auto` (default).
    pub format: &'a str,
    /// Optional explicit request timeout.
    pub timeout: Duration,
}

impl<'a> Default for ClassificationParams<'a> {
    fn default() -> Self {
        Self {
            format: "auto",
            timeout: Duration::from_secs(6),
        }
    }
}

/// Entrypoint: classify a channel's upstream URL and produce a streaming mode decision.
///
/// - `original_url`: the canonical upstream (before any proxy rewriting).
/// - `client`: a reqwest Client (callers can reuse a global instance).
/// - `params`: formatting + timeout preferences.
///
/// The function *never panics*; in worst case it returns `TransparentUnknown`.
pub async fn classify_stream(
    original_url: &str,
    client: &Client,
    params: ClassificationParams<'_>,
) -> Result<ClassificationResult, ClassificationError> {
    let mut reasons = Vec::new();
    let normalized = strip_query_and_fragment(original_url);

    // Format override semantics
    let forcing_raw = params.format.eq_ignore_ascii_case("raw");
    if forcing_raw {
        reasons.push("Format override requested: raw".into());
    }

    // Heuristic extension classification
    let ext_kind = classify_by_extension(&normalized);
    if let Some(kind) = ext_kind {
        reasons.push(format!("Heuristic extension classification: {kind:?}"));

        match kind {
            HeuristicKind::RawTs => {
                // Raw TS fits directly if raw override or auto
                let decision = StreamModeDecision::PassthroughRawTs;
                debug!(
                    target = "stream.classify",
                    url = original_url,
                    mode = "passthrough-raw-ts",
                    "classification complete"
                );
                return Ok(ClassificationResult {
                    decision,
                    variant_count: None,
                    target_duration: None,
                    reasons,
                    is_encrypted: false,
                    uses_fmp4: false,
                    eligible_for_collapse: false,
                    forced_raw_rejected: false,
                    selected_media_playlist_url: None,
                    selected_variant_bandwidth: None,
                    selected_variant_resolution: None,
                });
            }
            HeuristicKind::HlsPlaylist => {
                // Need to fetch to decide media vs master
            }
            HeuristicKind::Progressive => {
                reasons.push(
                    "Progressive container detected; falling back to TransparentUnknown".into(),
                );
                return Ok(ClassificationResult {
                    decision: StreamModeDecision::TransparentUnknown,
                    variant_count: None,
                    target_duration: None,
                    reasons,
                    is_encrypted: false,
                    uses_fmp4: false,
                    eligible_for_collapse: false,
                    forced_raw_rejected: forcing_raw, // cannot honor raw
                    selected_media_playlist_url: None,
                    selected_variant_bandwidth: None,
                    selected_variant_resolution: None,
                });
            }
        }
    } else {
        reasons.push(
            "No definitive extension heuristic; continuing sniff (playlist fetch if plausible)"
                .into(),
        );
    }

    // If extension didn't scream HLS, but raw forced -> we cannot serve raw.
    let mut forced_raw_rejected = false;
    if forcing_raw && !matches!(ext_kind, Some(HeuristicKind::RawTs)) {
        forced_raw_rejected = true;
        reasons.push(
            "Raw format override not compatible with detected kind; continuing with auto logic"
                .into(),
        );
    }

    // If we strongly did NOT classify as HLS, skip network call and return TransparentUnknown.
    if !looks_like_hls_path(&normalized) {
        reasons
            .push("Path does not resemble HLS (.m3u8/.m3u); returning TransparentUnknown".into());
        return Ok(ClassificationResult {
            decision: StreamModeDecision::TransparentUnknown,
            variant_count: None,
            target_duration: None,
            reasons,
            is_encrypted: false,
            uses_fmp4: false,
            eligible_for_collapse: false,
            forced_raw_rejected,
            selected_media_playlist_url: None,
            selected_variant_bandwidth: None,
            selected_variant_resolution: None,
        });
    }

    // Fetch playlist (bounded)
    let fetch_res = fetch_playlist_bounded(client, original_url, params.timeout).await;
    let playlist_text = match fetch_res {
        Ok(t) => {
            reasons.push(format!(
                "Fetched playlist ({} bytes, truncated={})",
                t.len(),
                if t.len() >= MAX_PLAYLIST_BYTES {
                    "yes"
                } else {
                    "no"
                }
            ));
            t
        }
        Err(e) => {
            reasons.push(format!("Failed to fetch playlist: {e}"));
            let result = ClassificationResult {
                decision: StreamModeDecision::TransparentUnknown,
                variant_count: None,
                target_duration: None,
                reasons,
                is_encrypted: false,
                uses_fmp4: false,
                eligible_for_collapse: false,
                forced_raw_rejected,
                selected_media_playlist_url: None,
                selected_variant_bandwidth: None,
                selected_variant_resolution: None,
            };
            crate::streaming::metrics::metrics()
                .classification_total
                .add(
                    1,
                    &[crate::streaming::KeyValue::new(
                        "decision",
                        "transparent-unknown",
                    )],
                );
            return Ok(result);
        }
    };

    // Parse playlist lines
    let parse = analyze_playlist(&playlist_text);
    reasons.extend(parse.reasons.clone());

    // Master playlist case
    if parse.is_master {
        let variant_count = parse.variant_count.unwrap_or(0);
        if variant_count == 0 {
            reasons.push(
                "Master indicator present but found 0 variants -> defaulting to TransparentHls"
                    .into(),
            );
        } else if variant_count == 1 {
            reasons.push(
                "Single-variant master playlist (future optimization: chase variant).".into(),
            );
        } else {
            reasons.push(format!(
                "Multi-variant master ({} variants).",
                variant_count
            ));
        }

        debug!(
            target = "stream.classify",
            url = original_url,
            mode = "transparent-hls",
            variants = variant_count,
            encrypted = parse.is_encrypted,
            fmp4 = parse.uses_fmp4,
            "classification complete (master playlist)"
        );
        // Master playlist path (variant selection attempt):
        // Try to collapse by selecting highest bandwidth *TS-only* unencrypted variant.
        {
            let master_text = playlist_text.clone();
            let variants = parse_master_variants(&master_text);
            if !variants.is_empty() {
                // Sort by bandwidth desc
                let mut sorted = variants;
                sorted.sort_by(|a, b| b.bandwidth.cmp(&a.bandwidth));
                for v in sorted {
                    let abs_url = absolutize_segment(&normalized, &v.uri);
                    reasons.push(format!(
                        "Probing master variant bw={} res={:?} {}",
                        v.bandwidth, v.resolution, abs_url
                    ));
                    match fetch_playlist_bounded(client, &abs_url, params.timeout).await {
                        Ok(v_text) => {
                            let v_analysis = analyze_playlist(&v_text);
                            if !v_analysis.is_media {
                                reasons.push("Variant not media playlist – skip".into());
                                continue;
                            }
                            if v_analysis.is_encrypted
                                || v_analysis.uses_fmp4
                                || !v_analysis.all_segments_ts
                                || v_analysis.segment_count == 0
                            {
                                reasons.push(format!(
                                    "Variant ineligible enc={} fmp4={} all_ts={} segs={}",
                                    v_analysis.is_encrypted,
                                    v_analysis.uses_fmp4,
                                    v_analysis.all_segments_ts,
                                    v_analysis.segment_count
                                ));
                                continue;
                            }
                            reasons.push("Selected variant for collapsing".into());
                            let result = ClassificationResult {
                                decision: StreamModeDecision::CollapsedSingleVariantTs,
                                variant_count: Some(variant_count),
                                target_duration: v_analysis.target_duration,
                                reasons,
                                is_encrypted: false,
                                uses_fmp4: false,
                                eligible_for_collapse: true,
                                forced_raw_rejected,
                                selected_media_playlist_url: Some(abs_url),
                                selected_variant_bandwidth: Some(v.bandwidth),
                                selected_variant_resolution: v.resolution,
                            };
                            // Metrics: collapsed
                            crate::streaming::metrics::metrics()
                                .classification_total
                                .add(
                                    1,
                                    &[crate::streaming::KeyValue::new("decision", "hls-to-ts")],
                                );
                            return Ok(result);
                        }
                        Err(e) => {
                            reasons.push(format!("Variant fetch error: {e} – skip"));
                            continue;
                        }
                    }
                }
                // No eligible variant
                reasons.push("unsupported-non-ts (no TS-only unencrypted variant)".into());
            } else {
                reasons.push("No parsable variants extracted".into());
            }
        }
        let result = ClassificationResult {
            decision: StreamModeDecision::TransparentHls { variant_count },
            variant_count: Some(variant_count),
            target_duration: parse.target_duration,
            reasons,
            is_encrypted: parse.is_encrypted,
            uses_fmp4: parse.uses_fmp4,
            eligible_for_collapse: false,
            forced_raw_rejected,
            selected_media_playlist_url: None,
            selected_variant_bandwidth: None,
            selected_variant_resolution: None,
        };
        crate::streaming::metrics::metrics()
            .classification_total
            .add(
                1,
                &[crate::streaming::KeyValue::new("decision", "passthrough")],
            );
        if result
            .reasons
            .iter()
            .any(|r| r.contains("unsupported-non-ts"))
        {
            crate::streaming::metrics::metrics()
                .classification_fallback_total
                .add(
                    1,
                    &[crate::streaming::KeyValue::new(
                        "reason",
                        "unsupported-non-ts",
                    )],
                );
        }
        return Ok(result);
    }

    // Media playlist
    if parse.is_media {
        if parse.is_encrypted {
            reasons.push("Encrypted media playlist => TransparentHls".into());
            return Ok(ClassificationResult {
                decision: StreamModeDecision::TransparentHls { variant_count: 1 },
                variant_count: Some(1),
                target_duration: parse.target_duration,
                reasons,
                is_encrypted: true,
                uses_fmp4: parse.uses_fmp4,
                eligible_for_collapse: false,
                forced_raw_rejected,
                selected_media_playlist_url: None,
                selected_variant_bandwidth: None,
                selected_variant_resolution: None,
            });
        }
        if parse.uses_fmp4 {
            reasons.push("fMP4 (EXT-X-MAP) detected => TransparentHls".into());
            return Ok(ClassificationResult {
                decision: StreamModeDecision::TransparentHls { variant_count: 1 },
                variant_count: Some(1),
                target_duration: parse.target_duration,
                reasons,
                is_encrypted: false,
                uses_fmp4: true,
                eligible_for_collapse: false,
                forced_raw_rejected,
                selected_media_playlist_url: None,
                selected_variant_bandwidth: None,
                selected_variant_resolution: None,
            });
        }
        if !parse.all_segments_ts {
            reasons.push("Not all media segments end with .ts => TransparentHls".into());
            return Ok(ClassificationResult {
                decision: StreamModeDecision::TransparentHls { variant_count: 1 },
                variant_count: Some(1),
                target_duration: parse.target_duration,
                reasons,
                is_encrypted: false,
                uses_fmp4: parse.uses_fmp4,
                eligible_for_collapse: false,
                forced_raw_rejected,
                selected_media_playlist_url: None,
                selected_variant_bandwidth: None,
                selected_variant_resolution: None,
            });
        }
        if parse.segment_count == 0 {
            reasons.push("No segments found in media playlist => TransparentHls".into());
            return Ok(ClassificationResult {
                decision: StreamModeDecision::TransparentHls { variant_count: 1 },
                variant_count: Some(1),
                target_duration: parse.target_duration,
                reasons,
                is_encrypted: false,
                uses_fmp4: parse.uses_fmp4,
                eligible_for_collapse: false,
                forced_raw_rejected,
                selected_media_playlist_url: None,
                selected_variant_bandwidth: None,
                selected_variant_resolution: None,
            });
        }

        // Collapsible!
        reasons
            .push("Eligible single-variant TS media playlist => CollapsedSingleVariantTs".into());
        debug!(target = "stream.classify", url = original_url, mode = "collapsed-single-variant-ts", target_duration = ?parse.target_duration, "classification complete (collapsible media playlist)");
        return Ok(ClassificationResult {
            decision: StreamModeDecision::CollapsedSingleVariantTs,
            variant_count: Some(1),
            target_duration: parse.target_duration,
            reasons,
            is_encrypted: false,
            uses_fmp4: false,
            eligible_for_collapse: true,
            forced_raw_rejected,
            selected_media_playlist_url: None,
            selected_variant_bandwidth: None,
            selected_variant_resolution: None,
        });
    }

    // Fallback
    reasons.push("Playlist analysis inconclusive => TransparentUnknown".into());
    debug!(
        target = "stream.classify",
        url = original_url,
        mode = "transparent-unknown",
        "classification fallback (inconclusive)"
    );
    {
        let result = ClassificationResult {
            decision: StreamModeDecision::TransparentUnknown,
            variant_count: None,
            target_duration: None,
            reasons,
            is_encrypted: false,
            uses_fmp4: false,
            eligible_for_collapse: false,
            forced_raw_rejected,
            selected_media_playlist_url: None,
            selected_variant_bandwidth: None,
            selected_variant_resolution: None,
        };
        crate::streaming::metrics::metrics()
            .classification_total
            .add(
                1,
                &[crate::streaming::KeyValue::new(
                    "decision",
                    "transparent-unknown",
                )],
            );
        Ok(result)
    }
}

/* -----------------------------
 * Heuristic Helpers
 * --------------------------- */

#[derive(Debug, Clone, Copy)]
enum HeuristicKind {
    RawTs,
    HlsPlaylist,
    Progressive,
}

fn strip_query_and_fragment(url: &str) -> String {
    let mut end = url.len();
    if let Some(pos) = url.find(['?', '#']) {
        end = pos;
    }
    url[..end].to_string()
}

fn classify_by_extension(base: &str) -> Option<HeuristicKind> {
    let lower = base.to_lowercase();
    if lower.ends_with(".ts") {
        return Some(HeuristicKind::RawTs);
    }
    if lower.ends_with(".m3u8") || lower.ends_with(".m3u") {
        return Some(HeuristicKind::HlsPlaylist);
    }
    if lower.ends_with(".mp4") || lower.ends_with(".m4v") || lower.ends_with(".mov") {
        return Some(HeuristicKind::Progressive);
    }
    None
}

fn looks_like_hls_path(base: &str) -> bool {
    let lower = base.to_lowercase();
    lower.ends_with(".m3u8") || lower.ends_with(".m3u")
}

/* -----------------------------
 * Playlist Fetch
 * --------------------------- */

async fn fetch_playlist_bounded(
    client: &Client,
    url: &str,
    timeout: Duration,
) -> Result<String, ClassificationError> {
    let resp = client
        .get(url)
        .timeout(timeout)
        .send()
        .await
        .map_err(|e| ClassificationError::Http(e.to_string()))?;

    if !resp.status().is_success() {
        return Err(ClassificationError::Http(format!(
            "Non-success status: {}",
            resp.status()
        )));
    }

    // Stream body in chunks, limit total
    let mut body = resp.bytes_stream();
    use futures::StreamExt;
    let mut collected: Vec<u8> = Vec::with_capacity(8192);
    while let Some(chunk) = body.next().await {
        let chunk = chunk.map_err(|e| ClassificationError::Io(e.to_string()))?;
        if collected.len() + chunk.len() > MAX_PLAYLIST_BYTES {
            // Truncate (we can still parse partial possibly)
            collected.extend_from_slice(&chunk[..(MAX_PLAYLIST_BYTES - collected.len())]);
            break;
        }
        collected.extend_from_slice(&chunk);
    }

    let text = String::from_utf8_lossy(&collected).to_string();
    Ok(text)
}

/* -----------------------------
 * Playlist Analysis (Very Lightweight Parser)
 * --------------------------- */

#[derive(Debug, Default)]
struct PlaylistAnalysis {
    reasons: Vec<String>,
    is_master: bool,
    is_media: bool,
    variant_count: Option<usize>,
    target_duration: Option<f32>,
    is_encrypted: bool,
    uses_fmp4: bool,
    all_segments_ts: bool,
    segment_count: usize,
}

fn analyze_playlist(text: &str) -> PlaylistAnalysis {
    let mut a = PlaylistAnalysis {
        all_segments_ts: true, // assume true until disproven
        ..Default::default()
    };

    let mut saw_extm3u = false;
    let mut variant_count = 0usize;
    let mut segment_count = 0usize;

    for raw_line in text.lines() {
        let line = raw_line.trim();
        if line.is_empty() {
            continue;
        }
        if line.starts_with("#EXTM3U") {
            saw_extm3u = true;
            continue;
        }
        if line.starts_with("#EXT-X-STREAM-INF") {
            a.is_master = true;
            variant_count += 1;
            continue;
        }
        if line.starts_with("#EXT-X-TARGETDURATION:") {
            if let Some(v) = line
                .split_once(':')
                .and_then(|(_, val)| val.parse::<f32>().ok())
            {
                a.target_duration = Some(v);
            }
            continue;
        }
        if line.starts_with("#EXT-X-KEY") {
            a.is_encrypted = true;
            continue;
        }
        if line.starts_with("#EXT-X-MAP") {
            a.uses_fmp4 = true;
            continue;
        }
        if line.starts_with('#') {
            // other tags ignored
            continue;
        }

        // Segment URI line (media playlist)
        a.is_media = true;
        segment_count += 1;
        if a.all_segments_ts {
            let stripped = strip_query_and_fragment(line);
            if !stripped.to_lowercase().ends_with(".ts") {
                a.all_segments_ts = false;
            }
        }
    }

    if !saw_extm3u {
        a.reasons
            .push("Missing #EXTM3U tag — not a valid HLS playlist (possibly truncated)".into());
    }

    if a.is_master {
        a.variant_count = Some(variant_count);
        a.reasons.push(format!(
            "Detected master playlist with {variant_count} variant(s)."
        ));
    } else if a.is_media {
        a.reasons.push(format!(
            "Detected media playlist with {segment_count} segment(s)."
        ));
    } else {
        a.reasons
            .push("No master or media markers found — ambiguous content.".into());
    }

    a.segment_count = segment_count;
    a
}

/* -----------------------------
 * URL Helper
 * --------------------------- */

/// Best-effort absolutization of a segment or variant URI relative to a playlist URL.
/// Handles:
///   - Absolute (http/https) -> returned unchanged
///   - Root-relative (/path) -> scheme + host from base + path
///   - Relative (segment.ts)  -> base directory + segment
fn absolutize_segment(base_playlist_url: &str, segment: &str) -> String {
    if segment.starts_with("http://") || segment.starts_with("https://") {
        return segment.to_string();
    }
    // Parse base
    if let Ok(base) = url::Url::parse(base_playlist_url) {
        if let Some(stripped) = segment.strip_prefix('/') {
            if let Ok(base_root) = base.join("/") {
                // Replace path with root then join segment
                if let Ok(final_url) = base_root.join(stripped) {
                    return final_url.to_string();
                }
            }
            // Fallback manual
            return format!(
                "{}://{}{}",
                base.scheme(),
                base.host_str().unwrap_or(""),
                stripped
            );
        } else {
            // Relative
            if let Ok(joined) = base.join(segment) {
                return joined.to_string();
            }
        }
    } else {
        // Very defensive fallback (string ops)
        if let Some(pos) = base_playlist_url.rfind('/') {
            return format!("{}{}", &base_playlist_url[..=pos], segment);
        }
    }
    segment.to_string()
}

/* -----------------------------
 * Tests
 * --------------------------- */

// Master variant parsing helper (moved out of test cfg for runtime use)
#[derive(Debug)]
pub(crate) struct MasterVariant {
    bandwidth: u64,
    resolution: Option<(u32, u32)>,
    uri: String,
}

pub(crate) fn parse_master_variants(text: &str) -> Vec<MasterVariant> {
    let mut out = Vec::new();
    let mut pending: Option<(u64, Option<(u32, u32)>)> = None;
    for raw in text.lines() {
        let line = raw.trim();
        if let Some(attrs) = line.strip_prefix("#EXT-X-STREAM-INF:") {
            let mut bw: u64 = 0;
            let mut res: Option<(u32, u32)> = None;
            for part in attrs.split(',') {
                let kv = part.trim();
                if let Some(v) = kv.strip_prefix("BANDWIDTH=") {
                    if let Ok(p) = v.parse::<u64>() {
                        bw = p;
                    }
                } else if let Some(v) = kv.strip_prefix("RESOLUTION=") {
                    if let Some((w, h)) = v.split_once('x') {
                        if let (Ok(wi), Ok(hi)) = (w.parse::<u32>(), h.parse::<u32>()) {
                            res = Some((wi, hi));
                        }
                    }
                }
            }
            pending = Some((bw, res));
            continue;
        }
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some((bw, res)) = pending.take() {
            out.push(MasterVariant {
                bandwidth: bw,
                resolution: res,
                uri: line.to_string(),
            });
        }
    }
    out
}
// (tests removed during refactor – classification now depends on runtime network behavior and
// simplified public surface; reintroduce focused tests in a separate integration module if needed)
//
// New focused unit tests (parse-only) for master variant extraction logic.
// These do NOT hit the network or exercise full classification; they ensure that
// `parse_master_variants` correctly extracts bandwidth, resolution, and URI in
// source order (sorting for selection happens later inside `classify_stream`).
#[cfg(test)]
mod variant_parse_tests {
    use super::parse_master_variants;

    #[test]
    fn master_variant_parse_orders_in_source_sequence() {
        let playlist = r#"#EXTM3U
#EXT-X-VERSION:4
#EXT-X-STREAM-INF:BANDWIDTH=800000,RESOLUTION=640x360
low/playlist.m3u8
#EXT-X-STREAM-INF:BANDWIDTH=1600000,RESOLUTION=1280x720
mid/playlist.m3u8
#EXT-X-STREAM-INF:BANDWIDTH=4000000,RESOLUTION=1920x1080
hi/playlist.m3u8
"#;
        let variants = parse_master_variants(playlist);
        assert_eq!(variants.len(), 3, "expected three variants");
        // parse_master_variants preserves textual order
        assert_eq!(variants[0].bandwidth, 800000);
        assert_eq!(variants[0].resolution, Some((640, 360)));
        assert_eq!(variants[1].bandwidth, 1600000);
        assert_eq!(variants[1].resolution, Some((1280, 720)));
        assert_eq!(variants[2].bandwidth, 4000000);
        assert_eq!(variants[2].resolution, Some((1920, 1080)));
    }

    #[test]
    fn master_variant_parse_handles_missing_resolution() {
        let playlist = r#"#EXTM3U
#EXT-X-STREAM-INF:BANDWIDTH=500000
a/playlist.m3u8
#EXT-X-STREAM-INF:BANDWIDTH=600000,RESOLUTION=800x600
b/playlist.m3u8
"#;
        let variants = parse_master_variants(playlist);
        assert_eq!(variants.len(), 2, "expected two variants");
        assert_eq!(variants[0].bandwidth, 500000);
        assert!(
            variants[0].resolution.is_none(),
            "first variant should have no resolution"
        );
        assert_eq!(variants[1].bandwidth, 600000);
        assert_eq!(variants[1].resolution, Some((800, 600)));
    }

    #[test]
    fn master_variant_parse_ignores_non_stream_inf_blocks() {
        let playlist = r#"#EXTM3U
#EXT-X-VERSION:4
#EXT-X-INDEPENDENT-SEGMENTS
#EXT-X-STREAM-INF:BANDWIDTH=1000000
one/playlist.m3u8
#COMMENT
#EXTINF:6.0,
segment.ts
#EXT-X-STREAM-INF:BANDWIDTH=2000000,RESOLUTION=1024x576
two/playlist.m3u8
"#;
        let variants = parse_master_variants(playlist);
        assert_eq!(variants.len(), 2);
        assert_eq!(variants[0].bandwidth, 1000000);
        assert_eq!(variants[1].bandwidth, 2000000);
        assert_eq!(variants[1].resolution, Some((1024, 576)));
    }
}
