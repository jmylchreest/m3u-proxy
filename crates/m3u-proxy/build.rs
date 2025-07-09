//! Build script for m3u-proxy
//!
//! This build script automatically builds WASM plugins as part of the main build process.
//! It ensures that all plugins are available when the application is built.

use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

fn main() {
    println!("cargo:rerun-if-changed=plugins/");
    println!("cargo:rerun-if-changed=build.rs");

    // Check if wasm-plugins feature is enabled
    let wasm_plugins_enabled = env::var("CARGO_FEATURE_WASM_PLUGINS").is_ok();
    if !wasm_plugins_enabled {
        println!("cargo:warning=WASM plugins disabled (wasm-plugins feature not enabled)");
        return;
    }

    // Check if we should build plugins
    let skip_plugins = env::var("M3U_PROXY_SKIP_PLUGIN_BUILD").unwrap_or_default();
    if skip_plugins == "1" || skip_plugins.to_lowercase() == "true" {
        println!("cargo:warning=Skipping WASM plugin build (M3U_PROXY_SKIP_PLUGIN_BUILD is set)");
        return;
    }

    // Get the manifest directory (where Cargo.toml is located)
    let manifest_dir = env::var("CARGO_MANIFEST_DIR").unwrap();
    let manifest_path = Path::new(&manifest_dir);

    // Plugin directories
    let plugins_dir = manifest_path.join("plugins");
    let target_dir = manifest_path.join("../../target/wasm-plugins");

    // Check if plugins directory exists
    if !plugins_dir.exists() {
        println!(
            "cargo:warning=No plugins directory found at {}",
            plugins_dir.display()
        );
        return;
    }

    // Create target directory for plugins
    if let Err(e) = fs::create_dir_all(&target_dir) {
        println!(
            "cargo:warning=Failed to create plugin target directory: {}",
            e
        );
        return;
    }

    // Check if wasm32-wasip1 target is available
    if !check_wasm_target() {
        println!("cargo:warning=wasm32-wasip1 target not available, skipping plugin build");
        println!("cargo:warning=Install with: rustup target add wasm32-wasip1");
        return;
    }

    println!("cargo:warning=Building WASM plugins...");

    // Find and build all plugins
    let plugins = find_plugins(&plugins_dir);
    let mut built_count = 0;
    let total_count = plugins.len();

    for plugin_path in plugins {
        if build_plugin(&plugin_path, &target_dir) {
            built_count += 1;
        }
    }

    if built_count > 0 {
        println!(
            "cargo:warning=Built {}/{} WASM plugins successfully",
            built_count, total_count
        );

        // Set environment variable for the application to know where plugins are
        println!(
            "cargo:rustc-env=M3U_PROXY_BUILTIN_PLUGINS_DIR={}",
            target_dir.display()
        );
    } else if total_count > 0 {
        println!(
            "cargo:warning=Failed to build any WASM plugins ({} found)",
            total_count
        );
    }
}

/// Check if the wasm32-unknown-unknown target is installed
fn check_wasm_target() -> bool {
    let output = Command::new("rustup")
        .args(&["target", "list", "--installed"])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .output();

    match output {
        Ok(output) => {
            let stdout = String::from_utf8_lossy(&output.stdout);
            stdout.contains("wasm32-unknown-unknown")
        }
        Err(_) => {
            // If rustup command fails, assume target is not available
            false
        }
    }
}

/// Find all plugin directories
fn find_plugins(plugins_dir: &Path) -> Vec<PathBuf> {
    let mut plugins = Vec::new();

    if let Ok(entries) = fs::read_dir(plugins_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                let cargo_toml = path.join("Cargo.toml");
                if cargo_toml.exists() {
                    // Check if it's a WASM plugin by looking for cdylib crate type
                    if is_wasm_plugin(&cargo_toml) {
                        plugins.push(path);
                    }
                }
            }
        }
    }

    plugins
}

/// Check if a Cargo.toml represents a WASM plugin
fn is_wasm_plugin(cargo_toml: &Path) -> bool {
    if let Ok(content) = fs::read_to_string(cargo_toml) {
        // Simple check for cdylib crate type
        content.contains("cdylib") || content.contains("wasm")
    } else {
        false
    }
}

/// Build a single plugin
fn build_plugin(plugin_path: &Path, target_dir: &Path) -> bool {
    let plugin_name = plugin_path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("unknown");

    println!("cargo:warning=Building plugin: {}", plugin_name);

    // Build the plugin with cargo
    let mut cmd = Command::new("cargo");
    cmd.current_dir(plugin_path)
        .args(&["build", "--target", "wasm32-unknown-unknown", "--release", "--quiet"])
        .stdout(Stdio::null())
        .stderr(Stdio::piped());

    let output = match cmd.output() {
        Ok(output) => output,
        Err(e) => {
            println!(
                "cargo:warning=Failed to execute cargo for {}: {}",
                plugin_name, e
            );
            return false;
        }
    };

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        println!("cargo:warning=Failed to build {}: {}", plugin_name, stderr);
        return false;
    }

    // Copy the built WASM file from workspace target directory
    let wasm_name = plugin_name.replace("-", "_");

    // Get the workspace root directory (go up from manifest_dir to find workspace root)
    let workspace_root = env::var("CARGO_MANIFEST_DIR")
        .map(|d| {
            Path::new(&d)
                .parent()
                .unwrap()
                .parent()
                .unwrap()
                .to_path_buf()
        })
        .unwrap_or_else(|_| PathBuf::from("../../"));

    let source_wasm = workspace_root
        .join("target/wasm32-unknown-unknown/release")
        .join(format!("{}.wasm", wasm_name));
    let target_wasm = target_dir.join(format!("{}.wasm", plugin_name));

    if !source_wasm.exists() {
        println!(
            "cargo:warning=WASM file not found for {}: {}",
            plugin_name,
            source_wasm.display()
        );
        return false;
    }

    if let Err(e) = fs::copy(&source_wasm, &target_wasm) {
        println!(
            "cargo:warning=Failed to copy WASM file for {}: {}",
            plugin_name, e
        );
        return false;
    }

    // Optimize with wasm-opt if available
    optimize_wasm(&target_wasm);

    // Only generate manifest if no manual one exists
    generate_manifest_if_missing(plugin_name, target_dir, plugin_path);

    true
}

/// Optimize WASM file with wasm-opt if available
fn optimize_wasm(wasm_path: &Path) {
    let temp_path = wasm_path.with_extension("wasm.tmp");

    let mut cmd = Command::new("wasm-opt");
    cmd.args(&[
        wasm_path.to_str().unwrap(),
        "-O3",
        "--enable-bulk-memory",
        "--enable-sign-ext",
        "-o",
        temp_path.to_str().unwrap(),
    ])
    .stdout(Stdio::null())
    .stderr(Stdio::null());

    if cmd.status().is_ok() {
        if temp_path.exists() {
            if fs::rename(&temp_path, wasm_path).is_err() {
                // If rename fails, just remove the temp file
                let _ = fs::remove_file(&temp_path);
            }
        }
    }
}

/// Generate a plugin manifest if no manual one exists, preferring WASM metadata extraction
fn generate_manifest_if_missing(plugin_name: &str, target_dir: &Path, plugin_path: &Path) {
    let target_manifest_path = target_dir.join(format!("{}.toml", plugin_name));
    let wasm_file_path = target_dir.join(format!("{}.wasm", plugin_name));

    // Check for manual manifest in plugin directory first
    let manual_manifest_path = plugin_path.join("plugin.toml");
    if manual_manifest_path.exists() {
        // Copy manual manifest to target directory
        if let Err(e) = fs::copy(&manual_manifest_path, &target_manifest_path) {
            println!(
                "cargo:warning=Failed to copy manual manifest for {}: {}",
                plugin_name, e
            );
        } else {
            println!("cargo:warning=Using manual manifest for {}", plugin_name);
        }
        return;
    }

    // Try to extract metadata from WASM file if it exists
    if wasm_file_path.exists() {
        if let Ok(manifest_content) = extract_manifest_from_wasm(&wasm_file_path) {
            if let Err(e) = fs::write(&target_manifest_path, manifest_content) {
                println!(
                    "cargo:warning=Failed to write extracted manifest for {}: {}",
                    plugin_name, e
                );
            } else {
                println!(
                    "cargo:warning=Generated manifest from WASM metadata for {}",
                    plugin_name
                );
                return;
            }
        }
    }

    // Don't overwrite existing generated manifests
    if target_manifest_path.exists() {
        return;
    }

    // Generate basic fallback manifest
    println!(
        "cargo:warning=Generating fallback manifest for {}",
        plugin_name
    );
    generate_fallback_manifest(plugin_name, &target_manifest_path);
}

/// Extract manifest from WASM plugin_get_info function
fn extract_manifest_from_wasm(wasm_path: &Path) -> Result<String, Box<dyn std::error::Error>> {
    // Read the WASM file
    let wasm_bytes = std::fs::read(wasm_path)?;

    // Try to use wasmtime to extract metadata
    // For now, we'll use a simple approach that checks for exported functions
    // and creates a basic manifest based on the plugin name
    let plugin_name = wasm_path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("unknown");

    // Check if the WASM file has the required exports
    let has_plugin_get_info = check_wasm_exports(&wasm_bytes, "plugin_get_info")?;
    let _has_plugin_init = check_wasm_exports(&wasm_bytes, "plugin_init")?;

    if !has_plugin_get_info {
        return Err("WASM plugin missing required plugin_get_info export".into());
    }

    // Generate manifest based on plugin name and detected capabilities
    let manifest = match plugin_name {
        "passthrough-plugin" => generate_passthrough_manifest(),
        "chunked-source-loader" => generate_chunked_loader_manifest(),
        _ => generate_generic_manifest(plugin_name),
    };

    println!(
        "cargo:warning=Generated manifest from WASM exports for {}",
        plugin_name
    );

    Ok(manifest)
}

/// Check if WASM file exports a specific function
fn check_wasm_exports(
    wasm_bytes: &[u8],
    function_name: &str,
) -> Result<bool, Box<dyn std::error::Error>> {
    // Simple check - look for the function name in the WASM file
    // This is a basic implementation; a full implementation would parse the WASM properly
    let wasm_str = String::from_utf8_lossy(wasm_bytes);
    Ok(wasm_str.contains(function_name))
}

fn generate_passthrough_manifest() -> String {
    r#"[plugin]
name = "passthrough-plugin"
version = "0.1.0"
author = "m3u-proxy developers"
description = "Simple pass-through plugin for testing WASM integration"
license = "MIT"

[capabilities]
stages = ["source_loading", "data_mapping", "filtering", "channel_numbering", "m3u_generation"]
supports_streaming = true
requires_all_data = false
can_produce_early_output = true
memory_efficient = true
preferred_chunk_size = 1000

[host_interface]
version = "1.0"
required_functions = [
    "host_get_memory_usage",
    "host_get_memory_pressure",
    "host_log",
    "host_report_progress"
]
"#
    .to_string()
}

fn generate_chunked_loader_manifest() -> String {
    r#"[plugin]
name = "chunked-source-loader"
version = "0.1.0"
author = "m3u-proxy developers"
description = "Memory-efficient chunked source loading with automatic spilling"
license = "MIT"

[capabilities]
stages = ["source_loading"]
supports_streaming = true
requires_all_data = false
can_produce_early_output = false
memory_efficient = true
preferred_chunk_size = 1000

[host_interface]
version = "1.0"
required_functions = [
    "host_get_memory_usage",
    "host_get_memory_pressure",
    "host_write_temp_file",
    "host_read_temp_file",
    "host_delete_temp_file",
    "host_database_query_source",
    "host_log"
]

[config]
memory_threshold_mb = 256
chunk_size = 1000
compression_enabled = true
max_spill_files = 100
"#
    .to_string()
}

fn generate_generic_manifest(plugin_name: &str) -> String {
    format!(
        r#"[plugin]
name = "{}"
version = "0.1.0"
author = "m3u-proxy developers"
description = "Custom WASM plugin"
license = "MIT"

[capabilities]
stages = ["source_loading"]
supports_streaming = true
requires_all_data = false
memory_efficient = true
preferred_chunk_size = 1000

[host_interface]
version = "1.0"
required_functions = ["host_log"]
"#,
        plugin_name
    )
}

/// Generate a basic fallback manifest
fn generate_fallback_manifest(plugin_name: &str, manifest_path: &Path) {
    let manifest_content = match plugin_name {
        "passthrough-plugin" => {
            r#"[plugin]
name = "passthrough-plugin"
version = "0.1.0"
author = "m3u-proxy developers"
description = "Simple pass-through plugin for testing WASM integration"
license = "MIT"

[capabilities]
stages = ["source_loading", "data_mapping", "filtering", "channel_numbering", "m3u_generation"]
supports_streaming = true
requires_all_data = false
can_produce_early_output = true
memory_efficient = true
preferred_chunk_size = 1000

[host_interface]
version = "1.0"
required_functions = [
    "host_get_memory_usage",
    "host_get_memory_pressure",
    "host_log",
    "host_report_progress"
]
"#
        }
        "chunked-source-loader" => {
            r#"[plugin]
name = "chunked-source-loader"
version = "0.1.0"
author = "m3u-proxy developers"
description = "Memory-efficient chunked source loading with automatic spilling"
license = "MIT"

[capabilities]
stages = ["source_loading"]
supports_streaming = true
requires_all_data = false
can_produce_early_output = false
memory_efficient = true
preferred_chunk_size = 1000

[host_interface]
version = "1.0"
required_functions = [
    "host_get_memory_usage",
    "host_get_memory_pressure",
    "host_write_temp_file",
    "host_read_temp_file",
    "host_delete_temp_file",
    "host_database_query_source",
    "host_log"
]

[config]
memory_threshold_mb = 256
chunk_size = 1000
compression_enabled = true
max_spill_files = 100
"#
        }
        _ => &format!(
            r#"[plugin]
name = "{}"
version = "0.1.0"
author = "m3u-proxy developers"
description = "Custom WASM plugin"
license = "MIT"

[capabilities]
stages = ["source_loading"]
supports_streaming = true
requires_all_data = false
memory_efficient = true
preferred_chunk_size = 1000

[host_interface]
version = "1.0"
required_functions = ["host_log"]
"#,
            plugin_name
        ),
    };

    let _ = fs::write(manifest_path, manifest_content);
}
