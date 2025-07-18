//! Human-readable formatting utilities for memory and time values

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
    let formatted = if unit_index == 0 {
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
    };

    formatted
}

/// Formats a time duration in milliseconds to a human-readable string
pub fn format_duration(millis: u64) -> String {
    if millis == 0 {
        return "0ms".to_string();
    }

    if millis < 1000 {
        // Less than 1 second - show milliseconds
        format!("{}ms", millis)
    } else if millis < 60_000 {
        // Less than 1 minute - show seconds with decimal
        let seconds = millis as f64 / 1000.0;
        if seconds >= 10.0 {
            format!("{:.1}s", seconds)
        } else {
            format!("{:.2}s", seconds)
        }
    } else if millis < 3_600_000 {
        // Less than 1 hour - show minutes and seconds
        let total_seconds = millis / 1000;
        let minutes = total_seconds / 60;
        let seconds = total_seconds % 60;

        if seconds == 0 {
            format!("{}m", minutes)
        } else {
            format!("{}m{}s", minutes, seconds)
        }
    } else {
        // 1 hour or more - show hours, minutes, and optionally seconds
        let total_seconds = millis / 1000;
        let hours = total_seconds / 3600;
        let minutes = (total_seconds % 3600) / 60;
        let seconds = total_seconds % 60;

        if seconds == 0 && minutes == 0 {
            format!("{}h", hours)
        } else if seconds == 0 {
            format!("{}h{}m", hours, minutes)
        } else {
            format!("{}h{}m{}s", hours, minutes, seconds)
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
}
