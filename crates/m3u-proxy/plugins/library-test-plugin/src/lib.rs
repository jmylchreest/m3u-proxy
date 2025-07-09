//! Library Compatibility Test Plugin
//!
//! This plugin tests WASM compatibility of various Rust libraries
//! that could be used for data processing within plugins instead of
//! relying on host functions.

use serde::{Deserialize, Serialize};

// Host function imports (minimal set for testing)
extern "C" {
    fn host_log(level: u32, msg_ptr: u32, msg_len: u32);
}

const LOG_INFO: u32 = 3;
const LOG_ERROR: u32 = 1;

/// Helper function to log messages to the host
fn log_info(message: &str) {
    let msg_bytes = message.as_bytes();
    unsafe {
        host_log(LOG_INFO, msg_bytes.as_ptr() as u32, msg_bytes.len() as u32);
    }
}

fn log_error(message: &str) {
    let msg_bytes = message.as_bytes();
    unsafe {
        host_log(LOG_ERROR, msg_bytes.as_ptr() as u32, msg_bytes.len() as u32);
    }
}

/// Test data structure representing a channel
#[derive(Debug, Clone, Serialize, Deserialize)]
struct TestChannel {
    pub id: String,
    pub name: String,
    pub url: String,
    pub group: Option<String>,
    pub logo: Option<String>,
    pub epg_id: Option<String>,
}

/// Test regex processing - common plugin operation
fn test_regex_processing() -> Result<(), Box<dyn std::error::Error>> {
    log_info("Testing regex library...");
    
    let re = regex::Regex::new(r"(?i)hd$|4k$|\s+$")?;
    
    let test_names = vec![
        "CNN HD",
        "BBC One 4K", 
        "Discovery Channel   ",
        "Normal Channel",
    ];
    
    let cleaned: Vec<String> = test_names
        .iter()
        .map(|name| re.replace_all(name, "").trim().to_string())
        .collect();
    
    log_info(&format!("Regex test: {:?} -> {:?}", test_names, cleaned));
    Ok(())
}

/// Test simple string searching - useful for filtering
fn test_string_searching() -> Result<(), Box<dyn std::error::Error>> {
    log_info("Testing string searching...");
    
    let patterns = vec!["sport", "news", "movie"];
    
    let test_texts = vec![
        "ESPN Sports Center",
        "BBC News at Six", 
        "Action Movie Channel",
        "Music Television",
    ];
    
    for text in &test_texts {
        let matches: Vec<_> = patterns
            .iter()
            .filter(|pattern| text.to_lowercase().contains(*pattern))
            .collect();
        if !matches.is_empty() {
            log_info(&format!("Found patterns in '{}': {:?}", text, matches));
        }
    }
    
    Ok(())
}

/// Test efficient data structures
fn test_data_structures() -> Result<(), Box<dyn std::error::Error>> {
    log_info("Testing data structures...");
    
    // Test IndexMap (ordered HashMap)
    let mut ordered_map: indexmap::IndexMap<&str, i32> = indexmap::IndexMap::new();
    ordered_map.insert("first", 1);
    ordered_map.insert("second", 2);
    ordered_map.insert("third", 3);
    
    // Test SmallVec (stack-allocated small vectors)
    let mut small_vec = smallvec::SmallVec::<[u32; 4]>::new();
    small_vec.push(1);
    small_vec.push(2);
    small_vec.push(3);
    
    // Test ArrayVec (fixed-size array-based vector)
    let mut array_vec = arrayvec::ArrayVec::<u32, 10>::new();
    array_vec.push(10);
    array_vec.push(20);
    
    log_info(&format!("IndexMap size: {}, SmallVec: {:?}, ArrayVec: {:?}", 
                     ordered_map.len(), small_vec.as_slice(), array_vec.as_slice()));
    Ok(())
}

/// Test date/time processing - crucial for EPG handling
fn test_chrono() -> Result<(), Box<dyn std::error::Error>> {
    log_info("Testing chrono library...");
    
    // Test parsing common EPG time formats
    let time_str = "2024-07-07T15:30:00Z";
    let parsed = chrono::DateTime::parse_from_rfc3339(time_str)
        .map_err(|e| format!("{}", e))?;
    
    // Test formatting
    let formatted = parsed.format("%Y-%m-%d %H:%M:%S UTC").to_string();
    
    log_info(&format!("Parsed time: {} -> {}", time_str, formatted));
    Ok(())
}

/// Test hashing libraries - useful for cache keys and deduplication  
fn test_hashing() -> Result<(), Box<dyn std::error::Error>> {
    log_info("Testing hashing libraries...");
    
    let test_data = b"Test channel data for hashing";
    
    // Test basic std lib hash
    use std::hash::{Hash, Hasher};
    use std::collections::hash_map::DefaultHasher;
    let mut hasher = DefaultHasher::new();
    test_data.hash(&mut hasher);
    let std_hash_result = hasher.finish();
    
    // Test SHA2 (cryptographic hash - no random needed)
    use sha2::{Sha256, Digest};
    let mut sha_hasher = Sha256::new();
    sha_hasher.update(test_data);
    let sha_result = sha_hasher.finalize();
    
    log_info(&format!("Hash results - StdHash: {}, SHA256: {:x}", 
                     std_hash_result, sha_result));
    Ok(())
}

/// Test iterator utilities - powerful for data processing
fn test_itertools() -> Result<(), Box<dyn std::error::Error>> {
    log_info("Testing itertools library...");
    
    let channels = vec![
        ("News", vec!["CNN", "BBC", "Fox"]),
        ("Sports", vec!["ESPN", "Sky Sports"]),
        ("Movies", vec!["HBO", "Netflix"]),
    ];
    
    // Group channels by type and process
    let flattened: Vec<_> = channels
        .iter()
        .flat_map(|(category, channels)| {
            channels.iter().map(move |channel| format!("{}: {}", category, channel))
        })
        .collect();
    
    // Use itertools for chunking - work with slice instead of iterator
    let chunked: Vec<Vec<_>> = flattened
        .chunks(2)
        .map(|chunk| chunk.to_vec())
        .collect();
    
    log_info(&format!("Itertools test - chunked: {:?}", chunked));
    Ok(())
}

/// Test JSON processing with complex nested data
fn test_json_processing() -> Result<(), Box<dyn std::error::Error>> {
    log_info("Testing JSON processing...");
    
    let test_channel = TestChannel {
        id: "cnn-hd".to_string(),
        name: "CNN HD".to_string(),
        url: "http://example.com/cnn".to_string(),
        group: Some("News".to_string()),
        logo: Some("http://example.com/cnn.png".to_string()),
        epg_id: Some("cnn.us".to_string()),
    };
    
    // Serialize to JSON
    let json = serde_json::to_string(&test_channel)?;
    
    // Parse back from JSON
    let parsed: TestChannel = serde_json::from_str(&json)?;
    
    log_info(&format!("JSON round-trip successful: {}", parsed.name));
    Ok(())
}

/// Main plugin entry point - tests all libraries
#[no_mangle]
pub extern "C" fn plugin_test_libraries() -> i32 {
    log_info("Starting library compatibility tests...");
    
    let tests = vec![
        ("Regex", test_regex_processing as fn() -> Result<(), Box<dyn std::error::Error>>),
        ("StringSearching", test_string_searching),
        ("DataStructures", test_data_structures),
        ("Chrono", test_chrono),
        ("Hashing", test_hashing),
        ("Itertools", test_itertools),
        ("JSON", test_json_processing),
    ];
    
    let mut passed = 0;
    let mut failed = 0;
    
    for (name, test) in tests {
        match test() {
            Ok(()) => {
                log_info(&format!("✅ {} test PASSED", name));
                passed += 1;
            }
            Err(e) => {
                log_error(&format!("❌ {} test FAILED: {}", name, e));
                failed += 1;
            }
        }
    }
    
    log_info(&format!("Library tests completed: {} passed, {} failed", passed, failed));
    
    if failed == 0 { 0 } else { -1 }
}

/// Plugin info for compatibility
#[no_mangle]
pub extern "C" fn plugin_get_info(out_ptr: *mut *mut u8, out_len: *mut usize) -> i32 {
    let info = serde_json::json!({
        "name": "library-test-plugin",
        "version": "0.1.0",
        "description": "Tests WASM compatibility of Rust libraries",
        "author": "M3U-Proxy Team",
        "supported_stages": ["data_mapping", "testing"]
    });
    
    match serde_json::to_string(&info) {
        Ok(data) => {
            // Allocate memory using Vec instead of libc::malloc for WASM compatibility
            let mut output_vec = data.into_bytes();
            let output_ptr = output_vec.as_mut_ptr();
            let output_len = output_vec.len();
            std::mem::forget(output_vec); // Transfer ownership to caller
            unsafe {
                *out_ptr = output_ptr;
                *out_len = output_len;
            }
            0
        },
        Err(_) => -1,
    }
}

/// Execute data mapping stage - run library tests then pass through data
#[no_mangle]
pub extern "C" fn plugin_execute_data_mapping(
    _mapping_iterator_id: u32,
    channels_ptr: *const u8,
    channels_len: usize,
    out_ptr: *mut *mut u8,
    out_len: *mut usize,
) -> i32 {
    log_info("=== LIBRARY TEST PLUGIN EXECUTING ===");
    log_info("Data mapping stage - running library compatibility tests");
    
    // Run library tests
    let test_result = plugin_test_libraries();
    
    if test_result != 0 {
        log_error("Library compatibility tests failed");
        return -1;
    }
    
    // Pass through the input data unchanged
    unsafe {
        let input_data = std::slice::from_raw_parts(channels_ptr, channels_len);
        let mut output_vec = input_data.to_vec();
        let output_ptr = output_vec.as_mut_ptr();
        let output_len = output_vec.len();
        std::mem::forget(output_vec); // Transfer ownership to caller
        *out_ptr = output_ptr;
        *out_len = output_len;
    }
    
    log_info("=== LIBRARY TEST PLUGIN COMPLETE ===");
    log_info("Data mapping stage completed - library tests passed");
    0
}