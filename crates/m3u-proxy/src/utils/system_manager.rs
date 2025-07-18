//! Centralized system monitoring manager
//!
//! This module provides a shared system monitor that can be used by multiple
//! components to avoid duplicate system monitoring overhead.

use std::sync::Arc;
use std::time::Duration;
use sysinfo::{System, SystemExt};
use tokio::sync::RwLock;
use tokio::time;

/// Centralized system manager with shared monitoring
pub struct SystemManager {
    system: Arc<RwLock<System>>,
    _refresh_task: tokio::task::JoinHandle<()>,
}

impl SystemManager {
    /// Create a new system manager with periodic refresh
    pub fn new(refresh_interval: Duration) -> Self {
        let system = Arc::new(RwLock::new(System::new_all()));
        let refresh_task = Self::start_refresh_task(system.clone(), refresh_interval);
        
        Self {
            system,
            _refresh_task: refresh_task,
        }
    }

    /// Get the shared system instance
    pub fn get_system(&self) -> Arc<RwLock<System>> {
        self.system.clone()
    }

    /// Start the background refresh task
    fn start_refresh_task(system: Arc<RwLock<System>>, interval: Duration) -> tokio::task::JoinHandle<()> {
        tokio::spawn(async move {
            let mut ticker = time::interval(interval);
            ticker.set_missed_tick_behavior(time::MissedTickBehavior::Skip);
            
            loop {
                ticker.tick().await;
                
                // Refresh system info
                let mut sys = system.write().await;
                sys.refresh_all();
                tracing::trace!("System monitoring refreshed");
            }
        })
    }
}

impl Drop for SystemManager {
    fn drop(&mut self) {
        self._refresh_task.abort();
    }
}