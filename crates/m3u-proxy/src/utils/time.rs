//! Time utilities for timezone detection and offset parsing

use chrono::{DateTime, FixedOffset, TimeZone, Utc};
use chrono_tz::Tz;
use regex::Regex;

use tracing::{debug, info, warn};

/// Parse a time offset string like "+1h30m", "-45m", "+5s", "0"
pub fn parse_time_offset(offset_str: &str) -> Result<i32, String> {
    let offset_str = offset_str.trim();

    // Handle the simple "0" case or empty string
    if offset_str == "0" || offset_str.is_empty() {
        return Ok(0);
    }

    // Regex to match patterns like +1h30m, -45m, +5s
    let re = Regex::new(r"^([+-]?)(?:(\d+)h)?(?:(\d+)m)?(?:(\d+)s)?$")
        .map_err(|e| format!("Invalid regex: {}", e))?;

    let caps = re.captures(offset_str).ok_or_else(|| {
        format!(
            "Invalid time offset format: '{}'. Expected format like '+1h30m', '-45m', '+5s', or '0'",
            offset_str
        )
    })?;

    let sign = match caps.get(1).map(|m| m.as_str()) {
        Some("-") => -1,
        _ => 1, // Default to positive, handles both "+" and empty
    };

    let hours: i32 = caps
        .get(2)
        .and_then(|m| m.as_str().parse().ok())
        .unwrap_or(0);

    let minutes: i32 = caps
        .get(3)
        .and_then(|m| m.as_str().parse().ok())
        .unwrap_or(0);

    let seconds: i32 = caps
        .get(4)
        .and_then(|m| m.as_str().parse().ok())
        .unwrap_or(0);

    // Convert everything to seconds
    let total_seconds = (hours * 3600) + (minutes * 60) + seconds;

    // Apply reasonable limits to prevent extreme values
    if hours > 23 {
        return Err(format!(
            "Hour offset too large: {}h. Maximum allowed is 23h",
            hours
        ));
    }

    if minutes > 59 {
        return Err(format!(
            "Minute offset too large: {}m. Maximum allowed is 59m",
            minutes
        ));
    }

    if seconds > 59 {
        return Err(format!(
            "Second offset too large: {}s. Maximum allowed is 59s",
            seconds
        ));
    }

    // Limit total offset to ±24 hours (86400 seconds)
    if total_seconds > 86400 {
        return Err(format!(
            "Total time offset too large: {}s ({}). Maximum allowed is ±24 hours",
            total_seconds,
            format_duration(total_seconds)
        ));
    }

    // Allow zero offset when explicitly specified components result in zero
    Ok(sign * total_seconds)
}

/// Apply a time offset in seconds to a DateTime
pub fn apply_time_offset(dt: DateTime<Utc>, offset_seconds: i32) -> DateTime<Utc> {
    if offset_seconds == 0 {
        return dt;
    }

    if offset_seconds > 0 {
        dt + chrono::Duration::seconds(offset_seconds as i64)
    } else {
        dt - chrono::Duration::seconds((-offset_seconds) as i64)
    }
}

/// Detect timezone from XMLTV content
pub fn detect_timezone_from_xmltv(content: &str) -> Option<String> {
    // Look for timezone information in XMLTV format
    // XMLTV times are typically in format: 20230101120000 +0100
    let time_regex = Regex::new(r"\d{14}\s+([+-]\d{4})").ok()?;

    if let Some(caps) = time_regex.captures(content) {
        if let Some(tz_match) = caps.get(1) {
            let tz_str = tz_match.as_str();
            debug!("Detected timezone offset from XMLTV: {}", tz_str);

            // Convert +0100 format to timezone
            if let Ok(offset_hours) = tz_str[1..3].parse::<i32>() {
                let offset_mins = tz_str[3..5].parse::<i32>().unwrap_or(0);
                let total_offset = if tz_str.starts_with('+') {
                    offset_hours * 3600 + offset_mins * 60
                } else {
                    -(offset_hours * 3600 + offset_mins * 60)
                };

                // Try to find a named timezone that matches this offset
                return find_timezone_by_offset(total_offset);
            }
        }
    }

    // Look for explicit timezone declarations in XML
    let tz_decl_regex = Regex::new(r#"timezone\s*=\s*["']([^"']+)["']"#).ok()?;
    if let Some(caps) = tz_decl_regex.captures(content) {
        if let Some(tz_match) = caps.get(1) {
            let tz_name = tz_match.as_str();
            debug!("Found explicit timezone declaration in XMLTV: {}", tz_name);
            return Some(tz_name.to_string());
        }
    }

    None
}

/// Find a timezone name by UTC offset (in seconds)
fn find_timezone_by_offset(offset_seconds: i32) -> Option<String> {
    let offset_hours = offset_seconds / 3600;

    // Map common UTC offsets to timezone names
    match offset_hours {
        -12 => Some("Pacific/Baker_Island".to_string()),
        -11 => Some("Pacific/Pago_Pago".to_string()),
        -10 => Some("Pacific/Honolulu".to_string()),
        -9 => Some("America/Anchorage".to_string()),
        -8 => Some("America/Los_Angeles".to_string()),
        -7 => Some("America/Denver".to_string()),
        -6 => Some("America/Chicago".to_string()),
        -5 => Some("America/New_York".to_string()),
        -4 => Some("America/Caracas".to_string()),
        -3 => Some("America/Argentina/Buenos_Aires".to_string()),
        -2 => Some("Atlantic/South_Georgia".to_string()),
        -1 => Some("Atlantic/Azores".to_string()),
        0 => Some("UTC".to_string()),
        1 => Some("Europe/London".to_string()),
        2 => Some("Europe/Berlin".to_string()),
        3 => Some("Europe/Moscow".to_string()),
        4 => Some("Asia/Dubai".to_string()),
        5 => Some("Asia/Karachi".to_string()),
        6 => Some("Asia/Dhaka".to_string()),
        7 => Some("Asia/Bangkok".to_string()),
        8 => Some("Asia/Shanghai".to_string()),
        9 => Some("Asia/Tokyo".to_string()),
        10 => Some("Australia/Sydney".to_string()),
        11 => Some("Pacific/Norfolk".to_string()),
        12 => Some("Pacific/Auckland".to_string()),
        _ => {
            warn!("Unknown timezone offset: {} hours", offset_hours);
            None
        }
    }
}

/// Parse timezone string and validate it
pub fn validate_timezone(tz_str: &str) -> Result<String, String> {
    // First try to parse as a named timezone
    if let Ok(_tz) = tz_str.parse::<Tz>() {
        return Ok(tz_str.to_string());
    }

    // Try to parse as UTC offset format like "+01:00" or "+0100"
    if let Ok(_offset) = parse_fixed_offset(tz_str) {
        return Ok(tz_str.to_string());
    }

    Err(format!("Invalid timezone: '{}'. Use either a named timezone (e.g., 'Europe/London') or UTC offset (e.g., '+01:00')", tz_str))
}

/// Parse fixed offset timezone formats like "+01:00", "+0100", etc.
fn parse_fixed_offset(offset_str: &str) -> Result<FixedOffset, String> {
    let offset_str = offset_str.trim();

    // Handle formats like +01:00, -05:30, +0100, -0530
    let re = Regex::new(r"^([+-])(\d{2}):?(\d{2})$").map_err(|e| format!("Regex error: {}", e))?;

    let caps = re
        .captures(offset_str)
        .ok_or_else(|| format!("Invalid offset format: '{}'", offset_str))?;

    let sign = if caps.get(1).unwrap().as_str() == "+" {
        1
    } else {
        -1
    };
    let hours: i32 = caps
        .get(2)
        .unwrap()
        .as_str()
        .parse()
        .map_err(|_| "Invalid hours in offset")?;
    let minutes: i32 = caps
        .get(3)
        .unwrap()
        .as_str()
        .parse()
        .map_err(|_| "Invalid minutes in offset")?;

    if hours > 23 || minutes > 59 {
        return Err("Invalid time values in offset".to_string());
    }

    let total_seconds = sign * (hours * 3600 + minutes * 60);

    FixedOffset::east_opt(total_seconds).ok_or_else(|| "Invalid timezone offset".to_string())
}

/// Convert a datetime string with timezone to UTC
#[allow(dead_code)]
pub fn parse_datetime_with_timezone(
    dt_str: &str,
    tz_str: &str,
    offset_seconds: i32,
) -> Result<DateTime<Utc>, String> {
    // First try to parse as XMLTV format (YYYYMMDDHHMMSS)
    if let Ok(naive_dt) = chrono::NaiveDateTime::parse_from_str(dt_str, "%Y%m%d%H%M%S") {
        let dt_with_tz = if let Ok(tz) = tz_str.parse::<Tz>() {
            tz.from_local_datetime(&naive_dt)
                .single()
                .ok_or_else(|| format!("Ambiguous local time: {}", dt_str))?
                .with_timezone(&Utc)
        } else if let Ok(fixed_offset) = parse_fixed_offset(tz_str) {
            fixed_offset
                .from_local_datetime(&naive_dt)
                .single()
                .ok_or_else(|| format!("Invalid local time: {}", dt_str))?
                .with_timezone(&Utc)
        } else {
            return Err(format!("Invalid timezone: {}", tz_str));
        };

        return Ok(apply_time_offset(dt_with_tz, offset_seconds));
    }

    // Try flexible datetime parsing (includes ISO 8601/RFC3339 and SQLite formats)
    if let Ok(dt) = crate::utils::datetime::DateTimeParser::parse_flexible(dt_str) {
        return Ok(apply_time_offset(dt, offset_seconds));
    }

    Err(format!("Unable to parse datetime: {}", dt_str))
}

/// Format duration in seconds to human readable string
fn format_duration(seconds: i32) -> String {
    let hours = seconds / 3600;
    let minutes = (seconds % 3600) / 60;
    let secs = seconds % 60;

    let mut parts = Vec::new();
    if hours > 0 {
        parts.push(format!("{}h", hours));
    }
    if minutes > 0 {
        parts.push(format!("{}m", minutes));
    }
    if secs > 0 {
        parts.push(format!("{}s", secs));
    }

    if parts.is_empty() {
        "0s".to_string()
    } else {
        parts.join("")
    }
}

/// Convert UTC time to local display timezone
#[allow(dead_code)]
pub fn convert_to_local_timezone(
    utc_time: DateTime<Utc>,
    local_tz_str: &str,
) -> Result<DateTime<Utc>, String> {
    // Parse the local timezone
    if let Ok(local_tz) = local_tz_str.parse::<Tz>() {
        // Convert UTC to local timezone, then back to UTC with local time
        let local_time = utc_time.with_timezone(&local_tz);
        Ok(local_time.with_timezone(&Utc))
    } else {
        Err(format!("Invalid local timezone: {}", local_tz_str))
    }
}

/// Format datetime for display in local timezone
#[allow(dead_code)]
pub fn format_for_display(utc_time: DateTime<Utc>, local_tz_str: &str) -> String {
    if let Ok(local_tz) = local_tz_str.parse::<Tz>() {
        let local_time = utc_time.with_timezone(&local_tz);
        format!(
            "{} {}",
            local_time.format("%Y-%m-%d %H:%M:%S"),
            local_tz.name()
        )
    } else {
        // Fallback to UTC if timezone is invalid
        format!("{} UTC", utc_time.format("%Y-%m-%d %H:%M:%S"))
    }
}

/// Log timezone detection result
pub fn log_timezone_detection(source_name: &str, detected_tz: Option<&str>, final_tz: &str) {
    match detected_tz {
        Some(detected) if detected != final_tz => {
            info!(
                "EPG source '{}': Detected timezone '{}', but using configured timezone '{}'",
                source_name, detected, final_tz
            );
        }
        Some(detected) => {
            info!(
                "EPG source '{}': Auto-detected and using timezone '{}'",
                source_name, detected
            );
        }
        None => {
            info!(
                "EPG source '{}': No timezone detected, using configured timezone '{}'",
                source_name, final_tz
            );
        }
    }
}

/// Parse various time string formats to Unix epoch timestamp
/// Uses the flexible datetime parser for comprehensive format support
pub fn parse_time_string(time_str: &str) -> Result<i64, String> {
    // Try parsing as Unix timestamp first (fastest check)
    if let Ok(epoch) = time_str.parse::<i64>() {
        return Ok(epoch);
    }
    
    // Use flexible datetime parser which supports all common formats
    if let Ok(dt) = crate::utils::datetime::DateTimeParser::parse_flexible(time_str) {
        return Ok(dt.timestamp());
    }
    
    Err(format!("Unable to parse time string: '{}'. Supported formats include Unix timestamps, RFC3339/ISO8601, SQLite datetime, European/US formats, and XMLTV format", time_str))
}

/// Resolve @time: functions in expressions to their numeric epoch values
/// Supports @time:now(), @time:parse("datestring"), @time:<epoch>, and @time:now() +/- offset
pub fn resolve_time_functions(expression: &str) -> Result<String, String> {
    let mut resolved = expression.to_string();
    let now_epoch = Utc::now().timestamp();
    
    // Replace @time:now() with current epoch
    resolved = resolved.replace("@time:now()", &now_epoch.to_string());
    
    // Handle @time:parse("datestring") patterns
    let parse_regex = Regex::new(r#"@time:parse\("([^"]+)"\)"#)
        .map_err(|e| format!("Regex compilation error: {}", e))?;
    resolved = parse_regex.replace_all(&resolved, |caps: &regex::Captures| {
        let date_string = &caps[1];
        match parse_time_string(date_string) {
            Ok(epoch) => epoch.to_string(),
            Err(e) => {
                warn!("Failed to parse time string '{}': {}", date_string, e);
                "0".to_string() // Fallback to epoch 0 on parse error
            }
        }
    }).to_string();
    
    // Handle @time:<epoch> patterns (direct epoch timestamps)
    let epoch_regex = Regex::new(r"@time:(\d+)")
        .map_err(|e| format!("Regex compilation error: {}", e))?;
    resolved = epoch_regex.replace_all(&resolved, |caps: &regex::Captures| {
        let epoch_str = &caps[1];
        // Validate the epoch is a valid number
        match epoch_str.parse::<i64>() {
            Ok(epoch) => epoch.to_string(),
            Err(_) => {
                warn!("Invalid epoch timestamp in @time:{}", epoch_str);
                "0".to_string()
            }
        }
    }).to_string();
    
    // Handle @time:now()+offset and @time:now()-offset patterns
    let offset_regex = Regex::new(r"@time:now\(\)\s*([+-])\s*(\d+)")
        .map_err(|e| format!("Regex compilation error: {}", e))?;
    resolved = offset_regex.replace_all(&resolved, |caps: &regex::Captures| {
        let operator = &caps[1];
        let offset_str = &caps[2];
        match offset_str.parse::<i64>() {
            Ok(offset) => {
                let result_epoch = if operator == "+" {
                    now_epoch + offset
                } else {
                    now_epoch - offset
                };
                result_epoch.to_string()
            },
            Err(_) => {
                warn!("Invalid offset in @time:now(){}{}", operator, offset_str);
                now_epoch.to_string()
            }
        }
    }).to_string();
    
    Ok(resolved)
}

/// Validate time function syntax for use in expression validators
/// Returns an error message if the syntax is invalid, None if valid
pub fn validate_time_function_syntax(expression: &str) -> Option<String> {
    // Check for invalid @time function patterns
    let invalid_patterns = [
        (r#"@time:[^()\d\s\+\-"]+"#, "Invalid @time function syntax"),
        (r#"@time:parse\([^)]*[^"]\)"#, "@time:parse() must contain quoted string"),
        (r#"@time:parse\(\)"#, "@time:parse() requires a date string parameter"),
        (r#"@time:now\(\)\s*[+\-]\s*[^\d]"#, "@time:now() offset must be a number"),
        (r#"@time:\s"#, "@time function cannot have space after colon"),
    ];
    
    for (pattern, error_msg) in &invalid_patterns {
        if let Ok(regex) = Regex::new(pattern) {
            if regex.is_match(expression) {
                return Some(error_msg.to_string());
            }
        }
    }
    
    // Validate specific @time function formats
    let time_functions = [
        r#"@time:now\(\)"#,                           // @time:now()
        r#"@time:parse\("[^"]+"\)"#,              // @time:parse("datestring")
        r#"@time:\d+"#,                              // @time:1234567890
        r#"@time:now\(\)\s*[+\-]\s*\d+"#,           // @time:now() + 3600
    ];
    
    // Find all @time: patterns in the expression
    if let Ok(at_time_regex) = Regex::new(r#"@time:[^,\s)]+"#) {
        for at_time_match in at_time_regex.find_iter(expression) {
            let time_func = at_time_match.as_str();
            
            // Check if this matches any valid pattern
            let is_valid = time_functions.iter().any(|pattern| {
                if let Ok(regex) = Regex::new(&format!("^{}$", pattern)) {
                    regex.is_match(time_func)
                } else {
                    false
                }
            });
            
            if !is_valid {
                return Some(format!("Invalid time function syntax: {}", time_func));
            }
        }
    }
    
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_time_offset() {
        assert_eq!(parse_time_offset("0").unwrap(), 0);
        assert_eq!(parse_time_offset("").unwrap(), 0); // Empty string should default to 0
        assert_eq!(parse_time_offset("+1h30m").unwrap(), 5400); // 1.5 hours in seconds
        assert_eq!(parse_time_offset("-45m").unwrap(), -2700); // -45 minutes in seconds
        assert_eq!(parse_time_offset("+5s").unwrap(), 5);
        assert_eq!(parse_time_offset("2h").unwrap(), 7200); // 2 hours
        assert_eq!(parse_time_offset("30m").unwrap(), 1800); // 30 minutes
        assert_eq!(parse_time_offset("+0h0m0s").unwrap(), 0); // Explicit zero components

        assert!(parse_time_offset("invalid").is_err());
        assert!(parse_time_offset("25h").is_err()); // Hour too large
        assert!(parse_time_offset("70m").is_err()); // Minutes too large
        assert!(parse_time_offset("90s").is_err()); // Seconds too large
    }

    #[test]
    fn test_validate_timezone() {
        assert!(validate_timezone("UTC").is_ok());
        assert!(validate_timezone("Europe/London").is_ok());
        assert!(validate_timezone("+01:00").is_ok());
        assert!(validate_timezone("-05:30").is_ok());
        assert!(validate_timezone("+0100").is_ok());

        assert!(validate_timezone("Invalid/Timezone").is_err());
        assert!(validate_timezone("+25:00").is_err());
    }

    #[test]
    fn test_parse_fixed_offset() {
        assert!(parse_fixed_offset("+01:00").is_ok());
        assert!(parse_fixed_offset("-05:30").is_ok());
        assert!(parse_fixed_offset("+0100").is_ok());
        assert!(parse_fixed_offset("-0530").is_ok());

        assert!(parse_fixed_offset("+25:00").is_err());
        assert!(parse_fixed_offset("invalid").is_err());
    }

    #[test]
    fn test_detect_timezone_from_xmltv() {
        let xmltv_content = r#"
            <programme start="20230101120000 +0100" stop="20230101130000 +0100" channel="test">
                <title>Test Program</title>
            </programme>
        "#;

        let detected = detect_timezone_from_xmltv(xmltv_content);
        assert!(detected.is_some());
    }

    #[test]
    fn test_parse_time_string() {
        // Test various date formats
        assert!(parse_time_string("2024-01-01 12:00:00").is_ok());
        assert!(parse_time_string("2024-01-01T12:00:00").is_ok());
        assert!(parse_time_string("2024-01-01T12:00:00Z").is_ok());
        assert!(parse_time_string("2024-01-01").is_ok());
        assert!(parse_time_string("01/01/2024 12:00:00").is_ok());
        assert!(parse_time_string("01/01/2024").is_ok());
        assert!(parse_time_string("20240101120000").is_ok()); // XMLTV format
        assert!(parse_time_string("20240101").is_ok());       // XMLTV date only
        
        // Test Unix timestamp
        assert_eq!(parse_time_string("1704110400").unwrap(), 1704110400); // 2024-01-01 12:00:00 UTC
        
        // Test RFC3339
        assert!(parse_time_string("2024-01-01T12:00:00Z").is_ok());
        assert!(parse_time_string("2024-01-01T12:00:00+01:00").is_ok());
        
        // Test invalid formats
        assert!(parse_time_string("invalid-date").is_err());
        assert!(parse_time_string("").is_err());
    }

    #[test]
    fn test_resolve_time_functions() {
        // Test @time:now() replacement
        let result = resolve_time_functions("@time:now()").unwrap();
        assert!(result.parse::<i64>().is_ok());
        
        // Test @time:parse() replacement
        let result = resolve_time_functions(r#"@time:parse("2024-01-01 12:00:00")"#).unwrap();
        assert_eq!(result, "1704110400");
        
        // Test @time:<epoch> replacement
        let result = resolve_time_functions("@time:1704110400").unwrap();
        assert_eq!(result, "1704110400");
        
        // Test @time:now() with offset
        let now = Utc::now().timestamp();
        let result = resolve_time_functions("@time:now() + 3600").unwrap();
        let expected = now + 3600;
        assert_eq!(result, expected.to_string());
        
        let result = resolve_time_functions("@time:now() - 1800").unwrap();
        let expected = now - 1800;
        assert_eq!(result, expected.to_string());
        
        // Test complex expression
        let result = resolve_time_functions(r#"field > @time:parse("2024-01-01") AND field < @time:now()"#).unwrap();
        assert!(result.contains("1704067200")); // 2024-01-01 00:00:00 UTC
        assert!(result.contains(" AND field < "));
        assert!(result.parse::<i64>().is_err()); // Should not be a single number
    }

    #[test]
    fn test_validate_time_function_syntax() {
        // Valid functions
        assert!(validate_time_function_syntax("@time:now()").is_none());
        assert!(validate_time_function_syntax(r#"@time:parse("2024-01-01")"#).is_none());
        assert!(validate_time_function_syntax("@time:1704110400").is_none());
        assert!(validate_time_function_syntax("@time:now() + 3600").is_none());
        assert!(validate_time_function_syntax("@time:now() - 1800").is_none());
        
        // Invalid functions
        assert!(validate_time_function_syntax("@time:invalid").is_some());
        assert!(validate_time_function_syntax("@time:parse()").is_some());
        assert!(validate_time_function_syntax("@time:parse(unquoted)").is_some());
        assert!(validate_time_function_syntax("@time:now() + invalid").is_some());
        assert!(validate_time_function_syntax("@time: ").is_some());
        
        // Complex expressions
        assert!(validate_time_function_syntax(r#"field > @time:parse("2024-01-01") AND field < @time:now()"#).is_none());
        assert!(validate_time_function_syntax("@time:invalid AND @time:now()").is_some());
    }
}
