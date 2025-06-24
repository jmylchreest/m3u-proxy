//! Channel similarity analysis for EPG conflict resolution
//!
//! This module provides functionality to analyze channel name similarity
//! and determine if channels are likely clones or variants of the same channel.

#![allow(dead_code)]

use crate::config::ChannelSimilarityConfig;
use regex::Regex;

/// Configuration for channel similarity analysis
#[derive(Debug, Clone)]
pub struct SimilarityConfig {
    /// Patterns to remove when comparing channels (cloned channel patterns)
    pub clone_patterns: Vec<String>,
    /// Patterns that indicate timeshift channels (as strings)
    pub timeshift_patterns: Vec<String>,
    /// Minimum confidence threshold for considering channels as clones (0.0-1.0)
    /// Channels above this threshold should share the same tvg-id/channel id
    pub clone_confidence_threshold: f64,
}

/// Timeshift pattern configuration
#[derive(Debug, Clone)]
pub struct TimeshiftPattern {
    /// Regex pattern to match (e.g., r"\+(\d+)" for +1, +24, etc.)
    pub pattern: String,
    /// Compiled regex for performance
    pub regex: Regex,
}

/// Result of channel similarity analysis
#[derive(Debug, Clone)]
pub struct SimilarityResult {
    /// Confidence score (0.0-1.0)
    pub confidence: f64,
    /// Type of similarity detected
    pub similarity_type: SimilarityType,
    /// Normalized channel names used for comparison
    pub normalized_existing: String,
    pub normalized_new: String,
    /// Detected timeshift (if any)
    pub timeshift_hours: Option<i32>,
    /// Differences found between normalized names
    pub differences: Vec<String>,
}

/// Types of similarity between channels
#[derive(Debug, Clone, PartialEq)]
pub enum SimilarityType {
    /// Exact match after normalization (100% confidence) - should share tvg-id
    ExactClone,
    /// High similarity above clone threshold - should share tvg-id
    Clone,
    /// Below clone threshold - channels should remain separate
    Different,
}

/// Channel similarity analyzer
pub struct ChannelSimilarityAnalyzer {
    config: SimilarityConfig,
    clone_regex: Vec<Regex>,
    timeshift_patterns: Vec<TimeshiftPattern>,
}

impl Default for SimilarityConfig {
    fn default() -> Self {
        Self {
            clone_patterns: Self::default_clone_patterns(),
            timeshift_patterns: Self::default_timeshift_patterns(),
            clone_confidence_threshold: 0.90,
        }
    }
}

impl SimilarityConfig {
    /// Get default clone patterns - these can be overridden via configuration
    pub fn default_clone_patterns() -> Vec<String> {
        vec![
            r"(?i)\b4K\b".to_string(),
            r"(?i)\bHD\b".to_string(),
            r"(?i)\bSD\b".to_string(),
            r"(?i)\bHEVC\b".to_string(),
            r"(?i)\b720P?\b".to_string(),
            r"(?i)\b1080P?\b".to_string(),
            r"(?i)\bUHD\b".to_string(),
            r"\[|\]|\(|\)".to_string(), // Remove brackets and parentheses
            r"(?i)\(SAT\)".to_string(),
            r"(?i)\(CABLE\)".to_string(),
            r"(?i)\(IPTV\)".to_string(),
        ]
    }

    /// Get default timeshift patterns - these can be overridden via configuration
    pub fn default_timeshift_patterns() -> Vec<String> {
        vec![r"(?i)\+(\d+)".to_string(), r"(?i)\+(\d+)H".to_string()]
    }

    /// Get default clone confidence threshold - this can be overridden via configuration
    pub fn default_clone_confidence_threshold() -> f64 {
        0.90
    }

    /// Create from application config
    pub fn from_config(config: &ChannelSimilarityConfig) -> Self {
        Self {
            clone_patterns: config.clone_patterns.clone(),
            timeshift_patterns: config.timeshift_patterns.clone(),
            clone_confidence_threshold: config.clone_confidence_threshold,
        }
    }
}

impl ChannelSimilarityAnalyzer {
    /// Create a new analyzer with the given configuration
    pub fn new(config: SimilarityConfig) -> Result<Self, Box<dyn std::error::Error>> {
        let mut clone_regex = Vec::new();
        for pattern in &config.clone_patterns {
            clone_regex.push(Regex::new(pattern)?);
        }

        let mut timeshift_patterns = Vec::new();
        for pattern_str in &config.timeshift_patterns {
            let regex = Regex::new(pattern_str)?;
            timeshift_patterns.push(TimeshiftPattern {
                pattern: pattern_str.clone(),
                regex,
            });
        }

        Ok(Self {
            config,
            clone_regex,
            timeshift_patterns,
        })
    }

    /// Create with default configuration
    pub fn with_default_config() -> Result<Self, Box<dyn std::error::Error>> {
        Self::new(SimilarityConfig::default())
    }

    /// Create from application config
    pub fn from_app_config(
        config: &crate::config::Config,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let similarity_config = if let Some(ref ch_config) = config.channel_similarity {
            SimilarityConfig::from_config(ch_config)
        } else {
            SimilarityConfig::default()
        };
        Self::new(similarity_config)
    }

    /// Analyze similarity between two channel names
    pub fn analyze_similarity(&self, existing_name: &str, new_name: &str) -> SimilarityResult {
        // Step 1: Extract timeshift information
        let (existing_normalized, existing_timeshift) = self.extract_timeshift_info(existing_name);
        let (new_normalized, new_timeshift) = self.extract_timeshift_info(new_name);

        // Step 2: Normalize names by removing clone patterns
        let existing_cleaned = self.normalize_channel_name(&existing_normalized);
        let new_cleaned = self.normalize_channel_name(&new_normalized);

        // Step 3: Calculate similarity
        let confidence = self.calculate_similarity_confidence(&existing_cleaned, &new_cleaned);

        // Step 4: Determine timeshift
        let timeshift_hours = match (existing_timeshift, new_timeshift) {
            (Some(e), Some(n)) => Some(n - e),
            (None, Some(n)) => Some(n),
            (Some(e), None) => Some(-e),
            (None, None) => None,
        };

        // Step 5: Find differences
        let differences = self.find_differences(&existing_cleaned, &new_cleaned);

        // Step 6: Determine similarity type
        let similarity_type = self.determine_similarity_type(confidence);

        SimilarityResult {
            confidence,
            similarity_type,
            normalized_existing: existing_cleaned,
            normalized_new: new_cleaned,
            timeshift_hours,
            differences,
        }
    }

    /// Extract timeshift information from channel name
    fn extract_timeshift_info(&self, name: &str) -> (String, Option<i32>) {
        for timeshift_pattern in &self.timeshift_patterns {
            if let Some(captures) = timeshift_pattern.regex.captures(name) {
                if let Some(hours_match) = captures.get(1) {
                    if let Ok(hours) = hours_match.as_str().parse::<i32>() {
                        let cleaned_name = timeshift_pattern.regex.replace(name, "").to_string();
                        return (cleaned_name.trim().to_string(), Some(hours));
                    }
                }
            }
        }
        (name.to_string(), None)
    }

    /// Normalize channel name by removing clone patterns
    fn normalize_channel_name(&self, name: &str) -> String {
        let mut normalized = name.to_string();

        // Apply clone pattern removals
        for regex in &self.clone_regex {
            normalized = regex.replace_all(&normalized, "").to_string();
        }

        // Clean up whitespace and normalize
        normalized = normalized
            .split_whitespace()
            .collect::<Vec<&str>>()
            .join(" ")
            .to_uppercase()
            .trim()
            .to_string();

        normalized
    }

    /// Calculate similarity confidence using multiple algorithms
    fn calculate_similarity_confidence(&self, name1: &str, name2: &str) -> f64 {
        if name1.is_empty() || name2.is_empty() {
            return 0.0;
        }

        if name1 == name2 {
            return 1.0;
        }

        // Use multiple similarity metrics and average them
        let jaro_winkler = self.jaro_winkler_similarity(name1, name2);
        let levenshtein = self.levenshtein_similarity(name1, name2);
        let word_overlap = self.word_overlap_similarity(name1, name2);

        // Weighted average - prioritize word overlap for channel names
        (jaro_winkler * 0.3 + levenshtein * 0.3 + word_overlap * 0.4).min(1.0)
    }

    /// Jaro-Winkler similarity (simplified implementation)
    fn jaro_winkler_similarity(&self, s1: &str, s2: &str) -> f64 {
        let s1_chars: Vec<char> = s1.chars().collect();
        let s2_chars: Vec<char> = s2.chars().collect();

        if s1_chars.is_empty() && s2_chars.is_empty() {
            return 1.0;
        }

        if s1_chars.is_empty() || s2_chars.is_empty() {
            return 0.0;
        }

        let match_window = (s1_chars.len().max(s2_chars.len()) / 2).saturating_sub(1);
        let mut s1_matches = vec![false; s1_chars.len()];
        let mut s2_matches = vec![false; s2_chars.len()];
        let mut matches = 0;

        // Find matches
        for i in 0..s1_chars.len() {
            let start = i.saturating_sub(match_window);
            let end = (i + match_window + 1).min(s2_chars.len());

            for j in start..end {
                if s2_matches[j] || s1_chars[i] != s2_chars[j] {
                    continue;
                }
                s1_matches[i] = true;
                s2_matches[j] = true;
                matches += 1;
                break;
            }
        }

        if matches == 0 {
            return 0.0;
        }

        // Calculate transpositions
        let mut transpositions = 0;
        let mut k = 0;
        for i in 0..s1_chars.len() {
            if !s1_matches[i] {
                continue;
            }
            while !s2_matches[k] {
                k += 1;
            }
            if s1_chars[i] != s2_chars[k] {
                transpositions += 1;
            }
            k += 1;
        }

        let jaro = (matches as f64 / s1_chars.len() as f64
            + matches as f64 / s2_chars.len() as f64
            + (matches as f64 - transpositions as f64 / 2.0) / matches as f64)
            / 3.0;

        // Winkler prefix bonus
        let prefix_length = s1_chars
            .iter()
            .zip(s2_chars.iter())
            .take(4)
            .take_while(|(a, b)| a == b)
            .count() as f64;

        jaro + (0.1 * prefix_length * (1.0 - jaro))
    }

    /// Levenshtein similarity
    fn levenshtein_similarity(&self, s1: &str, s2: &str) -> f64 {
        let len1 = s1.chars().count();
        let len2 = s2.chars().count();

        if len1 == 0 && len2 == 0 {
            return 1.0;
        }

        let max_len = len1.max(len2);
        if max_len == 0 {
            return 1.0;
        }

        let distance = self.levenshtein_distance(s1, s2);
        1.0 - (distance as f64 / max_len as f64)
    }

    /// Levenshtein distance calculation
    fn levenshtein_distance(&self, s1: &str, s2: &str) -> usize {
        let s1_chars: Vec<char> = s1.chars().collect();
        let s2_chars: Vec<char> = s2.chars().collect();
        let len1 = s1_chars.len();
        let len2 = s2_chars.len();

        let mut matrix = vec![vec![0; len2 + 1]; len1 + 1];

        for i in 0..=len1 {
            matrix[i][0] = i;
        }
        for j in 0..=len2 {
            matrix[0][j] = j;
        }

        for i in 1..=len1 {
            for j in 1..=len2 {
                let cost = if s1_chars[i - 1] == s2_chars[j - 1] {
                    0
                } else {
                    1
                };
                matrix[i][j] = (matrix[i - 1][j] + 1)
                    .min(matrix[i][j - 1] + 1)
                    .min(matrix[i - 1][j - 1] + cost);
            }
        }

        matrix[len1][len2]
    }

    /// Word overlap similarity
    fn word_overlap_similarity(&self, s1: &str, s2: &str) -> f64 {
        let words1: std::collections::HashSet<&str> = s1.split_whitespace().collect();
        let words2: std::collections::HashSet<&str> = s2.split_whitespace().collect();

        if words1.is_empty() && words2.is_empty() {
            return 1.0;
        }

        let intersection = words1.intersection(&words2).count();
        let union = words1.union(&words2).count();

        if union == 0 {
            return 0.0;
        }

        intersection as f64 / union as f64
    }

    /// Find differences between normalized names
    fn find_differences(&self, name1: &str, name2: &str) -> Vec<String> {
        let words1: std::collections::HashSet<&str> = name1.split_whitespace().collect();
        let words2: std::collections::HashSet<&str> = name2.split_whitespace().collect();

        let mut differences = Vec::new();

        // Words in name2 but not in name1
        for word in words2.difference(&words1) {
            differences.push(word.to_string());
        }

        differences
    }

    /// Determine similarity type based on confidence
    fn determine_similarity_type(&self, confidence: f64) -> SimilarityType {
        if confidence >= 1.0 {
            SimilarityType::ExactClone
        } else if confidence >= self.config.clone_confidence_threshold {
            SimilarityType::Clone
        } else {
            SimilarityType::Different
        }
    }

    /// Check if channels should be considered clones based on configuration
    pub fn are_channels_clones(&self, existing_name: &str, new_name: &str) -> bool {
        let result = self.analyze_similarity(existing_name, new_name);
        matches!(
            result.similarity_type,
            SimilarityType::ExactClone | SimilarityType::Clone
        )
    }

    /// Get the confidence threshold for clones
    pub fn clone_confidence_threshold(&self) -> f64 {
        self.config.clone_confidence_threshold
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_exact_clone_detection() {
        let analyzer = ChannelSimilarityAnalyzer::with_default_config().unwrap();
        let result = analyzer.analyze_similarity("RU: TLC HD", "RU: TLC");

        assert_eq!(result.similarity_type, SimilarityType::ExactClone);
        assert_eq!(result.normalized_existing, "RU: TLC");
        assert_eq!(result.normalized_new, "RU: TLC");
        assert!(analyzer.are_channels_clones("RU: TLC HD", "RU: TLC"));
    }

    #[test]
    fn test_timeshift_detection() {
        let analyzer = ChannelSimilarityAnalyzer::with_default_config().unwrap();
        let result = analyzer.analyze_similarity(
            "DE: SKY CINEMA PREMIEREN +24 HEVC",
            "DE: SKY CINEMA PREMIEREN +24 HD (SAT)",
        );

        assert!(result.timeshift_hours.is_some());
        assert_eq!(result.timeshift_hours.unwrap(), 0); // Both are +24
    }

    #[test]
    fn test_different_channels() {
        let analyzer = ChannelSimilarityAnalyzer::with_default_config().unwrap();
        let result = analyzer.analyze_similarity(
            "CA EN: NATIONAL GEOGRAPHIC HD",
            "CA EN: NATIONAL GEOGRAPHIC WILD HD",
        );

        assert!(result.differences.contains(&"WILD".to_string()));
        assert!(result.confidence < 1.0);
    }
}
