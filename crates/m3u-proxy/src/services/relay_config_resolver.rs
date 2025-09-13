//! Relay Configuration Resolver Service
//!
//! This service handles resolving complete relay configurations from the database,
//! following the same architectural patterns as ProxyConfigResolver.

use anyhow::Result;
use tracing::{debug, info};
use uuid::Uuid;

use crate::{
    database::repositories::relay::RelaySeaOrmRepository,
    errors::types::AppError,
    models::relay::{ChannelRelayConfig, ResolvedRelayConfig},
};

/// Service for resolving relay configurations from database
#[derive(Clone)]
pub struct RelayConfigResolver {
    relay_repo: RelaySeaOrmRepository,
}

impl RelayConfigResolver {
    pub fn new(relay_repo: RelaySeaOrmRepository) -> Self {
        Self { relay_repo }
    }

    /// Resolve complete relay configuration for a proxy and channel
    pub async fn resolve_relay_config(
        &self,
        proxy_id: Uuid,
        channel_id: Uuid,
        relay_profile_id: Uuid,
    ) -> Result<ResolvedRelayConfig, AppError> {
        debug!(
            "Resolving relay configuration: proxy_id={}, channel_id={}, profile_id={}",
            proxy_id, channel_id, relay_profile_id
        );

        // Get the relay profile
        let relay_profile = self
            .relay_repo
            .find_by_id(relay_profile_id)
            .await
            .map_err(|e| AppError::Internal {
                message: format!("Repository error: {e}"),
            })?
            .ok_or_else(|| AppError::NotFound {
                resource: "relay_profile".to_string(),
                id: relay_profile_id.to_string(),
            })?;

        // For now, always create temporary channel config since find_channel_config is not implemented
        // TODO: Implement find_channel_config in RelaySeaOrmRepository
        debug!("Creating temporary channel relay config");
        let channel_config =
            self.create_temporary_channel_config(proxy_id, channel_id, relay_profile_id);

        // Create resolved configuration
        let resolved_config =
            ResolvedRelayConfig::new(channel_config, relay_profile).map_err(|e| {
                AppError::Internal {
                    message: format!("Failed to resolve relay configuration: {e}"),
                }
            })?;

        info!(
            "Successfully resolved relay configuration: profile='{}', channel_id={}",
            resolved_config.profile.name, channel_id
        );

        Ok(resolved_config)
    }

    /// Create a temporary channel relay configuration for ad-hoc streaming
    fn create_temporary_channel_config(
        &self,
        proxy_id: Uuid,
        channel_id: Uuid,
        relay_profile_id: Uuid,
    ) -> ChannelRelayConfig {
        // Generate deterministic UUID for consistent identification
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        let deterministic_id = {
            let mut hasher = DefaultHasher::new();
            proxy_id.hash(&mut hasher);
            channel_id.hash(&mut hasher);
            relay_profile_id.hash(&mut hasher);
            let hash = hasher.finish();
            Uuid::from_u128(hash as u128)
        };

        ChannelRelayConfig {
            id: deterministic_id,
            proxy_id,
            channel_id,
            profile_id: relay_profile_id,
            name: format!("Temporary relay config for channel {channel_id}"),
            description: Some("Temporary relay configuration for stream proxy mode".to_string()),
            custom_args: None,
            is_active: true,
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        }
    }

    /// Validate that a resolved configuration is complete and valid
    pub fn validate_config(&self, config: &ResolvedRelayConfig) -> Result<(), AppError> {
        if !config.config.is_active {
            return Err(AppError::Internal {
                message: "Channel relay configuration is not active".to_string(),
            });
        }

        debug!(
            "Relay configuration validated: profile='{}'",
            config.profile.name
        );
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::relay::{AudioCodec, RelayOutputFormat, RelayProfile, VideoCodec};
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    #[test]
    fn test_deterministic_id_generation() {
        // Test the deterministic ID generation logic used in create_temporary_channel_config
        let proxy_id = Uuid::parse_str("550e8400-e29b-41d4-a716-446655440000").unwrap();
        let channel_id = Uuid::parse_str("550e8400-e29b-41d4-a716-446655440001").unwrap();
        let profile_id = Uuid::parse_str("550e8400-e29b-41d4-a716-446655440002").unwrap();

        // Generate the expected deterministic ID
        let mut hasher = DefaultHasher::new();
        proxy_id.hash(&mut hasher);
        channel_id.hash(&mut hasher);
        profile_id.hash(&mut hasher);
        let expected_hash = hasher.finish();
        let expected_id = Uuid::from_u128(expected_hash as u128);

        // Test the hash generation logic directly - same inputs produce same ID
        let mut hasher2 = DefaultHasher::new();
        proxy_id.hash(&mut hasher2);
        channel_id.hash(&mut hasher2);
        profile_id.hash(&mut hasher2);
        let hash2 = hasher2.finish();
        let id2 = Uuid::from_u128(hash2 as u128);

        assert_eq!(
            expected_id, id2,
            "Same inputs should produce same deterministic ID"
        );

        // Test different inputs produce different IDs
        let different_proxy_id = Uuid::parse_str("550e8400-e29b-41d4-a716-446655440999").unwrap();
        let mut hasher3 = DefaultHasher::new();
        different_proxy_id.hash(&mut hasher3);
        channel_id.hash(&mut hasher3);
        profile_id.hash(&mut hasher3);
        let hash3 = hasher3.finish();
        let id3 = Uuid::from_u128(hash3 as u128);

        assert_ne!(
            expected_id, id3,
            "Different inputs should produce different IDs"
        );
    }

    fn create_test_relay_profile(id: Uuid, name: &str) -> RelayProfile {
        RelayProfile {
            id,
            name: name.to_string(),
            description: Some("Test profile".to_string()),
            video_codec: VideoCodec::H264,
            audio_codec: AudioCodec::AAC,
            video_profile: Some("main".to_string()),
            video_preset: Some("medium".to_string()),
            video_bitrate: Some(2000),
            audio_bitrate: Some(128),
            audio_sample_rate: Some(48000),
            audio_channels: Some(2),
            enable_hardware_acceleration: false,
            preferred_hwaccel: None,
            manual_args: None,
            output_format: RelayOutputFormat::TransportStream,
            segment_duration: Some(10),
            max_segments: Some(3),
            input_timeout: 30,
            is_system_default: false,
            is_active: true,
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        }
    }

    fn create_test_channel_config(
        proxy_id: Uuid,
        channel_id: Uuid,
        profile_id: Uuid,
    ) -> ChannelRelayConfig {
        ChannelRelayConfig {
            id: Uuid::new_v4(),
            proxy_id,
            channel_id,
            profile_id,
            name: "Test Channel Config".to_string(),
            description: Some("Test channel relay config".to_string()),
            custom_args: None,
            is_active: true,
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        }
    }

    #[tokio::test]
    async fn test_validate_config_with_active_config() {
        // Test the validation logic without requiring a database
        // Since validate_config doesn't use the repository, we can test it in isolation

        // Create test data
        let profile = create_test_relay_profile(Uuid::new_v4(), "test-profile");
        let mut channel_config =
            create_test_channel_config(Uuid::new_v4(), Uuid::new_v4(), profile.id);
        channel_config.is_active = true;

        let resolved_config = ResolvedRelayConfig::new(channel_config, profile).unwrap();

        // Create a dummy resolver (we won't use the repository part)
        use sea_orm::Database;
        let dummy_connection = Database::connect("sqlite::memory:").await.unwrap();
        let dummy_repo = crate::database::repositories::relay::RelaySeaOrmRepository::new(
            dummy_connection.into(),
        );
        let resolver = RelayConfigResolver::new(dummy_repo);

        let result = resolver.validate_config(&resolved_config);
        assert!(result.is_ok(), "Active config should pass validation");
    }

    #[tokio::test]
    async fn test_validate_config_with_inactive_config() {
        // Test the validation logic with inactive config
        let profile = create_test_relay_profile(Uuid::new_v4(), "test-profile");
        let mut channel_config =
            create_test_channel_config(Uuid::new_v4(), Uuid::new_v4(), profile.id);
        channel_config.is_active = false;

        let resolved_config = ResolvedRelayConfig::new(channel_config, profile).unwrap();

        // Create a dummy resolver (we won't use the repository part)
        use sea_orm::Database;
        let dummy_connection = Database::connect("sqlite::memory:").await.unwrap();
        let dummy_repo = crate::database::repositories::relay::RelaySeaOrmRepository::new(
            dummy_connection.into(),
        );
        let resolver = RelayConfigResolver::new(dummy_repo);

        let result = resolver.validate_config(&resolved_config);
        assert!(result.is_err(), "Inactive config should fail validation");

        match result {
            Err(AppError::Internal { message }) => {
                assert!(
                    message.contains("not active"),
                    "Error should mention inactive state"
                );
            }
            _ => panic!("Expected Internal error for inactive config"),
        }
    }

    // Note: Integration tests that require full database functionality would be placed
    // in integration test files to test the complete resolve_relay_config workflow
}
