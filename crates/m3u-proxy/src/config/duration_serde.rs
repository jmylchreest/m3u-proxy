//! Common serde utilities for human-readable durations across configuration.

use serde::de::{self, Visitor};
use serde::{Deserializer, Serializer};
use std::{fmt, time::Duration};

/// Custom serde functions for Duration that support human-readable strings
pub mod duration {
    use super::*;

    pub fn serialize<S>(duration: &Duration, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        // Serialize as human-readable string
        let duration_str = humantime::format_duration(*duration).to_string();
        serializer.serialize_str(&duration_str)
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Duration, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct DurationVisitor;

        impl<'de> Visitor<'de> for DurationVisitor {
            type Value = Duration;

            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                formatter.write_str("a duration as seconds (number) or human-readable string (e.g., '3months', '5m', '1h30m')")
            }

            fn visit_u64<E>(self, seconds: u64) -> Result<Self::Value, E>
            where
                E: de::Error,
            {
                Ok(Duration::from_secs(seconds))
            }

            fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
            where
                E: de::Error,
            {
                humantime::parse_duration(value)
                    .map_err(|e| de::Error::custom(format!("Invalid duration '{value}': {e}")))
            }
        }

        deserializer.deserialize_any(DurationVisitor)
    }
}

/// Custom serde functions for Option<Duration> that support human-readable strings
pub mod option_duration {
    use super::*;

    pub fn serialize<S>(duration: &Option<Duration>, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match duration {
            Some(d) => {
                let duration_str = humantime::format_duration(*d).to_string();
                serializer.serialize_some(&duration_str)
            }
            None => serializer.serialize_none(),
        }
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Option<Duration>, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct OptionDurationVisitor;

        impl<'de> Visitor<'de> for OptionDurationVisitor {
            type Value = Option<Duration>;

            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                formatter
                    .write_str("null or a duration as seconds (number) or human-readable string")
            }

            fn visit_none<E>(self) -> Result<Self::Value, E>
            where
                E: de::Error,
            {
                Ok(None)
            }

            fn visit_some<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
            where
                D: Deserializer<'de>,
            {
                super::duration::deserialize(deserializer).map(Some)
            }
        }

        deserializer.deserialize_option(OptionDurationVisitor)
    }
}

/// Custom serde functions for i32 timeouts that support human-readable strings and convert to seconds
pub mod timeout_seconds {
    use super::*;

    pub fn serialize<S>(timeout_secs: &i32, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        if *timeout_secs <= 0 {
            serializer.serialize_i32(*timeout_secs)
        } else {
            let duration = Duration::from_secs(*timeout_secs as u64);
            let duration_str = humantime::format_duration(duration).to_string();
            serializer.serialize_str(&duration_str)
        }
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<i32, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct TimeoutVisitor;

        impl<'de> Visitor<'de> for TimeoutVisitor {
            type Value = i32;

            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                formatter.write_str(
                    "a timeout as seconds (number) or human-readable string (e.g., '30s', '5m')",
                )
            }

            fn visit_i32<E>(self, seconds: i32) -> Result<Self::Value, E>
            where
                E: de::Error,
            {
                Ok(seconds)
            }

            fn visit_u64<E>(self, seconds: u64) -> Result<Self::Value, E>
            where
                E: de::Error,
            {
                if seconds > i32::MAX as u64 {
                    Err(de::Error::custom(format!(
                        "Timeout {seconds} seconds is too large for i32"
                    )))
                } else {
                    Ok(seconds as i32)
                }
            }

            fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
            where
                E: de::Error,
            {
                let duration = humantime::parse_duration(value)
                    .map_err(|e| de::Error::custom(format!("Invalid duration '{value}': {e}")))?;

                let seconds = duration.as_secs();
                if seconds > i32::MAX as u64 {
                    Err(de::Error::custom(format!(
                        "Duration '{value}' is too large for i32 seconds"
                    )))
                } else {
                    Ok(seconds as i32)
                }
            }
        }

        deserializer.deserialize_any(TimeoutVisitor)
    }
}

/// Custom serde functions for Option<i32> timeouts that support human-readable strings
pub mod option_timeout_seconds {
    use super::*;

    pub fn serialize<S>(timeout_secs: &Option<i32>, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match timeout_secs {
            Some(secs) => {
                if *secs <= 0 {
                    serializer.serialize_some(secs)
                } else {
                    let duration = Duration::from_secs(*secs as u64);
                    let duration_str = humantime::format_duration(duration).to_string();
                    serializer.serialize_some(&duration_str)
                }
            }
            None => serializer.serialize_none(),
        }
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Option<i32>, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct OptionTimeoutVisitor;

        impl<'de> Visitor<'de> for OptionTimeoutVisitor {
            type Value = Option<i32>;

            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                formatter
                    .write_str("null or a timeout as seconds (number) or human-readable string")
            }

            fn visit_none<E>(self) -> Result<Self::Value, E>
            where
                E: de::Error,
            {
                Ok(None)
            }

            fn visit_some<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
            where
                D: Deserializer<'de>,
            {
                timeout_seconds::deserialize(deserializer).map(Some)
            }
        }

        deserializer.deserialize_option(OptionTimeoutVisitor)
    }
}
