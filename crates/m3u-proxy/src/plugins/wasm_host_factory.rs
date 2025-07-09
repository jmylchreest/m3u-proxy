//! WASM Host Function Factory
//!
//! This module provides reusable factory methods for creating WASM host functions,
//! eliminating duplication across different WASM plugin implementations.

use std::sync::Arc;
use tracing::{error, info};
use wasmtime::{Caller, Func, Memory, Store};

use crate::logo_assets::service::LogoAssetService;

/// Context for host function creation
#[derive(Clone)]
pub struct HostFunctionContext {
    pub logo_service: Option<Arc<LogoAssetService>>,
    pub base_url: String,
    pub memory: Memory,
}

/// Factory for creating reusable WASM host functions
pub struct WasmHostFunctionFactory {
    context: HostFunctionContext,
}

impl WasmHostFunctionFactory {
    pub fn new(context: HostFunctionContext) -> Self {
        Self { context }
    }

    /// Create logging host functions (standardized naming)
    pub fn create_logging_functions(&self, store: &mut Store<()>) -> Vec<(&'static str, Func)> {
        vec![
            ("host_log_write", self.create_host_log_write(store)),
        ]
    }

    /// Create memory management host functions (standardized naming)
    pub fn create_memory_functions(&self, store: &mut Store<()>) -> Vec<(&'static str, Func)> {
        vec![
            ("host_memory_get_usage", self.create_host_memory_get_usage(store)),
            ("host_memory_get_pressure", self.create_host_memory_get_pressure(store)),
            ("host_system_flush_memory", self.create_host_system_flush_memory(store)),
            // Backward compatibility
            ("host_get_memory_usage", self.create_host_memory_get_usage(store)),
            ("host_get_memory_pressure", self.create_host_memory_get_pressure(store)),
        ]
    }

    /// Create progress reporting host functions (standardized naming)
    pub fn create_progress_functions(&self, store: &mut Store<()>) -> Vec<(&'static str, Func)> {
        vec![
            ("host_progress_report", self.create_host_progress_report(store)),
            // Backward compatibility
            ("host_report_progress", self.create_host_progress_report(store)),
        ]
    }

    /// Create logo caching host functions (standardized naming)
    pub fn create_logo_functions(&self, store: &mut Store<()>) -> Vec<(&'static str, Func)> {
        if self.context.logo_service.is_some() {
            vec![
                ("host_logo_cache", self.create_host_logo_cache(store)),
                // Backward compatibility
                ("host_cache_logo", self.create_host_logo_cache(store)),
            ]
        } else {
            vec![]
        }
    }

    /// Create iterator management host functions (standardized naming)
    pub fn create_iterator_functions(&self, store: &mut Store<()>) -> Vec<(&'static str, Func)> {
        vec![
            // Input iterator functions (standardized)
            ("host_iterator_read_chunk", self.create_host_iterator_read_chunk(store)),
            ("host_iterator_close", self.create_host_iterator_close(store)),
            
            // Output iterator functions (standardized)
            ("host_iterator_create", self.create_host_iterator_create(store)),
            ("host_iterator_write_chunk", self.create_host_iterator_write_chunk(store)),
            ("host_iterator_finalize", self.create_host_iterator_finalize(store)),
            
            // Backward compatibility
            ("host_iterator_next_chunk", self.create_host_iterator_read_chunk(store)),
        ]
    }

    /// Get all host functions as a combined vector
    pub fn create_all_functions(&self, store: &mut Store<()>) -> Vec<(&'static str, Func)> {
        let mut functions = Vec::new();
        functions.extend(self.create_logging_functions(store));
        functions.extend(self.create_memory_functions(store));
        functions.extend(self.create_progress_functions(store));
        functions.extend(self.create_logo_functions(store));
        functions.extend(self.create_iterator_functions(store));
        functions
    }

    // Individual host function implementations

    fn create_host_log_write(&self, store: &mut Store<()>) -> Func {
        Func::wrap(store, |level: u32, msg_ptr: u32, msg_len: u32| {
            let log_level = match level {
                1 => "ERROR",
                2 => "WARN",
                3 => "INFO",
                4 => "DEBUG",
                _ => "INFO",
            };
            info!("WASM plugin log ({}): message at ptr={}, len={}", log_level, msg_ptr, msg_len);
        })
    }

    fn create_host_memory_get_usage(&self, store: &mut Store<()>) -> Func {
        Func::wrap(store, || -> u64 {
            256 * 1024 * 1024 // 256MB - could be made configurable
        })
    }

    fn create_host_memory_get_pressure(&self, store: &mut Store<()>) -> Func {
        Func::wrap(store, || -> u32 {
            1 // Optimal - could integrate with real memory monitoring
        })
    }

    fn create_host_progress_report(&self, store: &mut Store<()>) -> Func {
        Func::wrap(store, |stage_ptr: u32, stage_len: u32, processed: u32, total: u32| {
            info!("WASM plugin progress: stage@{}:{}, {}/{}", stage_ptr, stage_len, processed, total);
        })
    }

    fn create_host_logo_cache(&self, store: &mut Store<()>) -> Func {
        let memory_ref = self.context.memory.clone();
        let logo_service = self.context.logo_service.clone();
        let base_url = self.context.base_url.clone();

        Func::wrap(store, move |caller: Caller<'_, ()>, url_ptr: u32, url_len: u32, uuid_out_ptr: u32, uuid_out_len: u32| -> i32 {
            info!("host_cache_logo called: url_ptr={}, url_len={}", url_ptr, url_len);

            // Read URL string from WASM memory
            let memory_data = memory_ref.data(&caller);

            if url_ptr as usize + url_len as usize > memory_data.len() {
                error!("host_cache_logo: URL memory access out of bounds");
                return -1; // Error
            }

            let url_bytes = &memory_data[url_ptr as usize..(url_ptr + url_len) as usize];
            let url = match String::from_utf8(url_bytes.to_vec()) {
                Ok(s) => s,
                Err(e) => {
                    error!("host_cache_logo: Invalid UTF-8 in URL: {}", e);
                    return -1;
                }
            };

            info!("host_cache_logo: Processing URL: {}", url);

            // For now, return a mock UUID
            let mock_uuid = "12345678-1234-5678-9012-123456789012";
            let uuid_bytes = mock_uuid.as_bytes();
            
            if uuid_out_len < uuid_bytes.len() as u32 {
                error!("host_cache_logo: Output buffer too small");
                return -1;
            }

            // Write UUID to output buffer (would need mutable memory access in real implementation)
            info!("host_cache_logo: Would write UUID {} to ptr={}", mock_uuid, uuid_out_ptr);
            
            0 // Success
        })
    }

    fn create_host_iterator_read_chunk(&self, store: &mut Store<()>) -> Func {
        Func::wrap(store, |iterator_id: u32| -> i32 {
            info!("host_iterator_next_chunk called: iterator_id={}", iterator_id);
            // TODO: Implement actual iterator integration
            0 // Success
        })
    }

    fn create_host_iterator_close(&self, store: &mut Store<()>) -> Func {
        Func::wrap(store, |iterator_id: u32| -> i32 {
            info!("host_iterator_close called: iterator_id={}", iterator_id);
            // TODO: Implement actual iterator cleanup
            0 // Success
        })
    }
    
    /// Create host function for system memory flushing
    fn create_host_system_flush_memory(&self, store: &mut Store<()>) -> Func {
        Func::wrap(store, || -> i32 {
            info!("host_system_flush_memory called");
            // TODO: Implement actual memory flushing
            0 // Success
        })
    }
    
    /// Create host function for creating output iterators
    fn create_host_iterator_create(&self, store: &mut Store<()>) -> Func {
        Func::wrap(store, |iterator_type: u32| -> u32 {
            info!("host_iterator_create called: iterator_type={}", iterator_type);
            // TODO: Implement actual iterator creation with registry
            999 // Mock iterator ID
        })
    }
    
    /// Create host function for writing chunks to output iterators
    fn create_host_iterator_write_chunk(&self, store: &mut Store<()>) -> Func {
        Func::wrap(store, |iterator_id: u32, data_ptr: u32, data_len: u32, items_count: u32| -> i32 {
            info!("host_iterator_write_chunk called: iterator_id={}, data_len={}, items_count={}", 
                  iterator_id, data_len, items_count);
            // TODO: Implement actual chunk writing to iterator registry
            0 // Success
        })
    }
    
    /// Create host function for finalizing output iterators
    fn create_host_iterator_finalize(&self, store: &mut Store<()>) -> Func {
        Func::wrap(store, |iterator_id: u32| -> i32 {
            info!("host_iterator_finalize called: iterator_id={}", iterator_id);
            // TODO: Implement actual iterator finalization
            0 // Success
        })
    }
}

/// Builder pattern for creating host function context
pub struct HostFunctionContextBuilder {
    logo_service: Option<Arc<LogoAssetService>>,
    base_url: Option<String>,
    memory: Option<Memory>,
}

impl HostFunctionContextBuilder {
    pub fn new() -> Self {
        Self {
            logo_service: None,
            base_url: None,
            memory: None,
        }
    }

    pub fn with_logo_service(mut self, logo_service: Arc<LogoAssetService>) -> Self {
        self.logo_service = Some(logo_service);
        self
    }

    pub fn with_base_url(mut self, base_url: String) -> Self {
        self.base_url = Some(base_url);
        self
    }

    pub fn with_memory(mut self, memory: Memory) -> Self {
        self.memory = Some(memory);
        self
    }

    pub fn build(self) -> Result<HostFunctionContext, &'static str> {
        Ok(HostFunctionContext {
            logo_service: self.logo_service,
            base_url: self.base_url.unwrap_or_else(|| "http://localhost".to_string()),
            memory: self.memory.ok_or("Memory is required")?,
        })
    }
}

impl Default for HostFunctionContextBuilder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wasmtime::{Engine, MemoryType, Store};

    #[test]
    fn test_factory_creation() {
        let engine = Engine::default();
        let mut store = Store::new(&engine, ());
        let memory_type = MemoryType::new(1, Some(2));
        let memory = Memory::new(&mut store, memory_type).unwrap();

        let context = HostFunctionContextBuilder::new()
            .with_base_url("http://test.com".to_string())
            .with_memory(memory)
            .build()
            .unwrap();

        let factory = WasmHostFunctionFactory::new(context);
        let functions = factory.create_all_functions(&mut store);

        // Should have at least logging, memory, progress, and iterator functions
        assert!(functions.len() >= 6);
        
        // Check that essential functions are present
        let function_names: Vec<_> = functions.iter().map(|(name, _)| *name).collect();
        assert!(function_names.contains(&"host_log"));
        assert!(function_names.contains(&"host_get_memory_usage"));
        assert!(function_names.contains(&"host_report_progress"));
    }
}