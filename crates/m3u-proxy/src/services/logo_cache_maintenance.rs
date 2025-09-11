//! Logo cache maintenance service
//!
//! Provides automatic cleanup and optimization of the logo cache system.
//! Runs as exclusive jobs to avoid interference with ongoing ingestion.

use std::sync::Arc;
use tracing::info;
use anyhow::Result;

use crate::services::logo_cache::{LogoCacheService, MaintenanceStats};
use crate::job_scheduling::types::JobType;

/// Logo cache maintenance service
pub struct LogoCacheMaintenanceService {
    logo_cache: Arc<LogoCacheService>,
}

impl LogoCacheMaintenanceService {
    /// Create new logo cache maintenance service
    pub fn new(
        logo_cache: Arc<LogoCacheService>,
    ) -> Self {
        Self {
            logo_cache,
        }
    }
    
    /// Create a logo cache maintenance job type (for integration with job scheduler)
    pub fn create_maintenance_job(&self) -> JobType {
        JobType::Maintenance("logo_cache_cleanup".to_string())
    }
    
    /// Execute logo cache maintenance (called by job scheduler)
    pub async fn execute_maintenance(&self) -> Result<MaintenanceStats> {
        // Use hardcoded defaults instead of config fields (replaced sandboxed file manager approach)
        let max_cache_size_mb = 1024; // 1GB default cache size limit
        let max_age_days = 30; // 30 days default cache age limit
        
        let stats = self.logo_cache.run_maintenance(max_cache_size_mb, max_age_days).await?;
        
        info!(
            "Logo cache maintenance completed: kept={} removed_age={} removed_size={} freed={}MB duration={}ms memory={}MB",
            stats.kept_entries,
            stats.removed_by_age,
            stats.removed_by_size,
            stats.bytes_freed / 1024 / 1024,
            stats.duration_ms,
            stats.final_memory_mb
        );
        
        Ok(stats)
    }
    
    /// Initialize logo cache
    pub async fn initialize(&self) -> Result<()> {
        info!("Initializing logo cache maintenance service");
        
        // Initialize the logo cache
        self.logo_cache.initialize().await?;
        
        info!("Logo cache maintenance service initialized");
        Ok(())
    }
    
    /// Execute logo cache rescan (rebuild indices from filesystem)
    pub async fn execute_rescan(&self) -> Result<()> {
        info!("Starting logo cache rescan - rebuilding indices from filesystem");
        
        // Clear current cache indices and rebuild from filesystem
        self.logo_cache.scan_and_load_cache().await?;
        
        info!("Logo cache rescan completed - indices rebuilt from filesystem");
        Ok(())
    }
    
    /// Get logo cache statistics
    pub async fn get_cache_stats(&self) -> Result<crate::services::logo_cache::LogoCacheStats> {
        Ok(self.logo_cache.get_stats().await)
    }
}

#[cfg(test)]
mod tests {

    #[tokio::test]
    #[ignore] // Requires full service setup
    async fn test_maintenance_scheduling() {
        // This would require mocking the job scheduler and logo cache service
        // Implementation would test that jobs are scheduled with correct priorities
        // and that maintenance operations complete successfully
    }
}