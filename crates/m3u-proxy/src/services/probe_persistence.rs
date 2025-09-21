use std::collections::HashMap;
use std::sync::Arc;

use anyhow::Result;
use tokio::sync::Mutex;
use tracing::{debug, info, warn};

use crate::database::repositories::LastKnownCodecSeaOrmRepository;
use crate::models::last_known_codec::{CreateLastKnownCodecRequest, LastKnownCodec, ProbeMethod};
use crate::services::stream_prober::{ProbeResult, StreamProber};

/// Service responsible for persisting probe (codec) information derived from stream probing.
///
/// Responsibilities:
/// - Execute a probe (via `StreamProber`)
/// - Transform `ProbeResult` into a persistence model (`CreateLastKnownCodecRequest`)
/// - Perform upsert logic with change detection
/// - Avoid overwriting a previously successful record with a failed probe
/// - Prevent duplicate concurrent probe executions for the same URL
///
/// Design Decisions:
/// - `StreamProber` remains a pure ffprobe wrapper (no DB side-effects)
/// - Failure persistence only occurs if there is no existing record
/// - Change detection prevents unnecessary writes when nothing material changed
/// - No automatic scheduling / TTL logic included (per current requirements)
pub struct ProbePersistenceService {
    prober: StreamProber,
    repo: Arc<LastKnownCodecSeaOrmRepository>,
    /// Tracks per-stream in-flight probe operations so repeated triggers (relay + manual) don't run concurrently.
    in_flight: Mutex<HashMap<String, Arc<Mutex<()>>>>,
}

impl ProbePersistenceService {
    pub fn new(prober: StreamProber, repo: Arc<LastKnownCodecSeaOrmRepository>) -> Self {
        Self {
            prober,
            repo,
            in_flight: Mutex::new(HashMap::new()),
        }
    }

    /// Probe a stream and persist (or update) its codec information.
    ///
    /// Behavior:
    /// - Success: upsert (only if material changes) and return latest record.
    /// - Failure: if no prior record exists, create a failure placeholder (all None, probe_source contains context).
    ///   if record exists, do NOT overwrite; error is returned.
    pub async fn probe_and_persist(
        &self,
        stream_url: &str,
        method: ProbeMethod,
        probe_source: Option<String>,
    ) -> Result<LastKnownCodec> {
        let _lock_guard = self.acquire_in_flight_lock(stream_url).await;

        // Fetch existing before probing (used to decide failure handling & change detection)
        let existing = match self.repo.find_by_stream_url(stream_url).await {
            Ok(v) => v,
            Err(e) => {
                warn!(
                    error = %e,
                    %stream_url,
                    "Failed to lookup existing codec info prior to probe"
                );
                None
            }
        };

        debug!(
            %stream_url,
            method = %method,
            existing_record = existing.is_some(),
            "Starting codec probe"
        );

        let probe_result = match self.prober.probe_input(stream_url).await {
            Ok(r) => r,
            Err(e) => {
                // Only persist a failure if there is no existing successful record
                if existing.is_none() {
                    warn!(
                        error = %e,
                        %stream_url,
                        method = %method,
                        "Probe failed, storing initial failure placeholder"
                    );
                    let failure_req = self.build_failure_request(
                        method.clone(),
                        probe_source
                            .clone()
                            .map(|s| format!("{}_failed: {}", s, e))
                            .or_else(|| Some(format!("probe_failed: {}", e))),
                    );
                    if let Err(store_err) =
                        self.repo.upsert_codec_info(stream_url, failure_req).await
                    {
                        warn!(
                            error = %store_err,
                            %stream_url,
                            "Failed to persist failure placeholder"
                        );
                    }
                } else {
                    warn!(
                        error = %e,
                        %stream_url,
                        method = %method,
                        "Probe failed; retaining existing successful record"
                    );
                }
                return Err(e);
            }
        };

        // Build persistence request from probe
        let req = self.build_success_request(&probe_result, method, probe_source);

        if let Some(prev) = &existing {
            if self.no_material_change(prev, &req) {
                debug!(
                    %stream_url,
                    "Skipping persistence: no material codec changes detected"
                );
                return Ok(prev.clone());
            }
        }

        let stored = self.repo.upsert_codec_info(stream_url, req).await?;
        info!(
            %stream_url,
            video_codec = ?stored.video_codec,
            audio_codec = ?stored.audio_codec,
            resolution = ?stored.resolution,
            "Persisted (or updated) codec information"
        );
        Ok(stored)
    }

    async fn acquire_in_flight_lock(&self, stream_url: &str) -> Arc<Mutex<()>> {
        // Fast path: check without locking entire map if already present
        {
            let guard = self.in_flight.lock().await;
            if let Some(existing) = guard.get(stream_url) {
                return existing.clone();
            }
        }

        // Insert new lock
        let mut guard = self.in_flight.lock().await;
        guard
            .entry(stream_url.to_string())
            .or_insert_with(|| Arc::new(Mutex::new(())))
            .clone()
    }

    fn build_success_request(
        &self,
        probe: &ProbeResult,
        method: ProbeMethod,
        probe_source: Option<String>,
    ) -> CreateLastKnownCodecRequest {
        let video = probe.video_streams.first();
        let audio = probe.audio_streams.first();

        let framerate = video.and_then(|v| v.r_frame_rate.clone());

        CreateLastKnownCodecRequest {
            video_codec: video.map(|s| s.codec_name.clone()),
            audio_codec: audio.map(|s| s.codec_name.clone()),
            container_format: probe.format_name.clone(),
            video_width: video.and_then(|v| v.width.map(|w| w as i32)),
            video_height: video.and_then(|v| v.height.map(|h| h as i32)),
            framerate,
            bitrate: probe.bit_rate.map(|b| b as i32),
            video_bitrate: video.and_then(|v| v.bit_rate.map(|b| b as i32)),
            audio_bitrate: audio.and_then(|a| a.bit_rate.map(|b| b as i32)),
            // Store channel count as integer (matches DB schema INTEGER)
            audio_channels: audio.and_then(|a| a.channels.map(|c| c as i32)),
            audio_sample_rate: audio.and_then(|a| a.sample_rate.map(|sr| sr as i32)),
            probe_method: method,
            probe_source,
        }
    }

    fn build_failure_request(
        &self,
        method: ProbeMethod,
        probe_source: Option<String>,
    ) -> CreateLastKnownCodecRequest {
        CreateLastKnownCodecRequest {
            video_codec: None,
            audio_codec: None,
            container_format: None,
            video_width: None,
            video_height: None,
            framerate: None,
            bitrate: None,
            video_bitrate: None,
            audio_bitrate: None,
            audio_channels: None,
            audio_sample_rate: None,
            probe_method: method,
            probe_source,
        }
    }

    fn no_material_change(
        &self,
        prev: &LastKnownCodec,
        new_req: &CreateLastKnownCodecRequest,
    ) -> bool {
        prev.video_codec == new_req.video_codec
            && prev.audio_codec == new_req.audio_codec
            && prev.container_format == new_req.container_format
            && prev.video_width == new_req.video_width
            && prev.video_height == new_req.video_height
            && prev.framerate == new_req.framerate
            && prev.video_bitrate == new_req.video_bitrate
            && prev.audio_bitrate == new_req.audio_bitrate
            && prev.audio_channels.as_ref() == new_req.audio_channels.as_ref()
            && prev.audio_sample_rate == new_req.audio_sample_rate
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::last_known_codec::ProbeMethod;

    // These tests are limited to pure helper logic (no DB).
    // Full integration tests would live elsewhere with a test DB.
    #[test]
    fn test_no_material_change_true() {
        let dummy_prev = LastKnownCodec {
            id: uuid::Uuid::new_v4(),
            stream_url: "http://x".into(),
            video_codec: Some("h264".into()),
            audio_codec: Some("aac".into()),
            container_format: Some("mpegts".into()),
            video_width: Some(1920),
            video_height: Some(1080),
            resolution: Some("1920x1080".into()),
            framerate: Some("25/1".into()),
            bitrate: Some(2_000_000),
            video_bitrate: Some(2_000_000),
            audio_bitrate: Some(128_000),
            audio_channels: Some(2),
            audio_sample_rate: Some(48000),
            probe_method: ProbeMethod::FfprobeManual,
            probe_source: Some("test".into()),
            detected_at: chrono::Utc::now(),
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        };

        let req = CreateLastKnownCodecRequest {
            video_codec: Some("h264".into()),
            audio_codec: Some("aac".into()),
            container_format: Some("mpegts".into()),
            video_width: Some(1920),
            video_height: Some(1080),
            framerate: Some("25/1".into()),
            bitrate: Some(2_000_000),
            video_bitrate: Some(2_000_000),
            audio_bitrate: Some(128_000),
            audio_channels: Some(2),
            audio_sample_rate: Some(48000),
            probe_method: ProbeMethod::FfprobeManual,
            probe_source: Some("test".into()),
        };

        let service = ProbePersistenceService {
            prober: StreamProber::new(None),
            repo: {
                let rt = tokio::runtime::Runtime::new().expect("create rt");
                rt.block_on(async {
                    let db = sea_orm::Database::connect("sqlite::memory:")
                        .await
                        .expect("mem db");
                    Arc::new(LastKnownCodecSeaOrmRepository::new(Arc::new(db)))
                })
            },
            in_flight: Mutex::new(HashMap::new()),
        };

        assert!(service.no_material_change(&dummy_prev, &req));
    }

    #[test]
    fn test_no_material_change_false() {
        let dummy_prev = LastKnownCodec {
            id: uuid::Uuid::new_v4(),
            stream_url: "http://x".into(),
            video_codec: Some("h264".into()),
            audio_codec: Some("aac".into()),
            container_format: Some("mpegts".into()),
            video_width: Some(1920),
            video_height: Some(1080),
            resolution: Some("1920x1080".into()),
            framerate: Some("25/1".into()),
            bitrate: Some(2_000_000),
            video_bitrate: Some(2_000_000),
            audio_bitrate: Some(128_000),
            audio_channels: Some(2),
            audio_sample_rate: Some(48000),
            probe_method: ProbeMethod::FfprobeManual,
            probe_source: Some("test".into()),
            detected_at: chrono::Utc::now(),
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        };

        let req = CreateLastKnownCodecRequest {
            video_codec: Some("hevc".into()), // changed
            audio_codec: Some("aac".into()),
            container_format: Some("mpegts".into()),
            video_width: Some(1920),
            video_height: Some(1080),
            framerate: Some("25/1".into()),
            bitrate: Some(2_000_000),
            video_bitrate: Some(2_000_000),
            audio_bitrate: Some(128_000),
            audio_channels: Some(2),
            audio_sample_rate: Some(48000),
            probe_method: ProbeMethod::FfprobeManual,
            probe_source: Some("test".into()),
        };

        let service = ProbePersistenceService {
            prober: StreamProber::new(None),
            repo: {
                let rt = tokio::runtime::Runtime::new().expect("create rt");
                rt.block_on(async {
                    let db = sea_orm::Database::connect("sqlite::memory:")
                        .await
                        .expect("mem db");
                    Arc::new(LastKnownCodecSeaOrmRepository::new(Arc::new(db)))
                })
            },
            in_flight: Mutex::new(HashMap::new()),
        };

        assert!(!service.no_material_change(&dummy_prev, &req));
    }
}
