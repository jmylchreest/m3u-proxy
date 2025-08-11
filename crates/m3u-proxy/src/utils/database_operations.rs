use std::time::Duration;
use sqlx::{Pool, Sqlite};
use tracing::{debug, info, trace};
use tokio::time::sleep;

use crate::errors::{AppError, AppResult};
use crate::utils::jitter::generate_jitter_ms;

/// Database operation utilities with retry logic and robust transaction management
pub struct DatabaseOperations;

impl DatabaseOperations {
    /// Execute a database operation with retry logic for lock contention
    pub async fn execute_with_retry<F, R>(
        operation: F,
        operation_name: &str,
        max_attempts: u32,
    ) -> AppResult<R>
    where
        F: Fn() -> std::pin::Pin<Box<dyn std::future::Future<Output = AppResult<R>> + Send + 'static>>,
        R: Send,
    {
        let attempts = std::sync::Arc::new(std::sync::atomic::AtomicU32::new(0));
        let mut last_error = None;

        while attempts.load(std::sync::atomic::Ordering::SeqCst) < max_attempts {
            attempts.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            
            match operation().await {
                Ok(result) => {
                    if attempts.load(std::sync::atomic::Ordering::SeqCst) > 1 {
                        info!("Database operation '{}' succeeded on attempt {}", operation_name, attempts.load(std::sync::atomic::Ordering::SeqCst));
                    }
                    return Ok(result);
                }
                Err(e) => {
                    let error_msg = e.to_string().to_lowercase();
                    
                    // Check if this is a retryable database lock error
                    if (error_msg.contains("database is locked") || 
                       error_msg.contains("busy") || 
                       error_msg.contains("deadlock"))
                        
                        && attempts.load(std::sync::atomic::Ordering::SeqCst) < max_attempts {
                            // Calculate exponential backoff with jitter
                            let base_delay = Duration::from_millis(100);
                            let exponential_delay = base_delay * 2_u32.pow(attempts.load(std::sync::atomic::Ordering::SeqCst).saturating_sub(1));
                            let jitter = Duration::from_millis(generate_jitter_ms(50));
                            let total_delay = exponential_delay + jitter;
                            
                            debug!(
                                "Database operation '{}' failed on attempt {} with lock error: {}. Retrying in {:?}",
                                operation_name, attempts.load(std::sync::atomic::Ordering::SeqCst), e, total_delay
                            );
                            
                            sleep(total_delay).await;
                            last_error = Some(e);
                            continue;
                        }
                    
                    // Non-retryable error or max attempts reached
                    return Err(e);
                }
            }
        }

        // Max attempts reached with retryable errors
        Err(last_error.unwrap_or_else(|| 
            AppError::internal(format!("Database operation '{operation_name}' failed after {max_attempts} attempts"))
        ))
    }

    /// Save EPG programs in batches with retry logic - simplified version
    pub async fn save_epg_programs_in_batches(
        pool: &Pool<Sqlite>,
        _source_id: uuid::Uuid,
        programs: Vec<crate::models::EpgProgram>,
        batch_size: usize,
        progress_updater: Option<&crate::services::progress_service::ProgressStageUpdater>,
    ) -> AppResult<usize> {
        let total_items = programs.len();
        let mut total_processed = 0;
        
        info!("Starting batched EPG program insertion for {} items with batch size {}", 
              total_items, batch_size);

        for (chunk_index, chunk) in programs.chunks(batch_size).enumerate() {
            
            debug!("Processing EPG batch {}/{} ({} items)", 
                   chunk_index + 1, 
                   total_items.div_ceil(batch_size),
                   chunk.len());

            // Direct batch insert with retry logic
            let attempts = std::sync::Arc::new(std::sync::atomic::AtomicU32::new(0));
            let max_attempts = 3;
            
            loop {
                match Self::insert_epg_programs_batch(chunk.to_vec(), pool).await {
                    Ok(count) => {
                        total_processed += count;
                        debug!("Successfully processed EPG batch {}: {} items", chunk_index + 1, count);
                        
                        // Update progress if updater is available
                        if let Some(updater) = progress_updater {
                            // Calculate progress: 20% base + up to 80% for database saving
                            let save_progress = (total_processed as f64 / total_items as f64) * 80.0;
                            let total_progress = 20.0 + save_progress;
                            let batch_num = chunk_index + 1;
                            let total_batches = total_items.div_ceil(batch_size);
                            
                            let progress_message = format!("Inserting batch {batch_num}/{total_batches} ({total_processed} of {total_items} programs)");
                            
                            updater.update_progress(total_progress, &progress_message).await;
                        }
                        
                        break;
                    }
                    Err(e) => {
                        attempts.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                        if attempts.load(std::sync::atomic::Ordering::SeqCst) < max_attempts && e.to_string().to_lowercase().contains("database is locked") {
                            let delay = Duration::from_millis(100 * 2_u64.pow(attempts.load(std::sync::atomic::Ordering::SeqCst)));
                            debug!("EPG batch {} failed (attempt {}), retrying in {:?}: {}", 
                                  chunk_index + 1, attempts.load(std::sync::atomic::Ordering::SeqCst), delay, e);
                            sleep(delay).await;
                            continue;
                        } else {
                            return Err(AppError::internal(format!(
                                "Failed to process EPG batch {} after {} attempts: {}", 
                                chunk_index + 1, attempts.load(std::sync::atomic::Ordering::SeqCst), e
                            )));
                        }
                    }
                }
            }
        }

        info!("Completed batched EPG program insertion: {} items processed successfully", 
              total_processed);
        
        Ok(total_processed)
    }

    /// Optimize SQLite database for high-volume operations
    pub async fn optimize_for_bulk_operations(pool: &Pool<Sqlite>) -> AppResult<()> {
        debug!("Optimizing SQLite database for bulk operations");

        let optimizations = [
            ("PRAGMA busy_timeout = 30000", "Set busy timeout to 30 seconds"),
            ("PRAGMA cache_size = -64000", "Set cache size to 64MB"),
            ("PRAGMA temp_store = MEMORY", "Store temporary tables in memory"),
            ("PRAGMA synchronous = NORMAL", "Use normal synchronous mode for better performance"),
            ("PRAGMA wal_autocheckpoint = 1000", "Checkpoint WAL every 1000 pages"),
        ];

        for (pragma, description) in &optimizations {
            sqlx::query(pragma)
                .execute(pool)
                .await
                .map_err(|e| AppError::internal(format!("Failed to set pragma '{pragma}': {e}")))?;
            
            debug!("Applied optimization: {}", description);
        }

        Ok(())
    }

    /// Perform WAL checkpoint after large operations
    pub async fn checkpoint_wal(pool: &Pool<Sqlite>) -> AppResult<()> {
        debug!("Performing WAL checkpoint");
        
        sqlx::query("PRAGMA wal_checkpoint(TRUNCATE)")
            .execute(pool)
            .await
            .map_err(|e| AppError::internal(format!("Failed to checkpoint WAL: {e}")))?;
        
        debug!("WAL checkpoint completed");
        Ok(())
    }

    /// Create a batch insert operation for EPG programs using efficient multi-value INSERT
    pub async fn insert_epg_programs_batch(
        programs: Vec<crate::models::EpgProgram>,
        pool: &Pool<Sqlite>,
    ) -> AppResult<usize> {
        if programs.is_empty() {
            return Ok(0);
        }

        let batch_size = programs.len();
        debug!("Inserting batch of {} EPG programs using multi-value INSERT", batch_size);

        // Use a transaction for the batch
        let mut tx = pool.begin().await
            .map_err(|e| AppError::internal(format!("Failed to begin transaction: {e}")))?;

        // Use the full batch size - config already accounts for SQLite parameter limits
        // EPG programs have 12 fields, so 1800 * 12 = 21,600 parameters (well under 32,766 limit)
        let max_records_per_query = batch_size;
        
        let mut total_inserted = 0;
        
        for chunk in programs.chunks(max_records_per_query) {
            // Build multi-value INSERT statement
            let mut query = String::from(
                "INSERT INTO epg_programs (id, source_id, channel_id, channel_name, program_title, program_description, program_category, start_time, end_time, language, created_at, updated_at) VALUES "
            );
            
            let placeholders: Vec<String> = (0..chunk.len())
                .map(|_| "(?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)".to_string())
                .collect();
            query.push_str(&placeholders.join(", "));
            
            let mut db_query = sqlx::query(&query);
            
            // Bind all parameters
            for program in chunk {
                db_query = db_query
                    .bind(program.id.to_string())
                    .bind(program.source_id.to_string())
                    .bind(&program.channel_id)
                    .bind(&program.channel_name)
                    .bind(&program.program_title)
                    .bind(&program.program_description)
                    .bind(&program.program_category)
                    .bind(program.start_time)
                    .bind(program.end_time)
                    .bind(&program.language)
                    .bind(program.created_at)
                    .bind(program.updated_at);
            }
            
            let result = db_query.execute(&mut *tx).await
                .map_err(|e| AppError::internal(format!("Failed to insert EPG programs batch: {e}")))?;
            
            total_inserted += result.rows_affected() as usize;
            trace!("Inserted {} programs in multi-value query", result.rows_affected());
        }

        tx.commit().await
            .map_err(|e| AppError::internal(format!("Failed to commit batch transaction: {e}")))?;

        debug!("Successfully inserted {} EPG programs in optimized batch", total_inserted);
        Ok(total_inserted)
    }

    /// Delete EPG programs for a source with retry logic - simplified version
    pub async fn delete_epg_programs_for_source(
        source_id: uuid::Uuid,
        pool: &Pool<Sqlite>,
    ) -> AppResult<u64> {
        debug!("Deleting existing EPG programs for source: {}", source_id);

        let source_id_string = source_id.to_string();
        let attempts = std::sync::Arc::new(std::sync::atomic::AtomicU32::new(0));
        let max_attempts = 3;
        
        loop {
            match async {
                let mut tx = pool.begin().await?;

                let result = sqlx::query("DELETE FROM epg_programs WHERE source_id = ?")
                    .bind(&source_id_string)
                    .execute(&mut *tx)
                    .await?;

                tx.commit().await?;

                AppResult::Ok(result.rows_affected())
            }.await {
                Ok(result) => {
                    info!("Deleted {} existing EPG programs for source: {}", result, source_id);
                    return Ok(result);
                }
                Err(e) => {
                    attempts.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                    if attempts.load(std::sync::atomic::Ordering::SeqCst) < max_attempts && e.to_string().to_lowercase().contains("database is locked") {
                        let delay = Duration::from_millis(100 * 2_u64.pow(attempts.load(std::sync::atomic::Ordering::SeqCst)));
                        debug!("Delete EPG programs failed (attempt {}), retrying in {:?}: {}", 
                              attempts.load(std::sync::atomic::Ordering::SeqCst), delay, e);
                        sleep(delay).await;
                        continue;
                    } else {
                        return Err(AppError::internal(format!(
                            "Failed to delete EPG programs after {:?} attempts: {e}",
                            attempts.load(std::sync::atomic::Ordering::SeqCst)
                        )));
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[tokio::test]
    async fn test_retry_logic() {
        let attempts = std::sync::Arc::new(std::sync::atomic::AtomicU32::new(0));
        
        let result = DatabaseOperations::execute_with_retry(
            || {
                let attempts_clone = attempts.clone();
                attempts_clone.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                Box::pin(async move {
                    if attempts_clone.load(std::sync::atomic::Ordering::SeqCst) < 3 {
                        Err(AppError::internal("database is locked".to_string()))
                    } else {
                        Ok("success".to_string())
                    }
                })
            },
            "test_operation",
            5,
        ).await;

        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "success");
        assert_eq!(attempts.load(std::sync::atomic::Ordering::SeqCst), 3);
    }
}