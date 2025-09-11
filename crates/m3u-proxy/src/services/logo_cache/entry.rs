//! Compact logo cache entry structure with hash-based channel matching

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use super::dimension_encoder::DimensionEncoder;

/// Ultra-compact logo cache entry optimized for minimal memory usage
#[derive(Debug, Clone)]
pub struct LogoCacheEntry {
    /// xxHash64 hash of original_url for fast lookups
    pub url_hash: u64,
    /// xxHash64 hash of channel name (if available)
    pub channel_name_hash: Option<u64>,
    /// xxHash64 hash of channel group (if available)  
    pub channel_group_hash: Option<u64>,
    /// 12-bit encoded width
    pub encoded_width: u16,
    /// 12-bit encoded height
    pub encoded_height: u16,
    /// File size in bytes (u32 = max 4GB, sufficient for logos)
    pub file_size: u32,
    /// File path relative to logo cache directory (String needed for filesystem access)
    pub relative_path: String,
    /// Last accessed timestamp (for LRU maintenance)
    pub last_accessed: u64,
}

impl LogoCacheEntry {
    /// Create new entry from logo metadata
    pub fn new(
        original_url: &str,
        channel_name: Option<&str>,
        channel_group: Option<&str>,
        width: Option<i32>,
        height: Option<i32>,
        file_size: u64,
        relative_path: String,
    ) -> Self {
        Self {
            url_hash: Self::hash_string(original_url),
            channel_name_hash: channel_name.map(Self::hash_string),
            channel_group_hash: channel_group.map(Self::hash_string),
            encoded_width: DimensionEncoder::encode(width),
            encoded_height: DimensionEncoder::encode(height),
            file_size: file_size.min(u32::MAX as u64) as u32,
            relative_path,
            last_accessed: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
        }
    }
    
    /// Fast string hashing using DefaultHasher
    fn hash_string(s: &str) -> u64 {
        let mut hasher = DefaultHasher::new();
        s.hash(&mut hasher);
        hasher.finish()
    }
    
    /// Check if entry matches search criteria
    pub fn matches_search(&self, query_url_hash: Option<u64>, query_name_hash: Option<u64>, query_group_hash: Option<u64>) -> bool {
        // URL hash match (highest priority)
        if let Some(url_hash) = query_url_hash {
            return self.url_hash == url_hash;
        }
        
        // Channel name match
        if let (Some(query_hash), Some(entry_hash)) = (query_name_hash, self.channel_name_hash)
            && entry_hash == query_hash {
                return true;
            }
        
        // Channel group match (lowest priority)
        if let (Some(query_hash), Some(entry_hash)) = (query_group_hash, self.channel_group_hash)
            && entry_hash == query_hash {
                return true;
            }
        
        false
    }
    
    /// Update last accessed timestamp
    pub fn touch(&mut self) {
        self.last_accessed = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
    }
    
    /// Get decoded dimensions
    pub fn get_dimensions(&self) -> (Option<i32>, Option<i32>) {
        (
            DimensionEncoder::decode(self.encoded_width),
            DimensionEncoder::decode(self.encoded_height),
        )
    }
    
    /// Estimate memory usage of this entry in bytes
    pub fn memory_usage(&self) -> usize {
        std::mem::size_of::<Self>() + self.relative_path.capacity()
    }
}

/// Search query for logo cache lookups
#[derive(Debug, Clone)]
pub struct LogoCacheQuery {
    pub original_url: Option<String>,
    pub channel_name: Option<String>,
    pub channel_group: Option<String>,
}

impl LogoCacheQuery {
    pub fn from_url(url: &str) -> Self {
        Self {
            original_url: Some(url.to_string()),
            channel_name: None,
            channel_group: None,
        }
    }
    
    pub fn from_channel(name: &str, group: Option<&str>) -> Self {
        Self {
            original_url: None,
            channel_name: Some(name.to_string()),
            channel_group: group.map(str::to_string),
        }
    }
    
    /// Convert to hash-based query for fast matching
    pub fn to_hashes(&self) -> (Option<u64>, Option<u64>, Option<u64>) {
        (
            self.original_url.as_ref().map(|s| LogoCacheEntry::hash_string(s)),
            self.channel_name.as_ref().map(|s| LogoCacheEntry::hash_string(s)),
            self.channel_group.as_ref().map(|s| LogoCacheEntry::hash_string(s)),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_entry_creation() {
        let entry = LogoCacheEntry::new(
            "http://example.com/logo.png",
            Some("Channel 1"),
            Some("News"),
            Some(64),
            Some(64),
            1024,
            "channel1_64x64.png".to_string(),
        );
        
        assert!(entry.url_hash != 0);
        assert!(entry.channel_name_hash.is_some());
        assert!(entry.channel_group_hash.is_some());
        assert_eq!(entry.encoded_width, 64); // Direct encoding for small sizes
        assert_eq!(entry.encoded_height, 64);
        assert_eq!(entry.file_size, 1024);
        assert_eq!(entry.relative_path, "channel1_64x64.png");
    }
    
    #[test]
    fn test_search_matching() {
        let entry = LogoCacheEntry::new(
            "http://example.com/logo.png",
            Some("Channel 1"),
            Some("News"),
            Some(64), Some(64),
            1024,
            "test.png".to_string(),
        );
        
        // URL match
        let url_hash = LogoCacheEntry::hash_string("http://example.com/logo.png");
        assert!(entry.matches_search(Some(url_hash), None, None));
        
        // Channel name match
        let name_hash = LogoCacheEntry::hash_string("Channel 1");
        assert!(entry.matches_search(None, Some(name_hash), None));
        
        // Channel group match
        let group_hash = LogoCacheEntry::hash_string("News");
        assert!(entry.matches_search(None, None, Some(group_hash)));
        
        // No match
        let wrong_hash = LogoCacheEntry::hash_string("wrong");
        assert!(!entry.matches_search(Some(wrong_hash), None, None));
    }
    
    #[test]
    fn test_memory_efficiency() {
        let entry = LogoCacheEntry::new(
            "http://example.com/very/long/url/path/to/logo.png",
            Some("Very Long Channel Name Here"),
            Some("Very Long Group Name Here"),
            Some(1920), Some(1080),
            524288, // 512KB
            "cached_logo.png".to_string(),
        );
        
        // Memory usage should be significantly less than storing full strings
        let memory_usage = entry.memory_usage();
        let string_storage = "http://example.com/very/long/url/path/to/logo.png".len() +
                           "Very Long Channel Name Here".len() +
                           "Very Long Group Name Here".len();
        
        println!("Hash-based entry: {} bytes", memory_usage);
        println!("String storage would be: {} bytes", string_storage + entry.relative_path.len());
        
        // The entry should use much less memory than string storage
        assert!(memory_usage < string_storage + 100); // Allow for struct overhead
    }
}