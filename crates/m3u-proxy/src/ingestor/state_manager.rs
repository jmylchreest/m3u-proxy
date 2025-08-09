use chrono::{DateTime, Duration, Utc};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{broadcast, RwLock};
use uuid::Uuid;

use crate::utils::jitter::generate_jitter_percent;

use crate::models::*;


#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ProcessingInfo {
    pub started_at: DateTime<Utc>,
    pub triggered_by: ProcessingTrigger,
    pub failure_count: u32,
    pub next_retry_after: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ProcessingTrigger {
    Scheduler,
    Manual,
}

impl std::fmt::Display for ProcessingTrigger {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ProcessingTrigger::Scheduler => write!(f, "scheduler"),
            ProcessingTrigger::Manual => write!(f, "manual"),
        }
    }
}

#[derive(Clone)]
pub struct IngestionStateManager {
    states: Arc<RwLock<HashMap<Uuid, IngestionProgress>>>,
    progress_tx: broadcast::Sender<IngestionProgress>,
    cancellation_tokens: Arc<RwLock<HashMap<Uuid, broadcast::Sender<()>>>>,
    processing_info: Arc<RwLock<HashMap<Uuid, ProcessingInfo>>>,
}

impl IngestionStateManager {
    pub fn new() -> Self {
        let (progress_tx, _) = broadcast::channel(1000);
        Self {
            states: Arc::new(RwLock::new(HashMap::new())),
            progress_tx,
            cancellation_tokens: Arc::new(RwLock::new(HashMap::new())),
            processing_info: Arc::new(RwLock::new(HashMap::new())),
        }
    }



    /// Try to start processing for a source. Returns true if processing was started,
    /// false if already processing or in backoff period.
    pub async fn try_start_processing(&self, source_id: Uuid, trigger: ProcessingTrigger) -> bool {
        let mut processing = self.processing_info.write().await;
        let now = Utc::now();

        // Check if already processing
        if let Some(existing_info) = processing.get(&source_id) {
            // Check if we're still in a processing state (no next_retry_after set or in backoff)
            if existing_info.next_retry_after.is_none() {
                // Still actively processing
                return false;
            }
            
            // Check if we're in backoff period
            if let Some(retry_after) = existing_info.next_retry_after {
                if now < retry_after {
                    return false; // Still in backoff period
                }
            }
        }

        // Start processing - preserve failure count from any existing entry
        let failure_count = processing
            .get(&source_id)
            .map(|i| i.failure_count)
            .unwrap_or(0);

        let info = ProcessingInfo {
            started_at: now,
            triggered_by: trigger,
            failure_count,
            next_retry_after: None, // Clear retry time when starting new processing
        };

        processing.insert(source_id, info);
        true
    }

    /// Finish processing and update failure state
    pub async fn finish_processing(&self, source_id: Uuid, success: bool) {
        let mut processing = self.processing_info.write().await;

        if let Some(mut info) = processing.remove(&source_id) {
            if success {
                // Reset failure count on success
                info.failure_count = 0;
                info.next_retry_after = None;
            } else {
                // Increment failure count and calculate backoff with jitter
                info.failure_count += 1;
                let backoff_seconds = self.calculate_backoff_with_jitter(info.failure_count);
                info.next_retry_after =
                    Some(Utc::now() + Duration::seconds(backoff_seconds as i64));

                // Store the failure info for future attempts
                processing.insert(source_id, info);
            }
        }
    }

    /// Get processing info including backoff state
    pub async fn get_processing_info(&self, source_id: Uuid) -> Option<ProcessingInfo> {
        let processing = self.processing_info.read().await;
        processing.get(&source_id).cloned()
    }

    /// Calculate exponential backoff with jitter (25% jitter)
    fn calculate_backoff_with_jitter(&self, failure_count: u32) -> u64 {
        let base_delay = 2_u64.pow(failure_count.min(10)); // Cap at 2^10 = 1024 seconds
        let max_delay = 3600; // Cap at 1 hour
        let capped_delay = base_delay.min(max_delay);

        // Add 25% jitter
        let jitter = generate_jitter_percent(capped_delay, 25);

        capped_delay + jitter
    }

    pub async fn start_ingestion(&self, source_id: Uuid) {
        let progress = IngestionProgress {
            source_id,
            state: IngestionState::Connecting,
            progress: ProgressInfo {
                current_step: "Initializing connection".to_string(),
                total_bytes: None,
                downloaded_bytes: None,
                channels_parsed: None,
                channels_saved: None,
                programs_parsed: None,
                programs_saved: None,
                percentage: Some(0.0),
            },
            started_at: Utc::now(),
            updated_at: Utc::now(),
            completed_at: None,
            error: None,
        };

        // Create cancellation token for this ingestion
        let (cancel_tx, _) = broadcast::channel(1);
        {
            let mut tokens = self.cancellation_tokens.write().await;
            tokens.insert(source_id, cancel_tx);
        }

        {
            let mut states = self.states.write().await;
            states.insert(source_id, progress.clone());
        }

        let _ = self.progress_tx.send(progress);
    }

    pub async fn update_progress(
        &self,
        source_id: Uuid,
        state: IngestionState,
        progress_info: ProgressInfo,
    ) {
        let mut current_progress = {
            let states = self.states.read().await;
            states.get(&source_id).cloned()
        };

        if let Some(ref mut progress) = current_progress {
            progress.state = state.clone();
            progress.progress = progress_info;
            progress.updated_at = Utc::now();

            if matches!(state, IngestionState::Completed | IngestionState::Error) {
                progress.completed_at = Some(Utc::now());
            }

            {
                let mut states = self.states.write().await;
                states.insert(source_id, progress.clone());
            }

            let _ = self.progress_tx.send(progress.clone());
        }
    }

    pub async fn set_error(&self, source_id: Uuid, error: String) {
        let mut current_progress = {
            let states = self.states.read().await;
            states.get(&source_id).cloned()
        };

        if let Some(ref mut progress) = current_progress {
            progress.state = IngestionState::Error;
            progress.error = Some(error);
            progress.updated_at = Utc::now();
            progress.completed_at = Some(Utc::now());

            {
                let mut states = self.states.write().await;
                states.insert(source_id, progress.clone());
            }

            let _ = self.progress_tx.send(progress.clone());
        }

        // Clean up cancellation token
        {
            let mut tokens = self.cancellation_tokens.write().await;
            tokens.remove(&source_id);
        }
    }

    pub async fn complete_ingestion(&self, source_id: Uuid, channels_saved: usize) {
        self.complete_ingestion_with_programs(source_id, channels_saved, None).await;
    }

    pub async fn complete_ingestion_with_programs(&self, source_id: Uuid, channels_saved: usize, programs_saved: Option<usize>) {
        let current_step = match programs_saved {
            Some(programs) => format!("Completed - {} channels and {} programs saved", channels_saved, programs),
            None => format!("Completed - {} channels saved", channels_saved),
        };

        self.update_progress(
            source_id,
            IngestionState::Completed,
            ProgressInfo {
                current_step,
                total_bytes: None,
                downloaded_bytes: None,
                channels_parsed: Some(channels_saved),
                channels_saved: Some(channels_saved),
                programs_parsed: programs_saved,
                programs_saved,
                percentage: Some(100.0),
            },
        )
        .await;

        // Clean up cancellation token
        {
            let mut tokens = self.cancellation_tokens.write().await;
            tokens.remove(&source_id);
        }
    }

    pub async fn get_progress(&self, source_id: Uuid) -> Option<IngestionProgress> {
        let states = self.states.read().await;
        states.get(&source_id).cloned()
    }

    pub async fn get_all_progress(&self) -> HashMap<Uuid, IngestionProgress> {
        let states = self.states.read().await;
        states.clone()
    }

    pub async fn cancel_ingestion(&self, source_id: Uuid) -> bool {
        let cancel_tx = {
            let tokens = self.cancellation_tokens.read().await;
            tokens.get(&source_id).cloned()
        };

        if let Some(tx) = cancel_tx {
            // Send cancellation signal
            let _ = tx.send(());

            // Update state to show cancellation
            self.set_error(source_id, "Operation cancelled by user".to_string())
                .await;

            true
        } else {
            false
        }
    }

    pub async fn get_cancellation_receiver(
        &self,
        source_id: Uuid,
    ) -> Option<broadcast::Receiver<()>> {
        let tokens = self.cancellation_tokens.read().await;
        tokens.get(&source_id).map(|tx| tx.subscribe())
    }

    /// Check if there are any active ingestions in progress
    pub async fn has_active_ingestions(&self) -> Result<bool, Box<dyn std::error::Error>> {
        let states = self.states.read().await;
        
        for (_, progress) in states.iter() {
            match progress.state {
                crate::models::IngestionState::Idle 
                | crate::models::IngestionState::Completed 
                | crate::models::IngestionState::Error => {
                    // These are not active
                    continue;
                }
                crate::models::IngestionState::Connecting
                | crate::models::IngestionState::Downloading
                | crate::models::IngestionState::Parsing
                | crate::models::IngestionState::Saving
                | crate::models::IngestionState::Processing => {
                    // These are active ingestion states
                    return Ok(true);
                }
            }
        }
        
        Ok(false)
    }

}

impl Default for IngestionStateManager {
    fn default() -> Self {
        Self::new()
    }
}
