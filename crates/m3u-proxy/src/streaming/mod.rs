/**
 * streaming/mod.rs
 * =================
 * Public module entrypoint for the hybrid streaming subsystem.
 *
 * Currently exposes:
 *   - classification: Logic to decide how a given upstream channel URL should be
 *     handled (passthrough raw TS, collapsed single-variant HLS, unknown).
 *   - collapsing: Polling + segment fetch loop for single-variant TS media playlists (Phase 1).
 *
 * Roadmap (see TODO-HybridStream.md for full details):
 *   - collapsing: Implementation of the polling loop that concatenates single-variant
 *     HLS TS segments into a continuous TS byte stream.
 *   - (Transparent HLS module removed per current strategy: always TS passthrough/collapse.)
 *   - metrics: Prometheus instrumentation (classification counters, segment timings).
 *   - fanout: Shared collapsing session with multi-subscriber ring buffer.
 *   - resilience: Retry / backoff strategies, stale detection, discontinuity handling.
 *
 * This file deliberately keeps only re-exports / module wiring to keep crate root tidy.
 */
pub mod classification;
pub mod collapsing;

// Streaming metrics instrumentation module
pub mod metrics {
    use opentelemetry::global;
    use opentelemetry::metrics::{Counter, Meter};
    use std::sync::OnceLock;

    /// Aggregated metric instruments for streaming / collapsing.
    pub struct StreamingMetrics {
        pub classification_total: Counter<u64>,
        pub classification_fallback_total: Counter<u64>,
        pub collapsing_segments_emitted: Counter<u64>,
        pub collapsing_playlist_errors: Counter<u64>,
        pub collapsing_segment_errors: Counter<u64>,
        pub collapsing_loop_iterations: Counter<u64>,
    }

    impl StreamingMetrics {
        fn new() -> Self {
            let meter: Meter = global::meter("m3u-proxy.streaming");
            Self {
                classification_total: meter
                    .u64_counter("stream_classification_total")
                    .with_description("Total stream classifications")
                    .build(),
                classification_fallback_total: meter
                    .u64_counter("stream_classification_fallback_total")
                    .with_description("Classification fallbacks / unsupported cases")
                    .build(),
                collapsing_segments_emitted: meter
                    .u64_counter("collapsing_segments_emitted_total")
                    .with_description("Segments emitted by collapsing loop")
                    .build(),
                collapsing_playlist_errors: meter
                    .u64_counter("collapsing_playlist_errors_total")
                    .with_description("Playlist fetch errors in collapsing loop")
                    .build(),
                collapsing_segment_errors: meter
                    .u64_counter("collapsing_segment_errors_total")
                    .with_description("Segment fetch errors in collapsing loop")
                    .build(),
                collapsing_loop_iterations: meter
                    .u64_counter("collapsing_loop_iterations_total")
                    .with_description("Collapsing loop iterations")
                    .build(),
            }
        }
    }

    static METRICS: OnceLock<StreamingMetrics> = OnceLock::new();

    /// Public accessor for global streaming metrics instruments.
    pub fn metrics() -> &'static StreamingMetrics {
        METRICS.get_or_init(StreamingMetrics::new)
    }

    /// Public re-export (explicit) for label construction inside and outside this module.
    pub use opentelemetry::KeyValue as MetricsKeyValue;
}

/// Convenience public re-export of OpenTelemetry KeyValue at the streaming module root.
/// External modules can now import with: `use crate::streaming::KeyValue;`
pub use opentelemetry::KeyValue;
