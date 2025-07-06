//! WASM pipeline plugins
//!
//! This module provides WASM-based pipeline plugins for processing
//! data during generation stages with WebAssembly isolation and safety.

pub mod plugin;

// Re-export key types
pub use plugin::{WasmPlugin, WasmPluginConfig, WasmPluginManager, PluginIteratorContext};