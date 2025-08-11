//! Example demonstrating MIME type validation and restrictions
//!
//! This example shows how to configure the sandboxed file manager to only
//! allow specific file types based on their MIME types (detected via magic numbers).

use sandboxed_file_manager::{
    file_types::{FileTypeConfigBuilder, FileTypeValidator},
    CleanupPolicy, SandboxedManager,
};
use std::collections::HashSet;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let temp_dir = std::env::temp_dir().join("mime_validation_example");

    // Create manager
    let manager = SandboxedManager::builder()
        .base_directory(&temp_dir)
        .cleanup_policy(CleanupPolicy::disabled())
        .build()
        .await?;

    println!("MIME Type Validation Example");
    println!("Sandbox directory: {}", temp_dir.display());
    println!();

    // === EXAMPLE 1: Allow all file types (default) ===
    println!("1. Default validator (allows all file types):");
    let validator_all = FileTypeValidator::new();

    // Create test files with different types
    let png_bytes = vec![0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A]; // PNG header
    let jpeg_bytes = vec![0xFF, 0xD8, 0xFF, 0xE0]; // JPEG header
    let exe_bytes = vec![0x4D, 0x5A]; // EXE header (MZ)

    manager.write("test.png", &png_bytes).await?;
    manager.write("test.jpg", &jpeg_bytes).await?;
    manager.write("test.exe", &exe_bytes).await?;

    // Validate each file
    for filename in ["test.png", "test.jpg", "test.exe"] {
        match manager.validate_file_type(filename, &validator_all).await {
            Ok(info) => println!("  {} -> {} ({})", filename, info.mime_type, info.extension),
            Err(e) => println!("  {filename} -> Error: {e}"),
        }
    }
    println!();

    // === EXAMPLE 2: Restrict to only images ===
    println!("2. Image-only validator:");
    let validator_images = FileTypeValidator::with_config(
        FileTypeConfigBuilder::new()
            .allow_mime_type("image/png")
            .allow_mime_type("image/jpeg")
            .allow_mime_type("image/gif")
            .allow_mime_type("image/webp")
            .build(),
    );

    for filename in ["test.png", "test.jpg", "test.exe"] {
        match manager
            .validate_file_type(filename, &validator_images)
            .await
        {
            Ok(info) => println!(
                "  {} -> ALLOWED: {} ({})",
                filename, info.mime_type, info.extension
            ),
            Err(e) => println!("  {filename} -> BLOCKED: {e}"),
        }
    }
    println!();

    // === EXAMPLE 3: Custom MIME type set ===
    println!("3. Custom validator (documents + JSON only):");
    let mut allowed_types = HashSet::new();
    allowed_types.insert("application/json".to_string());
    allowed_types.insert("application/pdf".to_string());
    allowed_types.insert("text/plain".to_string());
    allowed_types.insert("application/xml".to_string());

    let validator_docs = FileTypeValidator::with_config(
        FileTypeConfigBuilder::new()
            .allowed_mime_types(allowed_types)
            .max_detection_bytes(4096) // Read first 4KB for detection
            .build(),
    );

    // Create document files
    let json_content = r#"{"name": "test", "value": 123}"#;
    let text_content = "This is plain text content";
    let xml_content = r#"<?xml version="1.0"?><root><item>test</item></root>"#;

    manager.write("data.json", json_content).await?;
    manager.write("readme.txt", text_content).await?;
    manager.write("config.xml", xml_content).await?;

    for filename in [
        "data.json",
        "readme.txt",
        "config.xml",
        "test.png",
        "test.exe",
    ] {
        match manager.validate_file_type(filename, &validator_docs).await {
            Ok(info) => println!(
                "  {} -> ALLOWED: {} ({})",
                filename, info.mime_type, info.extension
            ),
            Err(e) => println!("  {filename} -> BLOCKED: {e}"),
        }
    }
    println!();

    // === EXAMPLE 4: Check what MIME types infer supports ===
    println!("4. Testing infer crate MIME type support:");
    let test_mime_types = vec![
        "image/png",
        "image/jpeg",
        "image/gif",
        "application/pdf",
        "application/json",
        "text/plain",
        "video/mp4",
        "audio/mpeg",
        "application/zip",
        "application/x-executable",
        "application/fake-type", // This should not be supported
    ];

    for mime_type in test_mime_types {
        let supported = infer::is_mime_supported(mime_type);
        println!(
            "  {} -> {}",
            mime_type,
            if supported {
                "Supported"
            } else {
                "Not supported"
            }
        );
    }
    println!();

    // === EXAMPLE 5: Custom matcher for M3U playlists ===
    println!("5. Custom validator with M3U playlist support:");
    let validator_m3u = FileTypeValidator::with_custom_matchers(
        FileTypeConfigBuilder::new()
            .allow_mime_type("application/vnd.apple.mpegurl")
            .allow_mime_type("text/plain")
            .allow_custom_matchers(true)
            .build(),
        |infer| {
            // Add custom M3U detection
            infer.add("application/vnd.apple.mpegurl", "m3u", |buf| {
                if buf.len() >= 7 {
                    let header = std::str::from_utf8(&buf[..7]).unwrap_or("");
                    return header == "#EXTM3U";
                }
                false
            });
        },
    );

    // Create M3U playlist
    let m3u_content = "#EXTM3U\n#EXTINF:180,Artist - Song\nhttp://example.com/song.mp3\n";
    manager.write("playlist.m3u", m3u_content).await?;

    match manager
        .validate_file_type("playlist.m3u", &validator_m3u)
        .await
    {
        Ok(info) => println!(
            "  playlist.m3u -> DETECTED: {} ({})",
            info.mime_type, info.extension
        ),
        Err(e) => println!("  playlist.m3u -> Error: {e}"),
    }
    println!();

    // === EXAMPLE 6: Integration with file operations ===
    println!("6. Practical usage - validate before operations:");

    struct SecureFileManager {
        manager: SandboxedManager,
        validator: FileTypeValidator,
    }

    impl SecureFileManager {
        async fn secure_write(
            &self,
            path: &str,
            content: &[u8],
        ) -> Result<(), Box<dyn std::error::Error>> {
            // Write file first
            self.manager.write(path, content).await?;

            // Then validate its type
            match self.manager.validate_file_type(path, &self.validator).await {
                Ok(info) => {
                    println!(
                        "  Wrote and validated {}: {} ({})",
                        path, info.mime_type, info.extension
                    );
                    Ok(())
                }
                Err(e) => {
                    // Remove the file if validation fails
                    let _ = self.manager.remove_file(path).await;
                    Err(format!("File validation failed for {path}: {e}").into())
                }
            }
        }
    }

    let secure_manager = SecureFileManager {
        manager,
        validator: validator_images, // Only allow images
    };

    // Try to write different file types
    let _ = secure_manager
        .secure_write("valid_image.png", &png_bytes)
        .await;
    let _ = secure_manager
        .secure_write("invalid_exe.png", &exe_bytes)
        .await; // Will fail validation

    println!();
    println!(
        "Example completed! Check {} for created files.",
        temp_dir.display()
    );

    Ok(())
}
