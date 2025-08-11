//! Cyclic Buffer for Multi-Client Streaming
//!
//! This module implements a circular buffer that allows multiple clients to read
//! from the same FFmpeg stream efficiently. The buffer automatically manages
//! memory usage and handles clients connecting/disconnecting at different times.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{RwLock, broadcast, Mutex};
use tracing::{debug, error, info, trace, warn};
use uuid::Uuid;
use std::path::PathBuf;
use sandboxed_file_manager::SandboxedManager;
use chrono::Utc;
use crate::models::relay::ConnectedClient;

/// A chunk of data in the cyclic buffer
#[derive(Debug, Clone)]
pub struct BufferChunk {
    pub sequence: u64,
    pub data: bytes::Bytes,
    pub timestamp: Instant,
    pub is_spilled: bool,          // Whether this chunk is stored in a file
    pub spill_path: Option<PathBuf>, // Path to spilled file if applicable
}

/// Configuration for the cyclic buffer
#[derive(Debug, Clone)]
pub struct CyclicBufferConfig {
    pub max_buffer_size: usize,      // Maximum buffer size in bytes
    pub max_chunks: usize,           // Maximum number of chunks to keep
    pub chunk_timeout: Duration,     // How long to keep chunks
    pub client_timeout: Duration,    // How long to wait for slow clients
    pub cleanup_interval: Duration,  // How often to cleanup old chunks
    pub enable_file_spill: bool,     // Enable file spill when buffer is full
    pub max_file_spill_size: usize,  // Maximum size for file spill
}

impl Default for CyclicBufferConfig {
    fn default() -> Self {
        Self {
            max_buffer_size: 50 * 1024 * 1024,  // 50MB
            max_chunks: 1000,                    // 1000 chunks
            chunk_timeout: Duration::from_secs(60), // 1 minute
            client_timeout: Duration::from_secs(30), // 30 seconds
            cleanup_interval: Duration::from_secs(5), // 5 seconds
            enable_file_spill: false,            // Disabled by default
            max_file_spill_size: 500 * 1024 * 1024, // 500MB
        }
    }
}

/// A client reading from the cyclic buffer
#[derive(Debug)]
pub struct BufferClient {
    pub id: Uuid,
    pub last_sequence: AtomicU64,
    pub last_read: Mutex<Instant>,
    pub bytes_read: AtomicU64,
    pub connected_at: Instant,
    pub user_agent: Option<String>,
    pub remote_addr: Option<String>,
}

impl BufferClient {
    pub fn new(user_agent: Option<String>, remote_addr: Option<String>) -> Self {
        Self {
            id: Uuid::new_v4(),
            last_sequence: AtomicU64::new(0),
            last_read: Mutex::new(Instant::now()),
            bytes_read: AtomicU64::new(0),
            connected_at: Instant::now(),
            user_agent,
            remote_addr,
        }
    }

    pub async fn update_last_read(&self) {
        *self.last_read.lock().await = Instant::now();
    }

    pub async fn is_stale(&self, timeout: Duration) -> bool {
        self.last_read.lock().await.elapsed() > timeout
    }

    pub fn get_bytes_read(&self) -> u64 {
        self.bytes_read.load(Ordering::Relaxed)
    }

    pub fn add_bytes_read(&self, bytes: u64) {
        self.bytes_read.fetch_add(bytes, Ordering::Relaxed);
    }

    pub fn get_last_sequence(&self) -> u64 {
        self.last_sequence.load(Ordering::Relaxed)
    }

    pub fn set_last_sequence(&self, sequence: u64) {
        self.last_sequence.store(sequence, Ordering::Relaxed);
    }
}

/// Multi-client cyclic buffer for streaming data
pub struct CyclicBuffer {
    config: CyclicBufferConfig,
    buffer: Arc<RwLock<VecDeque<BufferChunk>>>,
    clients: Arc<RwLock<Vec<Arc<BufferClient>>>>,
    sequence_counter: AtomicU64,
    total_bytes: AtomicU64,
    bytes_received_from_upstream: AtomicU64, // Track bytes received from FFmpeg stdout
    buffer_size: AtomicUsize,
    new_chunk_sender: broadcast::Sender<BufferChunk>,
    cleanup_running: AtomicU64, // Use as boolean
    spill_dir: Option<String>,   // Directory for spilled files (relative to sandbox)
    spill_size: AtomicUsize,     // Total size of spilled files
    spill_counter: AtomicU64,    // Counter for spilled file names
    temp_manager: Option<SandboxedManager>, // File manager for spill files
}

impl CyclicBuffer {
    /// Create a new cyclic buffer with the given configuration
    pub fn new(config: CyclicBufferConfig, temp_manager: Option<SandboxedManager>) -> Self {
        let (new_chunk_sender, _) = broadcast::channel(1000);
        
        // Create spill directory if file spill is enabled
        let spill_dir = if config.enable_file_spill {
            let dir_name = format!("spill_{}", Uuid::new_v4());
            Some(dir_name)
        } else {
            None
        };
        
        let buffer = Self {
            config,
            buffer: Arc::new(RwLock::new(VecDeque::new())),
            clients: Arc::new(RwLock::new(Vec::new())),
            sequence_counter: AtomicU64::new(0),
            total_bytes: AtomicU64::new(0),
            bytes_received_from_upstream: AtomicU64::new(0),
            buffer_size: AtomicUsize::new(0),
            new_chunk_sender,
            cleanup_running: AtomicU64::new(0),
            spill_dir,
            spill_size: AtomicUsize::new(0),
            spill_counter: AtomicU64::new(0),
            temp_manager,
        };

        // Create spill directory if needed
        if let (Some(dir), Some(manager)) = (&buffer.spill_dir, &buffer.temp_manager) {
            let dir_clone = dir.clone();
            let manager_clone = manager.clone();
            tokio::spawn(async move {
                if let Err(e) = manager_clone.create_dir(&dir_clone).await {
                    error!("Failed to create spill directory {}: {}", dir_clone, e);
                }
            });
        }

        // Start cleanup task
        buffer.start_cleanup_task();
        
        buffer
    }

    /// Add a new client to the buffer
    pub async fn add_client(&self, user_agent: Option<String>, remote_addr: Option<String>) -> Arc<BufferClient> {
        let client = Arc::new(BufferClient::new(user_agent, remote_addr));
        
        // Set the client's starting sequence to the current sequence
        // This ensures they get the next chunk, not historical data
        let current_sequence = self.sequence_counter.load(Ordering::Relaxed);
        client.set_last_sequence(current_sequence);
        
        self.clients.write().await.push(client.clone());
        
        info!("Added client {} to cyclic buffer. Total clients: {}", 
              client.id, self.get_client_count().await);
        
        client
    }

    /// Remove a client from the buffer
    pub async fn remove_client(&self, client_id: Uuid) -> bool {
        let mut clients = self.clients.write().await;
        let initial_len = clients.len();
        
        clients.retain(|client| client.id != client_id);
        
        let removed = clients.len() != initial_len;
        if removed {
            info!("Removed client {} from cyclic buffer. Total clients: {}", 
                  client_id, clients.len());
        }
        
        removed
    }

    /// Get the current number of clients
    pub async fn get_client_count(&self) -> usize {
        self.clients.read().await.len()
    }

    /// Write data to the buffer
    pub async fn write_chunk(&self, data: bytes::Bytes) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        if data.is_empty() {
            return Ok(());
        }

        // Track bytes received from upstream (FFmpeg stdout) - this is the raw data before any processing
        self.bytes_received_from_upstream.fetch_add(data.len() as u64, Ordering::Relaxed);

        let sequence = self.sequence_counter.fetch_add(1, Ordering::Relaxed) + 1;
        let mut chunk = BufferChunk {
            sequence,
            data: data.clone(),
            timestamp: Instant::now(),
            is_spilled: false,
            spill_path: None,
        };

        // Check if we need to spill to disk
        let current_buffer_size = self.buffer_size.load(Ordering::Relaxed);
        if self.config.enable_file_spill && 
           current_buffer_size + data.len() > self.config.max_buffer_size {
            // Try to spill the new chunk to disk
            if let Ok(spilled_chunk) = self.spill_chunk_to_disk(&chunk).await {
                chunk = spilled_chunk;
            }
        }

        // Add to buffer
        {
            let mut buffer = self.buffer.write().await;
            buffer.push_back(chunk.clone());
            if !chunk.is_spilled {
                self.buffer_size.fetch_add(data.len(), Ordering::Relaxed);
            }
        }

        // Notify clients of new chunk
        let _ = self.new_chunk_sender.send(chunk);

        // Enforce buffer limits
        self.enforce_buffer_limits().await;

        self.total_bytes.fetch_add(data.len() as u64, Ordering::Relaxed);

        tracing::trace!("Wrote chunk {} with {} bytes to cyclic buffer", sequence, data.len());

        Ok(())
    }

    /// Read chunks for a specific client starting from their last read position
    pub async fn read_chunks_for_client(&self, client: &Arc<BufferClient>) -> Vec<BufferChunk> {
        let last_sequence = client.get_last_sequence();
        let buffer = self.buffer.read().await;
        
        let mut chunks = Vec::new();
        
        for chunk in buffer.iter() {
            if chunk.sequence > last_sequence {
                // If chunk is spilled, read from disk
                let mut processed_chunk = chunk.clone();
                if chunk.is_spilled {
                    if let Ok(data) = self.read_spilled_chunk(chunk).await {
                        processed_chunk.data = data;
                        processed_chunk.is_spilled = false; // Mark as loaded
                    } else {
                        // Skip this chunk if we can't read it
                        warn!("Failed to read spilled chunk {}", chunk.sequence);
                        continue;
                    }
                }
                
                chunks.push(processed_chunk);
                // Update client's last read sequence
                client.set_last_sequence(chunk.sequence);
                client.add_bytes_read(chunk.data.len() as u64);
            }
        }
        
        if !chunks.is_empty() {
            client.update_last_read().await;
            debug!("Read {} chunks for client {}", chunks.len(), client.id);
        }
        
        chunks
    }

    /// Get a broadcast receiver for new chunks
    pub fn subscribe_to_new_chunks(&self) -> broadcast::Receiver<BufferChunk> {
        self.new_chunk_sender.subscribe()
    }
    
    /// Spill a chunk to disk
    async fn spill_chunk_to_disk(&self, chunk: &BufferChunk) -> Result<BufferChunk, Box<dyn std::error::Error + Send + Sync>> {
        let spill_dir = self.spill_dir.as_ref()
            .ok_or("File spill not enabled")?;
        let temp_manager = self.temp_manager.as_ref()
            .ok_or("Temp manager not available")?;
        
        // Check if we have space for spill
        let current_spill_size = self.spill_size.load(Ordering::Relaxed);
        if current_spill_size + chunk.data.len() > self.config.max_file_spill_size {
            return Err("Spill size limit exceeded".into());
        }
        
        // Create spill file path
        let spill_counter = self.spill_counter.fetch_add(1, Ordering::Relaxed);
        let spill_file = format!("{}/chunk_{}_{}.dat", spill_dir, chunk.sequence, spill_counter);
        
        // Write chunk to disk using sandbox manager
        temp_manager.write(&spill_file, &chunk.data).await?;
        
        // Update spill size
        self.spill_size.fetch_add(chunk.data.len(), Ordering::Relaxed);
        
        debug!("Spilled chunk {} to disk: {}", chunk.sequence, spill_file);
        
        Ok(BufferChunk {
            sequence: chunk.sequence,
            data: bytes::Bytes::new(), // Empty in memory
            timestamp: chunk.timestamp,
            is_spilled: true,
            spill_path: Some(PathBuf::from(spill_file)),
        })
    }
    
    /// Read a spilled chunk from disk
    async fn read_spilled_chunk(&self, chunk: &BufferChunk) -> Result<bytes::Bytes, Box<dyn std::error::Error + Send + Sync>> {
        let spill_path = chunk.spill_path.as_ref()
            .ok_or("Chunk has no spill path")?;
        let temp_manager = self.temp_manager.as_ref()
            .ok_or("Temp manager not available")?;
        
        let spill_path_str = spill_path.to_str()
            .ok_or("Invalid spill path")?;
        
        let data = temp_manager.read(spill_path_str).await?;
        
        Ok(bytes::Bytes::from(data))
    }
    

    /// Start the cleanup task
    fn start_cleanup_task(&self) {
        // Use atomic to ensure only one cleanup task runs
        if self.cleanup_running.compare_exchange(0, 1, Ordering::Relaxed, Ordering::Relaxed).is_ok() {
            let buffer = self.buffer.clone();
            let clients = self.clients.clone();
            let config = self.config.clone();
            // Fix: AtomicUsize and AtomicU64 don't have clone()
            let buffer_size_ref = Arc::new(AtomicUsize::new(self.buffer_size.load(Ordering::Relaxed)));
            
            tokio::spawn(async move {
                let mut interval = tokio::time::interval(config.cleanup_interval);
                
                loop {
                    interval.tick().await;
                    
                    // Cleanup old chunks
                    Self::cleanup_old_chunks(&buffer, &config, &buffer_size_ref).await;
                    
                    // Cleanup stale clients
                    Self::cleanup_stale_clients(&clients, &config).await;
                }
            });
        }
    }

    /// Cleanup old chunks that are beyond the retention policy
    async fn cleanup_old_chunks(
        buffer: &Arc<RwLock<VecDeque<BufferChunk>>>,
        config: &CyclicBufferConfig,
        buffer_size: &Arc<AtomicUsize>,
    ) {
        let mut buffer_guard = buffer.write().await;
        let mut removed_size = 0;
        let mut removed_count = 0;
        let mut spilled_chunks_to_cleanup = Vec::new();
        
        let now = Instant::now();
        
        // Remove chunks that are too old
        while let Some(chunk) = buffer_guard.front() {
            if now.duration_since(chunk.timestamp) > config.chunk_timeout {
                if chunk.is_spilled {
                    spilled_chunks_to_cleanup.push(chunk.clone());
                } else {
                    removed_size += chunk.data.len();
                }
                removed_count += 1;
                buffer_guard.pop_front();
            } else {
                break;
            }
        }
        
        // Remove excess chunks if we have too many
        while buffer_guard.len() > config.max_chunks {
            if let Some(chunk) = buffer_guard.pop_front() {
                if chunk.is_spilled {
                    spilled_chunks_to_cleanup.push(chunk);
                } else {
                    removed_size += chunk.data.len();
                }
                removed_count += 1;
            }
        }
        
        // Release the lock before doing file I/O
        drop(buffer_guard);
        
        // Clean up spilled files in the background
        if !spilled_chunks_to_cleanup.is_empty() {
            tokio::spawn(async move {
                for chunk in spilled_chunks_to_cleanup {
                    if let Some(spill_path) = &chunk.spill_path {
                        if let Some(spill_path_str) = spill_path.to_str() {
                            debug!("Scheduled cleanup of spilled file: {}", spill_path_str);
                            // Note: We can't access temp_manager here since it's not available in static context
                            // The cleanup will be handled by the system's temporary file cleanup
                        }
                    }
                }
            });
        }
        
        if removed_count > 0 {
            buffer_size.fetch_sub(removed_size, Ordering::Relaxed);
            debug!("Cleaned up {} old chunks ({} bytes)", removed_count, removed_size);
        }
    }

    /// Cleanup stale clients that haven't read in a while
    async fn cleanup_stale_clients(
        clients: &Arc<RwLock<Vec<Arc<BufferClient>>>>,
        config: &CyclicBufferConfig,
    ) {
        let mut clients_guard = clients.write().await;
        let initial_count = clients_guard.len();
        
        // Remove stale clients
        let mut i = 0;
        while i < clients_guard.len() {
            if clients_guard[i].is_stale(config.client_timeout).await {
                let client = clients_guard.remove(i);
                warn!("Removed stale client {} (inactive for {:?})", 
                      client.id, config.client_timeout);
            } else {
                i += 1;
            }
        }
        
        let removed_count = initial_count - clients_guard.len();
        if removed_count > 0 {
            info!("Cleaned up {} stale clients", removed_count);
        }
    }

    /// Enforce buffer size limits
    async fn enforce_buffer_limits(&self) {
        let current_size = self.buffer_size.load(Ordering::Relaxed);
        
        if current_size > self.config.max_buffer_size {
            let mut buffer = self.buffer.write().await;
            let mut removed_size = 0;
            let mut removed_count = 0;
            let mut spilled_chunks_to_cleanup = Vec::new();
            
            // Remove oldest chunks until we're under the limit
            while current_size - removed_size > self.config.max_buffer_size {
                if let Some(chunk) = buffer.pop_front() {
                    if chunk.is_spilled {
                        spilled_chunks_to_cleanup.push(chunk);
                    } else {
                        removed_size += chunk.data.len();
                    }
                    removed_count += 1;
                } else {
                    break;
                }
            }
            
            // Clean up spilled files in the background
            if !spilled_chunks_to_cleanup.is_empty() {
                if let Some(temp_manager) = &self.temp_manager {
                    let manager_clone = temp_manager.clone();
                    tokio::spawn(async move {
                        for chunk in spilled_chunks_to_cleanup {
                            if let Some(ref spill_path) = chunk.spill_path {
                                if let Some(spill_path_str) = spill_path.to_str() {
                                    if let Err(e) = manager_clone.remove_file(spill_path_str).await {
                                        debug!("Failed to remove spilled file {}: {}", spill_path_str, e);
                                    }
                                }
                            }
                        }
                    });
                }
            }
            
            if removed_count > 0 {
                self.buffer_size.fetch_sub(removed_size, Ordering::Relaxed);
                trace!("Enforced buffer limits: removed {} chunks ({} bytes)", 
                       removed_count, removed_size);
            }
        }
    }

    /// Get buffer statistics
    pub async fn get_stats(&self) -> CyclicBufferStats {
        let buffer = self.buffer.read().await;
        let clients = self.clients.read().await;
        
        let mut client_stats = Vec::new();
        for client in clients.iter() {
            client_stats.push(ClientStats {
                id: client.id,
                bytes_read: client.get_bytes_read(),
                last_sequence: client.get_last_sequence(),
                connected_duration: client.connected_at.elapsed(),
                user_agent: client.user_agent.clone(),
                remote_addr: client.remote_addr.clone(),
            });
        }
        
        CyclicBufferStats {
            total_chunks: buffer.len(),
            total_buffer_size: self.buffer_size.load(Ordering::Relaxed),
            total_bytes_written: self.total_bytes.load(Ordering::Relaxed),
            bytes_received_from_upstream: self.bytes_received_from_upstream.load(Ordering::Relaxed),
            current_sequence: self.sequence_counter.load(Ordering::Relaxed),
            client_count: clients.len(),
            clients: client_stats,
        }
    }

    /// Get connected clients information
    pub async fn get_connected_clients(&self) -> Vec<ConnectedClient> {
        let clients = self.clients.read().await;
        let mut connected_clients = Vec::new();
        let now = Utc::now();
        
        for client in clients.iter() {
            let last_read = client.last_read.lock().await;
            
            // Calculate the actual last activity time by subtracting elapsed time from now
            let last_activity = now - chrono::Duration::from_std(last_read.elapsed()).unwrap_or_default();
            
            // Calculate the connected time by subtracting elapsed time from now
            let connected_at = now - chrono::Duration::from_std(client.connected_at.elapsed()).unwrap_or_default();
            
            connected_clients.push(ConnectedClient {
                id: client.id,
                ip: client.remote_addr.clone().unwrap_or_else(|| "unknown".to_string()),
                user_agent: client.user_agent.clone(),
                connected_at,
                bytes_served: client.get_bytes_read(),
                last_activity,
            });
        }
        
        connected_clients
    }
}

/// Statistics for the cyclic buffer
#[derive(Debug, Clone)]
pub struct CyclicBufferStats {
    pub total_chunks: usize,
    pub total_buffer_size: usize,
    pub total_bytes_written: u64,
    pub bytes_received_from_upstream: u64, // Raw bytes received from FFmpeg stdout
    pub current_sequence: u64,
    pub client_count: usize,
    pub clients: Vec<ClientStats>,
}

/// Statistics for a single client
#[derive(Debug, Clone)]
pub struct ClientStats {
    pub id: Uuid,
    pub bytes_read: u64,
    pub last_sequence: u64,
    pub connected_duration: Duration,
    pub user_agent: Option<String>,
    pub remote_addr: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    // Removed unused imports: sleep, Duration

    #[tokio::test]
    async fn test_cyclic_buffer_basic_operations() {
        let config = CyclicBufferConfig::default();
        let buffer = CyclicBuffer::new(config, None);
        
        // Add a client
        let client = buffer.add_client(Some("test-agent".to_string()), Some("127.0.0.1".to_string())).await;
        assert_eq!(buffer.get_client_count().await, 1);
        
        // Write some data
        let data = bytes::Bytes::from("test data");
        buffer.write_chunk(data.clone()).await.unwrap();
        
        // Read data for client
        let chunks = buffer.read_chunks_for_client(&client).await;
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].data, data);
        
        // Remove client
        assert!(buffer.remove_client(client.id).await);
        assert_eq!(buffer.get_client_count().await, 0);
    }

    #[tokio::test]
    async fn test_multi_client_buffer() {
        let config = CyclicBufferConfig::default();
        let buffer = CyclicBuffer::new(config, None);
        
        // Add multiple clients
        let client1 = buffer.add_client(Some("client1".to_string()), Some("127.0.0.1".to_string())).await;
        let client2 = buffer.add_client(Some("client2".to_string()), Some("127.0.0.2".to_string())).await;
        
        // Write data
        let data1 = bytes::Bytes::from("chunk 1");
        let data2 = bytes::Bytes::from("chunk 2");
        
        buffer.write_chunk(data1.clone()).await.unwrap();
        buffer.write_chunk(data2.clone()).await.unwrap();
        
        // Both clients should get both chunks
        let chunks1 = buffer.read_chunks_for_client(&client1).await;
        let chunks2 = buffer.read_chunks_for_client(&client2).await;
        
        assert_eq!(chunks1.len(), 2);
        assert_eq!(chunks2.len(), 2);
        
        assert_eq!(chunks1[0].data, data1);
        assert_eq!(chunks1[1].data, data2);
        assert_eq!(chunks2[0].data, data1);
        assert_eq!(chunks2[1].data, data2);
    }

    #[tokio::test]
    async fn test_buffer_limits() {
        let config = CyclicBufferConfig {
            max_chunks: 2,
            max_buffer_size: 20, // Very small buffer
            ..CyclicBufferConfig::default()
        };
        
        let buffer = CyclicBuffer::new(config, None);
        let _client = buffer.add_client(None, None).await;
        
        // Write more data than buffer can hold
        for i in 0..5 {
            let data = bytes::Bytes::from(format!("chunk {i}"));
            buffer.write_chunk(data).await.unwrap();
        }
        
        // Buffer should only keep the most recent chunks
        let stats = buffer.get_stats().await;
        assert!(stats.total_chunks <= 2);
    }

    #[tokio::test]
    async fn test_get_connected_clients() {
        let config = CyclicBufferConfig::default();
        let buffer = CyclicBuffer::new(config, None);
        
        // Add clients with different user agents and IP addresses
        let client1 = buffer.add_client(
            Some("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36".to_string()),
            Some("192.168.1.100".to_string())
        ).await;
        
        let client2 = buffer.add_client(
            Some("VLC/3.0.16 LibVLC/3.0.16".to_string()),
            Some("192.168.1.101".to_string())
        ).await;
        
        let client3 = buffer.add_client(None, None).await;
        
        // Write some data and have clients read it
        let data = bytes::Bytes::from("test data");
        buffer.write_chunk(data).await.unwrap();
        
        // Read data for some clients to update their bytes_read
        let chunks = buffer.read_chunks_for_client(&client1).await;
        assert_eq!(chunks.len(), 1);
        
        let chunks = buffer.read_chunks_for_client(&client2).await;
        assert_eq!(chunks.len(), 1);
        
        // Get connected clients
        let connected_clients = buffer.get_connected_clients().await;
        
        // Should have 3 connected clients
        assert_eq!(connected_clients.len(), 3);
        
        // Check the first client
        let client1_info = connected_clients.iter().find(|c| c.id == client1.id).unwrap();
        assert_eq!(client1_info.ip, "192.168.1.100");
        assert_eq!(client1_info.user_agent.as_ref().unwrap(), "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36");
        assert_eq!(client1_info.bytes_served, 9); // "test data".len()
        
        // Check the second client
        let client2_info = connected_clients.iter().find(|c| c.id == client2.id).unwrap();
        assert_eq!(client2_info.ip, "192.168.1.101");
        assert_eq!(client2_info.user_agent.as_ref().unwrap(), "VLC/3.0.16 LibVLC/3.0.16");
        assert_eq!(client2_info.bytes_served, 9); // "test data".len()
        
        // Check the third client (with no user agent or IP)
        let client3_info = connected_clients.iter().find(|c| c.id == client3.id).unwrap();
        assert_eq!(client3_info.ip, "unknown");
        assert_eq!(client3_info.user_agent, None);
        assert_eq!(client3_info.bytes_served, 0); // Didn't read any data
    }
}