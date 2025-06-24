use anyhow::Result;
use chrono::Utc;
use sqlx::Row;
use tracing::{debug, info, warn};
use uuid::Uuid;

use crate::models::{ChannelEpgMapping, EpgMappingType};

#[derive(Debug)]
pub struct AutoMapChannelsResult {
    pub potential_matches: usize,
    pub mappings_created: usize,
}

impl super::Database {
    /// Get channel mappings with optional filters
    pub async fn get_channel_mappings(
        &self,
        stream_channel_id: Option<Uuid>,
        epg_channel_id: Option<Uuid>,
        mapping_type: Option<&str>,
    ) -> Result<Vec<ChannelEpgMapping>> {
        let mut query = String::from(
            "SELECT id, stream_channel_id, epg_channel_id, mapping_type, created_at
             FROM channel_epg_mapping WHERE 1=1",
        );
        let mut bind_values: Vec<String> = Vec::new();

        if let Some(stream_id) = stream_channel_id {
            query.push_str(" AND stream_channel_id = ?");
            bind_values.push(stream_id.to_string());
        }

        if let Some(epg_id) = epg_channel_id {
            query.push_str(" AND epg_channel_id = ?");
            bind_values.push(epg_id.to_string());
        }

        if let Some(mapping_type_str) = mapping_type {
            query.push_str(" AND mapping_type = ?");
            bind_values.push(mapping_type_str.to_string());
        }

        query.push_str(" ORDER BY created_at DESC");

        let mut query_builder = sqlx::query(&query);
        for value in &bind_values {
            query_builder = query_builder.bind(value);
        }

        let rows = query_builder.fetch_all(&self.pool).await?;
        let mut mappings = Vec::new();

        for row in rows {
            let id_str = row.get::<String, _>("id");
            let stream_channel_id_str = row.get::<String, _>("stream_channel_id");
            let epg_channel_id_str = row.get::<String, _>("epg_channel_id");
            let mapping_type_str = row.get::<String, _>("mapping_type");
            let created_at_str = row.get::<String, _>("created_at");

            let mapping_type = match mapping_type_str.as_str() {
                "manual" => EpgMappingType::Manual,
                "auto_name" => EpgMappingType::AutoName,
                "auto_tvg_id" => EpgMappingType::AutoTvgId,
                _ => EpgMappingType::Manual,
            };

            mappings.push(ChannelEpgMapping {
                id: Uuid::parse_str(&id_str)?,
                stream_channel_id: Uuid::parse_str(&stream_channel_id_str)?,
                epg_channel_id: Uuid::parse_str(&epg_channel_id_str)?,
                mapping_type,
                created_at: chrono::DateTime::parse_from_rfc3339(&created_at_str)?
                    .with_timezone(&Utc),
            });
        }

        debug!("Retrieved {} channel mappings", mappings.len());
        Ok(mappings)
    }

    /// Create a new channel mapping
    pub async fn create_channel_mapping(
        &self,
        stream_channel_id: Uuid,
        epg_channel_id: Uuid,
        mapping_type: EpgMappingType,
    ) -> Result<ChannelEpgMapping> {
        let id = Uuid::new_v4();
        let created_at = Utc::now();
        let mapping_type_str = match mapping_type {
            EpgMappingType::Manual => "manual",
            EpgMappingType::AutoName => "auto_name",
            EpgMappingType::AutoTvgId => "auto_tvg_id",
        };

        sqlx::query(
            "INSERT INTO channel_epg_mapping (id, stream_channel_id, epg_channel_id, mapping_type, created_at)
             VALUES (?, ?, ?, ?, ?)"
        )
        .bind(id.to_string())
        .bind(stream_channel_id.to_string())
        .bind(epg_channel_id.to_string())
        .bind(mapping_type_str)
        .bind(created_at.to_rfc3339())
        .execute(&self.pool)
        .await?;

        info!(
            "Created channel mapping: {} -> {} (type: {})",
            stream_channel_id, epg_channel_id, mapping_type_str
        );

        Ok(ChannelEpgMapping {
            id,
            stream_channel_id,
            epg_channel_id,
            mapping_type,
            created_at,
        })
    }

    /// Delete a channel mapping
    pub async fn delete_channel_mapping(&self, mapping_id: Uuid) -> Result<bool> {
        let result = sqlx::query("DELETE FROM channel_epg_mapping WHERE id = ?")
            .bind(mapping_id.to_string())
            .execute(&self.pool)
            .await?;

        let deleted = result.rows_affected() > 0;
        if deleted {
            info!("Deleted channel mapping: {}", mapping_id);
        } else {
            warn!("Channel mapping not found for deletion: {}", mapping_id);
        }

        Ok(deleted)
    }

    /// Get unmapped stream channels
    pub async fn get_unmapped_stream_channels(
        &self,
        source_id: Option<Uuid>,
    ) -> Result<Vec<(Uuid, String, Option<String>)>> {
        let mut query = String::from(
            "SELECT c.id, c.channel_name, c.tvg_id
             FROM channels c
             LEFT JOIN channel_epg_mapping m ON c.id = m.stream_channel_id
             WHERE m.stream_channel_id IS NULL",
        );
        let mut bind_values: Vec<String> = Vec::new();

        if let Some(source_id) = source_id {
            query.push_str(" AND c.source_id = ?");
            bind_values.push(source_id.to_string());
        }

        query.push_str(" ORDER BY c.channel_name");

        let mut query_builder = sqlx::query(&query);
        for value in &bind_values {
            query_builder = query_builder.bind(value);
        }

        let rows = query_builder.fetch_all(&self.pool).await?;
        let mut channels = Vec::new();

        for row in rows {
            let id_str = row.get::<String, _>("id");
            let channel_name = row.get::<String, _>("channel_name");
            let tvg_id = row.get::<Option<String>, _>("tvg_id");

            channels.push((Uuid::parse_str(&id_str)?, channel_name, tvg_id));
        }

        debug!("Found {} unmapped stream channels", channels.len());
        Ok(channels)
    }

    /// Get available EPG channels for mapping
    pub async fn get_available_epg_channels(
        &self,
        source_id: Option<Uuid>,
    ) -> Result<Vec<(Uuid, String, String)>> {
        let mut query = String::from(
            "SELECT id, channel_id, channel_name
             FROM epg_channels",
        );
        let mut bind_values: Vec<String> = Vec::new();

        if let Some(source_id) = source_id {
            query.push_str(" WHERE source_id = ?");
            bind_values.push(source_id.to_string());
        }

        query.push_str(" ORDER BY channel_name");

        let mut query_builder = sqlx::query(&query);
        for value in &bind_values {
            query_builder = query_builder.bind(value);
        }

        let rows = query_builder.fetch_all(&self.pool).await?;
        let mut channels = Vec::new();

        for row in rows {
            let id_str = row.get::<String, _>("id");
            let channel_id = row.get::<String, _>("channel_id");
            let channel_name = row.get::<String, _>("channel_name");

            channels.push((Uuid::parse_str(&id_str)?, channel_id, channel_name));
        }

        debug!("Found {} available EPG channels", channels.len());
        Ok(channels)
    }

    /// Automatically map channels based on name or tvg_id matching
    pub async fn auto_map_channels(
        &self,
        source_id: Option<Uuid>,
        mapping_type: EpgMappingType,
        dry_run: bool,
    ) -> Result<AutoMapChannelsResult> {
        let unmapped_channels = self.get_unmapped_stream_channels(source_id).await?;
        let epg_channels = self.get_available_epg_channels(None).await?;

        let mut potential_matches = 0;
        let mut mappings_created = 0;

        for (stream_channel_id, stream_name, stream_tvg_id) in unmapped_channels {
            let mut found_match = false;

            // Try to find a matching EPG channel
            for (epg_channel_id, epg_channel_id_str, epg_name) in &epg_channels {
                let is_match = match mapping_type {
                    EpgMappingType::AutoName => {
                        // Normalize names for comparison
                        let normalized_stream = normalize_channel_name(&stream_name);
                        let normalized_epg = normalize_channel_name(epg_name);
                        normalized_stream == normalized_epg
                    }
                    EpgMappingType::AutoTvgId => {
                        // Match by TVG ID
                        if let Some(tvg_id) = &stream_tvg_id {
                            tvg_id == epg_channel_id_str
                        } else {
                            false
                        }
                    }
                    EpgMappingType::Manual => false, // Manual mappings don't auto-match
                };

                if is_match {
                    potential_matches += 1;
                    found_match = true;

                    if !dry_run {
                        match self
                            .create_channel_mapping(
                                stream_channel_id,
                                *epg_channel_id,
                                mapping_type.clone(),
                            )
                            .await
                        {
                            Ok(_) => {
                                mappings_created += 1;
                                info!(
                                    "Auto-mapped channel '{}' to EPG channel '{}' ({})",
                                    stream_name, epg_name, epg_channel_id_str
                                );
                            }
                            Err(e) => {
                                warn!(
                                    "Failed to create auto-mapping for '{}' -> '{}': {}",
                                    stream_name, epg_name, e
                                );
                            }
                        }
                    }
                    break; // Only create one mapping per stream channel
                }
            }

            if !found_match {
                debug!(
                    "No EPG match found for stream channel '{}' (TVG ID: {:?})",
                    stream_name, stream_tvg_id
                );
            }
        }

        info!(
            "Auto-mapping completed: {} potential matches, {} mappings created (dry_run: {})",
            potential_matches, mappings_created, dry_run
        );

        Ok(AutoMapChannelsResult {
            potential_matches,
            mappings_created,
        })
    }
}

/// Normalize channel name for comparison
fn normalize_channel_name(name: &str) -> String {
    name.to_lowercase()
        .replace(" hd", "")
        .replace(" sd", "")
        .replace(" 4k", "")
        .replace("◉", "")
        .trim()
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normalize_channel_name() {
        assert_eq!(normalize_channel_name("CNN HD"), "cnn");
        assert_eq!(normalize_channel_name("BBC One SD"), "bbc one");
        assert_eq!(normalize_channel_name("Discovery 4K ◉"), "discovery");
        assert_eq!(normalize_channel_name("  MTV  "), "mtv");
    }
}
