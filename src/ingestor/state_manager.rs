use chrono::Utc;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{broadcast, RwLock};
use uuid::Uuid;

use crate::models::*;

#[allow(dead_code)]
pub type ProgressSender = broadcast::Sender<IngestionProgress>;
#[allow(dead_code)]
pub type ProgressReceiver = broadcast::Receiver<IngestionProgress>;

#[derive(Clone)]
pub struct IngestionStateManager {
    states: Arc<RwLock<HashMap<Uuid, IngestionProgress>>>,
    progress_tx: ProgressSender,
}

impl IngestionStateManager {
    pub fn new() -> Self {
        let (progress_tx, _) = broadcast::channel(1000);
        Self {
            states: Arc::new(RwLock::new(HashMap::new())),
            progress_tx,
        }
    }

    #[allow(dead_code)]
    pub fn subscribe(&self) -> ProgressReceiver {
        self.progress_tx.subscribe()
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
                percentage: Some(0.0),
            },
            started_at: Utc::now(),
            updated_at: Utc::now(),
            completed_at: None,
            error: None,
        };

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
    }

    pub async fn complete_ingestion(&self, source_id: Uuid, channels_saved: usize) {
        self.update_progress(
            source_id,
            IngestionState::Completed,
            ProgressInfo {
                current_step: format!("Completed - {} channels saved", channels_saved),
                total_bytes: None,
                downloaded_bytes: None,
                channels_parsed: Some(channels_saved),
                channels_saved: Some(channels_saved),
                percentage: Some(100.0),
            },
        )
        .await;
    }

    pub async fn get_progress(&self, source_id: Uuid) -> Option<IngestionProgress> {
        let states = self.states.read().await;
        states.get(&source_id).cloned()
    }

    pub async fn get_all_progress(&self) -> HashMap<Uuid, IngestionProgress> {
        let states = self.states.read().await;
        states.clone()
    }

    #[allow(dead_code)]
    pub async fn cleanup_completed(&self, max_age_hours: i64) {
        let cutoff = Utc::now() - chrono::Duration::hours(max_age_hours);

        let mut states = self.states.write().await;
        states.retain(|_, progress| {
            match progress.completed_at {
                Some(completed_at) => completed_at > cutoff,
                None => true, // Keep in-progress items
            }
        });
    }
}

impl Default for IngestionStateManager {
    fn default() -> Self {
        Self::new()
    }
}
