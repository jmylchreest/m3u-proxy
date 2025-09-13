//! Simple jitter utility for retry mechanisms
//!
//! Provides lightweight jitter generation using system time instead of external random crates.

/// Generate jitter for retry mechanisms using system time as pseudo-randomness
///
/// # Arguments
/// * `max_jitter_ms` - Maximum jitter value in milliseconds
///
/// # Returns
/// A pseudo-random jitter value between 0 and `max_jitter_ms` (inclusive)
///
/// # Examples
/// ```
/// use m3u_proxy::utils::jitter::generate_jitter_ms;
///
/// let jitter = generate_jitter_ms(100); // 0-100ms jitter
/// assert!(jitter <= 100);
/// ```
pub fn generate_jitter_ms(max_jitter_ms: u64) -> u64 {
    if max_jitter_ms == 0 {
        return 0;
    }

    (std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos()
        % (max_jitter_ms + 1) as u128) as u64
}

/// Generate jitter as a percentage of a base value
///
/// # Arguments
/// * `base_value` - The base value to calculate jitter from
/// * `jitter_percent` - Jitter percentage (e.g., 25 for 25%)
///
/// # Returns
/// A pseudo-random jitter value between 0 and `base_value * (jitter_percent/100)` (inclusive)
///
/// # Examples
/// ```
/// use m3u_proxy::utils::jitter::generate_jitter_percent;
///
/// let jitter = generate_jitter_percent(1000, 25); // 0-250ms jitter (25% of 1000)
/// assert!(jitter <= 250);
/// ```
pub fn generate_jitter_percent(base_value: u64, jitter_percent: u8) -> u64 {
    if jitter_percent == 0 || base_value == 0 {
        return 0;
    }

    let max_jitter = (base_value * jitter_percent as u64) / 100;
    generate_jitter_ms(max_jitter)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_jitter_ms() {
        // Test with zero returns zero
        assert_eq!(generate_jitter_ms(0), 0);

        // Test that jitter is within bounds
        for _ in 0..100 {
            let jitter = generate_jitter_ms(50);
            assert!(jitter <= 50);
        }
    }

    #[test]
    fn test_generate_jitter_percent() {
        // Test with zero percent returns zero
        assert_eq!(generate_jitter_percent(1000, 0), 0);

        // Test with zero base value returns zero
        assert_eq!(generate_jitter_percent(0, 25), 0);

        // Test 25% jitter
        for _ in 0..100 {
            let jitter = generate_jitter_percent(1000, 25);
            assert!(jitter <= 250); // 25% of 1000
        }
    }
}
