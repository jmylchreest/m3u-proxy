use anyhow::Result;
use chrono::Utc;
use std::path::PathBuf;
use tracing::{debug, info, warn};
use uuid::Uuid;

use crate::config::StorageConfig;
use crate::data_mapping::service::DataMappingService;
use crate::database::Database;
use crate::logo_assets::service::LogoAssetService;
use crate::models::*;
use crate::proxy::filter_engine::FilterEngine;
use crate::utils::uuid_parser::uuid_to_base64;
use sandboxed_file_manager::SandboxedManager;

pub struct ProxyGenerator {
    storage_config: StorageConfig,
    proxy_output_file_manager: Option<SandboxedManager>,
}

impl ProxyGenerator {
    pub fn new(storage_config: StorageConfig) -> Self {
        Self {
            storage_config,
            proxy_output_file_manager: None,
        }
    }

    pub fn with_file_manager(
        storage_config: StorageConfig,
        file_manager: SandboxedManager,
    ) -> Self {
        Self {
            storage_config,
            proxy_output_file_manager: Some(file_manager),
        }
    }

    /// Generate a complete proxy M3U using dependency injection (new architecture)
    pub async fn generate_with_config(
        &self,
        config: ResolvedProxyConfig,
        output: GenerationOutput,
        database: &Database,
        data_mapping_service: &DataMappingService,
        logo_service: &LogoAssetService,
        base_url: &str,
        engine_config: Option<crate::config::DataMappingEngineConfig>,
    ) -> Result<ProxyGeneration> {
        use std::time::Instant;
        
        // Initialize comprehensive stats tracking
        let mut stats = GenerationStats::new("dependency_injection".to_string());
        info!("Starting proxy generation for '{}' using dependency injection", config.proxy.name);

        if config.sources.is_empty() {
            warn!(
                "No sources found for proxy '{}', generating empty M3U",
                config.proxy.name
            );
            let m3u_content = "#EXTM3U\n".to_string();
            
            // Finalize stats for empty generation
            stats.total_channels_processed = 0;
            stats.sources_processed = 0;
            stats.m3u_size_bytes = m3u_content.len();
            stats.m3u_lines_generated = 1; // Just the header
            stats.finalize();
            
            let generation = ProxyGeneration {
                id: Uuid::new_v4(),
                proxy_id: config.proxy.id,
                version: 1,
                channel_count: 0,
                total_channels: 0,
                filtered_channels: 0,
                applied_filters: Vec::new(),
                m3u_content: m3u_content.clone(),
                created_at: Utc::now(),
                stats: Some(stats.clone()),
                processed_channels: None,
            };

            // Handle output based on destination
            self.write_output(&generation, &output, None).await?;
            
            info!("Empty generation completed: {}", stats.summary());
            return Ok(generation);
        }

        // Step 1: Get all channels from sources (with timing)
        let source_loading_start = Instant::now();
        let mut all_channels = Vec::new();
        
        for source_config in &config.sources {
            let source_start = Instant::now();
            let channels = database.get_source_channels(source_config.source.id).await?;
            let source_duration = source_start.elapsed().as_millis() as u64;
            
            info!(
                "Retrieved {} channels from source '{}' in {}ms",
                channels.len(),
                source_config.source.name,
                source_duration
            );
            
            // Track per-source metrics
            stats.channels_by_source.insert(source_config.source.name.clone(), channels.len());
            stats.source_processing_times.insert(source_config.source.name.clone(), source_duration);
            stats.sources_processed += 1;
            
            all_channels.extend(channels);
        }
        
        stats.add_stage_timing("source_loading", source_loading_start.elapsed().as_millis() as u64);
        stats.total_channels_processed = all_channels.len();
        info!("Total channels before processing: {}", all_channels.len());

        // Step 2: Apply data mapping to transform channels (with timing)
        let data_mapping_start = Instant::now();
        let mut mapped_channels = Vec::new();
        let mut total_transformations = 0;
        
        for source_config in &config.sources {
            let source_channels: Vec<Channel> = all_channels
                .iter()
                .filter(|ch| ch.source_id == source_config.source.id)
                .cloned()
                .collect();

            if source_channels.is_empty() {
                continue;
            }

            info!(
                "Applying data mapping to {} channels from source '{}'",
                source_channels.len(),
                source_config.source.name
            );

            let mapping_start = Instant::now();
            let transformed_channels = data_mapping_service
                .apply_mapping_for_proxy(
                    source_channels.clone(),
                    source_config.source.id,
                    logo_service,
                    base_url,
                    engine_config.clone(),
                )
                .await?;

            let mapping_duration = mapping_start.elapsed().as_millis() as u64;
            total_transformations += source_channels.len(); // Assume each channel gets processed

            info!(
                "Data mapping completed: {} channels from source '{}' in {}ms",
                transformed_channels.len(),
                source_config.source.name,
                mapping_duration
            );

            mapped_channels.extend(transformed_channels);
        }
        
        stats.data_mapping_duration_ms = data_mapping_start.elapsed().as_millis() as u64;
        stats.add_stage_timing("data_mapping", stats.data_mapping_duration_ms);
        stats.channels_mapped = mapped_channels.len();
        stats.mapping_transformations_applied = total_transformations;
        
        info!("Total channels after data mapping: {}", mapped_channels.len());

        // Step 3: Apply filters using resolved configuration (with timing)
        let filtering_start = Instant::now();
        stats.channels_before_filtering = mapped_channels.len();
        
        let filtered_channels = if !config.filters.is_empty() {
            info!("Applying {} filters", config.filters.len());
            let mut filter_engine = FilterEngine::new();
            
            // Convert to format expected by filter engine and track filter names
            let filter_tuples: Vec<(Filter, ProxyFilter)> = config.filters.iter()
                .filter(|f| f.is_active)
                .map(|f| {
                    let proxy_filter = ProxyFilter {
                        proxy_id: config.proxy.id,
                        filter_id: f.filter.id,
                        priority_order: f.priority_order,
                        is_active: f.is_active,
                        created_at: chrono::Utc::now(),
                    };
                    stats.filters_applied.push(f.filter.name.clone());
                    (f.filter.clone(), proxy_filter)
                })
                .collect();

            let filter_apply_start = Instant::now();
            let result = filter_engine.apply_filters(mapped_channels, filter_tuples).await?;
            let filter_duration = filter_apply_start.elapsed().as_millis() as u64;
            
            // Track individual filter timing (simplified - all filters get same duration)
            for filter_name in &stats.filters_applied {
                stats.filter_processing_times.insert(filter_name.clone(), filter_duration);
            }
            
            result
        } else {
            info!("No filters to apply");
            mapped_channels
        };
        
        stats.channels_after_filtering = filtered_channels.len();
        stats.add_stage_timing("filtering", filtering_start.elapsed().as_millis() as u64);
        
        info!("Total channels after filtering: {}", filtered_channels.len());

        // Step 4: Apply channel numbering algorithm (with timing)
        let numbering_start = Instant::now();
        let numbered_channels = self
            .apply_channel_numbering(&filtered_channels, config.proxy.starting_channel_number)
            .await?;

        stats.channel_numbering_duration_ms = numbering_start.elapsed().as_millis() as u64;
        stats.add_stage_timing("channel_numbering", stats.channel_numbering_duration_ms);
        stats.numbering_strategy = "sequential".to_string(); // Simple strategy for dependency injection
        stats.number_conflicts_resolved = 0; // No conflicts in simple sequential numbering

        info!("Channel numbering completed: {} numbered channels in {}ms", 
            numbered_channels.len(), stats.channel_numbering_duration_ms);

        // Step 5: Generate M3U content from numbered channels (with timing)
        let m3u_generation_start = Instant::now();
        let m3u_content = self
            .generate_m3u_content_from_numbered(&numbered_channels, &config.proxy.id.to_string(), base_url, config.proxy.cache_channel_logos, logo_service)
            .await?;

        stats.m3u_generation_duration_ms = m3u_generation_start.elapsed().as_millis() as u64;
        stats.add_stage_timing("m3u_generation", stats.m3u_generation_duration_ms);
        stats.m3u_size_bytes = m3u_content.len();
        stats.m3u_lines_generated = m3u_content.lines().count();

        // Step 6: Finalize stats and create generation record
        stats.finalize();
        
        let generation = ProxyGeneration {
            id: Uuid::new_v4(),
            proxy_id: config.proxy.id,
            version: 1,
            channel_count: numbered_channels.len() as i32,
            total_channels: all_channels.len(),
            filtered_channels: filtered_channels.len(),
            applied_filters: config.filters.iter().filter(|f| f.is_active).map(|f| f.filter.name.clone()).collect(),
            m3u_content,
            created_at: Utc::now(),
            stats: Some(stats.clone()),
            processed_channels: Some(numbered_channels),
        };

        // Step 7: Handle output based on destination
        self.write_output(&generation, &output, Some(&config)).await?;

        // Step 8: Log comprehensive summary
        info!("Proxy generation completed for '{}': {}", config.proxy.name, stats.summary());
        debug!("Detailed generation stats: {}", stats.detailed_summary());

        Ok(generation)
    }

    /// Generate a complete proxy M3U with data mapping and filters applied (legacy method)
    /// 
    /// **DEPRECATED**: Use `generate_with_config()` instead for better architecture.
    /// This method performs database queries during generation which can be inefficient.
    #[deprecated(note = "Use generate_with_config() with ProxyConfigResolver for better performance")]
    pub async fn generate(
        &self,
        proxy: &StreamProxy,
        database: &Database,
        data_mapping_service: &DataMappingService,
        logo_service: &LogoAssetService,
        base_url: &str,
        engine_config: Option<crate::config::DataMappingEngineConfig>,
    ) -> Result<ProxyGeneration> {
        info!("Starting proxy generation for '{}'", proxy.name);

        // Step 1: Get all sources attached to this proxy
        let sources = database.get_proxy_sources(proxy.id).await?;
        info!("Found {} sources for proxy '{}'", sources.len(), proxy.name);

        if sources.is_empty() {
            warn!(
                "No sources found for proxy '{}', generating empty M3U",
                proxy.name
            );
            let m3u_content = "#EXTM3U\n".to_string();
            return Ok(ProxyGeneration {
                id: Uuid::new_v4(),
                proxy_id: proxy.id,
                version: 1, // TODO: Get next version number from database
                channel_count: 0,
                m3u_content,
                created_at: Utc::now(),
                // New fields for enhanced tracking
                total_channels: 0,
                filtered_channels: 0,
                applied_filters: Vec::new(),
                stats: None, // Legacy method doesn't collect stats
                processed_channels: None,
            });
        }

        // Step 2: Get all channels from those sources (original, unmapped data)
        let mut all_channels = Vec::new();
        for source in &sources {
            let channels = database.get_source_channels(source.id).await?;
            info!(
                "Retrieved {} channels from source '{}'",
                channels.len(),
                source.name
            );
            all_channels.extend(channels);
        }

        info!("Total channels before processing: {}", all_channels.len());

        // Step 3: Apply data mapping to transform channels
        let mut mapped_channels = Vec::new();
        for source in &sources {
            let source_channels: Vec<Channel> = all_channels
                .iter()
                .filter(|ch| ch.source_id == source.id)
                .cloned()
                .collect();

            if source_channels.is_empty() {
                continue;
            }

            info!(
                "Applying data mapping to {} channels from source '{}'",
                source_channels.len(),
                source.name
            );

            let transformed_channels = data_mapping_service
                .apply_mapping_for_proxy(
                    source_channels,
                    source.id,
                    logo_service,
                    base_url,
                    engine_config.clone(),
                )
                .await?;

            info!(
                "Data mapping completed for source '{}', {} channels transformed",
                source.name,
                transformed_channels.len()
            );
            mapped_channels.extend(transformed_channels);
        }

        info!(
            "Total channels after data mapping: {}",
            mapped_channels.len()
        );

        // Step 4: Get all active filters for this proxy (sorted by order)
        let proxy_filters = database.get_proxy_filters_with_details(proxy.id).await?;
        info!(
            "Found {} filters for proxy '{}'",
            proxy_filters.len(),
            proxy.name
        );

        // Step 5: Apply filters to mapped channels
        let (filtered_channels, applied_filters) = if proxy_filters.is_empty() {
            info!(
                "No filters found for proxy '{}', using all {} mapped channels",
                proxy.name,
                mapped_channels.len()
            );
            (mapped_channels, Vec::new())
        } else {
            info!(
                "Applying {} filters to {} mapped channels for proxy '{}'",
                proxy_filters.len(),
                mapped_channels.len(),
                proxy.name
            );

            // Log filter details before application
            for (index, proxy_filter) in proxy_filters.iter().enumerate() {
                let filter_type = if proxy_filter.filter.is_inverse {
                    "EXCLUDE"
                } else {
                    "INCLUDE"
                };
                info!(
                    "Filter #{}: '{}' ({}), Priority: {}, Active: {}",
                    index + 1,
                    proxy_filter.filter.name,
                    filter_type,
                    proxy_filter.proxy_filter.priority_order,
                    proxy_filter.proxy_filter.is_active
                );
            }

            let mut filter_engine = FilterEngine::new();

            // Convert proxy filters to the format expected by filter engine
            let mut filter_tuples = Vec::new();
            let mut applied_filter_list = Vec::new();
            for proxy_filter in proxy_filters {
                filter_tuples.push((proxy_filter.filter.clone(), proxy_filter.proxy_filter));
                applied_filter_list.push(proxy_filter.filter);
            }

            let channel_count_before = mapped_channels.len();
            let filtered = filter_engine
                .apply_filters(mapped_channels, filter_tuples)
                .await?;
            info!(
                "All filters applied for proxy '{}': {} channels → {} channels ({} channels filtered out)",
                proxy.name,
                channel_count_before,
                filtered.len(),
                channel_count_before - filtered.len()
            );
            (filtered, applied_filter_list)
        };

        // Step 6: Assign channel numbers using sophisticated algorithm
        info!("Assigning channel numbers with sophisticated algorithm");
        let filtered_channels_len = filtered_channels.len();
        let numbered_channels = self
            .assign_channel_numbers(filtered_channels, &applied_filters)
            .await?;
        info!(
            "Channel numbering completed: {} channels assigned",
            numbered_channels.len()
        );

        // Step 7: Generate M3U content from numbered channels
        let m3u_content = self
            .generate_m3u_content_from_numbered(&numbered_channels, &proxy.id.to_string(), base_url, proxy.cache_channel_logos, logo_service)
            .await?;

        // Step 8: Save to database and return generation record
        let channel_count = numbered_channels.len() as i32;
        let generation = ProxyGeneration {
            id: Uuid::new_v4(),
            proxy_id: proxy.id,
            version: 1, // TODO: Get next version number from database
            channel_count,
            m3u_content,
            created_at: Utc::now(),
            // New fields for enhanced tracking
            total_channels: all_channels.len(),
            filtered_channels: filtered_channels_len,
            applied_filters: applied_filters.iter().map(|f| f.name.clone()).collect(),
            stats: None, // Legacy method doesn't collect stats
            processed_channels: Some(numbered_channels),
        };

        info!(
            "Proxy generation completed for '{}': {} channels in final M3U",
            proxy.name,
            channel_count
        );

        Ok(generation)
    }

    /// Generate M3U content from a list of channels
    #[allow(dead_code)]
    async fn generate_m3u_content(
        &self,
        channels: &[Channel],
        proxy_id: &str,
        base_url: &str,
    ) -> Result<String> {
        let mut m3u = String::from("#EXTM3U\n");

        for (index, channel) in channels.iter().enumerate() {
            let channel_number = index + 1;

            // Build EXTINF line
            let mut extinf = format!("#EXTINF:-1");

            if let Some(tvg_id) = &channel.tvg_id {
                if !tvg_id.is_empty() {
                    extinf.push_str(&format!(" tvg-id=\"{}\"", tvg_id));
                }
            }

            if let Some(tvg_name) = &channel.tvg_name {
                if !tvg_name.is_empty() {
                    extinf.push_str(&format!(" tvg-name=\"{}\"", tvg_name));
                }
            }

            if let Some(tvg_logo) = &channel.tvg_logo {
                if !tvg_logo.is_empty() {
                    // Ensure logo URL is full URL if it's a relative cached path
                    let full_logo_url = if tvg_logo.starts_with("/api/v1/logos/cached/") {
                        format!("{}{}", base_url.trim_end_matches('/'), tvg_logo)
                    } else {
                        tvg_logo.clone()
                    };
                    extinf.push_str(&format!(" tvg-logo=\"{}\"", full_logo_url));
                }
            }

            if let Some(group_title) = &channel.group_title {
                if !group_title.is_empty() {
                    extinf.push_str(&format!(" group-title=\"{}\"", group_title));
                }
            }

            extinf.push_str(&format!(" tvg-chno=\"{}\"", channel_number));
            extinf.push_str(&format!(",{}\n", channel.channel_name));

            m3u.push_str(&extinf);

            // Generate proxy URL for stream instead of original URL
            // This allows us to capture metrics and implement relays
            let proxy_stream_url = format!(
                "{}/stream/{}/{}",
                base_url.trim_end_matches('/'),
                uuid_to_base64(&Uuid::parse_str(proxy_id).unwrap_or_else(|_| Uuid::new_v4())),
                uuid_to_base64(&channel.id)
            );
            m3u.push_str(&format!("{}\n", proxy_stream_url));
        }

        Ok(m3u)
    }

    /// Save M3U content to the configured storage path
    pub async fn save_m3u_file(&self, proxy_id: Uuid, content: &str) -> Result<PathBuf> {
        // Ensure the M3U storage directory exists
        std::fs::create_dir_all(&self.storage_config.m3u_path)?;

        // Generate filename: proxy_id.m3u8
        let filename = format!("{}.m3u8", proxy_id);
        let file_path = self.storage_config.m3u_path.join(filename);

        // Write content to file
        std::fs::write(&file_path, content)?;

        Ok(file_path)
    }

    /// Save XMLTV content to the configured storage path
    pub async fn save_xmltv_file(&self, proxy_id: Uuid, content: &str) -> Result<PathBuf> {
        // Ensure the storage directory exists
        std::fs::create_dir_all(&self.storage_config.m3u_path)?;

        // Generate filename: proxy_id.xmltv
        let filename = format!("{}.xmltv", proxy_id);
        let file_path = self.storage_config.m3u_path.join(filename);

        // Write content to file
        std::fs::write(&file_path, content)?;

        Ok(file_path)
    }

    /// Get the storage path for M3U files
    #[allow(dead_code)]
    pub fn get_m3u_storage_path(&self) -> &PathBuf {
        &self.storage_config.m3u_path
    }

    /// Generate proxy and save to disk with comprehensive timing
    pub async fn generate_and_save(
        &self,
        proxy: &StreamProxy,
        database: &Database,
        data_mapping_service: &DataMappingService,
        logo_service: &LogoAssetService,
        base_url: &str,
        engine_config: Option<crate::config::DataMappingEngineConfig>,
    ) -> Result<ProxyGeneration> {
        use crate::models::GenerationTiming;
        use std::time::Instant;

        let generation_start = Instant::now();
        let mut timing = GenerationTiming::new();

        info!(
            "Starting proxy generation for '{}' (ID: {})",
            proxy.name, proxy.id
        );
        debug!(
            "Generation parameters: proxy_mode={:?}, base_url={}",
            proxy.proxy_mode, base_url
        );

        // Generate the proxy content with existing method
        #[allow(deprecated)]
        let generation = self
            .generate(
                proxy,
                database,
                data_mapping_service,
                logo_service,
                base_url,
                engine_config,
            )
            .await?;

        // Save M3U file to disk using ID as filename
        let step_start = Instant::now();
        debug!("Saving M3U file to disk for proxy '{}'", proxy.id);

        let file_path = self
            .save_m3u_file_by_id(&proxy.id.to_string(), &generation.m3u_content)
            .await?;
        timing.file_writing_ms = step_start.elapsed().as_millis();

        info!(
            "M3U file saved: {} -> {} bytes in {}ms",
            file_path.display(),
            generation.m3u_content.len(),
            timing.file_writing_ms
        );

        timing.total_duration_ms = generation_start.elapsed().as_millis();
        timing.log_statistics(&proxy.name, generation.channel_count as usize);

        info!(
            "Proxy generation completed successfully for '{}'",
            proxy.name
        );
        Ok(generation)
    }

    /// Save M3U content using proxy ID as filename
    pub async fn save_m3u_file_by_id(&self, proxy_id: &str, content: &str) -> Result<PathBuf> {
        if let Some(file_manager) = &self.proxy_output_file_manager {
            // Use sandboxed file manager for safer file handling
            let file_id = format!("m3u-{}.m3u", proxy_id);
            file_manager
                .write(&file_id, content)
                .await
                .map_err(|e| anyhow::anyhow!("Failed to store M3U content: {}", e))?;

            // Also write to the traditional location for serving
            // (this provides backwards compatibility while using sandboxed storage)
            self.save_m3u_file_traditional(proxy_id, content).await?;

            // Return the traditional path for backwards compatibility
            let filename = format!("{}.m3u8", proxy_id);
            Ok(self.storage_config.m3u_path.join(filename))
        } else {
            // Fallback to traditional method
            self.save_m3u_file_traditional(proxy_id, content).await
        }
    }

    /// Traditional atomic write method (private helper)
    async fn save_m3u_file_traditional(&self, proxy_id: &str, content: &str) -> Result<PathBuf> {
        // Ensure the M3U storage directory exists
        tokio::fs::create_dir_all(&self.storage_config.m3u_path).await?;

        // Generate filename: {ulid}.m3u8 (using ID for serving)
        let filename = format!("{}.m3u8", proxy_id);
        let file_path = self.storage_config.m3u_path.join(filename);

        // Write content to file atomically using temp file
        let temp_path = file_path.with_extension("m3u8.tmp");
        tokio::fs::write(&temp_path, content).await?;
        tokio::fs::rename(&temp_path, &file_path).await?;

        Ok(file_path)
    }

    /// Assign channel numbers with sophisticated algorithm
    pub async fn assign_channel_numbers(
        &self,
        channels: Vec<Channel>,
        applied_filters: &[Filter],
    ) -> Result<Vec<crate::models::NumberedChannel>> {
        use crate::models::ChannelNumberingState;
        use std::collections::{BTreeMap, HashSet};

        debug!(
            "Starting channel numbering algorithm for {} channels",
            channels.len()
        );

        // Find starting number from filters
        let starting_number = applied_filters
            .iter()
            .filter(|f| f.starting_channel_number > 0)
            .max_by_key(|f| f.starting_channel_number)
            .map(|f| {
                debug!(
                    "Using starting channel number {} from filter '{}'",
                    f.starting_channel_number, f.name
                );
                f.starting_channel_number
            })
            .unwrap_or_else(|| {
                debug!("No filters specify starting channel number, using default: 1");
                1
            });

        let mut state = ChannelNumberingState {
            reserved_numbers: HashSet::new(),
            explicit_assignments: BTreeMap::new(),
            sequential_channels: Vec::new(),
        };

        // Step 1: Categorize channels
        debug!("Categorizing channels by assignment type");
        self.categorize_channels_with_logging(channels, &mut state)?;

        // Step 2: Assign explicit numbers
        debug!("Assigning explicit channel numbers");
        let mut numbered_channels = self.assign_explicit_numbers_with_logging(&mut state)?;

        // Step 3: Assign sequential numbers
        debug!(
            "Assigning sequential channel numbers starting from {}",
            starting_number
        );
        numbered_channels
            .extend(self.assign_sequential_numbers_with_logging(&mut state, starting_number)?);

        // Step 4: Sort and log final results
        numbered_channels.sort_by_key(|nc| nc.assigned_number);

        let channel_range = if !numbered_channels.is_empty() {
            let min = numbered_channels.first().unwrap().assigned_number;
            let max = numbered_channels.last().unwrap().assigned_number;
            format!("{}-{}", min, max)
        } else {
            "none".to_string()
        };

        debug!(
            "Channel numbering complete: {} channels assigned numbers in range {}",
            numbered_channels.len(),
            channel_range
        );

        Ok(numbered_channels)
    }

    /// Extract tvg-channo from channel (could be from data mapping transformations)
    ///
    /// Data mapping can set tvg-channo values through transformations. Since the Channel
    /// model doesn't have a dedicated tvg_channo field, we need to check where data mapping
    /// would store this information. Currently checking tvg_shift as a placeholder,
    /// but this should be updated once data mapping integration is complete.
    fn extract_tvg_channo(&self, channel: &Channel) -> Result<Option<i32>> {
        // TODO: Once data mapping is fully integrated, this should check the actual
        // field where tvg-channo values are stored after data mapping transformations

        // For now, check if tvg_shift contains a numeric channel number
        if let Some(channo_str) = &channel.tvg_shift {
            if channo_str.chars().all(|c| c.is_ascii_digit()) {
                if let Ok(channo) = channo_str.parse::<i32>() {
                    if channo > 0 && channo < 10000 {
                        // Reasonable channel number range
                        return Ok(Some(channo));
                    }
                }
            }
        }

        // Also check tvg_id for explicit channel numbers
        if let Some(tvg_id) = &channel.tvg_id {
            if tvg_id.chars().all(|c| c.is_ascii_digit()) {
                if let Ok(channo) = tvg_id.parse::<i32>() {
                    if channo > 0 && channo < 10000 {
                        // Reasonable channel number range
                        return Ok(Some(channo));
                    }
                }
            }
        }

        Ok(None)
    }

    /// Categorize channels with detailed logging
    fn categorize_channels_with_logging(
        &self,
        channels: Vec<Channel>,
        state: &mut ChannelNumberingState,
    ) -> Result<()> {
        let mut explicit_count = 0;
        let mut sequential_count = 0;

        for channel in channels {
            if let Some(explicit_channo) = self.extract_tvg_channo(&channel)? {
                debug!(
                    "Channel '{}' has explicit channo: {}",
                    channel.channel_name, explicit_channo
                );
                state
                    .explicit_assignments
                    .entry(explicit_channo)
                    .or_insert_with(Vec::new)
                    .push(channel);
                explicit_count += 1;
            } else {
                state.sequential_channels.push(channel);
                sequential_count += 1;
            }
        }

        // Sort sequential channels alphabetically
        state.sequential_channels.sort_by(|a, b| {
            a.channel_name
                .to_lowercase()
                .cmp(&b.channel_name.to_lowercase())
        });

        debug!(
            "Categorization complete: {} explicit, {} sequential",
            explicit_count, sequential_count
        );

        // Log explicit assignment groups with conflicts
        for (channo, channels) in &state.explicit_assignments {
            if channels.len() > 1 {
                debug!(
                    "Conflict detected at channo {}: {} channels will be incremented",
                    channo,
                    channels.len()
                );
                for (index, channel) in channels.iter().enumerate() {
                    debug!(
                        "  {}: '{}' -> will become channo {}",
                        index + 1,
                        channel.channel_name,
                        channo + index as i32
                    );
                }
            }
        }

        Ok(())
    }

    /// Assign explicit numbers with detailed logging
    fn assign_explicit_numbers_with_logging(
        &self,
        state: &mut ChannelNumberingState,
    ) -> Result<Vec<crate::models::NumberedChannel>> {
        use crate::models::{ChannelNumberAssignmentType, NumberedChannel};

        let mut numbered_channels = Vec::new();
        let mut total_explicit = 0;
        let mut total_conflicts = 0;

        for (base_channo, mut channels) in std::mem::take(&mut state.explicit_assignments) {
            debug!("Processing explicit channo group: {}", base_channo);

            // Sort channels alphabetically for consistent conflict resolution
            channels.sort_by(|a, b| {
                a.channel_name
                    .to_lowercase()
                    .cmp(&b.channel_name.to_lowercase())
            });

            if channels.len() > 1 {
                debug!(
                    "Resolving conflict: {} channels want channo {}",
                    channels.len(),
                    base_channo
                );
                total_conflicts += 1;
            }

            for (index, channel) in channels.into_iter().enumerate() {
                let assigned_number = base_channo + index as i32;
                state.reserved_numbers.insert(assigned_number);

                let assignment_type = if index == 0 {
                    debug!(
                        "'{}' -> channo {} (explicit)",
                        channel.channel_name, assigned_number
                    );
                    ChannelNumberAssignmentType::Explicit
                } else {
                    debug!(
                        "'{}' -> channo {} (incremented due to conflict)",
                        channel.channel_name, assigned_number
                    );
                    ChannelNumberAssignmentType::ExplicitIncremented
                };

                numbered_channels.push(NumberedChannel {
                    channel,
                    assigned_number,
                    assignment_type,
                });
                total_explicit += 1;
            }
        }

        debug!(
            "Explicit assignment complete: {} channels assigned, {} conflicts resolved",
            total_explicit, total_conflicts
        );

        Ok(numbered_channels)
    }

    /// Assign sequential numbers with detailed logging
    fn assign_sequential_numbers_with_logging(
        &self,
        state: &mut ChannelNumberingState,
        starting_number: i32,
    ) -> Result<Vec<crate::models::NumberedChannel>> {
        use crate::models::{ChannelNumberAssignmentType, NumberedChannel};

        let mut numbered_channels = Vec::new();
        let mut current_number = starting_number;
        let mut gaps_skipped = 0;

        debug!(
            "Starting sequential assignment from channo {}",
            starting_number
        );

        for channel in state.sequential_channels.drain(..) {
            let original_number = current_number;

            // Find next available number, skipping reserved ones
            while state.reserved_numbers.contains(&current_number) {
                current_number += 1;
                gaps_skipped += 1;
            }

            if current_number != original_number {
                debug!(
                    "'{}' -> channo {} (gap filled, skipped {} reserved)",
                    channel.channel_name, current_number, gaps_skipped
                );
            }

            numbered_channels.push(NumberedChannel {
                channel,
                assigned_number: current_number,
                assignment_type: ChannelNumberAssignmentType::Sequential,
            });

            current_number += 1;
        }

        debug!(
            "Sequential assignment complete: {} channels assigned, {} gaps skipped",
            numbered_channels.len(),
            gaps_skipped
        );

        Ok(numbered_channels)
    }

    /// Generate M3U content from numbered channels
    pub async fn generate_m3u_content_from_numbered(
        &self,
        numbered_channels: &[crate::models::NumberedChannel],
        proxy_id: &str,
        base_url: &str,
        cache_channel_logos: bool,
        logo_service: &LogoAssetService,
    ) -> Result<String> {
        debug!(
            "Generating M3U content for {} numbered channels",
            numbered_channels.len()
        );

        let mut m3u = String::from("#EXTM3U\n");
        let mut bytes_written = 0;

        for (index, nc) in numbered_channels.iter().enumerate() {
            // Handle logo URL - use cached version if cache_channel_logos is enabled
            let logo_url = if cache_channel_logos {
                if let Some(original_logo) = &nc.channel.tvg_logo {
                    if !original_logo.is_empty() {
                        // Check if this is already a data mapping reference like @logo:UUID
                        if original_logo.starts_with("@logo:") {
                            // It's already a logo reference, keep it as is
                            original_logo.clone()
                        } else {
                            // Get or create cached logo URL
                            match logo_service.cache_logo_from_url(original_logo).await {
                                Ok(cache_id) => {
                                    logo_service.get_cached_logo_url(&cache_id, base_url)
                                },
                                Err(e) => {
                                    debug!("Failed to cache logo URL for '{}': {}", original_logo, e);
                                    // For failed caches, still ensure we return full URL if it's a relative path
                                    if original_logo.starts_with("/api/v1/logos/cached/") {
                                        format!("{}{}", base_url.trim_end_matches('/'), original_logo)
                                    } else {
                                        original_logo.clone()
                                    }
                                }
                            }
                        }
                    } else {
                        String::new()
                    }
                } else {
                    String::new()
                }
            } else {
                // Even when not caching, ensure logo URLs are full URLs if they're relative cached paths
                let logo = nc.channel.tvg_logo.as_deref().unwrap_or("");
                if logo.starts_with("/api/v1/logos/cached/") {
                    format!("{}{}", base_url.trim_end_matches('/'), logo)
                } else {
                    logo.to_string()
                }
            };

            let extinf = format!(
                "#EXTINF:-1 tvg-id=\"{}\" tvg-name=\"{}\" tvg-logo=\"{}\" tvg-shift=\"{}\" tvg-channo=\"{}\" group-title=\"{}\",{}",
                nc.channel.tvg_id.as_deref().unwrap_or(""),
                nc.channel.tvg_name.as_deref().unwrap_or(""),
                logo_url,
                nc.channel.tvg_shift.as_deref().unwrap_or(""),
                nc.assigned_number,
                nc.channel.group_title.as_deref().unwrap_or(""),
                nc.channel.channel_name
            );

            let proxy_stream_url = format!(
                "{}/stream/{}/{}",
                base_url.trim_end_matches('/'),
                uuid_to_base64(&Uuid::parse_str(proxy_id).unwrap_or_else(|_| Uuid::new_v4())),
                uuid_to_base64(&nc.channel.id)
            );

            let entry = format!("{}\n{}\n", extinf, proxy_stream_url);
            m3u.push_str(&entry);
            bytes_written += entry.len();

            // Log progress for large lists
            if (index + 1) % 500 == 0 {
                debug!(
                    "M3U generation progress: {}/{} channels, {} bytes",
                    index + 1,
                    numbered_channels.len(),
                    bytes_written
                );
            }
        }

        debug!(
            "M3U generation complete: {} entries, {} total bytes",
            numbered_channels.len(),
            m3u.len()
        );

        Ok(m3u)
    }

    /// Get the storage path for logos
    #[allow(dead_code)]
    pub fn get_logo_storage_path(&self) -> &PathBuf {
        &self.storage_config.cached_logo_path
    }

    /// Clean up old proxy versions (keep only the configured number)
    pub async fn cleanup_old_versions(&self, proxy_id: Uuid) -> Result<()> {
        let m3u_dir = &self.storage_config.m3u_path;
        if !m3u_dir.exists() {
            return Ok(());
        }

        // Find all files matching proxy_id pattern
        let proxy_pattern = format!("{}_", proxy_id);
        let mut versions = Vec::new();

        for entry in std::fs::read_dir(m3u_dir)? {
            let entry = entry?;
            let file_name = entry.file_name().to_string_lossy().to_string();

            if file_name.starts_with(&proxy_pattern) && file_name.ends_with(".m3u8") {
                if let Ok(metadata) = entry.metadata() {
                    if let Ok(modified) = metadata.modified() {
                        versions.push((file_name, modified, entry.path()));
                    }
                }
            }
        }

        // Sort by modification time (newest first)
        versions.sort_by(|a, b| b.1.cmp(&a.1));

        // Keep only the configured number of versions
        let keep_count = self.storage_config.proxy_versions_to_keep as usize;
        if versions.len() > keep_count {
            for (_, _, path) in versions.into_iter().skip(keep_count) {
                if let Err(e) = std::fs::remove_file(&path) {
                    warn!(
                        "Failed to remove old proxy version {}: {}",
                        path.display(),
                        e
                    );
                }
            }
        }

        Ok(())
    }

    /// Handle output writing based on destination
    async fn write_output(
        &self,
        generation: &ProxyGeneration,
        output: &GenerationOutput,
        config: Option<&ResolvedProxyConfig>,
    ) -> Result<()> {
        match output {
            GenerationOutput::Preview { file_manager, proxy_name } => {
                // Write M3U to preview file manager
                let m3u_file_id = format!("{}.m3u", proxy_name);
                file_manager
                    .write(&m3u_file_id, &generation.m3u_content)
                    .await
                    .map_err(|e| anyhow::anyhow!("Failed to write preview M3U: {}", e))?;
                
                // TODO: Generate and write XMLTV if EPG sources are present
                info!("Preview content written to file manager");
            }
            GenerationOutput::Production { file_manager, update_database } => {
                if let Some(config) = config {
                    // Write M3U to production file manager
                    let m3u_file_id = format!("{}.m3u", config.proxy.id);
                    file_manager
                        .write(&m3u_file_id, &generation.m3u_content)
                        .await
                        .map_err(|e| anyhow::anyhow!("Failed to write production M3U: {}", e))?;

                    // Also write to traditional location for serving
                    self.save_m3u_file_traditional(&config.proxy.id.to_string(), &generation.m3u_content).await?;

                    if *update_database {
                        // TODO: Save generation record to database
                        info!("Generation record would be saved to database");
                    }

                    info!("Production content written to file manager and traditional location");
                }
            }
            GenerationOutput::InMemory => {
                // Do nothing - content is just returned
                debug!("In-memory generation complete, no file output");
            }
        }
        Ok(())
    }

    /// Apply channel numbering to filtered channels (simplified version for dependency injection)
    async fn apply_channel_numbering(
        &self,
        channels: &[Channel],
        starting_number: i32,
    ) -> Result<Vec<crate::models::NumberedChannel>> {
        use crate::models::{NumberedChannel, ChannelNumberAssignmentType};
        
        debug!("Applying channel numbering starting from {}", starting_number);
        
        let mut numbered_channels = Vec::new();
        
        for (index, channel) in channels.iter().enumerate() {
            let assigned_number = starting_number + index as i32;
            
            numbered_channels.push(NumberedChannel {
                channel: channel.clone(),
                assigned_number,
                assignment_type: ChannelNumberAssignmentType::Sequential, // Simple sequential assignment
            });
        }
        
        debug!("Channel numbering completed: {} channels assigned", numbered_channels.len());
        Ok(numbered_channels)
    }
}
