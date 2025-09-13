//! Logo cache service with ultra-compact index and maintenance

use anyhow::{Context, Result};
use lru::LruCache;
use std::collections::HashMap;
use std::num::NonZeroUsize;
use std::path::Path;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{debug, info, warn};

use super::entry::{LogoCacheEntry, LogoCacheQuery};
use super::metadata::CachedLogoMetadata;

/// Logo cache service with memory-optimized index
pub struct LogoCacheService {
    /// Primary cache index: url_hash -> entry
    cache_index: Arc<RwLock<HashMap<u64, LogoCacheEntry>>>,
    /// Channel name hash -> Vec<url_hash> for channel-based lookups  
    channel_name_index: Arc<RwLock<HashMap<u64, Vec<u64>>>>,
    /// Channel group hash -> Vec<url_hash> for group-based lookups
    channel_group_index: Arc<RwLock<HashMap<u64, Vec<u64>>>>,
    /// LRU cache for search result strings (only for API responses)
    search_string_cache: Arc<RwLock<LruCache<u64, String>>>,
    /// Sandboxed file manager for safe file operations
    logo_file_manager: sandboxed_file_manager::SandboxedManager,
}

impl LogoCacheService {
    /// Create new logo cache service
    pub fn new(logo_file_manager: sandboxed_file_manager::SandboxedManager) -> Result<Self> {
        // The sandboxed file manager handles directory creation and access

        Ok(Self {
            cache_index: Arc::new(RwLock::new(HashMap::new())),
            channel_name_index: Arc::new(RwLock::new(HashMap::new())),
            channel_group_index: Arc::new(RwLock::new(HashMap::new())),
            search_string_cache: Arc::new(RwLock::new(
                LruCache::new(NonZeroUsize::new(1000).unwrap()), // Cache 1000 search strings
            )),
            logo_file_manager,
        })
    }

    /// Initialize cache service (lazy loading - no filesystem scan)
    pub async fn initialize(&self) -> Result<()> {
        info!(
            "Logo cache service initialized (lazy loading - filesystem scan will happen in background)"
        );
        Ok(())
    }

    /// Scan and load existing cached logos from filesystem (for background jobs)
    pub async fn scan_and_load_cache(&self) -> Result<()> {
        info!("Starting logo cache filesystem scan");

        let start_time = std::time::Instant::now();
        let mut scanned_files = 0;
        let mut loaded_entries = 0;

        // Clear existing cache before reload
        {
            let mut cache = self.cache_index.write().await;
            let mut channel_index = self.channel_name_index.write().await;
            let mut group_index = self.channel_group_index.write().await;
            let mut search_cache = self.search_string_cache.write().await;

            cache.clear();
            channel_index.clear();
            group_index.clear();
            search_cache.clear();
        }

        // Scan cache directory for existing logos using sandboxed file manager
        match self.logo_file_manager.list_files(".").await {
            Ok(files) => {
                for file_name in files {
                    scanned_files += 1;

                    // Only process actual image files (skip metadata files)
                    if file_name.ends_with(".json") {
                        continue;
                    }

                    // Get file size using metadata instead of reading the whole file
                    match self.logo_file_manager.metadata(&file_name).await {
                        Ok(metadata) => {
                            let file_size = metadata.len();
                            if let Ok(cache_entry) = self
                                .create_entry_from_filesystem(&file_name, file_size)
                                .await
                            {
                                self.add_entry_to_indices(cache_entry).await;
                                loaded_entries += 1;
                            }
                        }
                        Err(e) => {
                            debug!("Failed to get metadata for {}: {}", file_name, e);
                        }
                    }
                }
            }
            Err(e) => {
                warn!("Failed to list files in logo cache directory: {}", e);
            }
        }

        let duration = start_time.elapsed();
        let memory_usage = self.estimate_memory_usage().await;

        info!(
            "Logo cache scan completed: {} entries from {} files in {:.2}s (memory: {:.1}MB)",
            loaded_entries,
            scanned_files,
            duration.as_secs_f64(),
            memory_usage as f64 / 1024.0 / 1024.0
        );

        Ok(())
    }

    /// Create cache entry from filesystem scan, loading JSON metadata if available
    async fn create_entry_from_filesystem(
        &self,
        file_name: &str,
        file_size: u64,
    ) -> Result<LogoCacheEntry> {
        // Extract cache_id from filename (remove extension)
        let cache_id = std::path::Path::new(file_name)
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or(file_name);

        let metadata_file = format!("{}.json", cache_id);

        // Try to load JSON metadata using sandboxed file manager
        let (original_url, channel_name, channel_group, width, height) =
            if let Ok(metadata_bytes) = self.logo_file_manager.read(&metadata_file).await {
                // Convert bytes to string
                match String::from_utf8(metadata_bytes) {
                    Ok(metadata_content) => {
                        // Parse metadata JSON
                        match serde_json::from_str::<CachedLogoMetadata>(&metadata_content) {
                            Ok(metadata) => {
                                debug!(
                                    "Loaded metadata for {}: channel={:?}, group={:?}",
                                    cache_id, metadata.channel_name, metadata.channel_group
                                );
                                (
                                    metadata.original_url,
                                    metadata.channel_name,
                                    metadata.channel_group,
                                    metadata.width,
                                    metadata.height,
                                )
                            }
                            Err(e) => {
                                debug!("Failed to parse metadata for {}: {}", cache_id, e);
                                // Fallback to filename-based dimensions
                                let (w, h) = self.parse_dimensions_from_filename(file_name);
                                (None, None, None, w, h)
                            }
                        }
                    }
                    Err(e) => {
                        debug!(
                            "Failed to convert metadata bytes to string for {}: {}",
                            cache_id, e
                        );
                        let (w, h) = self.parse_dimensions_from_filename(file_name);
                        (None, None, None, w, h)
                    }
                }
            } else {
                // No metadata file, try to extract dimensions from filename
                let (w, h) = self.parse_dimensions_from_filename(file_name);
                (None, None, None, w, h)
            };

        // Create entry with loaded metadata
        let entry = LogoCacheEntry::new(
            &original_url.unwrap_or_else(|| format!("filesystem:{}", file_name)),
            channel_name.as_deref(),
            channel_group.as_deref(),
            width,
            height,
            file_size,
            file_name.to_string(),
        );

        // Store channel name in string cache for future API responses
        if let Some(ref name) = channel_name {
            let mut string_cache = self.search_string_cache.write().await;
            string_cache.put(entry.url_hash, name.clone());
        }

        Ok(entry)
    }

    /// Parse dimensions from filename like "WxH.ext"
    fn parse_dimensions_from_filename(&self, filename_part: &str) -> (Option<i32>, Option<i32>) {
        if let Some(dimensions_part) = filename_part.split('.').next()
            && let Some((w, h)) = dimensions_part.split_once('x')
            && let (Ok(width), Ok(height)) = (w.parse::<i32>(), h.parse::<i32>())
        {
            return (Some(width), Some(height));
        }
        (None, None)
    }

    /// Add new logo to cache with metadata
    pub async fn add_logo(
        &self,
        original_url: &str,
        channel_name: Option<&str>,
        channel_group: Option<&str>,
        file_path: &Path,
        width: Option<i32>,
        height: Option<i32>,
    ) -> Result<()> {
        let metadata = tokio::fs::metadata(file_path)
            .await
            .with_context(|| format!("Failed to get metadata for {}", file_path.display()))?;

        let relative_path = file_path
            .file_name()
            .unwrap_or_else(|| file_path.as_os_str())
            .to_string_lossy()
            .to_string();

        let entry = LogoCacheEntry::new(
            original_url,
            channel_name,
            channel_group,
            width,
            height,
            metadata.len(),
            relative_path,
        );

        debug!(
            "Adding logo to cache: {} ({}x{:?}, {} bytes)",
            original_url,
            width.unwrap_or(0),
            height.unwrap_or(0),
            metadata.len()
        );

        self.add_entry_to_indices(entry).await;
        Ok(())
    }

    /// Add entry to all indices
    async fn add_entry_to_indices(&self, entry: LogoCacheEntry) {
        let url_hash = entry.url_hash;

        // Add to primary index
        {
            let mut cache_index = self.cache_index.write().await;
            cache_index.insert(url_hash, entry.clone());
        }

        // Add to channel name index
        if let Some(name_hash) = entry.channel_name_hash {
            let mut name_index = self.channel_name_index.write().await;
            name_index.entry(name_hash).or_default().push(url_hash);
        }

        // Add to channel group index
        if let Some(group_hash) = entry.channel_group_hash {
            let mut group_index = self.channel_group_index.write().await;
            group_index.entry(group_hash).or_default().push(url_hash);
        }
    }

    /// Search for logos by query
    pub async fn search(&self, query: &LogoCacheQuery) -> Result<Vec<LogoCacheSearchResult>> {
        let (url_hash, name_hash, group_hash) = query.to_hashes();
        let mut results = Vec::new();

        let cache_index = self.cache_index.read().await;

        // Priority 1: Direct URL match
        if let Some(hash) = url_hash
            && let Some(entry) = cache_index.get(&hash)
        {
            results.push(
                self.entry_to_search_result(entry, &query.original_url)
                    .await,
            );
            return Ok(results);
        }

        // Priority 2: Channel name matches
        if let Some(hash) = name_hash {
            let name_index = self.channel_name_index.read().await;
            if let Some(url_hashes) = name_index.get(&hash) {
                for &url_hash in url_hashes {
                    if let Some(entry) = cache_index.get(&url_hash) {
                        results.push(
                            self.entry_to_search_result(entry, &query.channel_name)
                                .await,
                        );
                    }
                }
            }
        }

        // Priority 3: Channel group matches
        if let Some(hash) = group_hash {
            let group_index = self.channel_group_index.read().await;
            if let Some(url_hashes) = group_index.get(&hash) {
                for &url_hash in url_hashes {
                    if let Some(entry) = cache_index.get(&url_hash) {
                        // Avoid duplicates from name matches
                        if !results.iter().any(|r| r.url_hash == entry.url_hash) {
                            results.push(
                                self.entry_to_search_result(entry, &query.channel_group)
                                    .await,
                            );
                        }
                    }
                }
            }
        }

        // Priority 4: Fallback to partial/substring search if no exact matches found
        if results.is_empty() {
            results = self.search_substring(query).await?;
        }

        // Sort by relevance (URL matches first, then by file size)
        results.sort_by(|a, b| b.file_size.cmp(&a.file_size));

        Ok(results)
    }

    /// Fallback search method using substring matching on metadata
    async fn search_substring(&self, query: &LogoCacheQuery) -> Result<Vec<LogoCacheSearchResult>> {
        use tracing::debug;
        let mut results = Vec::new();
        let mut searched_count = 0;
        let mut matched_count = 0;

        let cache_index = self.cache_index.read().await;

        // Get search term (combine all query parts into one search string)
        let search_term = [
            query.original_url.as_ref(),
            query.channel_name.as_ref(),
            query.channel_group.as_ref(),
        ]
        .into_iter()
        .filter_map(|opt| opt.map(|s| s.as_str()))
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase();

        if search_term.trim().is_empty() {
            return Ok(results);
        }

        // Iterate through all cache entries and check metadata for substring matches
        for (_, entry) in cache_index.iter() {
            searched_count += 1;

            // Try to read the JSON metadata file for this entry
            let metadata_filename = format!(
                "{}.json",
                std::path::Path::new(&entry.relative_path)
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or("unknown")
            );

            if let Ok(Some(metadata)) = self.read_metadata_file(&metadata_filename).await {
                let mut score = 0u32;
                let mut matched_field = None;

                // Search ALL metadata fields for the term (case-insensitive)
                if let Some(ref original_url) = metadata.original_url
                    && original_url.to_lowercase().contains(&search_term)
                {
                    score += 10; // URL matches are high priority
                    matched_field = Some(original_url.clone());
                }

                if let Some(ref channel_name) = metadata.channel_name
                    && channel_name.to_lowercase().contains(&search_term)
                {
                    score += 8; // Channel name matches are medium-high priority
                    if matched_field.is_none() {
                        matched_field = Some(channel_name.clone());
                    }
                }

                if let Some(ref channel_group) = metadata.channel_group
                    && channel_group.to_lowercase().contains(&search_term)
                {
                    score += 6; // Channel group matches are medium priority
                    if matched_field.is_none() {
                        matched_field = Some(channel_group.clone());
                    }
                }

                if let Some(ref description) = metadata.description
                    && description.to_lowercase().contains(&search_term)
                {
                    score += 3; // Description matches are lower priority
                    if matched_field.is_none() {
                        matched_field = Some(description.clone());
                    }
                }

                if let Some(ref tags) = metadata.tags {
                    for tag in tags {
                        if tag.to_lowercase().contains(&search_term) {
                            score += 4; // Tag matches are medium-low priority
                            if matched_field.is_none() {
                                matched_field = Some(tag.clone());
                            }
                        }
                    }
                }

                // Check extra_fields if they exist
                if let Some(ref extra_fields) = metadata.extra_fields {
                    for (key, value) in extra_fields {
                        if key.to_lowercase().contains(&search_term)
                            || value.to_lowercase().contains(&search_term)
                        {
                            score += 2; // Extra field matches are lowest priority
                            if matched_field.is_none() {
                                matched_field = Some(format!("{}: {}", key, value));
                            }
                        }
                    }
                }

                if score > 0 {
                    matched_count += 1;
                    let mut result = self.entry_to_search_result(entry, &matched_field).await;
                    result.relevance_score = Some(score); // Add relevance scoring
                    results.push(result);
                }
            }
        }

        debug!(
            "Substring search for '{}': {} searched, {} matched, {} results",
            search_term,
            searched_count,
            matched_count,
            results.len()
        );

        // Sort by relevance score (highest first), then by file size
        results.sort_by(|a, b| match (a.relevance_score, b.relevance_score) {
            (Some(score_a), Some(score_b)) => score_b.cmp(&score_a),
            (Some(_), None) => std::cmp::Ordering::Less,
            (None, Some(_)) => std::cmp::Ordering::Greater,
            (None, None) => b.file_size.cmp(&a.file_size),
        });

        Ok(results)
    }

    /// Read metadata file for a given filename
    async fn read_metadata_file(
        &self,
        filename: &str,
    ) -> Result<Option<crate::services::logo_cache::CachedLogoMetadata>> {
        // Try to read metadata using the sandboxed file manager
        let bytes = match self.logo_file_manager.read(filename).await {
            Ok(bytes) => bytes,
            Err(_) => return Ok(None), // File doesn't exist or can't be read
        };

        match serde_json::from_slice::<crate::services::logo_cache::CachedLogoMetadata>(&bytes) {
            Ok(metadata) => Ok(Some(metadata)),
            Err(_) => Ok(None), // Invalid JSON, skip
        }
    }

    /// Remove logo from cache by cache_id (used when logos are deleted via API)
    pub async fn remove_by_cache_id(&self, cache_id: &str) -> bool {
        use tracing::debug;

        let mut removed = false;
        let mut url_hash_to_remove = None;

        // Find the entry by cache_id by scanning the primary index
        {
            let cache_index = self.cache_index.read().await;
            for (url_hash, entry) in cache_index.iter() {
                // Extract cache_id from relative_path (filename without extension)
                let entry_cache_id = std::path::Path::new(&entry.relative_path)
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or("");

                if entry_cache_id == cache_id {
                    url_hash_to_remove = Some(*url_hash);
                    debug!(
                        "Found logo cache entry to remove: cache_id={}, url_hash={}",
                        cache_id, url_hash
                    );
                    break;
                }
            }
        }

        // Remove the entry if found
        if let Some(url_hash) = url_hash_to_remove {
            let mut cache_index = self.cache_index.write().await;
            if let Some(entry) = cache_index.remove(&url_hash) {
                debug!("Removed logo cache entry for cache_id: {}", cache_id);

                // Remove from secondary indices
                if let Some(name_hash) = entry.channel_name_hash {
                    let mut name_index = self.channel_name_index.write().await;
                    if let Some(url_hashes) = name_index.get_mut(&name_hash) {
                        url_hashes.retain(|&h| h != url_hash);
                        if url_hashes.is_empty() {
                            name_index.remove(&name_hash);
                        }
                    }
                }

                if let Some(group_hash) = entry.channel_group_hash {
                    let mut group_index = self.channel_group_index.write().await;
                    if let Some(url_hashes) = group_index.get_mut(&group_hash) {
                        url_hashes.retain(|&h| h != url_hash);
                        if url_hashes.is_empty() {
                            group_index.remove(&group_hash);
                        }
                    }
                }

                removed = true;
            }

            // Remove from string cache if present
            {
                let mut string_cache = self.search_string_cache.write().await;
                string_cache.pop(&url_hash);
            }
        }

        removed
    }

    /// Remove logo from cache by filename (used when logos are deleted from filesystem)
    pub async fn remove_by_filename(&self, filename: &str) -> bool {
        // Extract cache ID from filename (without extension)
        let cache_id = std::path::Path::new(filename)
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or(filename);

        // Use the cache_id removal method for consistency
        self.remove_by_cache_id(cache_id).await
    }

    /// Detect mime type from file extension
    fn detect_mime_type(&self, relative_path: &str) -> String {
        let extension = std::path::Path::new(relative_path)
            .extension()
            .and_then(|ext| ext.to_str())
            .unwrap_or("");

        match extension.to_lowercase().as_str() {
            "png" => "image/png",
            "jpg" | "jpeg" => "image/jpeg",
            "gif" => "image/gif",
            "svg" => "image/svg+xml",
            "webp" => "image/webp",
            "ico" => "image/x-icon",
            "bmp" => "image/bmp",
            "tiff" | "tif" => "image/tiff",
            _ => "image/png", // Default to PNG for cached images as they are typically converted
        }
        .to_string()
    }

    /// List all logos in cache (for general browsing)
    pub async fn list_all(&self, limit: Option<usize>) -> Result<Vec<LogoCacheSearchResult>> {
        let cache_index = self.cache_index.read().await;
        let mut results = Vec::new();

        let mut entries: Vec<_> = cache_index.values().collect();

        // Sort by last_accessed descending (most recently used first)
        entries.sort_by(|a, b| b.last_accessed.cmp(&a.last_accessed));

        // Apply limit if specified
        if let Some(limit) = limit {
            entries.truncate(limit);
        }

        for entry in entries {
            results.push(self.entry_to_search_result(entry, &None).await);
        }

        Ok(results)
    }

    /// Convert entry to search result with cached string lookup
    async fn entry_to_search_result(
        &self,
        entry: &LogoCacheEntry,
        matched_string: &Option<String>,
    ) -> LogoCacheSearchResult {
        let (width, height) = entry.get_dimensions();

        // Use LRU cache for string resolution (only for API responses)
        let matched_text = if let Some(text) = matched_string {
            Some(text.clone())
        } else {
            // Try to resolve from string cache
            let mut string_cache = self.search_string_cache.write().await;
            if let Some(cached) = string_cache.get(&entry.url_hash) {
                Some(cached.clone())
            } else {
                // Try to load channel name from JSON metadata file
                self.load_channel_name_from_metadata(&entry.relative_path)
                    .await
            }
        };

        // Try to load metadata for timestamps and additional info
        // Convert PNG path to JSON path by changing extension
        let json_path = std::path::Path::new(&entry.relative_path)
            .with_extension("json")
            .to_string_lossy()
            .to_string();
        let (cached_at, updated_at) =
            if let Ok(Some(metadata)) = self.read_metadata_file(&json_path).await {
                (Some(metadata.cached_at), Some(metadata.updated_at))
            } else {
                (None, None)
            };

        // Determine mime type from file extension
        let mime_type = self.detect_mime_type(&entry.relative_path);

        LogoCacheSearchResult {
            url_hash: entry.url_hash,
            relative_path: entry.relative_path.clone(),
            width,
            height,
            file_size: entry.file_size as u64,
            matched_text,
            last_accessed: entry.last_accessed,
            relevance_score: None, // Set by caller if needed
            cached_at,
            updated_at,
            mime_type: Some(mime_type),
        }
    }

    /// Load channel name from JSON metadata file
    async fn load_channel_name_from_metadata(&self, relative_path: &str) -> Option<String> {
        // Extract cache_id from relative path (without extension)
        let cache_id = std::path::Path::new(relative_path)
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or_else(|| {
                // Strip .json suffix if present
                relative_path.strip_suffix(".json").unwrap_or(relative_path)
            });

        let metadata_file = format!("{}.json", cache_id);

        // Try to load JSON metadata using sandboxed file manager
        if let Ok(metadata_bytes) = self.logo_file_manager.read(&metadata_file).await {
            // Convert bytes to string and parse JSON metadata
            if let Ok(metadata_content) = String::from_utf8(metadata_bytes)
                && let Ok(metadata) = serde_json::from_str::<CachedLogoMetadata>(&metadata_content)
            {
                return metadata.channel_name;
            }
        }
        None
    }

    /// Run maintenance tasks
    pub async fn run_maintenance(
        &self,
        max_cache_size_mb: u64,
        max_age_days: u64,
    ) -> Result<MaintenanceStats> {
        info!("Starting logo cache maintenance");
        let start_time = std::time::Instant::now();

        let mut stats = MaintenanceStats::default();
        let max_cache_bytes = max_cache_size_mb * 1024 * 1024;
        let max_age_seconds = max_age_days * 24 * 60 * 60;
        let current_time = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        let mut cache_index = self.cache_index.write().await;
        let mut name_index = self.channel_name_index.write().await;
        let mut group_index = self.channel_group_index.write().await;

        // Collect entries for analysis
        let mut entries: Vec<_> = cache_index.iter().collect();
        entries.sort_by_key(|(_, entry)| entry.last_accessed);

        let mut total_size = 0u64;
        let mut removed_entries = Vec::new();

        for (url_hash, entry) in entries {
            stats.total_entries += 1;
            total_size += entry.file_size as u64;

            let age_seconds = current_time.saturating_sub(entry.last_accessed);
            let should_remove_by_age = age_seconds > max_age_seconds;
            let should_remove_by_size = total_size > max_cache_bytes;

            if should_remove_by_age || should_remove_by_size {
                // Remove file using sandboxed file manager
                match self
                    .logo_file_manager
                    .remove_file(&entry.relative_path)
                    .await
                {
                    Ok(()) => {
                        removed_entries.push(*url_hash);
                        if should_remove_by_age {
                            stats.removed_by_age += 1;
                        } else {
                            stats.removed_by_size += 1;
                        }
                        stats.bytes_freed += entry.file_size as u64;
                        debug!(
                            "Removed cached logo: {} (age: {}d)",
                            entry.relative_path,
                            age_seconds / 86400
                        );
                    }
                    Err(e) => {
                        warn!(
                            "Failed to remove cached logo {}: {}",
                            entry.relative_path, e
                        );
                        stats.removal_errors += 1;
                    }
                }
            } else {
                stats.kept_entries += 1;
            }
        }

        // Remove from indices
        for url_hash in removed_entries {
            if let Some(entry) = cache_index.remove(&url_hash) {
                // Remove from secondary indices
                if let Some(name_hash) = entry.channel_name_hash
                    && let Some(urls) = name_index.get_mut(&name_hash)
                {
                    urls.retain(|&h| h != url_hash);
                    if urls.is_empty() {
                        name_index.remove(&name_hash);
                    }
                }

                if let Some(group_hash) = entry.channel_group_hash
                    && let Some(urls) = group_index.get_mut(&group_hash)
                {
                    urls.retain(|&h| h != url_hash);
                    if urls.is_empty() {
                        group_index.remove(&group_hash);
                    }
                }
            }
        }

        stats.duration_ms = start_time.elapsed().as_millis() as u64;
        stats.final_memory_mb =
            (self.estimate_memory_usage().await as f64 / 1024.0 / 1024.0) as u64;

        info!(
            "Logo cache maintenance completed: removed {} entries ({:.1}MB freed) in {:.2}s",
            stats.removed_by_age + stats.removed_by_size,
            stats.bytes_freed as f64 / 1024.0 / 1024.0,
            stats.duration_ms as f64 / 1000.0
        );

        Ok(stats)
    }

    /// Clear all cached logos and metadata files
    pub async fn clear_all_cache(&self) -> Result<u64> {
        info!("Starting to clear all cached logos");
        // Clear in-memory indices first
        let deleted_count = {
            let mut cache_index = self.cache_index.write().await;
            let mut name_index = self.channel_name_index.write().await;
            let mut group_index = self.channel_group_index.write().await;

            let count = cache_index.len() as u64;
            cache_index.clear();
            name_index.clear();
            group_index.clear();
            count
        };

        // Clear filesystem cache directory by listing all files and removing them
        match self.logo_file_manager.list_files(".").await {
            Ok(files) => {
                let mut filesystem_deleted = 0;
                for file in files {
                    if let Err(e) = self.logo_file_manager.remove_file(&file).await {
                        warn!("Failed to remove cached file {}: {}", file, e);
                    } else {
                        filesystem_deleted += 1;
                    }
                }
                info!(
                    "Successfully cleared {} cached logos from filesystem (memory: {})",
                    filesystem_deleted, deleted_count
                );
            }
            Err(e) => {
                warn!("Failed to list cache directory for clearing: {}", e);
                // Continue anyway since memory indices are cleared
            }
        }

        Ok(deleted_count)
    }

    /// Estimate total memory usage of cache indices
    pub async fn estimate_memory_usage(&self) -> usize {
        let cache_index = self.cache_index.read().await;
        let name_index = self.channel_name_index.read().await;
        let group_index = self.channel_group_index.read().await;

        let mut total = 0;

        // Main cache index
        for entry in cache_index.values() {
            total += entry.memory_usage();
        }

        // Secondary indices (rough estimate)
        total += name_index.len() * (8 + 24); // Hash + Vec overhead
        total += group_index.len() * (8 + 24);

        // Count Vec entries
        for urls in name_index.values() {
            total += urls.len() * 8; // u64 per URL hash
        }
        for urls in group_index.values() {
            total += urls.len() * 8;
        }

        total
    }

    /// Get cache statistics
    pub async fn get_stats(&self) -> LogoCacheStats {
        let cache_index = self.cache_index.read().await;
        let memory_usage = self.estimate_memory_usage().await;

        let mut total_file_size = 0u64;
        for entry in cache_index.values() {
            total_file_size += entry.file_size as u64;
        }

        LogoCacheStats {
            total_entries: cache_index.len() as u64,
            memory_usage_bytes: memory_usage as u64,
            storage_usage_bytes: total_file_size,
            avg_entry_size_bytes: if !cache_index.is_empty() {
                memory_usage / cache_index.len()
            } else {
                0
            },
        }
    }
}

#[derive(Debug, Clone)]
pub struct LogoCacheSearchResult {
    pub url_hash: u64,
    pub relative_path: String,
    pub width: Option<i32>,
    pub height: Option<i32>,
    pub file_size: u64,
    pub matched_text: Option<String>,
    pub last_accessed: u64,
    pub relevance_score: Option<u32>,
    pub cached_at: Option<chrono::DateTime<chrono::Utc>>,
    pub updated_at: Option<chrono::DateTime<chrono::Utc>>,
    pub mime_type: Option<String>,
}

#[derive(Debug, Default)]
pub struct MaintenanceStats {
    pub total_entries: u64,
    pub kept_entries: u64,
    pub removed_by_age: u64,
    pub removed_by_size: u64,
    pub removal_errors: u64,
    pub bytes_freed: u64,
    pub duration_ms: u64,
    pub final_memory_mb: u64,
}

#[derive(Debug)]
pub struct LogoCacheStats {
    pub total_entries: u64,
    pub memory_usage_bytes: u64,
    pub storage_usage_bytes: u64,
    pub avg_entry_size_bytes: usize,
}
