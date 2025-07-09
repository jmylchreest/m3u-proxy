//! Streaming support for WASM plugins
//!
//! This module provides streaming iterators that allow WASM plugins to
//! output data progressively rather than in a single batch.

use std::sync::Arc;
use tokio::sync::mpsc;
use anyhow::Result;
use async_trait::async_trait;

use crate::pipeline::{PipelineIterator, IteratorResult};
use crate::models::Channel;

/// Writer side of a streaming channel
pub struct ChannelStreamWriter {
    tx: mpsc::Sender<Vec<Channel>>,
}

impl ChannelStreamWriter {
    pub fn new(tx: mpsc::Sender<Vec<Channel>>) -> Self {
        Self { tx }
    }
    
    /// Push a chunk of channels to the stream
    pub async fn push_chunk(&self, channels: Vec<Channel>) -> Result<()> {
        self.tx.send(channels).await
            .map_err(|_| anyhow::anyhow!("Stream receiver dropped"))?;
        Ok(())
    }
    
    /// Signal completion of the stream
    pub fn complete(self) {
        // Dropping the sender signals completion
        drop(self.tx);
    }
}

/// Reader side of a streaming channel iterator
pub struct ChannelStreamIterator {
    rx: mpsc::Receiver<Vec<Channel>>,
    current_chunk: Vec<Channel>,
    position: usize,
}

impl ChannelStreamIterator {
    pub fn new(rx: mpsc::Receiver<Vec<Channel>>) -> Self {
        Self {
            rx,
            current_chunk: Vec::new(),
            position: 0,
        }
    }
}

#[async_trait]
impl PipelineIterator<Channel> for ChannelStreamIterator {
    async fn next_chunk(&mut self) -> Result<IteratorResult<Channel>> {
        // If we have data in current chunk, return it
        if self.position < self.current_chunk.len() {
            let chunk = self.current_chunk[self.position..].to_vec();
            self.position = self.current_chunk.len();
            return Ok(IteratorResult::Chunk(chunk));
        }
        
        // Try to get next chunk from stream
        match self.rx.recv().await {
            Some(chunk) => {
                self.current_chunk = chunk;
                self.position = 0;
                self.next_chunk().await
            }
            None => Ok(IteratorResult::Exhausted),
        }
    }
    
    fn reset(&mut self) -> Result<()> {
        Err(anyhow::anyhow!("Streaming iterators cannot be reset"))
    }
}

/// Streaming context for WASM plugin execution
pub struct StreamingPluginContext {
    /// Input iterator for the plugin to consume
    pub input_iterator_id: u32,
    
    /// Output writer for the plugin to stream results
    pub output_writer: Arc<tokio::sync::Mutex<ChannelStreamWriter>>,
    
    /// Iterator registry for managing iterators
    pub iterator_registry: Arc<crate::pipeline::IteratorRegistry>,
}

/// Host function implementations for streaming
pub mod host_functions {
    use super::*;
    use wasmtime::*;
    
    /// Create host function for streaming output chunks
    pub fn create_host_output_chunk(
        output_writer: Arc<tokio::sync::Mutex<ChannelStreamWriter>>,
    ) -> impl Fn(Caller<'_, ()>, u32, u32, u32, u32) -> i32 {
        move |mut caller: Caller<'_, ()>, stage_id: u32, chunk_ptr: u32, chunk_len: u32, is_last: u32| {
            let memory = match caller.get_export("memory").and_then(|e| e.into_memory()) {
                Some(mem) => mem,
                None => return -1,
            };
            
            // Read chunk data from WASM memory
            let data = memory.data(&caller);
            if chunk_ptr as usize + chunk_len as usize > data.len() {
                return -1;
            }
            
            let chunk_data = &data[chunk_ptr as usize..(chunk_ptr as usize + chunk_len as usize)];
            
            // Deserialize channels
            let channels: Vec<Channel> = match serde_json::from_slice(chunk_data) {
                Ok(ch) => ch,
                Err(_) => return -2,
            };
            
            // Push to stream
            let writer = output_writer.clone();
            let rt = tokio::runtime::Handle::current();
            rt.block_on(async {
                let writer = writer.lock().await;
                if let Err(_) = writer.push_chunk(channels).await {
                    return -3;
                }
                
                // If last chunk, complete the stream
                if is_last != 0 {
                    drop(writer); // Release lock before complete
                    let writer = output_writer.lock().await;
                    // Taking ownership to call complete() would require restructuring
                    // For now, we'll handle completion differently
                }
                
                0
            })
        }
    }
}

/// Modified execute method that returns a streaming iterator
impl WasmPlugin {
    pub async fn execute_with_streaming(
        &self,
        stage: &str,
        input_iterator: Box<dyn PipelineIterator<Channel> + Send + Sync>,
    ) -> Result<Box<dyn PipelineIterator<Channel> + Send + Sync>> {
        // Create streaming channel
        let (tx, rx) = mpsc::channel(100); // Buffered for backpressure
        let output_iterator = ChannelStreamIterator::new(rx);
        let output_writer = Arc::new(tokio::sync::Mutex::new(ChannelStreamWriter::new(tx)));
        
        // Clone what we need for the spawned task
        let module = self.module.as_ref()
            .ok_or_else(|| anyhow::anyhow!("WASM module not loaded"))?
            .clone();
        let engine = self.engine.clone();
        let stage_name = stage.to_string();
        let plugin_name = self.info.name.clone();
        let writer_clone = output_writer.clone();
        
        // Spawn WASM execution in background
        tokio::spawn(async move {
            let result = tokio::task::spawn_blocking(move || -> Result<()> {
                let mut store = Store::new(&engine, ());
                
                // Set up WASM instance with streaming host functions
                let mut linker = Linker::new(&engine);
                
                // Add streaming host function
                linker.func_wrap("env", "host_output_chunk", 
                    host_functions::create_host_output_chunk(writer_clone)
                )?;
                
                // Create instance and execute
                let instance = Instance::new(&mut store, &module, &[])?;
                
                // Execute plugin in streaming mode
                // ... plugin execution logic ...
                
                Ok(())
            }).await;
            
            match result {
                Ok(Ok(())) => {
                    tracing::info!("Plugin {} streaming completed successfully", plugin_name);
                }
                Ok(Err(e)) => {
                    tracing::error!("Plugin {} streaming failed: {}", plugin_name, e);
                }
                Err(e) => {
                    tracing::error!("Plugin {} task panicked: {}", plugin_name, e);
                }
            }
            
            // Ensure stream is completed
            drop(output_writer);
        });
        
        // Return streaming iterator immediately
        Ok(Box::new(output_iterator))
    }
}