//! Ingestion Guard Stage
//!
//! Purpose:
//!   Acts as a gate before any real transformation pipeline stages run. It gives
//!   active ingestion operations (EPG / Stream source ingests) time to finish,
//!   reducing contention on the database and lowering chances of partial data
//!   appearing in a freshly generated proxy.
//!
//! Behavior:
//!   - Polls the `IngestionStateManager` for any active ingestions
//!   - Waits in fixed delay increments (default 15s) up to a max number of
//!     attempts (default 20 → 5 minutes total)
//!   - If ingestion finishes early, proceeds immediately
//!   - If ingestion still active after max attempts, logs a warning and continues
//!   - Non-fatal: never fails the pipeline (only coordinates timing)
//!   - Reports granular progress (attempt/total) via ProgressManager if available
//!
//! The stage is intentionally conservative: regeneration should eventually
//! proceed even if long-running ingestion processes are stuck.
//!
//! Integration:
//!   Added as the very first stage (index 0) by the orchestrator.
//!
//! Extensibility:
//!   Now supports dynamic configuration (delay, max attempts) via
//!   `IngestionGuardStage::new_with_config`, allowing the orchestrator to pass
//!   feature-derived settings instead of relying on compile-time constants.

use std::sync::Arc;
use std::time::Duration;
use tokio::time::sleep;
use tracing::{debug, info, warn};

use crate::ingestor::IngestionStateManager;
use crate::pipeline::error::PipelineError;
use crate::pipeline::models::PipelineArtifact;
use crate::pipeline::traits::{PipelineStage, ProgressAware};
use crate::services::progress_service::ProgressManager;
use async_trait::async_trait;

/// Default delay (seconds) between polling attempts if not overridden
pub const DEFAULT_INGESTION_GUARD_DELAY_SECS: u64 = 15;
/// Default maximum number of polling attempts if not overridden
pub const DEFAULT_INGESTION_GUARD_MAX_ATTEMPTS: u32 = 20;

/// Pipeline stage that waits for active ingestion activity to settle
pub struct IngestionGuardStage {
    state_manager: Arc<IngestionStateManager>,
    progress_manager: Option<Arc<ProgressManager>>,
    attempts: u32,
    delay_secs: u64,
    max_attempts: u32,
}

impl IngestionGuardStage {
    pub fn new(
        state_manager: Arc<IngestionStateManager>,
        progress_manager: Option<Arc<ProgressManager>>,
    ) -> Self {
        Self::new_with_config(
            state_manager,
            progress_manager,
            DEFAULT_INGESTION_GUARD_DELAY_SECS,
            DEFAULT_INGESTION_GUARD_MAX_ATTEMPTS,
        )
    }
    /// Create with explicit configuration (preferred path when using feature flags)
    pub fn new_with_config(
        state_manager: Arc<IngestionStateManager>,
        progress_manager: Option<Arc<ProgressManager>>,
        delay_secs: u64,
        max_attempts: u32,
    ) -> Self {
        Self {
            state_manager,
            progress_manager,
            attempts: 0,
            delay_secs,
            max_attempts,
        }
    }

    async fn any_active_ingestion(&self) -> bool {
        match self.state_manager.has_active_ingestions().await {
            Ok(active) => active,
            Err(e) => {
                warn!(
                    "Ingestion Guard: failed to query ingestion state: {e}; assuming none active"
                );
                false
            }
        }
    }

    fn total_wait_window_secs(&self) -> u64 {
        self.delay_secs * self.max_attempts as u64
    }

    async fn update_progress(&self, attempt: u32, message: &str, finished: bool) {
        if let Some(pm) = &self.progress_manager {
            let frac = if finished {
                1.0
            } else {
                (attempt as f32 / self.max_attempts as f32).clamp(0.0, 1.0)
            };
            // ProgressManager::update_stage_progress expects percentage in 0-100 range
            pm.update_stage_progress(self.stage_id(), (frac * 100.0) as f64, message)
                .await;
        }
    }
}

impl ProgressAware for IngestionGuardStage {
    fn get_progress_manager(&self) -> Option<&Arc<ProgressManager>> {
        self.progress_manager.as_ref()
    }
}

#[async_trait]
impl PipelineStage for IngestionGuardStage {
    async fn execute(
        &mut self,
        input: Vec<PipelineArtifact>,
    ) -> Result<Vec<PipelineArtifact>, PipelineError> {
        // Fast path: nothing active at start
        if !self.any_active_ingestion().await {
            info!("Ingestion Guard: no active ingestion detected; continuing immediately");
            self.update_progress(self.max_attempts, "No ingestion active", true)
                .await;
            return Ok(input);
        }

        info!(
            "Ingestion Guard: active ingestion detected; waiting up to {} attempts ({}s total max)",
            self.max_attempts,
            self.total_wait_window_secs()
        );
        self.update_progress(0, "Waiting for active ingestion", false)
            .await;

        for attempt in 1..=self.max_attempts {
            self.attempts = attempt;
            if !self.any_active_ingestion().await {
                let msg = format!(
                    "Ingestion finished (attempt {}/{}); continuing",
                    attempt, self.max_attempts
                );
                info!("Ingestion Guard: {}", msg);
                self.update_progress(attempt, &msg, true).await;
                return Ok(input);
            }

            let wait_msg = if attempt == 1 {
                format!(
                    "Initial wait ({}s) before re-check (1/{})",
                    self.delay_secs, self.max_attempts
                )
            } else {
                format!(
                    "Ingestion still active (attempt {}/{}); sleeping {}s",
                    attempt, self.max_attempts, self.delay_secs
                )
            };

            if attempt == 1 {
                debug!("Ingestion Guard: {}", wait_msg);
            } else {
                debug!("Ingestion Guard: {}", wait_msg);
            }
            self.update_progress(attempt, &wait_msg, false).await;

            sleep(Duration::from_secs(self.delay_secs)).await;
        }

        let timeout_msg = format!(
            "Maximum wait time ({}s) exceeded; proceeding despite active ingestion",
            self.total_wait_window_secs()
        );
        warn!("Ingestion Guard: {}", timeout_msg);
        self.update_progress(self.max_attempts, &timeout_msg, true)
            .await;

        Ok(input)
    }

    fn stage_id(&self) -> &'static str {
        "ingestion_guard"
    }

    fn stage_name(&self) -> &'static str {
        "Ingestion Guard"
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }

    async fn cleanup(&mut self) -> Result<(), PipelineError> {
        // Nothing to clean
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    // (Removed unused IngestionProgress/IngestionState/ProgressInfo import)
    // (Removed unused chrono::Utc import)
    use uuid::Uuid;

    // Helper to insert a fake active ingestion state directly
    async fn insert_active_ingestion(state_manager: &IngestionStateManager) -> Uuid {
        let source_id = Uuid::new_v4();
        // Public API to start ingestion (puts it into an active state)
        state_manager.start_ingestion(source_id).await;
        source_id
    }

    #[tokio::test]
    async fn test_fast_path_no_ingestion() {
        let manager = Arc::new(IngestionStateManager::new());
        let mut stage = IngestionGuardStage::new(manager, None);
        let result = stage.execute(vec![]).await.unwrap();
        assert!(result.is_empty());
        assert_eq!(stage.attempts, 0); // No waiting attempted
    }

    #[tokio::test]
    async fn test_waits_until_cleared() {
        let manager = Arc::new(IngestionStateManager::new());
        let source_id = insert_active_ingestion(&manager).await;

        let mut stage = IngestionGuardStage::new(manager.clone(), None);

        // Spawn a task to clear ingestion after a brief delay
        let manager_clone = manager.clone();
        tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_millis(150)).await;
            // Mark ingestion as completed so guard no longer sees it as active
            manager_clone.complete_ingestion(source_id, 0).await;
        });

        let _ = stage.execute(vec![]).await.unwrap();
        assert!(stage.attempts >= 1);
    }
}
