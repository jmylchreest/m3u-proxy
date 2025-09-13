//! Memory Statistics Collection
//!
//! This module provides comprehensive memory usage statistics for the health endpoint

use crate::web::responses::{MemoryBreakdown, ProcessMemoryBreakdown};
use std::sync::Arc;
use sysinfo::{Pid, ProcessesToUpdate, System};
use tokio::sync::RwLock;

/// Collect comprehensive memory statistics
pub async fn get_memory_breakdown(
    system: Arc<RwLock<System>>,
    relay_manager: &crate::services::relay_manager::RelayManager,
) -> MemoryBreakdown {
    // Minimize write lock duration - refresh and extract data quickly
    let (
        total_memory,
        used_memory,
        free_memory,
        available_memory,
        swap_used,
        swap_total,
        current_pid,
    ) = {
        let mut sys = system.write().await;
        sys.refresh_memory();
        sys.refresh_processes(ProcessesToUpdate::All, true);

        let total_memory = sys.total_memory() as f64 / (1024.0 * 1024.0); // Convert from bytes to MB
        let used_memory = sys.used_memory() as f64 / (1024.0 * 1024.0);
        let free_memory = sys.free_memory() as f64 / (1024.0 * 1024.0);
        let available_memory = sys.available_memory() as f64 / (1024.0 * 1024.0);
        let swap_used = sys.used_swap() as f64 / (1024.0 * 1024.0);
        let swap_total = sys.total_swap() as f64 / (1024.0 * 1024.0);
        let current_pid = std::process::id();

        (
            total_memory,
            used_memory,
            free_memory,
            available_memory,
            swap_used,
            swap_total,
            current_pid,
        )
    }; // Write lock released here

    // Calculate process memory usage with read-only access
    let process_memory = {
        let sys = system.read().await;
        calculate_process_memory(&sys, current_pid, relay_manager).await
    };

    MemoryBreakdown {
        total_memory_mb: total_memory,
        used_memory_mb: used_memory,
        free_memory_mb: free_memory,
        available_memory_mb: available_memory,
        swap_used_mb: swap_used,
        swap_total_mb: swap_total,
        process_memory,
    }
}

/// Calculate memory usage for the m3u-proxy process tree
async fn calculate_process_memory(
    system: &System,
    main_pid: u32,
    relay_manager: &crate::services::relay_manager::RelayManager,
) -> ProcessMemoryBreakdown {
    let mut main_process_memory = 0.0f64;
    let mut child_processes_memory = 0.0f64;
    let mut child_process_count = 0u32;

    // Get main process memory
    if let Some(process) = system.process(Pid::from(main_pid as usize)) {
        main_process_memory = process.memory() as f64 / (1024.0 * 1024.0); // Convert from bytes to MB
    }

    // Get memory usage from relay processes (FFmpeg children)
    if let Ok(relay_processes) = relay_manager.get_relay_metrics().await {
        for _process_info in relay_processes {
            // RelayProcessMetrics doesn't have direct memory usage,
            // so we'll calculate it from process IDs if available
            child_process_count += 1;
            // TODO: Get actual memory usage from process ID when available
            // For now, estimate based on typical FFmpeg memory usage
            child_processes_memory += 50.0; // Estimate 50MB per relay process
        }
    }

    // Find additional child processes by scanning the process tree
    let additional_children = find_child_processes(system, main_pid);
    for child_pid in additional_children {
        if let Some(process) = system.process(Pid::from(child_pid as usize)) {
            let child_memory = process.memory() as f64 / (1024.0 * 1024.0); // Convert from bytes to MB
            child_processes_memory += child_memory;
            child_process_count += 1;
        }
    }

    let total_process_tree_memory = main_process_memory + child_processes_memory;

    // Calculate percentage of system memory
    let total_system_memory = system.total_memory() as f64 / (1024.0 * 1024.0); // Convert from bytes to MB
    let percentage_of_system = if total_system_memory > 0.0 {
        (total_process_tree_memory / total_system_memory) * 100.0
    } else {
        0.0
    };

    ProcessMemoryBreakdown {
        main_process_mb: main_process_memory,
        child_processes_mb: child_processes_memory,
        total_process_tree_mb: total_process_tree_memory,
        percentage_of_system,
        child_process_count,
    }
}

/// Find child processes of the main process
fn find_child_processes(system: &System, parent_pid: u32) -> Vec<u32> {
    let mut children = Vec::new();

    for (pid, process) in system.processes() {
        if let Some(parent) = process.parent()
            && parent.as_u32() == parent_pid
        {
            children.push(pid.as_u32());
            // Recursively find grandchildren
            let grandchildren = find_child_processes(system, pid.as_u32());
            children.extend(grandchildren);
        }
    }

    children
}
