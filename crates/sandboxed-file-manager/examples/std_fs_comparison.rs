//! Comparison example showing how to use `SandboxedManager` like `std::fs`
//
//! This example demonstrates how the `SandboxedManager` provides a drop-in
//! replacement for standard Rust file operations, but with sandbox security.
//
//! NOTE: This example uses stdout output for demonstration purposes; printing
//! is intentional to illustrate typical usage patterns during development.
#![allow(clippy::print_stdout, clippy::doc_markdown)]

use sandboxed_file_manager::{CleanupPolicy, SandboxedManager};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Set up a sandboxed manager
    let temp_dir = std::env::temp_dir().join("sandbox_example");
    let manager = SandboxedManager::builder()
        .base_directory(&temp_dir)
        .cleanup_policy(CleanupPolicy::disabled()) // Disable for this example
        .build()
        .await?;

    println!("Sandboxed file operations example");
    println!("Sandbox directory: {}", temp_dir.display());
    println!();

    // === STANDARD std::fs OPERATIONS ===
    println!("Standard file operations (but sandboxed):");

    // Write a file (like std::fs::write)
    manager
        .write("hello.txt", "Hello, sandboxed world!")
        .await?;
    println!("Wrote file: hello.txt");

    // Read a file (like std::fs::read_to_string)
    let content = manager.read_to_string("hello.txt").await?;
    println!("Read content: '{content}'");

    // Read as bytes (like std::fs::read)
    let bytes = manager.read("hello.txt").await?;
    println!("Read {} bytes", bytes.len());

    // Get file metadata (like std::fs::metadata)
    let metadata = manager.metadata("hello.txt").await?;
    println!("File size: {} bytes", metadata.len());

    println!();

    // === DIRECTORY OPERATIONS ===
    println!("Directory operations:");

    // Create nested directories (like std::fs::create_dir_all)
    manager.create_dir_all("config/app/logs").await?;
    println!("Created nested directories: config/app/logs");

    // Write files in nested structure
    manager
        .write("config/app.json", r#"{"name": "MyApp", "debug": true}"#)
        .await?;
    manager
        .write(
            "config/app/settings.toml",
            "[database]\nurl = \"localhost\"",
        )
        .await?;
    manager
        .write(
            "config/app/logs/app.log",
            "2024-01-01 INFO: Application started",
        )
        .await?;
    println!("Created configuration files");

    println!();

    // === PATH TRAVERSAL HANDLING ===
    println!("Path traversal and security:");

    // Valid path traversal (resolves within sandbox)
    manager
        .write("deep/nested/../other/file.txt", "Valid traversal")
        .await?;
    let content = manager.read_to_string("deep/other/file.txt").await?;
    println!("Valid traversal: deep/nested/../other/file.txt -> deep/other/file.txt");
    println!("   Content: '{content}'");

    // Show what happens with invalid paths
    println!("Testing security violations:");

    let results = vec![
        ("../../../etc/passwd", "Path escape attempt"),
        ("/etc/passwd", "Absolute path"),
        ("file\0.txt", "Null byte in filename"),
    ];

    for (bad_path, description) in results {
        match manager.write(bad_path, "malicious").await {
            Ok(()) => println!("   SECURITY BREACH: {description} should have been blocked!"),
            Err(_) => println!("   Blocked: {description}"),
        }
    }

    println!();

    // === FILE MANAGEMENT ===
    println!("File management:");

    // List what we created (via registry)
    let stats = manager.stats().await;
    println!("Total files managed: {}", stats.total_files);
    println!("Total size: {} bytes", stats.total_size_bytes);

    println!();
    println!("Example completed successfully!");
    println!(
        "All operations were safely contained within: {}",
        temp_dir.display()
    );

    Ok(())
}
