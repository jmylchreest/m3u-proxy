use std::io::Read;
use anyhow::{Context, Result};
use bytes::Bytes;
use flate2::read::GzDecoder;
use bzip2::read::BzDecoder;
use xz2::read::XzDecoder;
use zip::read::ZipArchive;
use lz4::Decoder as Lz4Decoder;

/// Supported compression formats detected by magic bytes
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompressionFormat {
    Gzip,
    Bzip2,
    Xz,
    Zip,
    Lz4,
    Uncompressed,
}

/// Magic file detection and decompression utility
pub struct DecompressionService;

impl DecompressionService {
    /// Detect compression format using magic bytes
    pub fn detect_compression_format(data: &[u8]) -> CompressionFormat {
        // Check for LZ4 magic bytes first (infer doesn't support LZ4)
        if data.len() >= 4 && data[0..4] == [0x04, 0x22, 0x4D, 0x18] {
            return CompressionFormat::Lz4;
        }
        
        if let Some(kind) = infer::get(data) {
            match kind.mime_type() {
                "application/gzip" => CompressionFormat::Gzip,
                "application/x-bzip2" => CompressionFormat::Bzip2,
                "application/x-xz" => CompressionFormat::Xz,
                "application/zip" => CompressionFormat::Zip,
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
            CompressionFormat::Gzip => Self::decompress_gzip(data),
            CompressionFormat::Bzip2 => Self::decompress_bzip2(data),
            CompressionFormat::Xz => Self::decompress_xz(data),
            CompressionFormat::Zip => Self::decompress_zip(data),
            CompressionFormat::Lz4 => Self::decompress_lz4(data),
            CompressionFormat::Uncompressed => Ok(data.to_vec()),
        }
    }

    /// Decompress gzip data
    fn decompress_gzip(data: Bytes) -> Result<Vec<u8>> {
        let mut decoder = GzDecoder::new(data.as_ref());
        let mut decompressed = Vec::new();
        decoder
            .read_to_end(&mut decompressed)
            .context("Failed to decompress gzip data")?;
        Ok(decompressed)
    }

    /// Decompress bzip2 data
    fn decompress_bzip2(data: Bytes) -> Result<Vec<u8>> {
        let mut decoder = BzDecoder::new(data.as_ref());
        let mut decompressed = Vec::new();
        decoder
            .read_to_end(&mut decompressed)
            .context("Failed to decompress bzip2 data")?;
        Ok(decompressed)
    }

    /// Decompress xz data
    fn decompress_xz(data: Bytes) -> Result<Vec<u8>> {
        let mut decoder = XzDecoder::new(data.as_ref());
        let mut decompressed = Vec::new();
        decoder
            .read_to_end(&mut decompressed)
            .context("Failed to decompress xz data")?;
        Ok(decompressed)
    }

    /// Decompress zip data (extracts first file)
    fn decompress_zip(data: Bytes) -> Result<Vec<u8>> {
        let cursor = std::io::Cursor::new(data);
        let mut archive = ZipArchive::new(cursor)
            .context("Failed to read zip archive")?;
        
        if archive.len() == 0 {
            anyhow::bail!("Zip archive is empty");
        }

        let mut file = archive.by_index(0)
            .context("Failed to get first file from zip archive")?;
        
        let mut decompressed = Vec::new();
        file.read_to_end(&mut decompressed)
            .context("Failed to read file from zip archive")?;
        
        Ok(decompressed)
    }

    /// Decompress LZ4 data
    fn decompress_lz4(data: Bytes) -> Result<Vec<u8>> {
        let mut decoder = Lz4Decoder::new(data.as_ref())
            .context("Failed to create LZ4 decoder")?;
        let mut decompressed = Vec::new();
        decoder
            .read_to_end(&mut decompressed)
            .context("Failed to decompress LZ4 data")?;
        Ok(decompressed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use flate2::write::GzEncoder;
    use flate2::Compression;

    #[test]
    fn test_detect_uncompressed() {
        let data = b"Hello, world!";
        let format = DecompressionService::detect_compression_format(data);
        assert_eq!(format, CompressionFormat::Uncompressed);
    }

    #[test]
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

    #[test]
    fn test_detect_lz4_magic_bytes() {
        // LZ4 magic bytes: 0x04, 0x22, 0x4D, 0x18
        let lz4_magic = &[0x04, 0x22, 0x4D, 0x18, 0x00, 0x01, 0x02, 0x03];
        let format = DecompressionService::detect_compression_format(lz4_magic);
        assert_eq!(format, CompressionFormat::Lz4);
    }
}