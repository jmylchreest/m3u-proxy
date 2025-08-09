#[cfg(any(feature = "compression-gzip", feature = "compression-bzip2", feature = "compression-xz"))]
use std::io::Read;
#[cfg(any(feature = "compression-gzip", feature = "compression-bzip2", feature = "compression-xz"))]
use anyhow::Context;
use anyhow::Result;
use bytes::Bytes;

// Conditional imports based on enabled features
#[cfg(feature = "compression-gzip")]
use flate2::read::GzDecoder;

#[cfg(feature = "compression-bzip2")]
use bzip2::read::BzDecoder;

#[cfg(feature = "compression-xz")]
use xz2::read::XzDecoder;

/// Supported compression formats for M3U/XMLTV content
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompressionFormat {
    #[cfg(feature = "compression-gzip")]
    Gzip,
    #[cfg(feature = "compression-bzip2")]
    Bzip2,
    #[cfg(feature = "compression-xz")]
    Xz,
    Uncompressed,
}

/// Magic file detection and decompression utility
pub struct DecompressionService;

impl DecompressionService {
    /// Detect compression format using magic bytes
    pub fn detect_compression_format(data: &[u8]) -> CompressionFormat {
        if let Some(kind) = infer::get(data) {
            match kind.mime_type() {
                #[cfg(feature = "compression-gzip")]
                "application/gzip" => CompressionFormat::Gzip,
                #[cfg(feature = "compression-bzip2")]
                "application/x-bzip2" => CompressionFormat::Bzip2,
                #[cfg(feature = "compression-xz")]
                "application/x-xz" => CompressionFormat::Xz,
                _ => CompressionFormat::Uncompressed,
            }
        } else {
            CompressionFormat::Uncompressed
        }
    }

    /// Decompress data based on detected format
    pub fn decompress(data: Bytes) -> Result<Vec<u8>> {
        let format = Self::detect_compression_format(&data);
        
        match format {
            #[cfg(feature = "compression-gzip")]
            CompressionFormat::Gzip => Self::decompress_gzip(data),
            #[cfg(feature = "compression-bzip2")]
            CompressionFormat::Bzip2 => Self::decompress_bzip2(data),
            #[cfg(feature = "compression-xz")]
            CompressionFormat::Xz => Self::decompress_xz(data),
            CompressionFormat::Uncompressed => Ok(data.to_vec()),
        }
    }

    /// Decompress gzip data
    #[cfg(feature = "compression-gzip")]
    fn decompress_gzip(data: Bytes) -> Result<Vec<u8>> {
        let mut decoder = GzDecoder::new(data.as_ref());
        let mut decompressed = Vec::new();
        decoder
            .read_to_end(&mut decompressed)
            .context("Failed to decompress gzip data")?;
        Ok(decompressed)
    }

    /// Decompress bzip2 data
    #[cfg(feature = "compression-bzip2")]
    fn decompress_bzip2(data: Bytes) -> Result<Vec<u8>> {
        let mut decoder = BzDecoder::new(data.as_ref());
        let mut decompressed = Vec::new();
        decoder
            .read_to_end(&mut decompressed)
            .context("Failed to decompress bzip2 data")?;
        Ok(decompressed)
    }

    /// Decompress xz data
    #[cfg(feature = "compression-xz")]
    fn decompress_xz(data: Bytes) -> Result<Vec<u8>> {
        let mut decoder = XzDecoder::new(data.as_ref());
        let mut decompressed = Vec::new();
        decoder
            .read_to_end(&mut decompressed)
            .context("Failed to decompress xz data")?;
        Ok(decompressed)
    }

}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    
    #[cfg(feature = "compression-gzip")]
    use flate2::write::GzEncoder;
    #[cfg(feature = "compression-gzip")]
    use flate2::Compression;

    #[test]
    fn test_detect_uncompressed() {
        let data = b"Hello, world!";
        let format = DecompressionService::detect_compression_format(data);
        assert_eq!(format, CompressionFormat::Uncompressed);
    }

    #[test]
    #[cfg(feature = "compression-gzip")]
    fn test_detect_and_decompress_gzip() {
        let original_data = b"Hello, world!";
        
        // Compress data
        let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
        encoder.write_all(original_data).unwrap();
        let compressed = encoder.finish().unwrap();
        
        // Detect format
        let format = DecompressionService::detect_compression_format(&compressed);
        assert_eq!(format, CompressionFormat::Gzip);
        
        // Decompress
        let decompressed = DecompressionService::decompress(Bytes::from(compressed)).unwrap();
        assert_eq!(decompressed, original_data);
    }

    #[test]
    fn test_decompress_uncompressed() {
        let data = b"Hello, world!";
        let result = DecompressionService::decompress(Bytes::from(data.as_ref())).unwrap();
        assert_eq!(result, data);
    }
}