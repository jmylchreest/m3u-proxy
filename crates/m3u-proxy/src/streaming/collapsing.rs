/*!
 * collapsing.rs
 * =============
 * Phase 1 scaffolding for the **Collapsed Single-Variant HLS → Continuous TS** mode.
 *
 * Goal (current):
 *   Provide a minimal, composable async component that:
 *     1. Polls a single *media* HLS playlist (already classified as single-variant & TS segments).
 *     2. Sequentially fetches *new* `.ts` segments.
 *     3. Streams raw segment bytes out as a unified byte stream (channel-based).
 *     4. Emits concise debug-level tracing about decisions (poll intervals, segments appended, gaps).
 *
 * Non-Goals (Phase 1):
 *   - Multi-subscriber fan-out (only single consumer supported now).
 *   - Discontinuity normalization (PTS / continuity counters).
 *   - PAT/PMT reinsertion / TS inspection.
 *   - Error recovery beyond simple retry/backoff.
 *   - Handling encrypted or fMP4 playlists (these must be filtered by classification before use here).
 *
 * Future Enhancements (see TODO-HybridStream.md):
 *   - Broadcast channel for multi-client subscription.
 *   - Metrics (Prometheus counters / histograms).
 *   - Adaptive poll interval tuning & jitter.
 *   - Resilience policies (segment fetch retry matrix, stale detection escalation).
 *   - Graceful fallback to transparent HLS if repeated failures exceed threshold.
 *   - PAT/PMT reinforcement & discontinuity introspection.
 *
 * Usage Outline:
 * ```
 * let handle = CollapsingSession::spawn(
 *     Arc::new(reqwest::Client::new()),
 *     CollapsingConfig::default(),
 *     playlist_url,
 *     initial_target_duration, // Option<f32>
 * );
 * // Consume bytes:
 * while let Some(chunk) = handle.next().await {
 *     match chunk {
 *         Ok(bytes) => { /* write to response body */ }
 *         Err(e) => { /* decide to break / fallback */ }
 *     }
 * }
 * ```
 *
 * Safety / Concurrency:
 *   - Internally uses a `tokio::sync::mpsc` channel (bounded).
 *   - Producer (poll loop) terminates on shutdown request or channel close.
 *   - Public consumer side implements `futures::Stream`.
 *
 * Cancellation:
 *   - User may call `handle.stop()` to request termination.
 *   - Drop of consumer (channel receiver) will also end the loop naturally.
 *
 * Additions (this revision):
 *   - Lazy spawn: collapsing loop starts only when the first poll on the handle occurs.
 *   - Media sequence tracking: honors EXT-X-MEDIA-SEQUENCE to avoid duplicate emission when URIs shift.
 *
 * License: Inherits project license.
 */
use crate::streaming::metrics::metrics;
use bytes::Bytes;
use futures::Stream;
use rand::{Rng, rng};
use reqwest::Client;
use std::{
    collections::HashSet,
    pin::Pin,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    task::{Context, Poll},
    time::Duration,
};
use tokio::sync::mpsc;
use tracing::{debug, error, info, warn};

/// Internal bounded channel size (segment buffering). Keep small to minimize memory.
const DEFAULT_CHANNEL_BUFFER: usize = 4;

/// Minimum poll interval guard (avoid thrashing).
const MIN_POLL_INTERVAL: Duration = Duration::from_millis(800);

/// Maximum consecutive playlist fetch failures before aborting the session.
const MAX_PLAYLIST_ERRORS: usize = 6;

/// Maximum consecutive segment fetch failures before aborting.
const MAX_SEGMENT_ERRORS: usize = 6;

/// Default assumed target duration if playlist omits or parse fails.
const DEFAULT_TARGET_DURATION_FALLBACK: f32 = 6.0;

/// Collapsing session configuration.
#[derive(Debug, Clone)]
pub struct CollapsingConfig {
    /// Channel buffer size for segment byte chunks (not TS packets, entire segment bodies).
    pub channel_buffer: usize,
    /// HTTP request timeout for playlist fetch.
    pub playlist_timeout: Duration,
    /// HTTP request timeout per segment fetch.
    pub segment_timeout: Duration,
    /// Max playlist body bytes to read (defensive).
    pub max_playlist_bytes: usize,
}

impl Default for CollapsingConfig {
    fn default() -> Self {
        Self {
            channel_buffer: DEFAULT_CHANNEL_BUFFER,
            playlist_timeout: Duration::from_secs(5),
            segment_timeout: Duration::from_secs(10),
            max_playlist_bytes: 256 * 1024,
        }
    }
}

/// Errors surfaced to consumer.
#[derive(thiserror::Error, Debug)]
pub enum CollapsingError {
    #[error("Playlist fetch failed: {0}")]
    PlaylistFetch(String),
    #[error("Segment fetch failed: {0}")]
    SegmentFetch(String),
    #[error("Session aborted: {0}")]
    Aborted(String),
    #[error("Internal channel closed")]
    ChannelClosed,
}

/// Public handle (implements `Stream<Item=Result<Bytes, CollapsingError>>`).
pub struct CollapsingHandle {
    rx: mpsc::Receiver<Result<Bytes, CollapsingError>>,
    inner: Arc<CollapsingInner>,
}

impl CollapsingHandle {
    /// Signal the session to stop (idempotent).
    pub fn stop(&self) {
        self.inner.shutdown.store(true, Ordering::SeqCst);
    }

    /// Whether shutdown was requested.
    pub fn is_stopped(&self) -> bool {
        self.inner.shutdown.load(Ordering::SeqCst)
    }
}

impl Stream for CollapsingHandle {
    type Item = Result<Bytes, CollapsingError>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let me = self.get_mut();

        // LAZY START: spawn collapsing loop only on first poll from consumer.
        if !me.inner.started.load(Ordering::SeqCst)
            && !me.inner.started.swap(true, Ordering::SeqCst)
        {
            let inner_clone = me.inner.clone();
            tokio::spawn(async move {
                if let Err(e) = run_collapsing_loop(inner_clone.clone()).await {
                    debug!(session_id=%inner_clone.session_id, error=?e, "Collapsing loop terminated");
                }
            });
            debug!(session_id=%me.inner.session_id, "Activated collapsing session (lazy start)");
        }

        match Pin::new(&mut me.rx).poll_recv(cx) {
            Poll::Ready(Some(item)) => Poll::Ready(Some(item)),
            Poll::Ready(None) => Poll::Ready(None),
            Poll::Pending => Poll::Pending,
        }
    }
}

struct CollapsingInner {
    client: Arc<Client>,
    playlist_url: String,
    cfg: CollapsingConfig,
    shutdown: AtomicBool,
    session_id: String,
    started: AtomicBool,
    initial_target_duration: f32,
    tx: mpsc::Sender<Result<Bytes, CollapsingError>>,
}

impl CollapsingInner {
    fn new(
        client: Arc<Client>,
        playlist_url: String,
        cfg: CollapsingConfig,
        initial_target_duration: f32,
        tx: mpsc::Sender<Result<Bytes, CollapsingError>>,
    ) -> Self {
        Self {
            client,
            playlist_url,
            cfg,
            shutdown: AtomicBool::new(false),
            session_id: uuid::Uuid::new_v4().to_string(),
            started: AtomicBool::new(false),
            initial_target_duration,
            tx,
        }
    }
}

/// Spawn a collapsing session for a previously classified *eligible* media playlist (single-variant TS).
///
/// Lazy behavior: the internal polling loop is only started when the returned `CollapsingHandle`
/// is first polled (prevents speculative / duplicate sessions that never stream any bytes).
pub fn spawn_collapsing_session(
    client: Arc<Client>,
    playlist_url: String,
    initial_target_duration: Option<f32>,
    cfg: CollapsingConfig,
) -> CollapsingHandle {
    let (tx, rx) = mpsc::channel(cfg.channel_buffer);
    let initial_td = initial_target_duration.unwrap_or(DEFAULT_TARGET_DURATION_FALLBACK);
    let inner = Arc::new(CollapsingInner::new(
        client,
        playlist_url,
        cfg.clone(),
        initial_td,
        tx,
    ));

    CollapsingHandle { rx, inner }
}

async fn run_collapsing_loop(inner: Arc<CollapsingInner>) -> Result<(), CollapsingError> {
    let tx = inner.tx.clone();
    let mut target_duration = inner.initial_target_duration;

    info!(
        session_id = %inner.session_id,
        playlist_url = %inner.playlist_url,
        target_duration = target_duration,
        "Starting collapsing session"
    );

    // Track seen URIs (fallback) and seen sequence numbers (preferred when playlist provides them).
    let mut seen_segments: HashSet<String> = HashSet::new();
    let mut seen_sequences: HashSet<u64> = HashSet::new();
    let mut playlist_errors = 0usize;
    let mut segment_errors = 0usize;
    let mut loop_iter = 0u64;

    // Main polling loop
    while !inner.shutdown.load(Ordering::SeqCst) {
        loop_iter += 1;
        let fetch_started = std::time::Instant::now();

        // 1. Fetch playlist
        metrics().collapsing_loop_iterations.add(1, &[]);
        let playlist_text = match fetch_playlist_text(
            &inner.client,
            &inner.playlist_url,
            inner.cfg.playlist_timeout,
            inner.cfg.max_playlist_bytes,
        )
        .await
        {
            Ok(t) => {
                playlist_errors = 0;
                t
            }
            Err(e) => {
                playlist_errors += 1;
                metrics().collapsing_playlist_errors.add(1, &[]);
                warn!(
                    session_id=%inner.session_id,
                    error = %e,
                    attempt = playlist_errors,
                    "Playlist fetch error"
                );
                if playlist_errors >= MAX_PLAYLIST_ERRORS {
                    let _ = tx
                        .send(Err(CollapsingError::PlaylistFetch(format!(
                            "Exceeded playlist retry limit: {e}"
                        ))))
                        .await;
                    break;
                }
                tokio::time::sleep(Duration::from_millis(500)).await;
                continue;
            }
        };

        debug!(
            session_id = %inner.session_id,
            bytes = playlist_text.len(),
            iter = loop_iter,
            "Fetched playlist snapshot"
        );

        // 2. Parse
        let parsed = parse_media_playlist(&playlist_text);
        if let Some(td) = parsed.target_duration {
            target_duration = td;
        }

        let new_segments_count = parsed
            .segments
            .iter()
            .enumerate()
            .filter(|(idx, uri)| {
                if let Some(base) = parsed.media_sequence {
                    let seq = base + (*idx as u64);
                    !seen_sequences.contains(&seq)
                } else {
                    !seen_segments.contains(&uri.to_string())
                }
            })
            .count();

        debug!(
            session_id = %inner.session_id,
            segment_count = parsed.segments.len(),
            target_duration = target_duration,
            new_segments = new_segments_count,
            media_sequence_start = parsed.media_sequence.map(|s| s as i64),
            "Parsed media playlist"
        );

        // 3. Emit unseen segments in order
        let mut new_any = false;

        for (idx, seg_url) in parsed.segments.iter().enumerate() {
            if inner.shutdown.load(Ordering::SeqCst) {
                break;
            }

            let seq_opt = parsed.media_sequence.map(|base| base + idx as u64);

            // Dedup logic: prefer sequence-based when available.
            let already_seen = if let Some(seq) = seq_opt {
                seen_sequences.contains(&seq)
            } else {
                seen_segments.contains(seg_url)
            };

            if already_seen {
                continue;
            }

            if let Some(seq) = seq_opt {
                seen_sequences.insert(seq);
            } else {
                seen_segments.insert(seg_url.clone());
            }

            new_any = true;

            // Derive absolute segment URL
            let absolute = absolutize_segment(&inner.playlist_url, seg_url);

            match fetch_segment_bytes(&inner.client, &absolute, inner.cfg.segment_timeout).await {
                Ok(bytes) => {
                    segment_errors = 0;
                    let len = bytes.len();
                    metrics().collapsing_segments_emitted.add(1, &[]);
                    if tx.send(Ok(bytes)).await.is_err() {
                        debug!(
                            session_id=%inner.session_id,
                            segment=?seg_url,
                            "Consumer dropped; ending collapsing loop"
                        );
                        // Ensure we terminate the outer loop promptly
                        inner.shutdown.store(true, Ordering::SeqCst);
                        break;
                    }
                    debug!(
                        session_id=%inner.session_id,
                        segment = seg_url,
                        sequence = seq_opt.map(|s| s as i64),
                        size = len,
                        total_seen = (seen_sequences.len() + seen_segments.len()),
                        "Emitted segment"
                    );
                }
                Err(e) => {
                    segment_errors += 1;
                    metrics().collapsing_segment_errors.add(1, &[]);
                    warn!(
                        session_id=%inner.session_id,
                        error = %e,
                        attempt = segment_errors,
                        segment = %seg_url,
                        sequence = seq_opt.map(|s| s as i64),
                        "Segment fetch error"
                    );
                    if segment_errors >= MAX_SEGMENT_ERRORS {
                        let _ = tx
                            .send(Err(CollapsingError::SegmentFetch(format!(
                                "Exceeded segment retry limit: {e}"
                            ))))
                            .await;
                        return Ok(());
                    }
                }
            }
        }

        if inner.shutdown.load(Ordering::SeqCst) || tx.is_closed() {
            break;
        }

        // 4. Decide poll interval
        let elapsed = fetch_started.elapsed();
        let mut interval_ms =
            ((target_duration * 1000.0) * 0.5).clamp(800.0, (target_duration * 1000.0).max(1500.0));
        if !new_any {
            // Add slight jitter when no new segments to reduce synchronized thundering herd behavior
            let mut r = rng();
            let jitter: f32 = r.random_range(0.85..1.15);
            interval_ms = (interval_ms * 0.8 * jitter).max(700.0);
        }
        let interval = Duration::from_millis(interval_ms as u64);
        if interval > elapsed && interval >= MIN_POLL_INTERVAL {
            tokio::time::sleep(interval - elapsed).await;
        } else {
            tokio::task::yield_now().await;
        }
    }

    if inner.shutdown.load(Ordering::SeqCst) {
        debug!(session_id=%inner.session_id, "Collapsing session shutdown requested");
        let _ = tx
            .send(Err(CollapsingError::Aborted("shutdown".into())))
            .await;
    }

    info!(session_id=%inner.session_id, "Collapsing session ended");
    Ok(())
}

/* ------------------------------------------------------------------------------------------------
 * Fetch Helpers
 * --------------------------------------------------------------------------------------------- */

async fn fetch_playlist_text(
    client: &Client,
    url: &str,
    timeout: Duration,
    max_bytes: usize,
) -> Result<String, CollapsingError> {
    let resp = client
        .get(url)
        .timeout(timeout)
        .send()
        .await
        .map_err(|e| CollapsingError::PlaylistFetch(e.to_string()))?;

    if !resp.status().is_success() {
        return Err(CollapsingError::PlaylistFetch(format!(
            "HTTP {}",
            resp.status()
        )));
    }

    let mut stream = resp.bytes_stream();
    use futures::StreamExt;
    let mut buf: Vec<u8> = Vec::with_capacity(8192);
    while let Some(chunk) = stream.next().await {
        let c = chunk.map_err(|e| CollapsingError::PlaylistFetch(e.to_string()))?;
        if buf.len() + c.len() > max_bytes {
            buf.extend_from_slice(&c[..(max_bytes - buf.len())]);
            break;
        }
        buf.extend_from_slice(&c);
    }

    Ok(String::from_utf8_lossy(&buf).to_string())
}

async fn fetch_segment_bytes(
    client: &Client,
    url: &str,
    timeout: Duration,
) -> Result<Bytes, CollapsingError> {
    let resp = client
        .get(url)
        .timeout(timeout)
        .send()
        .await
        .map_err(|e| CollapsingError::SegmentFetch(e.to_string()))?;

    if !resp.status().is_success() {
        return Err(CollapsingError::SegmentFetch(format!(
            "HTTP {}",
            resp.status()
        )));
    }

    resp.bytes()
        .await
        .map_err(|e| CollapsingError::SegmentFetch(e.to_string()))
}

/* ------------------------------------------------------------------------------------------------
 * Simple Playlist Parsing (Media Only)
 * --------------------------------------------------------------------------------------------- */

#[derive(Debug)]
struct ParsedMedia {
    segments: Vec<String>,
    target_duration: Option<f32>,
    media_sequence: Option<u64>,
}

/// Lightweight parse of a presumed MEDIA playlist.
/// Extracts:
///   - EXT-X-TARGETDURATION
///   - EXT-X-MEDIA-SEQUENCE (if present)
///   - Raw segment URI lines (non-# lines)
fn parse_media_playlist(text: &str) -> ParsedMedia {
    let mut target_duration = None;
    let mut segments = Vec::new();
    let mut media_sequence: Option<u64> = None;

    for raw_line in text.lines() {
        let line = raw_line.trim();
        if line.is_empty() {
            continue;
        }
        if line.starts_with("#EXT-X-TARGETDURATION:") {
            if let Some(val) = line
                .split_once(':')
                .and_then(|(_, v)| v.parse::<f32>().ok())
            {
                target_duration = Some(val);
            }
            continue;
        }
        if line.starts_with("#EXT-X-MEDIA-SEQUENCE:") {
            if let Some(val) = line
                .split_once(':')
                .and_then(|(_, v)| v.parse::<u64>().ok())
            {
                media_sequence = Some(val);
            }
            continue;
        }
        if line.starts_with('#') {
            continue;
        }
        // Segment URI line
        segments.push(line.to_string());
    }

    ParsedMedia {
        segments,
        target_duration,
        media_sequence,
    }
}

/* ------------------------------------------------------------------------------------------------
 * URL Helpers
 * --------------------------------------------------------------------------------------------- */

/// Build absolute segment URL relative to playlist.
/// Basic approach; for robust path resolution consider `url` crate (added if needed).
fn absolutize_segment(playlist_url: &str, seg: &str) -> String {
    if seg.starts_with("http://") || seg.starts_with("https://") {
        return seg.to_string();
    }
    if let Some(pos) = playlist_url.rfind('/') {
        format!("{}{}", &playlist_url[..=pos], seg)
    } else {
        seg.to_string()
    }
}

/* ------------------------------------------------------------------------------------------------
 * Tests (basic)
 * --------------------------------------------------------------------------------------------- */

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_media_playlist_basic() {
        let sample = r#"#EXTM3U
#EXT-X-TARGETDURATION:6
#EXT-X-MEDIA-SEQUENCE:42
#EXTINF:6,
seg1.ts
#EXTINF:6,
seg2.ts
"#;
        let p = parse_media_playlist(sample);
        assert_eq!(p.segments.len(), 2);
        assert_eq!(p.target_duration, Some(6.0));
        assert_eq!(p.media_sequence, Some(42));
    }

    #[test]
    fn test_parse_media_playlist_no_seq() {
        let sample = r#"#EXTM3U
#EXT-X-TARGETDURATION:4
#EXTINF:4,
a.ts
#EXTINF:4,
b.ts
"#;
        let p = parse_media_playlist(sample);
        assert_eq!(p.segments.len(), 2);
        assert_eq!(p.media_sequence, None);
    }

    #[test]
    fn test_absolutize() {
        assert_eq!(
            absolutize_segment("http://x/y/master.m3u8", "seg1.ts"),
            "http://x/y/seg1.ts"
        );
        assert_eq!(
            absolutize_segment("http://x/y/master.m3u8", "http://z/a.ts"),
            "http://z/a.ts"
        );
    }
}
