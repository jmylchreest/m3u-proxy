//! Human-readable formatting utilities for memory and time values

use std::time::Duration;

/// Formats a memory value in bytes to a human-readable string with appropriate units
pub fn format_memory(bytes: f64) -> String {
    const UNITS: &[&str] = &["B", "KB", "MB", "GB", "TB"];
    const THRESHOLD: f64 = 1024.0;

    if bytes == 0.0 {
        return "0B".to_string();
    }

    let mut size = bytes.abs();
    let mut unit_index = 0;

    while size >= THRESHOLD && unit_index < UNITS.len() - 1 {
        size /= THRESHOLD;
        unit_index += 1;
    }

    let sign = if bytes < 0.0 { "-" } else { "" };

    // Choose precision based on unit and size

    if unit_index == 0 {
        // Bytes - no decimal places
        format!("{}{:.0}{}", sign, size, UNITS[unit_index])
    } else if size >= 100.0 {
        // Large values - 1 decimal place
        format!("{}{:.1}{}", sign, size, UNITS[unit_index])
    } else if size >= 10.0 {
        // Medium values - 1 decimal place
        format!("{}{:.1}{}", sign, size, UNITS[unit_index])
    } else {
        // Small values - 2 decimal places
        format!("{}{:.2}{}", sign, size, UNITS[unit_index])
    }
}

/// Flexible duration parser that can handle various duration string formats
pub struct DurationParser;

impl DurationParser {
    /// Parse a duration string with flexible formats
    /// Supports: "1.5s", "500ms", "200μs", "1m30s", "1h2m3s", "123" (assumes milliseconds), etc.
    pub fn parse_flexible(input: &str) -> Result<Duration, anyhow::Error> {
        let input = input.trim();

        if input.is_empty() {
            return Err(anyhow::anyhow!("Empty duration string"));
        }

        // Try parsing as a plain number (assume milliseconds)
        if let Ok(millis) = input.parse::<f64>() {
            if millis < 0.0 {
                return Err(anyhow::anyhow!("Negative duration not supported"));
            }
            return Ok(Duration::from_millis(millis as u64));
        }

        // Parse complex duration strings like "1h2m3s", "1.5s", "500ms", etc.
        let mut total_micros = 0u128;
        let mut current_number = String::new();
        let mut chars = input.chars().peekable();

        while let Some(ch) = chars.next() {
            if ch.is_ascii_digit() || ch == '.' {
                current_number.push(ch);
            } else if ch.is_alphabetic() || ch == 'μ' {
                // Found a unit, parse the number and unit
                if current_number.is_empty() {
                    return Err(anyhow::anyhow!("Missing number before unit '{}'", ch));
                }

                let number: f64 = current_number
                    .parse()
                    .map_err(|_| anyhow::anyhow!("Invalid number '{}'", current_number))?;

                if number < 0.0 {
                    return Err(anyhow::anyhow!("Negative duration not supported"));
                }

                // Parse the unit
                let mut unit = String::new();
                unit.push(ch);

                // Collect the rest of the unit
                while let Some(&next_ch) = chars.peek() {
                    if next_ch.is_alphabetic() || next_ch == 's' {
                        unit.push(chars.next().unwrap());
                    } else {
                        break;
                    }
                }

                // Convert to microseconds based on unit
                let unit_micros = match unit.as_str() {
                    "μs" | "us" | "micros" | "microseconds" => number as u128,
                    "ms" | "millis" | "milliseconds" => (number * 1_000.0) as u128,
                    "s" | "sec" | "secs" | "second" | "seconds" => (number * 1_000_000.0) as u128,
                    "m" | "min" | "mins" | "minute" | "minutes" => {
                        (number * 60.0 * 1_000_000.0) as u128
                    }
                    "h" | "hr" | "hrs" | "hour" | "hours" => {
                        (number * 3600.0 * 1_000_000.0) as u128
                    }
                    "d" | "day" | "days" => (number * 24.0 * 3600.0 * 1_000_000.0) as u128,
                    _ => return Err(anyhow::anyhow!("Unknown time unit '{}'", unit)),
                };

                total_micros += unit_micros;
                current_number.clear();
            } else if ch.is_whitespace() {
                // Skip whitespace
                continue;
            } else {
                return Err(anyhow::anyhow!(
                    "Invalid character '{}' in duration string",
                    ch
                ));
            }
        }

        // Check if there's a trailing number without unit
        if !current_number.is_empty() {
            return Err(anyhow::anyhow!(
                "Number '{}' without unit at end of string",
                current_number
            ));
        }

        if total_micros == 0 {
            return Err(anyhow::anyhow!("Duration cannot be zero"));
        }

        Ok(Duration::from_micros(total_micros as u64))
    }
}

/// Formats a std::time::Duration to a human-readable string with microsecond precision for very small durations
pub fn format_duration_precise(duration: std::time::Duration) -> String {
    let micros = duration.as_micros();

    if micros == 0 {
        return "0μs".to_string();
    }

    if micros < 1000 {
        // Less than 1 millisecond - show microseconds
        format!("{micros}μs")
    } else if micros < 1_000_000 {
        // Less than 1 second - show milliseconds with microsecond precision if significant
        let millis = micros as f64 / 1000.0;
        if micros % 1000 == 0 {
            format!("{}ms", micros / 1000)
        } else {
            format!("{millis:.3}ms")
        }
    } else {
        // 1 second or more - use the existing logic but convert to millis first
        format_duration((micros / 1000) as u64)
    }
}

/// Formats a time duration in milliseconds to a human-readable string
pub fn format_duration(millis: u64) -> String {
    if millis == 0 {
        return "0ms".to_string();
    }

    if millis < 1000 {
        // Less than 1 second - show milliseconds
        format!("{millis}ms")
    } else if millis < 60_000 {
        // Less than 1 minute - show seconds with decimal
        let seconds = millis as f64 / 1000.0;
        if seconds >= 10.0 {
            format!("{seconds:.1}s")
        } else {
            format!("{seconds:.2}s")
        }
    } else if millis < 3_600_000 {
        // Less than 1 hour - show minutes and seconds
        let total_seconds = millis / 1000;
        let minutes = total_seconds / 60;
        let seconds = total_seconds % 60;

        if seconds == 0 {
            format!("{minutes}m")
        } else {
            format!("{minutes}m{seconds}s")
        }
    } else {
        // 1 hour or more - show hours, minutes, and optionally seconds
        let total_seconds = millis / 1000;
        let hours = total_seconds / 3600;
        let minutes = (total_seconds % 3600) / 60;
        let seconds = total_seconds % 60;

        if seconds == 0 && minutes == 0 {
            format!("{hours}h")
        } else if seconds == 0 {
            format!("{hours}h{minutes}m")
        } else {
            format!("{hours}h{minutes}m{seconds}s")
        }
    }
}

/// Formats a memory delta with appropriate sign and units
pub fn format_memory_delta(bytes: f64) -> String {
    if bytes == 0.0 {
        return "±0B".to_string();
    }

    let sign = if bytes > 0.0 { "+" } else { "" }; // negative sign is handled by format_memory
    format!("{}{}", sign, format_memory(bytes))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_memory() {
        assert_eq!(format_memory(0.0), "0B");
        assert_eq!(format_memory(512.0), "512B");
        assert_eq!(format_memory(1024.0), "1.00KB");
        assert_eq!(format_memory(1536.0), "1.50KB");
        assert_eq!(format_memory(1048576.0), "1.00MB");
        assert_eq!(format_memory(1572864.0), "1.50MB");
        assert_eq!(format_memory(1073741824.0), "1.00GB");
        assert_eq!(format_memory(104857600.0), "100.0MB");
        assert_eq!(format_memory(10485760.0), "10.0MB");
        assert_eq!(format_memory(1048576.0 * 1.234), "1.23MB");
    }

    #[test]
    fn test_format_memory_negative() {
        assert_eq!(format_memory(-1024.0), "-1.00KB");
        assert_eq!(format_memory(-1048576.0), "-1.00MB");
    }

    #[test]
    fn test_format_duration() {
        assert_eq!(format_duration(0), "0ms");
        assert_eq!(format_duration(500), "500ms");
        assert_eq!(format_duration(999), "999ms");
        assert_eq!(format_duration(1000), "1.00s");
        assert_eq!(format_duration(1500), "1.50s");
        assert_eq!(format_duration(10000), "10.0s");
        assert_eq!(format_duration(59000), "59.0s");
        assert_eq!(format_duration(60000), "1m");
        assert_eq!(format_duration(90000), "1m30s");
        assert_eq!(format_duration(3600000), "1h");
        assert_eq!(format_duration(3660000), "1h1m");
        assert_eq!(format_duration(3661000), "1h1m1s");
    }

    #[test]
    fn test_format_memory_delta() {
        assert_eq!(format_memory_delta(0.0), "±0B");
        assert_eq!(format_memory_delta(1024.0), "+1.00KB");
        assert_eq!(format_memory_delta(-1024.0), "-1.00KB");
        assert_eq!(format_memory_delta(1048576.0), "+1.00MB");
    }

    #[test]
    fn test_format_duration_precise() {
        use std::time::Duration;

        assert_eq!(format_duration_precise(Duration::from_micros(0)), "0μs");
        assert_eq!(format_duration_precise(Duration::from_micros(500)), "500μs");
        assert_eq!(format_duration_precise(Duration::from_micros(999)), "999μs");
        assert_eq!(format_duration_precise(Duration::from_micros(1000)), "1ms");
        assert_eq!(
            format_duration_precise(Duration::from_micros(1500)),
            "1.500ms"
        );
        assert_eq!(format_duration_precise(Duration::from_millis(1)), "1ms");
        assert_eq!(format_duration_precise(Duration::from_millis(999)), "999ms");
        assert_eq!(
            format_duration_precise(Duration::from_millis(1000)),
            "1.00s"
        );
        assert_eq!(
            format_duration_precise(Duration::from_millis(1500)),
            "1.50s"
        );
    }

    #[test]
    fn test_duration_parser_basic() {
        use std::time::Duration;

        // Basic units
        assert_eq!(
            DurationParser::parse_flexible("500μs").unwrap(),
            Duration::from_micros(500)
        );
        assert_eq!(
            DurationParser::parse_flexible("500us").unwrap(),
            Duration::from_micros(500)
        );
        assert_eq!(
            DurationParser::parse_flexible("100ms").unwrap(),
            Duration::from_millis(100)
        );
        assert_eq!(
            DurationParser::parse_flexible("1.5s").unwrap(),
            Duration::from_millis(1500)
        );
        assert_eq!(
            DurationParser::parse_flexible("2m").unwrap(),
            Duration::from_secs(120)
        );
        assert_eq!(
            DurationParser::parse_flexible("1h").unwrap(),
            Duration::from_secs(3600)
        );

        // Plain numbers (assumes milliseconds)
        assert_eq!(
            DurationParser::parse_flexible("1000").unwrap(),
            Duration::from_millis(1000)
        );
        assert_eq!(
            DurationParser::parse_flexible("1500.5").unwrap(),
            Duration::from_millis(1500)
        );
    }

    #[test]
    fn test_duration_parser_complex() {
        use std::time::Duration;

        // Complex formats
        assert_eq!(
            DurationParser::parse_flexible("1h30m").unwrap(),
            Duration::from_secs(5400)
        ); // 1.5 hours
        assert_eq!(
            DurationParser::parse_flexible("1m30s").unwrap(),
            Duration::from_secs(90)
        );
        assert_eq!(
            DurationParser::parse_flexible("1h2m3s").unwrap(),
            Duration::from_secs(3723)
        );
        assert_eq!(
            DurationParser::parse_flexible("500ms200μs").unwrap(),
            Duration::from_micros(500200)
        );

        // With whitespace
        assert_eq!(
            DurationParser::parse_flexible(" 1h 30m ").unwrap(),
            Duration::from_secs(5400)
        );
        assert_eq!(
            DurationParser::parse_flexible("1h 2m 3s").unwrap(),
            Duration::from_secs(3723)
        );
    }

    #[test]
    fn test_duration_parser_errors() {
        // Empty string
        assert!(DurationParser::parse_flexible("").is_err());
        assert!(DurationParser::parse_flexible("   ").is_err());

        // Invalid formats
        assert!(DurationParser::parse_flexible("1x").is_err()); // Unknown unit
        assert!(DurationParser::parse_flexible("abc").is_err()); // Invalid number
        assert!(DurationParser::parse_flexible("1.2.3s").is_err()); // Invalid number
        assert!(DurationParser::parse_flexible("s1").is_err()); // Missing number before unit
        assert!(DurationParser::parse_flexible("123abc").is_err()); // Number without unit at end

        // Negative durations
        assert!(DurationParser::parse_flexible("-1s").is_err());
        assert!(DurationParser::parse_flexible("-100").is_err());
    }
}
