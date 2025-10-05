//! Unified HTTP stream proxy utilities.
//!
//! This module provides a single streaming proxy implementation used by:
//!   - Direct channel streaming
//!   - Proxy (SeaORM) mode streaming (raw passthrough or classified passthrough)
//!   - Collapsing classification caller paths (when not collapsing, still proxying)
//!   - Relay mode (for attaching uniform headers around relay-served bodies, though
//!     relay content itself is sourced elsewhere)
//!
//! Key behaviors:
//!   - No total request timeout (live streams must remain open).
//!   - Configurable connect timeout via `web.proxy_upstream_connect_timeout` (default 15s).
//!   - Normalized User-Agent rewriting:
//!     If client supplies UA -> `m3u-proxy/<version> (<original>)`
//!     Else -> use configured `web.user_agent` (which already holds a sensible default).
//!   - Adds `m3u-proxy-version` header upstream (hyphenated; HTTP header names cannot contain '/').
//!   - Optional uniform response headers added via `StreamHeaderMeta`.
//!
//! This keeps the proxy logic DRY and consistent across handlers.

use std::sync::Arc;
use std::time::Duration;

use axum::body::Body;
use axum::http::{HeaderMap, HeaderValue, Response, StatusCode, header};
use futures::StreamExt;
use reqwest::Client;
use tracing::{debug, error, info};

/// Metadata used to decorate the outgoing proxied response with normalized headers.
/// All fields are optional; only present values are emitted.
#[derive(Debug, Clone, Default)]
pub struct StreamHeaderMeta {
    pub origin_kind: Option<String>, // RAW_TS | HLS_PLAYLIST | UNKNOWN | RELAY
    pub decision: Option<String>,    // normalized decision label
    pub mode: Option<String>,        // passthrough | hls-to-ts | relay
    pub variant_count: Option<usize>,
    pub variant_bandwidth: Option<u64>,
    pub variant_resolution: Option<(u32, u32)>,
    pub target_duration: Option<f32>,
    pub fallback: Option<String>, // forced-raw-rejected | unsupported-non-ts | etc.
    pub relay_profile_id: Option<String>, // when in relay mode
}

impl StreamHeaderMeta {
    pub fn is_empty(&self) -> bool {
        self.origin_kind.is_none()
            && self.decision.is_none()
            && self.mode.is_none()
            && self.variant_count.is_none()
            && self.variant_bandwidth.is_none()
            && self.variant_resolution.is_none()
            && self.target_duration.is_none()
            && self.fallback.is_none()
            && self.relay_profile_id.is_none()
    }
}

/// Apply uniform stream headers to a response.
/// Does not overwrite existing header values if already set (idempotent safety).
pub fn apply_uniform_stream_headers(resp: &mut Response<Body>, meta: &StreamHeaderMeta) {
    let headers = resp.headers_mut();

    // Always expose headers for browser-based clients
    headers
        .entry(header::ACCESS_CONTROL_EXPOSE_HEADERS)
        .or_insert_with(|| HeaderValue::from_static("*"));

    if let Some(kind) = &meta.origin_kind {
        insert_if_absent(headers, "X-Stream-Origin-Kind", kind);
    }
    if let Some(decision) = &meta.decision {
        insert_if_absent(headers, "X-Stream-Decision", decision);
    }
    if let Some(mode) = &meta.mode {
        insert_if_absent(headers, "X-Stream-Mode", mode);
    }
    if let Some(vc) = meta.variant_count {
        insert_if_absent(headers, "X-Variant-Count", &vc.to_string());
    }
    if let Some(bw) = meta.variant_bandwidth {
        insert_if_absent(headers, "X-Variant-Bandwidth", &bw.to_string());
    }
    if let Some((w, h)) = meta.variant_resolution {
        insert_if_absent(headers, "X-Variant-Resolution", &format!("{}x{}", w, h));
    }
    if let Some(td) = meta.target_duration {
        insert_if_absent(headers, "X-Target-Duration", &format!("{:.3}", td));
    }
    if let Some(fb) = &meta.fallback {
        insert_if_absent(headers, "X-Stream-Fallback", fb);
    }
    if let Some(relay_id) = &meta.relay_profile_id {
        insert_if_absent(headers, "X-Relay-Profile-ID", relay_id);
    }
}

fn insert_if_absent(headers: &mut HeaderMap, name: &str, value: &str) {
    if !headers.contains_key(name) {
        if let Ok(v) = HeaderValue::from_str(value) {
            if let Ok(hn) = header::HeaderName::from_bytes(name.as_bytes()) {
                headers.insert(hn, v);
            }
        }
    }
}

/// Build the upstream User-Agent according to spec:
///  - If client supplied UA -> "m3u-proxy/<version> (<original>)"
///  - Else -> configured web.user_agent (already versioned)
///    Adds a side header `m3u-proxy/version` for observability.
///    Returns (final_user_agent_string, version).
fn build_upstream_user_agent(
    request_headers: &HeaderMap,
    config: &crate::config::Config,
) -> (String, String) {
    static VERSION: &str = env!("CARGO_PKG_VERSION");
    let configured = config.web.user_agent.trim();
    let original = request_headers
        .get(header::USER_AGENT)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.trim())
        .filter(|s| !s.is_empty());

    let final_ua = if let Some(orig) = original {
        format!("m3u-proxy/{VERSION} ({orig})")
    } else if !configured.is_empty() {
        configured.to_string()
    } else {
        format!("m3u-proxy/{VERSION}")
    };

    (final_ua, VERSION.to_string())
}

/// Unified stream proxy function.
/// - Establishes upstream connection (connect timeout only).
/// - Streams body indefinitely (no total timeout).
/// - Tracks bytes served via `SessionTracker`.
/// - Optionally decorates response with uniform stream headers (meta).
///
/// On failure, ends the session and returns an error response.
#[allow(clippy::too_many_arguments)]
pub async fn proxy_http_stream(
    stream_url: &str,
    request_headers: &HeaderMap,
    app_config: &crate::config::Config,
    session_tracker: Arc<crate::proxy::session_tracker::SessionTracker>,
    session_stats: crate::proxy::session_tracker::SessionStats,
    meta: Option<StreamHeaderMeta>,
) -> Response<Body> {
    info!("Proxying upstream stream: {}", stream_url);

    // Compose UA
    let (user_agent, version) = build_upstream_user_agent(request_headers, app_config);

    let connect_timeout: Duration = app_config.web.proxy_upstream_connect_timeout_duration();

    let client = match Client::builder()
        .user_agent(user_agent)
        .connect_timeout(connect_timeout)
        .pool_max_idle_per_host(8)
        .build()
    {
        Ok(c) => c,
        Err(e) => {
            error!("Failed to build reqwest client: {}", e);
            session_tracker.end_session(&session_stats.session_id).await;
            return error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Failed to initialize upstream client",
            );
        }
    };

    // Prepare minimal header forwarding (optional extension)
    let mut forwarded = reqwest::header::HeaderMap::new();
    // Always send version header for observability (use hyphen, '/' is invalid in header names)
    if let Ok(v) = reqwest::header::HeaderValue::from_str(&version) {
        forwarded.insert(
            reqwest::header::HeaderName::from_static("m3u-proxy-version"),
            v,
        );
    }

    // (Optional) Range header forwarding for partial media files if present
    if let Some(range) = request_headers.get(header::RANGE) {
        if let Ok(v) = reqwest::header::HeaderValue::from_bytes(range.as_bytes()) {
            forwarded.insert(header::RANGE, v);
        }
    }

    let upstream_resp = match client.get(stream_url).headers(forwarded).send().await {
        Ok(r) => r,
        Err(e) => {
            error!("Failed to connect to upstream {}: {}", stream_url, e);
            session_tracker.end_session(&session_stats.session_id).await;
            return error_response(StatusCode::BAD_GATEWAY, "Failed to connect to upstream");
        }
    };

    let status = upstream_resp.status();
    if !status.is_success() {
        error!(
            "Upstream responded with error status {} for {}",
            status, stream_url
        );
        session_tracker.end_session(&session_stats.session_id).await;
        return error_response(StatusCode::BAD_GATEWAY, "Upstream error status");
    }

    let upstream_headers = upstream_resp.headers().clone();
    let content_type = upstream_headers
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("video/mp2t")
        .to_string();

    // Attempt playlist rewriting via helper (secondary fetch) without consuming primary response.
    if let Some(rewritten_resp) = attempt_rewrite_hls_playlist(
        &client,
        stream_url,
        &content_type,
        &meta,
        &version,
        &session_tracker,
        &session_stats,
    )
    .await
    {
        return rewritten_resp;
    }

    let content_length = upstream_headers
        .get(header::CONTENT_LENGTH)
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.parse::<u64>().ok());

    debug!(
        "Upstream accepted: ct={} cl={:?} url={}",
        content_type, content_length, stream_url
    );

    // Convert reqwest stream -> tracked stream -> axum body
    let byte_counter_session_id = session_stats.session_id.clone();
    let tracker_clone = session_tracker.clone();

    let byte_stream = upstream_resp.bytes_stream().map(move |chunk_result| {
        if let Ok(ref chunk) = chunk_result {
            let len = chunk.len() as u64;
            let tracker = tracker_clone.clone();
            let session_id_clone = byte_counter_session_id.clone();
            // Fire-and-forget update
            tokio::spawn(async move {
                tracker.update_session_bytes(&session_id_clone, len).await;
            });
        }
        chunk_result
    });

    let body = Body::from_stream(byte_stream);

    let mut builder = Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, content_type)
        .header(header::CACHE_CONTROL, "no-cache")
        .header(header::ACCESS_CONTROL_ALLOW_ORIGIN, "*")
        .header(header::ACCESS_CONTROL_ALLOW_METHODS, "GET, OPTIONS, HEAD")
        .header(
            header::ACCESS_CONTROL_ALLOW_HEADERS,
            "Content-Type, Range, Accept",
        )
        .header("m3u-proxy-version", version);

    if let Some(len) = content_length {
        builder = builder.header(header::CONTENT_LENGTH, len);
    }

    let mut response = match builder.body(body) {
        Ok(r) => r,
        Err(e) => {
            error!("Failed building response object: {}", e);
            session_tracker.end_session(&session_stats.session_id).await;
            return error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Failed to build response",
            );
        }
    };

    if let Some(meta) = &meta {
        if !meta.is_empty() {
            apply_uniform_stream_headers(&mut response, meta);
        }
    }

    info!("Streaming proxy established for {}", stream_url);
    response
}

/// Simple helper to construct a uniform error response.
async fn attempt_rewrite_hls_playlist(
    client: &Client,
    stream_url: &str,
    content_type: &str,
    meta: &Option<StreamHeaderMeta>,
    version: &str,
    session_tracker: &Arc<crate::proxy::session_tracker::SessionTracker>,
    session_stats: &crate::proxy::session_tracker::SessionStats,
) -> Option<Response<Body>> {
    let origin_kind_is_hls = meta
        .as_ref()
        .and_then(|m| m.origin_kind.as_deref())
        .map(|k| k == "HLS_PLAYLIST")
        .unwrap_or(false);
    let is_playlist = origin_kind_is_hls
        && (content_type.to_ascii_lowercase().contains("mpegurl")
            || stream_url.to_ascii_lowercase().ends_with(".m3u8"));
    if !is_playlist {
        return None;
    }

    let rewrite_resp = client.get(stream_url).send().await.ok()?;
    if !rewrite_resp.status().is_success() {
        debug!(
            "Playlist rewrite helper: secondary fetch status {} for {}",
            rewrite_resp.status(),
            stream_url
        );
        return None;
    }
    let raw_playlist = rewrite_resp.text().await.ok()?;
    let base = match url::Url::parse(stream_url) {
        Ok(b) => b,
        Err(_) => {
            debug!("Playlist rewrite helper: base parse failed {}", stream_url);
            return None;
        }
    };

    let mut rewritten = String::with_capacity(raw_playlist.len() + 128);
    for line in raw_playlist.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty()
            || trimmed.starts_with('#')
            || trimmed.starts_with("http://")
            || trimmed.starts_with("https://")
        {
            rewritten.push_str(line);
            rewritten.push('\n');
            continue;
        }
        if let Ok(joined) = base.join(trimmed) {
            rewritten.push_str(joined.as_str());
            rewritten.push('\n');
        } else {
            rewritten.push_str(line);
            rewritten.push('\n');
        }
    }

    let bytes = rewritten.into_bytes();
    let len = bytes.len();

    let builder = Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "application/vnd.apple.mpegurl")
        .header(header::CACHE_CONTROL, "no-cache")
        .header(header::ACCESS_CONTROL_ALLOW_ORIGIN, "*")
        .header(header::ACCESS_CONTROL_ALLOW_METHODS, "GET, OPTIONS, HEAD")
        .header(
            header::ACCESS_CONTROL_ALLOW_HEADERS,
            "Content-Type, Range, Accept",
        )
        .header("m3u-proxy-version", version)
        .header(header::CONTENT_LENGTH, len.to_string())
        .header("X-Playlist-Rewritten", "absolute-uris");

    let mut response = match builder.body(Body::from(bytes)) {
        Ok(r) => r,
        Err(e) => {
            error!("Playlist rewrite helper: build response failed: {e}");
            session_tracker.end_session(&session_stats.session_id).await;
            return None;
        }
    };

    if let Some(m) = meta {
        if !m.is_empty() {
            apply_uniform_stream_headers(&mut response, m);
        }
    }

    info!(
        "Playlist rewrite helper: served absolute-URI playlist for {}",
        stream_url
    );
    Some(response)
}

fn error_response(status: StatusCode, msg: &str) -> Response<Body> {
    Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, "text/plain; charset=utf-8")
        .header(header::ACCESS_CONTROL_ALLOW_ORIGIN, "*")
        .body(Body::from(msg.to_string()))
        .unwrap_or_else(|_| {
            Response::builder()
                .status(StatusCode::INTERNAL_SERVER_ERROR)
                .body(Body::from("internal error"))
                .unwrap()
        })
}
