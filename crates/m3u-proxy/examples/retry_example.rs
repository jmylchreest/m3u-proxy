//! Example demonstrating how to use the database retry wrapper
//! 
//! This example shows how to add retry functionality to any existing repository
//! to handle database locking and other transient failures gracefully.

use m3u_proxy::repositories::{
    traits::Repository,
    stream_source::{StreamSourceRepository, StreamSourceQuery},
    retry_wrapper::RepositoryRetryExt,
};
use m3u_proxy::models::{StreamSourceCreateRequest, StreamSourceType};
use m3u_proxy::utils::database_retry::RetryConfig;
use sqlx::{Pool, Sqlite};
use uuid::Uuid;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // This is just a demonstration - actual database connection would be required
    println!("Database Retry Wrapper Example");
    println!("==============================");
    println!();
    
    // Example 1: Adding read retries to a repository
    println!("Example 1: Repository with read retries");
    println!("   let base_repo = StreamSourceRepository::new(pool.clone());");
    println!("   let retry_repo = base_repo.with_read_retries();");
    println!("   // All operations now have retry logic for transient failures");
    println!();
    
    // Example 2: Adding write retries to a repository  
    println!("Example 2: Repository with write retries");
    println!("   let base_repo = StreamSourceRepository::new(pool.clone());");
    println!("   let retry_repo = base_repo.with_write_retries();");
    println!("   // Write operations have more aggressive retry configuration");
    println!();
    
    // Example 3: Custom retry configuration
    println!("Example 3: Repository with custom retry configuration");
    println!("   let config = RetryConfig {{");
    println!("       max_attempts: 5,");
    println!("       initial_delay: Duration::from_millis(200),");
    println!("       max_delay: Duration::from_secs(10),");
    println!("       backoff_multiplier: 2.0,");
    println!("       jitter: true,");
    println!("   }};");
    println!("   let retry_repo = base_repo.with_retries(config);");
    println!();
    
    // Example 4: Usage in API handlers
    println!("Example 4: Usage in API handlers");
    println!("   // In your API handler or service:");
    println!("   async fn create_stream_source(");
    println!("       pool: &Pool<Sqlite>, ");
    println!("       request: StreamSourceCreateRequest");
    println!("   ) -> Result<StreamSource, RepositoryError> {{");
    println!("       let repo = StreamSourceRepository::new(pool.clone())");
    println!("           .with_write_retries(); // Add retry logic");
    println!("       ");
    println!("       // This create operation will automatically retry on database locks");
    println!("       repo.create(request).await");
    println!("   }}");
    println!();
    
    println!("Retry Configurations:");
    println!("  - Read retries: 3 attempts, 50-500ms delays, 1.5x backoff");
    println!("  - Write retries: 5 attempts, 100ms-3s delays, 2.0x backoff");  
    println!("  - Critical retries: 7 attempts, 200ms-5s delays, 2.0x backoff, no jitter");
    println!();
    
    println!("Retryable Errors:");
    println!("  - SQLite database locked (code 5)");
    println!("  - SQLite database busy (SQLITE_BUSY)");
    println!("  - Connection pool timeouts");
    println!("  - Connection pool closed");
    println!("  - Error messages containing 'locked', 'busy', 'timeout', 'connection reset'");
    
    Ok(())
}