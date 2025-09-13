//! Embedded Font Support for FFmpeg Error Videos
//!
//! This module provides embedded font capabilities for FFmpeg text rendering
//! to avoid external font dependencies.

use anyhow::Result;
use std::path::PathBuf;
use tempfile::NamedTempFile;
use tracing::{debug, warn};

/// Embedded font data (using Liberation Sans Narrow as a free alternative to Arial)
/// This is a compact, readable sans-serif font suitable for error messages
const LIBERATION_SANS_TTF: &[u8] =
    include_bytes!("../assets/fonts/LiberationSansNarrow-Regular.ttf");

/// Font manager for handling embedded fonts
pub struct EmbeddedFontManager {
    temp_font_path: Option<PathBuf>,
}

impl Default for EmbeddedFontManager {
    fn default() -> Self {
        Self::new()
    }
}

impl EmbeddedFontManager {
    pub fn new() -> Self {
        Self {
            temp_font_path: None,
        }
    }

    /// Get the path to the embedded font, creating a temporary file if needed
    /// Returns None if no embedded font is available
    pub async fn get_font_path(&mut self) -> Result<Option<&PathBuf>> {
        if self.temp_font_path.is_none() {
            self.create_temp_font_file().await?;
        }

        Ok(self.temp_font_path.as_ref())
    }

    /// Create a temporary file with the embedded font data
    async fn create_temp_font_file(&mut self) -> Result<()> {
        let mut temp_file = NamedTempFile::new()?;

        // Write embedded font data to temporary file
        use std::io::Write;
        temp_file.write_all(LIBERATION_SANS_TTF)?;
        temp_file.flush()?;

        // Get the path and persist the file
        let temp_path = temp_file.into_temp_path();
        let font_path = temp_path.to_path_buf();

        // Keep the temp file alive by not calling temp_path.close()
        temp_path.keep()?;

        self.temp_font_path = Some(font_path.clone());
        debug!("Created temporary font file at: {:?}", font_path);

        Ok(())
    }

    /// Get font file parameter for FFmpeg
    pub async fn get_ffmpeg_font_param(&mut self) -> Result<Option<String>> {
        if let Some(font_path) = self.get_font_path().await? {
            Ok(Some(format!("fontfile={}", font_path.display())))
        } else {
            Ok(None)
        }
    }

    /// Get font family name for FFmpeg
    pub fn get_font_family() -> &'static str {
        "Liberation Sans Narrow"
    }
}

impl Drop for EmbeddedFontManager {
    fn drop(&mut self) {
        if let Some(font_path) = &self.temp_font_path {
            if let Err(e) = std::fs::remove_file(font_path) {
                warn!(
                    "Failed to clean up temporary font file {:?}: {}",
                    font_path, e
                );
            } else {
                debug!("Cleaned up temporary font file: {:?}", font_path);
            }
        }
    }
}

/// Create a fallback font configuration when embedded font is not available
pub fn get_fallback_font_config() -> String {
    // Try common system fonts as fallback
    let fallback_fonts = ["DejaVu Sans", "Arial", "Helvetica", "sans-serif"];

    format!("font={}", fallback_fonts[0])
}

/// Placeholder for the actual font file - in a real implementation,
/// you would download Liberation Sans Regular from:
/// https://github.com/liberationfonts/liberation-fonts/releases
/// and place it in crates/m3u-proxy/src/assets/fonts/LiberationSans-Regular.ttf
///
/// For now, we'll create a minimal placeholder
const _FONT_PLACEHOLDER: &[u8] = &[
    // TTF header placeholder - this would be replaced with actual font data
    0x00, 0x01, 0x00, 0x00, // version
    0x00, 0x0A, // numTables
    0x00, 0x80, // searchRange
    0x00, 0x03, // entrySelector
    0x00, 0x20, // rangeShift
          // ... rest of font data would go here
];

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_font_manager() {
        let mut font_manager = EmbeddedFontManager::new();

        // Test getting font path (should now be available)
        let font_path = font_manager.get_font_path().await.unwrap();
        assert!(
            font_path.is_some(),
            "Font path should be Some when embedded font is available"
        );

        if let Some(path) = font_path {
            assert!(path.exists(), "Font file should exist");
            // Temporary files might not end with .ttf, just check that we have a path
            assert!(
                !path.to_string_lossy().is_empty(),
                "Font file path should not be empty"
            );
        }

        // Test FFmpeg parameter generation (should now be available)
        let ffmpeg_param = font_manager.get_ffmpeg_font_param().await.unwrap();
        assert!(
            ffmpeg_param.is_some(),
            "FFmpeg param should be Some when embedded font is available"
        );

        if let Some(param) = ffmpeg_param {
            assert!(
                param.starts_with("fontfile="),
                "FFmpeg param should start with fontfile="
            );
        }

        // Test font family
        assert_eq!(
            EmbeddedFontManager::get_font_family(),
            "Liberation Sans Narrow"
        );

        // Test fallback font config
        let fallback = get_fallback_font_config();
        assert!(
            fallback.contains("font="),
            "Fallback should provide a font parameter"
        );
    }
}
