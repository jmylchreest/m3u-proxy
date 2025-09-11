//! Smart 12-bit dimension encoding for logo cache optimization
//!
//! Encodes dimensions using smart ranges that provide exact accuracy for small sizes
//! and acceptable approximation for larger sizes, optimized for real-world logo dimensions.

/// Practical 12-bit dimension encoder optimized for logo sizes
#[derive(Debug, Clone, Copy)]
pub struct DimensionEncoder;

impl DimensionEncoder {
    /// Encode dimension to 12 bits with smart ranges:
    /// - 0: None/unknown
    /// - 1-512: Direct encoding (1-512px) - covers most small logos with exact precision
    /// - 513-1024: Encoded as 513 + ((value-513)/2) - covers 514-1536px in 2px steps
    /// - 1025-2048: Encoded as 1025 + ((value-1537)/4) - covers 1540-3584px in 4px steps  
    /// - 2049-4095: Encoded as 2049 + ((value-3585)/8) - covers 3592-19672px in 8px steps
    pub fn encode(dimension: Option<i32>) -> u16 {
        match dimension {
            None | Some(0) => 0,
            Some(d) if d <= 512 => d as u16,
            Some(d) if d <= 1536 => 513 + ((d - 513) / 2) as u16,
            Some(d) if d <= 3584 => 1025 + ((d - 1537) / 4) as u16,
            Some(d) => {
                let encoded = 2049 + ((d - 3585) / 8) as u16;
                encoded.min(4095) // Cap at 12-bit max
            }
        }
    }
    
    /// Decode 12-bit value back to approximate dimension
    pub fn decode(encoded: u16) -> Option<i32> {
        match encoded {
            0 => None,
            1..=512 => Some(encoded as i32),
            513..=1024 => Some(513 + ((encoded - 513) * 2) as i32),
            1025..=2048 => Some(1537 + ((encoded - 1025) * 4) as i32),
            2049..=4095 => Some(3585 + ((encoded - 2049) * 8) as i32),
            _ => Some(3585 + ((4095 - 2049) * 8)), // Cap at maximum value
        }
    }
    
    /// Get encoding error for validation (difference between original and decoded)
    pub fn encoding_error(original: i32) -> i32 {
        let encoded = Self::encode(Some(original));
        let decoded = Self::decode(encoded).unwrap_or(0);
        (original - decoded).abs()
    }
    
    /// Check if dimension fits in 12-bit encoding without significant loss
    pub fn is_encodable_precisely(dimension: i32) -> bool {
        Self::encoding_error(dimension) <= 2
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_small_dimensions_exact() {
        // Small dimensions should be encoded exactly
        for size in [16, 32, 64, 128, 256, 512] {
            assert_eq!(
                DimensionEncoder::decode(DimensionEncoder::encode(Some(size))), 
                Some(size),
                "Size {} should encode exactly", size
            );
        }
    }
    
    #[test]
    fn test_medium_dimensions_acceptable() {
        // Medium sizes should have acceptable approximation
        for size in [600, 800, 1000, 1200] {
            let error = DimensionEncoder::encoding_error(size);
            assert!(error <= 2, "Size {} error {} should be <= 2px", size, error);
        }
    }
    
    #[test]
    fn test_large_dimensions_approximate() {
        // Large sizes have more approximation but still reasonable
        for size in [2000, 3000, 5000, 8000] {
            let error = DimensionEncoder::encoding_error(size);
            assert!(error <= 8, "Size {} error {} should be <= 8px", size, error);
        }
    }
    
    #[test]
    fn test_none_encoding() {
        assert_eq!(DimensionEncoder::encode(None), 0);
        assert_eq!(DimensionEncoder::decode(0), None);
    }
}