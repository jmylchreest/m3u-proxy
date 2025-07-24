use anyhow::{anyhow, Result};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use uuid::Uuid;

/// Trait for types that can be converted to UUID flexibly
pub trait FlexibleUuidSource {
    fn to_uuid_flexible(&self) -> Result<Uuid>;
}

impl FlexibleUuidSource for &str {
    fn to_uuid_flexible(&self) -> Result<Uuid> {
        parse_uuid_flexible(self)
    }
}

impl FlexibleUuidSource for String {
    fn to_uuid_flexible(&self) -> Result<Uuid> {
        parse_uuid_flexible(self)
    }
}

impl FlexibleUuidSource for Uuid {
    fn to_uuid_flexible(&self) -> Result<Uuid> {
        Ok(*self)
    }
}

/// Parse a UUID from any supported format to a standard UUID
/// Supports:
/// - 36 characters with hyphens: "550e8400-e29b-41d4-a716-446655440000"
/// - 32 characters without hyphens: "550e8400e29b41d4a716446655440000"
/// - 22 characters base64: "VQ6EAOKbQdSnFkRmVUQAAA"
pub fn parse_uuid_flexible(input: &str) -> Result<Uuid> {
    let trimmed = input.trim();
    
    match trimmed.len() {
        36 => {
            // Standard UUID format with hyphens
            Uuid::parse_str(trimmed)
                .map_err(|e| anyhow!("Invalid 36-character UUID format: {}", e))
        }
        32 => {
            // UUID without hyphens - insert hyphens at correct positions
            if trimmed.chars().all(|c| c.is_ascii_hexdigit()) {
                let formatted = format!(
                    "{}-{}-{}-{}-{}",
                    &trimmed[0..8],
                    &trimmed[8..12],
                    &trimmed[12..16],
                    &trimmed[16..20],
                    &trimmed[20..32]
                );
                Uuid::parse_str(&formatted)
                    .map_err(|e| anyhow!("Invalid 32-character UUID format: {}", e))
            } else {
                Err(anyhow!("32-character string contains non-hex characters"))
            }
        }
        22 => {
            // Base64 encoded UUID (128 bits = 16 bytes = 22 base64 chars with no padding)
            let decoded = URL_SAFE_NO_PAD
                .decode(trimmed)
                .map_err(|e| anyhow!("Invalid base64 UUID format: {}", e))?;
            
            if decoded.len() != 16 {
                return Err(anyhow!("Base64 UUID must decode to exactly 16 bytes, got {}", decoded.len()));
            }
            
            let mut bytes = [0u8; 16];
            bytes.copy_from_slice(&decoded);
            Ok(Uuid::from_bytes(bytes))
        }
        _ => {
            Err(anyhow!(
                "UUID must be 36 characters (with hyphens), 32 characters (without hyphens), or 22 characters (base64). Got {} characters: '{}'",
                trimmed.len(),
                trimmed
            ))
        }
    }
}

/// Parse UUID from any supported source type (String, &str, or Uuid)
/// This is the recommended function to use for maximum flexibility
pub fn parse_uuid_from_any<T: FlexibleUuidSource>(input: T) -> Result<Uuid> {
    input.to_uuid_flexible()
}

/// Resolve a proxy ID from any supported format to a standard UUID
/// Alias for parse_uuid_flexible for backward compatibility
pub fn resolve_proxy_id(input: &str) -> Result<Uuid> {
    parse_uuid_flexible(input)
}

/// Convert UUID to base64 format (22 characters)
pub fn uuid_to_base64(uuid: &Uuid) -> String {
    URL_SAFE_NO_PAD.encode(uuid.as_bytes())
}

/// Convert UUID to 32-character hex string (no hyphens)
pub fn uuid_to_hex32(uuid: &Uuid) -> String {
    uuid.simple().to_string()
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_resolve_36_char_uuid() {
        let uuid_str = "550e8400-e29b-41d4-a716-446655440000";
        let result = resolve_proxy_id(uuid_str).unwrap();
        assert_eq!(result.to_string(), uuid_str);
    }

    #[test]
    fn test_resolve_32_char_uuid() {
        let uuid_32 = "550e8400e29b41d4a716446655440000";
        let expected = "550e8400-e29b-41d4-a716-446655440000";
        let result = resolve_proxy_id(uuid_32).unwrap();
        assert_eq!(result.to_string(), expected);
    }

    #[test]
    fn test_resolve_base64_uuid() {
        let uuid = Uuid::parse_str("550e8400-e29b-41d4-a716-446655440000").unwrap();
        let base64_str = uuid_to_base64(&uuid);
        let result = resolve_proxy_id(&base64_str).unwrap();
        assert_eq!(result, uuid);
    }

    #[test]
    fn test_roundtrip_conversions() {
        let original = Uuid::parse_str("550e8400-e29b-41d4-a716-446655440000").unwrap();
        
        // Test base64 roundtrip
        let base64 = uuid_to_base64(&original);
        let from_base64 = resolve_proxy_id(&base64).unwrap();
        assert_eq!(original, from_base64);
        
        // Test 32-char hex roundtrip
        let hex32 = uuid_to_hex32(&original);
        let from_hex32 = resolve_proxy_id(&hex32).unwrap();
        assert_eq!(original, from_hex32);
    }

    #[test]
    fn test_invalid_formats() {
        assert!(resolve_proxy_id("invalid").is_err());
        assert!(resolve_proxy_id("550e8400-e29b-41d4-a716").is_err()); // too short
        assert!(resolve_proxy_id("gggggggggggggggggggggggggggggggg").is_err()); // invalid hex
        assert!(resolve_proxy_id("InvalidBase64!!!!!").is_err()); // invalid base64
    }

    #[test]
    fn test_parse_uuid_from_any() {
        let uuid = Uuid::parse_str("550e8400-e29b-41d4-a716-446655440000").unwrap();
        
        // Test with Uuid type
        assert_eq!(parse_uuid_from_any(uuid).unwrap(), uuid);
        
        // Test with String
        let uuid_string = "550e8400-e29b-41d4-a716-446655440000".to_string();
        assert_eq!(parse_uuid_from_any(uuid_string).unwrap(), uuid);
        
        // Test with &str
        assert_eq!(parse_uuid_from_any("550e8400-e29b-41d4-a716-446655440000").unwrap(), uuid);
        
        // Test with 32-char string
        assert_eq!(parse_uuid_from_any("550e8400e29b41d4a716446655440000").unwrap(), uuid);
        
        // Test with base64 string
        let base64_str = uuid_to_base64(&uuid);
        assert_eq!(parse_uuid_from_any(base64_str).unwrap(), uuid);
    }
}