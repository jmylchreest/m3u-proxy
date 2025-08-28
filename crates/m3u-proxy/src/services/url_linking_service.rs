//! URL-based source linking service implementation
//!
//! This service provides clean URL-based linking between stream and EPG sources
//! using SeaORM repositories, replacing the complex legacy UrlLinkingRepository.

use anyhow::Result;
use tracing::{info, debug};
use uuid::Uuid;

use crate::{
    database::repositories::{
        stream_source::StreamSourceSeaOrmRepository,
        epg_source::EpgSourceSeaOrmRepository,
    },
    models::{StreamSource, EpgSource, StreamSourceType, EpgSourceType},
};

/// Clean SeaORM-based URL linking service for automatic source relationship management
pub struct UrlLinkingService {
    stream_source_repo: StreamSourceSeaOrmRepository,
    epg_source_repo: EpgSourceSeaOrmRepository,
}

impl UrlLinkingService {
    /// Create a new URL linking service
    pub fn new(
        stream_source_repo: StreamSourceSeaOrmRepository,
        epg_source_repo: EpgSourceSeaOrmRepository,
    ) -> Self {
        Self {
            stream_source_repo,
            epg_source_repo,
        }
    }

    /// Find stream sources with the same URL as the given EPG source
    pub async fn find_linked_stream_sources(&self, epg_source: &EpgSource) -> Result<Vec<StreamSource>> {
        if epg_source.source_type != EpgSourceType::Xtream {
            return Ok(Vec::new());
        }

        // Get all active stream sources
        let all_sources = self.stream_source_repo.find_active().await?;
        
        // Filter for Xtream sources with same URL but different ID
        let linked_sources = all_sources
            .into_iter()
            .filter(|source| {
                source.source_type == StreamSourceType::Xtream
                    && source.url == epg_source.url
                    && source.id != epg_source.id
                    && source.is_active
            })
            .collect();

        Ok(linked_sources)
    }

    /// Find EPG sources with the same URL as the given stream source
    pub async fn find_linked_epg_sources(&self, stream_source: &StreamSource) -> Result<Vec<EpgSource>> {
        if stream_source.source_type != StreamSourceType::Xtream {
            return Ok(Vec::new());
        }

        // Get all active EPG sources
        let all_sources = self.epg_source_repo.find_active().await?;
        
        // Filter for Xtream sources with same URL but different ID
        let linked_sources = all_sources
            .into_iter()
            .filter(|source| {
                source.source_type == EpgSourceType::Xtream
                    && source.url == stream_source.url
                    && source.id != stream_source.id
                    && source.is_active
            })
            .collect();

        Ok(linked_sources)
    }

    /// Auto-populate EPG source credentials from linked stream sources (URL-based)
    pub async fn auto_populate_epg_credentials(&self, epg_source_id: Uuid) -> Result<Option<EpgSource>> {
        // Get the EPG source
        let epg_source = match self.epg_source_repo.find_by_id(&epg_source_id).await? {
            Some(source) => source,
            None => return Ok(None),
        };

        // Only handle Xtream sources without credentials
        if epg_source.source_type != EpgSourceType::Xtream 
            || (epg_source.username.is_some() && epg_source.password.is_some()) {
            return Ok(Some(epg_source));
        }

        // Find linked stream sources
        let linked_stream_sources = self.find_linked_stream_sources(&epg_source).await?;
        
        // Look for a stream source with credentials
        for stream_source in &linked_stream_sources {
            if let (Some(username), Some(password)) = (&stream_source.username, &stream_source.password) {
                // Update EPG source with credentials from stream source
                let update_request = crate::models::EpgSourceUpdateRequest {
                    name: epg_source.name.clone(),
                    source_type: epg_source.source_type,
                    url: epg_source.url.clone(),
                    update_cron: epg_source.update_cron.clone(),
                    username: Some(username.clone()),
                    password: Some(password.clone()),
                    timezone: epg_source.original_timezone.clone(),
                    time_offset: Some(epg_source.time_offset.clone()),
                    is_active: epg_source.is_active,
                    update_linked: false, // Don't create circular updates
                };

                let updated_epg = self.epg_source_repo.update(&epg_source_id, update_request).await?;

                info!(
                    "Auto-populated EPG source '{}' credentials from linked stream source '{}'",
                    epg_source.name, stream_source.name
                );

                return Ok(Some(updated_epg));
            }
        }

        Ok(Some(epg_source))
    }

    /// Update linked sources when a source's URL, username, or password changes
    pub async fn update_linked_sources(
        &self,
        source_id: Uuid,
        source_type: &str,
        url: Option<&String>,
        username: Option<&String>,
        password: Option<&String>,
        update_linked: bool,
    ) -> Result<u64> {
        if !update_linked {
            debug!("update_linked=false, skipping linked source updates");
            return Ok(0);
        }

        let mut updated_count = 0;

        match source_type {
            "stream" => {
                // Get the stream source
                let stream_source = match self.stream_source_repo.find_by_id(&source_id).await? {
                    Some(source) => source,
                    None => return Ok(0),
                };

                // Update linked EPG sources
                let linked_epg_sources = self.find_linked_epg_sources(&stream_source).await?;
                
                for epg_source in linked_epg_sources {
                    let mut needs_update = false;
                    let mut update_request = crate::models::EpgSourceUpdateRequest {
                        name: epg_source.name.clone(),
                        source_type: epg_source.source_type,
                        url: epg_source.url.clone(),
                        update_cron: epg_source.update_cron.clone(),
                        username: epg_source.username.clone(),
                        password: epg_source.password.clone(),
                        timezone: epg_source.original_timezone.clone(),
                        time_offset: Some(epg_source.time_offset.clone()),
                        is_active: epg_source.is_active,
                        update_linked: false, // Prevent circular updates
                    };

                    if let Some(update_url) = url {
                        if epg_source.url != **update_url {
                            update_request.url = update_url.clone();
                            needs_update = true;
                        }
                    }

                    if let Some(update_username) = username {
                        if !update_username.is_empty() && epg_source.username.as_deref() != Some(update_username) {
                            update_request.username = Some(update_username.clone());
                            needs_update = true;
                            debug!("Username will be updated for linked EPG source '{}'", epg_source.name);
                        }
                    }

                    if let Some(update_password) = password {
                        if !update_password.is_empty() && epg_source.password.as_deref() != Some(update_password) {
                            update_request.password = Some(update_password.clone());
                            needs_update = true;
                            debug!("Password will be updated for linked EPG source '{}'", epg_source.name);
                        }
                    }

                    if needs_update {
                        self.epg_source_repo.update(&epg_source.id, update_request).await?;
                        updated_count += 1;
                        info!("Updated linked EPG source '{}' to match stream source changes", epg_source.name);
                    }
                }
            }
            "epg" => {
                // Get the EPG source
                let epg_source = match self.epg_source_repo.find_by_id(&source_id).await? {
                    Some(source) => source,
                    None => return Ok(0),
                };

                // Update linked stream sources
                let linked_stream_sources = self.find_linked_stream_sources(&epg_source).await?;
                
                for stream_source in linked_stream_sources {
                    let mut needs_update = false;
                    let mut update_request = crate::models::StreamSourceUpdateRequest {
                        name: stream_source.name.clone(),
                        source_type: stream_source.source_type,
                        url: stream_source.url.clone(),
                        max_concurrent_streams: stream_source.max_concurrent_streams,
                        update_cron: stream_source.update_cron.clone(),
                        username: stream_source.username.clone(),
                        password: stream_source.password.clone(),
                        field_map: stream_source.field_map.clone(),
                        ignore_channel_numbers: stream_source.ignore_channel_numbers,
                        is_active: stream_source.is_active,
                        update_linked: false, // Prevent circular updates
                    };

                    if let Some(update_url) = url {
                        if stream_source.url != **update_url {
                            update_request.url = update_url.clone();
                            needs_update = true;
                        }
                    }

                    if let Some(update_username) = username {
                        if !update_username.is_empty() && stream_source.username.as_deref() != Some(update_username) {
                            update_request.username = Some(update_username.clone());
                            needs_update = true;
                            debug!("Username will be updated for linked stream source '{}'", stream_source.name);
                        }
                    }

                    if let Some(update_password) = password {
                        if !update_password.is_empty() && stream_source.password.as_deref() != Some(update_password) {
                            update_request.password = Some(update_password.clone());
                            needs_update = true;
                            debug!("Password will be updated for linked stream source '{}'", stream_source.name);
                        }
                    }

                    if needs_update {
                        self.stream_source_repo.update(&stream_source.id, update_request).await?;
                        updated_count += 1;
                        info!("Updated linked stream source '{}' to match EPG source changes", stream_source.name);
                    }
                }
            }
            _ => {
                debug!("Unknown source type '{}', skipping linked updates", source_type);
            }
        }

        Ok(updated_count)
    }
}