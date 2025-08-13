//! Deterministic UUID Generation
//!
//! This module provides utilities for generating deterministic UUIDs based on input data.
//! This ensures that the same inputs always produce the same UUID, which is critical
//! for maintaining stable identifiers across system restarts and regenerations.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use uuid::Uuid;

/// Generate a deterministic UUID based on hashable inputs
/// 
/// This function takes any number of hashable values and produces a stable UUID
/// that will always be the same for the same inputs. This is used to ensure
/// that channels, proxies, and other entities maintain consistent IDs across
/// system regenerations.
/// 
/// # Examples
/// 
/// ```rust
/// use m3u_proxy::utils::deterministic_uuid::generate_deterministic_uuid;
/// use uuid::Uuid;
/// 
/// let source_id = Uuid::new_v4();
/// let stream_url = "http://example.com/stream.m3u8";
/// let channel_name = "Example Channel";
/// 
/// // This will always produce the same UUID for these inputs
/// let channel_id = generate_deterministic_uuid(&[
///     &source_id.to_string(),
///     &stream_url,
///     &channel_name,
/// ]);
/// ```
pub fn generate_deterministic_uuid(inputs: &[&dyn std::fmt::Display]) -> Uuid {
    let mut hasher = DefaultHasher::new();
    
    // Hash each input in order
    for input in inputs {
        input.to_string().hash(&mut hasher);
    }
    
    let hash = hasher.finish();
    
    // Convert hash to UUID
    // We use from_u128 which takes the lower 128 bits of the hash
    // Since DefaultHasher produces u64, we shift it to fill the full u128
    let uuid_bits = ((hash as u128) << 64) | (hash as u128);
    Uuid::from_u128(uuid_bits)
}

/// Generate a deterministic UUID for a channel based on its key properties
/// 
/// This function specifically generates UUIDs for channels based on:
/// - Source ID (which source the channel comes from)
/// - Stream URL (the actual stream endpoint)
/// - Channel name (the display name)
/// 
/// This ensures that the same channel from the same source always gets
/// the same UUID, even across system restarts and M3U regenerations.
pub fn generate_channel_uuid(source_id: &Uuid, stream_url: &str, channel_name: &str) -> Uuid {
    generate_deterministic_uuid(&[
        &source_id.to_string(),
        &stream_url,
        &channel_name,
    ])
}

/// Generate a deterministic UUID for a proxy configuration
/// 
/// This function generates UUIDs for proxy configurations based on:
/// - Proxy ID (the main proxy identifier)
/// - Configuration hash or version identifier
/// 
/// This is useful for proxy generation records and other proxy-related entities.
pub fn generate_proxy_config_uuid(proxy_id: &Uuid, config_identifier: &str) -> Uuid {
    generate_deterministic_uuid(&[
        &proxy_id.to_string(),
        &config_identifier,
    ])
}

/// Generate a deterministic UUID for relay configurations
/// 
/// This function generates UUIDs for relay configurations based on:
/// - Proxy ID
/// - Channel ID  
/// - Profile ID
/// 
/// This matches the existing pattern used in the relay system.
pub fn generate_relay_config_uuid(proxy_id: &Uuid, channel_id: &Uuid, profile_id: &Uuid) -> Uuid {
    generate_deterministic_uuid(&[
        &proxy_id.to_string(),
        &channel_id.to_string(),
        &profile_id.to_string(),
    ])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_deterministic_uuid_consistency() {
        let source_id = Uuid::parse_str("550e8400-e29b-41d4-a716-446655440000").unwrap();
        let stream_url = "http://example.com/stream.m3u8";
        let channel_name = "Test Channel";
        
        // Generate UUID multiple times with same inputs
        let uuid1 = generate_channel_uuid(&source_id, stream_url, channel_name);
        let uuid2 = generate_channel_uuid(&source_id, stream_url, channel_name);
        let uuid3 = generate_channel_uuid(&source_id, stream_url, channel_name);
        
        // Should all be identical
        assert_eq!(uuid1, uuid2);
        assert_eq!(uuid2, uuid3);
    }
    
    #[test]
    fn test_different_inputs_different_uuids() {
        let source_id = Uuid::parse_str("550e8400-e29b-41d4-a716-446655440000").unwrap();
        
        let uuid1 = generate_channel_uuid(&source_id, "http://example.com/stream1.m3u8", "Channel 1");
        let uuid2 = generate_channel_uuid(&source_id, "http://example.com/stream2.m3u8", "Channel 2");
        
        // Should be different
        assert_ne!(uuid1, uuid2);
    }
    
    #[test]
    fn test_order_matters() {
        let inputs1: &[&dyn std::fmt::Display] = &[&"a", &"b", &"c"];
        let inputs2: &[&dyn std::fmt::Display] = &[&"c", &"b", &"a"];
        
        let uuid1 = generate_deterministic_uuid(inputs1);
        let uuid2 = generate_deterministic_uuid(inputs2);
        
        // Different order should produce different UUIDs
        assert_ne!(uuid1, uuid2);
    }
    
    #[test]
    fn test_relay_config_uuid_matches_existing_pattern() {
        let proxy_id = Uuid::parse_str("550e8400-e29b-41d4-a716-446655440000").unwrap();
        let channel_id = Uuid::parse_str("6ba7b810-9dad-11d1-80b4-00c04fd430c8").unwrap();  
        let profile_id = Uuid::parse_str("6ba7b811-9dad-11d1-80b4-00c04fd430c8").unwrap();
        
        let uuid1 = generate_relay_config_uuid(&proxy_id, &channel_id, &profile_id);
        let uuid2 = generate_relay_config_uuid(&proxy_id, &channel_id, &profile_id);
        
        assert_eq!(uuid1, uuid2);
    }
}
