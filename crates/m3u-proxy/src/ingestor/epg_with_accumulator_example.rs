//! Example: EPG Ingestor with Accumulator Integration
//!
//! This demonstrates how to integrate the ingestion accumulator pattern
//! into the existing EPG ingestion workflow for better memory management
//! and streaming capabilities.

use anyhow::Result;
use chrono::{DateTime, Utc};
use reqwest::Client;
use std::sync::Arc;
use tracing::{debug, info, warn};
use xmltv::{Programme, Tv};

use crate::database::Database;
use crate::models::*;
use crate::services::sandboxed_file::SandboxedFileManager;
use super::ingestion_accumulator::{IngestionAccumulator, IngestionAccumulatorFactory};
use super::state_manager::IngestionStateManager;

/// Enhanced EPG ingestor with accumulator pattern
pub struct AccumulatedEpgIngestor {
    client: Client,
    database: Database,
    file_manager: Arc<dyn SandboxedFileManager>,
    state_manager: Arc<IngestionStateManager>,
}

impl AccumulatedEpgIngestor {
    pub fn new(
        database: Database,
        file_manager: Arc<dyn SandboxedFileManager>,
        state_manager: Arc<IngestionStateManager>,
    ) -> Self {
        Self {
            client: Client::new(),
            database,
            file_manager,
            state_manager,
        }
    }

    /// Ingest EPG source using accumulator pattern for optimal memory usage
    pub async fn ingest_epg_with_accumulator(
        &self,
        source: &EpgSource,
    ) -> Result<(usize, usize)> {
        info!("Starting EPG ingestion with accumulator pattern for source: {}", source.name);

        // Create accumulator optimized for EPG (large XML files)
        let mut accumulator: IngestionAccumulator<EpgProgramme> = 
            IngestionAccumulatorFactory::create_for_source(
                "xmltv",
                self.file_manager.clone(),
                Some(self.state_manager.clone()),
            );

        // Phase 1: HTTP Download with accumulation
        self.state_manager.update_progress(10, "Starting EPG download").await;
        
        let response = self.client
            .get(&source.url)
            .send()
            .await?;
        
        if !response.status().is_success() {
            return Err(anyhow::anyhow!("HTTP error: {}", response.status()));
        }

        let content_length = response.content_length();
        let mut bytes_downloaded = 0usize;

        // Stream download with accumulator
        let mut stream = response.bytes_stream();
        
        while let Some(chunk) = tokio_stream::StreamExt::next(&mut stream).await {
            let chunk = chunk?;
            
            // Accumulate HTTP chunk - accumulator handles memory/file decisions
            accumulator.accumulate_http_chunk(&chunk).await?;
            
            bytes_downloaded += chunk.len();
            
            // Update progress
            if let Some(total) = content_length {
                let progress = 10 + ((bytes_downloaded as f64 / total as f64) * 40.0) as u8;
                accumulator.update_progress("Downloading EPG", progress).await;
            }
            
            // Check cancellation
            if self.state_manager.is_cancelled().await {
                return Err(anyhow::anyhow!("EPG ingestion cancelled during download"));
            }
        }

        info!("Downloaded {:.1}MB EPG data", bytes_downloaded as f64 / 1024.0 / 1024.0);

        // Phase 2: Parse accumulated data
        self.state_manager.update_progress(60, "Parsing EPG data").await;
        
        let xml_data = accumulator.finalize_accumulation().await?;
        let xml_content = String::from_utf8(xml_data)?;
        
        // Parse XMLTV (this could also be done incrementally with streaming parser)
        let tv_data: Tv = quick_xml::de::from_str(&xml_content)?;
        
        // Convert to our internal format with batch accumulation
        let mut programmes = Vec::new();
        let total_programmes = tv_data.programmes.len();
        
        for (index, programme) in tv_data.programmes.into_iter().enumerate() {
            let epg_programme = self.convert_xmltv_programme(programme, source)?;
            programmes.push(epg_programme);
            
            // Update parsing progress
            if index % 1000 == 0 {
                let progress = 60 + ((index as f64 / total_programmes as f64) * 20.0) as u8;
                accumulator.update_progress("Parsing programmes", progress).await;
            }
            
            // Check cancellation periodically
            if index % 500 == 0 && self.state_manager.is_cancelled().await {
                return Err(anyhow::anyhow!("EPG ingestion cancelled during parsing"));
            }
        }

        // Add parsed entries to accumulator for potential batching
        accumulator.accumulate_parsed_entries(programmes).await?;

        // Phase 3: Database storage with batch optimization
        self.state_manager.update_progress(80, "Storing EPG data").await;
        
        let final_programmes = accumulator.drain_parsed_entries();
        let stored_count = self.store_programmes_in_batches(&final_programmes, source).await?;
        
        self.state_manager.update_progress(100, "EPG ingestion complete").await;
        
        // Log accumulator statistics
        let stats = accumulator.get_stats();
        info!(
            "EPG ingestion complete: {:.1}MB downloaded, {} programmes parsed, {} stored, {} batch operations",
            stats.total_bytes_downloaded as f64 / 1024.0 / 1024.0,
            stats.total_entries_parsed,
            stats.total_entries_stored,
            stats.batch_operations_completed
        );

        Ok((final_programmes.len(), stored_count))
    }

    /// Store programmes in optimized batches
    async fn store_programmes_in_batches(
        &self,
        programmes: &[EpgProgramme],
        source: &EpgSource,
    ) -> Result<usize> {
        const BATCH_SIZE: usize = 500;
        let mut stored_count = 0;
        
        // Start transaction
        let mut tx = self.database.pool().begin().await?;
        
        // Clear existing programmes for this source
        sqlx::query!(
            "DELETE FROM epg_programmes WHERE source_id = ?",
            source.id.to_string()
        )
        .execute(&mut *tx)
        .await?;
        
        // Insert in batches
        for (batch_index, batch) in programmes.chunks(BATCH_SIZE).enumerate() {
            for programme in batch {
                sqlx::query!(
                    r#"
                    INSERT INTO epg_programmes (
                        id, source_id, channel_id, title, description, 
                        start_time, end_time, created_at
                    ) VALUES (?, ?, ?, ?, ?, ?, ?, ?)
                    "#,
                    programme.id.to_string(),
                    programme.source_id.to_string(),
                    programme.channel_id,
                    programme.title,
                    programme.description,
                    programme.start_time.to_rfc3339(),
                    programme.end_time.to_rfc3339(),
                    programme.created_at.to_rfc3339()
                )
                .execute(&mut *tx)
                .await?;
            }
            
            stored_count += batch.len();
            
            // Progress update
            let progress = 80 + ((batch_index as f64 / (programmes.len() / BATCH_SIZE) as f64) * 20.0) as u8;
            self.state_manager.update_progress(progress, &format!("Stored {} programmes", stored_count)).await;
            
            // Check cancellation
            if self.state_manager.is_cancelled().await {
                return Err(anyhow::anyhow!("EPG ingestion cancelled during storage"));
            }
        }
        
        // Commit transaction
        tx.commit().await?;
        
        Ok(stored_count)
    }

    /// Convert XMLTV programme to internal format
    fn convert_xmltv_programme(
        &self,
        programme: Programme,
        source: &EpgSource,
    ) -> Result<EpgProgramme> {
        Ok(EpgProgramme {
            id: uuid::Uuid::new_v4(),
            source_id: source.id,
            channel_id: programme.channel,
            title: programme.titles.first()
                .map(|t| t.content.clone())
                .unwrap_or_default(),
            description: programme.descriptions.first()
                .map(|d| d.content.clone()),
            start_time: self.parse_xmltv_time(&programme.start)?,
            end_time: programme.stop
                .as_ref()
                .map(|stop| self.parse_xmltv_time(stop))
                .transpose()?
                .unwrap_or_else(|| self.parse_xmltv_time(&programme.start).unwrap() + chrono::Duration::hours(1)),
            created_at: Utc::now(),
        })
    }

    /// Parse XMLTV time format
    fn parse_xmltv_time(&self, time_str: &str) -> Result<DateTime<Utc>> {
        // Simplified XMLTV time parsing - real implementation would be more robust
        let time_part = time_str.split_whitespace().next().unwrap_or(time_str);
        let dt = chrono::NaiveDateTime::parse_from_str(time_part, "%Y%m%d%H%M%S")?;
        Ok(DateTime::from_naive_utc_and_offset(dt, Utc))
    }
}

/// Example usage demonstrating the accumulator pattern benefits
pub async fn example_epg_ingestion_comparison() -> Result<()> {
    // This example shows how the accumulator pattern improves EPG ingestion:
    
    // 1. **Memory Management**: Large EPG files (100MB+) are automatically
    //    streamed to temporary files when they exceed memory thresholds
    
    // 2. **Progress Tracking**: Integrated with existing state manager for
    //    consistent progress reporting across download/parse/store phases
    
    // 3. **Cancellation Support**: Respects cancellation at all phases
    
    // 4. **Batch Optimization**: Parsed programmes are accumulated and
    //    stored in optimized database batches
    
    // 5. **Strategy Selection**: Different accumulation strategies for
    //    different source types (EPG vs M3U vs Xtream)
    
    info!("Accumulator pattern provides:");
    info!("- Automatic memory management with file spilling");
    info!("- Integrated progress tracking and cancellation");
    info!("- Optimized batch database operations");  
    info!("- Source-specific accumulation strategies");
    
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    // Add tests for accumulator integration
}