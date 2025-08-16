use tracing::trace;

/// Information about a regex quantifier
#[derive(Debug, Clone)]
struct QuantifierInfo {
    min: usize,
    
    max: Option<usize>, // None means unbounded
}

/// Configuration for regex preprocessing
#[derive(Debug, Clone)]
pub struct RegexPreprocessorConfig {
    pub enable_first_pass_filtering: bool,
    pub precheck_special_chars: String,
    pub minimum_literal_length: usize,
    /// Maximum allowed quantifier limit to prevent ReDoS attacks
    pub max_quantifier_limit: usize,
}

impl Default for RegexPreprocessorConfig {
    fn default() -> Self {
        Self {
            enable_first_pass_filtering: true,
            precheck_special_chars: "+-@#$%&*=<>!~`€£{}[].".to_string(),
            minimum_literal_length: 2,
            // Security limit to prevent ReDoS attacks
            max_quantifier_limit: 100,
        }
    }
}

/// Shared regex preprocessing utility for performance optimization
#[derive(Clone)]
pub struct RegexPreprocessor {
    config: RegexPreprocessorConfig,
}

impl RegexPreprocessor {
    pub fn new(config: RegexPreprocessorConfig) -> Self {
        Self { config }
    }
    
    /// Validate a regex pattern for security vulnerabilities (ReDoS prevention)
    /// Returns an error if the pattern contains dangerous quantifiers or ReDoS patterns
    pub fn validate_regex_security(&self, pattern: &str) -> Result<(), String> {
        // Comprehensive ReDoS detection implementation
        
        // 1. Check for nested quantifiers (classic ReDoS pattern)
        self.detect_nested_quantifiers(pattern)?;
        
        // 2. Check for alternation with overlapping patterns
        self.detect_alternation_overlap(pattern)?;
        
        // 3. Check individual quantifier limits
        self.validate_quantifier_limits(pattern)?;
        
        // 4. Check for exponential backtracking patterns
        self.detect_exponential_backtracking(pattern)?;
        
        // 5. Calculate overall pattern complexity
        let complexity = self.calculate_pattern_complexity(pattern);
        if complexity > 50 { // Threshold for complex patterns
            return Err(format!(
                "Pattern complexity score {} exceeds safety threshold of 50. \
                 Consider simplifying the regex to prevent performance issues.", 
                complexity
            ));
        }
        
        Ok(())
    }
    
    /// Detect nested quantifiers like (a+)+ or (a*)* which cause exponential backtracking
    fn detect_nested_quantifiers(&self, pattern: &str) -> Result<(), String> {
        let mut chars = pattern.chars().peekable();
        let mut paren_depth = 0;
        let mut quantifier_levels: Vec<bool> = Vec::new(); // Track quantifiers at each nesting level
        
        while let Some(ch) = chars.next() {
            match ch {
                '(' => {
                    paren_depth += 1;
                    if quantifier_levels.len() < paren_depth {
                        quantifier_levels.push(false);
                    }
                }
                ')' => {
                    if paren_depth > 0 {
                        // Check if this group is followed by a quantifier
                        if let Some(&next_ch) = chars.peek() {
                            if matches!(next_ch, '*' | '+' | '?' | '{') {
                                // Group has quantifier - check if it contains quantifiers
                                if paren_depth > 0 && quantifier_levels.get(paren_depth - 1) == Some(&true) {
                                    return Err(
                                        "Nested quantifiers detected (e.g., (a+)+). \
                                         This pattern can cause catastrophic backtracking and ReDoS attacks.".to_string()
                                    );
                                }
                            }
                        }
                        paren_depth -= 1;
                        if quantifier_levels.len() > paren_depth {
                            quantifier_levels.truncate(paren_depth);
                        }
                    }
                }
                '*' | '+' | '?' => {
                    // Mark current nesting level as having quantifiers
                    if paren_depth > 0 && paren_depth <= quantifier_levels.len() {
                        quantifier_levels[paren_depth - 1] = true;
                    }
                }
                '{' => {
                    // Skip quantifier content
                    while let Some(ch) = chars.next() {
                        if ch == '}' { break; }
                    }
                    // Mark current nesting level as having quantifiers
                    if paren_depth > 0 && paren_depth <= quantifier_levels.len() {
                        quantifier_levels[paren_depth - 1] = true;
                    }
                }
                _ => {}
            }
        }
        
        Ok(())
    }
    
    /// Detect alternation with overlapping patterns like (a|a)* which cause backtracking
    fn detect_alternation_overlap(&self, pattern: &str) -> Result<(), String> {
        // Look for patterns like (x|x)* or (ab|a)* where alternatives overlap
        let alternation_pattern = regex::Regex::new(r"\([^)]*\|[^)]*\)[*+]").unwrap();
        
        if alternation_pattern.is_match(pattern) {
            // This is a simplified check - in practice, we'd need more sophisticated analysis
            tracing::warn!(
                "Alternation with quantifiers detected. \
                 Verify that alternatives don't have overlapping matches to prevent ReDoS."
            );
        }
        
        Ok(())
    }
    
    /// Validate individual quantifier limits (existing functionality)
    fn validate_quantifier_limits(&self, pattern: &str) -> Result<(), String> {
        let mut chars = pattern.chars().peekable();
        
        while let Some(ch) = chars.next() {
            match ch {
                '{' => {
                    let quantifier = self.parse_quantifier(&mut chars);
                    
                    // Check min limit
                    if quantifier.min > self.config.max_quantifier_limit {
                        return Err(format!(
                            "Quantifier min value {} exceeds security limit of {}", 
                            quantifier.min, self.config.max_quantifier_limit
                        ));
                    }
                    
                    // Check max limit if specified
                    if let Some(max) = quantifier.max {
                        if max > self.config.max_quantifier_limit {
                            return Err(format!(
                                "Quantifier max value {} exceeds security limit of {}", 
                                max, self.config.max_quantifier_limit
                            ));
                        }
                    }
                    
                    // Check for unbounded quantifiers
                    if quantifier.max.is_none() && quantifier.min == 0 {
                        tracing::warn!(
                            "Unbounded quantifier detected: consider adding upper bounds for performance"
                        );
                    }
                }
                _ => {}
            }
        }
        
        Ok(())
    }
    
    /// Detect patterns that can cause exponential backtracking
    fn detect_exponential_backtracking(&self, pattern: &str) -> Result<(), String> {
        // Check for common problematic patterns
        let dangerous_patterns = [
            r"\([^)]*[*+]\)[*+]",      // Nested quantifiers
            r"[*+][*+]",                // Adjacent quantifiers  
            r"\.[*+].*[*+]",            // Multiple .* or .+ patterns
            r"\([^)]*\)[*+].*\([^)]*\)[*+]", // Multiple quantified groups
        ];
        
        for dangerous_pattern in &dangerous_patterns {
            if let Ok(regex) = regex::Regex::new(dangerous_pattern) {
                if regex.is_match(pattern) {
                    return Err(format!(
                        "Potentially dangerous regex pattern detected. \
                         Pattern '{}' matches rule '{}' which can cause exponential backtracking.",
                        pattern, dangerous_pattern
                    ));
                }
            }
        }
        
        Ok(())
    }
    
    /// Calculate pattern complexity score for performance assessment
    fn calculate_pattern_complexity(&self, pattern: &str) -> u32 {
        let mut complexity = 0;
        let mut chars = pattern.chars().peekable();
        
        while let Some(ch) = chars.next() {
            match ch {
                '*' | '+' => complexity += 3,     // High impact quantifiers
                '?' => complexity += 1,           // Low impact quantifier
                '{' => {
                    // Count custom quantifiers
                    let quantifier = self.parse_quantifier(&mut chars);
                    complexity += if quantifier.max.is_none() { 5 } else { 2 };
                }
                '(' => complexity += 2,           // Grouping adds complexity
                '|' => complexity += 2,           // Alternation adds complexity
                '.' => complexity += 2,           // Wildcard matching
                '[' => {
                    complexity += 1;
                    // Skip character class content
                    while let Some(ch) = chars.next() {
                        if ch == ']' { break; }
                    }
                }
                '\\' => {
                    chars.next(); // Skip escaped character
                    complexity += 1;
                }
                _ => {}
            }
        }
        
        complexity
    }

    /// Determine if a regex should be executed on a field value based on preprocessing heuristics
    /// This provides significant performance improvement by avoiding expensive regex operations
    /// when the field value is unlikely to match the pattern
    pub fn should_run_regex(&self, field_value: &str, regex_pattern: &str, debug_context: &str) -> bool {
        // If first-pass filtering is disabled, always run the regex
        if !self.config.enable_first_pass_filtering {
            return true;
        }

        // Extract literal strings from the regex pattern for string-based precheck
        let literal_strings = self.extract_literal_strings_from_regex(regex_pattern);
        
        // Extract special characters that are explicitly matched in the regex
        let regex_special_chars = self.extract_special_chars_from_regex(regex_pattern);
        
        // Check if any of the regex's literal special characters are present in the field value
        let regex_special_chars_present = regex_special_chars.iter()
            .any(|&c| field_value.contains(c));
        
        // Only use regex-specific special chars, not general precheck chars
        // The regex can only match if the field contains the literal special characters it's looking for
        let special_chars_present = regex_special_chars_present;
        
        // Check if any significant literal strings (length >= minimum) are present in field value
        let literal_strings_present = if literal_strings.iter().any(|s| s.len() >= self.config.minimum_literal_length) {
            // At least one literal string meets minimum length, check if any are present in field
            literal_strings.iter()
                .filter(|s| s.len() >= self.config.minimum_literal_length)
                .any(|s| field_value.contains(s))
        } else {
            // No literal strings meet minimum length - default to executing regex (regex without literals is valid)
            true
        };

        let should_run = special_chars_present || literal_strings_present;
        
        if !should_run {
            trace!(
                "{} regex preprocessor skipped: field='{}' pattern='{}' (no special chars or required literals found)",
                debug_context, field_value, regex_pattern
            );
        }

        should_run
    }

    /// Extract literal strings from a regex pattern for preprocessing
    /// This extracts contiguous sequences of non-regex-special characters,
    /// but excludes any literals that are followed by optional quantifiers
    fn extract_literal_strings_from_regex(&self, pattern: &str) -> Vec<String> {
        let mut required_literal_strings = Vec::new();
        let mut current_literal = String::new();
        let mut chars = pattern.chars().peekable();
        
        while let Some(ch) = chars.next() {
            match ch {
                // Skip regex metacharacters and their content
                '\\' => { 
                    // Handle escaped characters - they can be part of literals
                    if let Some(escaped_char) = chars.next() {
                        // Check if this escaped char + current literal will be optional
                        let mut temp_literal = current_literal.clone();
                        temp_literal.push(escaped_char);
                        
                        if !self.is_followed_by_optional_quantifier(&mut chars.clone()) {
                            current_literal = temp_literal;
                        } else {
                            // This escaped char is optional, finish current literal if any
                            if !current_literal.is_empty() {
                                required_literal_strings.push(current_literal.clone());
                                current_literal.clear();
                            }
                        }
                    }
                },
                '[' => {
                    // Add current literal if we have one
                    if !current_literal.is_empty() {
                        required_literal_strings.push(current_literal.clone());
                        current_literal.clear();
                    }
                    // Skip character class [...] and check if it's optional
                    for c in chars.by_ref() {
                        if c == ']' { break; }
                    }
                    // Character classes are handled separately - don't add to literals
                },
                '(' => {
                    // Add current literal if we have one
                    if !current_literal.is_empty() {
                        required_literal_strings.push(current_literal.clone());
                        current_literal.clear();
                    }
                    // Skip to matching closing paren - groups are complex
                    let mut paren_depth = 1;
                    for c in chars.by_ref() {
                        match c {
                            '(' => paren_depth += 1,
                            ')' => {
                                paren_depth -= 1;
                                if paren_depth == 0 { break; }
                            },
                            _ => {}
                        }
                    }
                },
                ')' | '|' | '^' | '$' | '.' => {
                    // Add current literal if we have one
                    if !current_literal.is_empty() {
                        required_literal_strings.push(current_literal.clone());
                        current_literal.clear();
                    }
                },
                '*' | '+' | '?' => {
                    // These are quantifiers - if we have a current literal, it might be optional
                    // Remove the last character from current literal if it exists
                    if !current_literal.is_empty() {
                        current_literal.pop(); // Remove the character that's being quantified
                        if !current_literal.is_empty() {
                            required_literal_strings.push(current_literal.clone());
                        }
                        current_literal.clear();
                    }
                },
                '{' => {
                    // Quantifier like {0,3} or {2,} - need to parse it
                    let quantifier = self.parse_quantifier(&mut chars);
                    if quantifier.min == 0 && !current_literal.is_empty() {
                        // This quantifier makes the previous element optional
                        current_literal.pop(); // Remove the character that's being quantified
                        if !current_literal.is_empty() {
                            required_literal_strings.push(current_literal.clone());
                        }
                        current_literal.clear();
                    }
                },
                '}' => {
                    // End of quantifier - already handled in '{'
                },
                // Collect literal characters into strings
                c if c.is_alphanumeric() || " -_".contains(c) => {
                    current_literal.push(c);
                },
                _ => {
                    // Add current literal if we have one for other characters
                    if !current_literal.is_empty() {
                        required_literal_strings.push(current_literal.clone());
                        current_literal.clear();
                    }
                }
            }
        }
        
        // Don't forget the last literal if we have one
        if !current_literal.is_empty() {
            required_literal_strings.push(current_literal);
        }
        
        required_literal_strings
    }

    /// Check if the next characters indicate an optional quantifier (?, *, {0,n})
    fn is_followed_by_optional_quantifier(&self, chars: &mut std::iter::Peekable<std::str::Chars>) -> bool {
        match chars.peek() {
            Some('?') | Some('*') => true,
            Some('{') => {
                // Need to parse the quantifier to see if min is 0
                let mut chars_clone = chars.clone();
                chars_clone.next(); // consume '{'
                let quantifier = self.parse_quantifier(&mut chars_clone);
                quantifier.min == 0
            },
            _ => false
        }
    }

    /// Parse a quantifier like {0,3} or {2,} and return min/max values
    /// TODO: Make this public for security validation
    fn parse_quantifier(&self, chars: &mut std::iter::Peekable<std::str::Chars>) -> QuantifierInfo {
        let mut quantifier_str = String::new();
        
        // Collect everything until '}'
        for ch in chars.by_ref() {
            if ch == '}' { break; }
            quantifier_str.push(ch);
        }
        
        // Parse the quantifier string
        if quantifier_str.contains(',') {
            let parts: Vec<&str> = quantifier_str.split(',').collect();
            let min = parts[0].parse().unwrap_or(1);
            let max = if parts.len() > 1 && !parts[1].is_empty() {
                parts[1].parse().ok()
            } else {
                None // Unbounded
            };
            QuantifierInfo { min, max }
        } else {
            // Exact count like {3}
            let count = quantifier_str.parse().unwrap_or(1);
            QuantifierInfo { min: count, max: Some(count) }
        }
    }

    /// Extract special characters that are explicitly matched in the regex pattern
    /// This looks for literal special characters that the regex is specifically looking for,
    /// but excludes any that are followed by optional quantifiers
    fn extract_special_chars_from_regex(&self, pattern: &str) -> Vec<char> {
        let mut required_special_chars = Vec::new();
        let mut chars = pattern.chars().peekable();
        
        while let Some(ch) = chars.next() {
            match ch {
                '\\' => {
                    // Check escaped characters for special chars we care about
                    if let Some(escaped_char) = chars.next() {
                        if self.config.precheck_special_chars.contains(escaped_char) {
                            // Check if this escaped char is followed by optional quantifier
                            if !self.is_followed_by_optional_quantifier(&mut chars.clone()) {
                                required_special_chars.push(escaped_char);
                            }
                        }
                    }
                },
                '[' => {
                    // Character classes are more complex - collect chars but check if class is optional
                    let mut class_chars = Vec::new();
                    for c in chars.by_ref() {
                        if c == ']' { 
                            break; 
                        } else if self.config.precheck_special_chars.contains(c) {
                            class_chars.push(c);
                        }
                    }
                    
                    // Only add if the character class is not optional
                    if !self.is_followed_by_optional_quantifier(&mut chars.clone()) {
                        required_special_chars.extend(class_chars);
                    }
                },
                // Look for literal special characters (not escaped, not regex syntax)
                c if self.config.precheck_special_chars.contains(c) && 
                     !matches!(c, '^' | '$' | '.' | '*' | '+' | '?' | '(' | ')' | '[' | ']' | '{' | '}' | '|') => {
                    // This is a literal special character, check if it's optional
                    if !self.is_followed_by_optional_quantifier(&mut chars.clone()) {
                        required_special_chars.push(c);
                    }
                },
                _ => {} // Skip regex syntax characters and other characters
            }
        }
        
        // Remove duplicates
        required_special_chars.sort();
        required_special_chars.dedup();
        
        required_special_chars
    }
}

impl Default for RegexPreprocessor {
    fn default() -> Self {
        Self::new(RegexPreprocessorConfig::default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_should_run_regex_with_special_chars() {
        let preprocessor = RegexPreprocessor::default();
        
        // Test with + character where regex pattern requires + (should run regex)
        assert!(preprocessor.should_run_regex("UK: ITV 1 +1 ◉", ".*\\+.*", "test"));
        
        // Test with - character where regex pattern requires - (should run regex) 
        assert!(preprocessor.should_run_regex("Channel -2H", ".*\\-.*", "test"));
        
        // Test without special chars but with pattern that has no significant literals (should run)
        assert!(preprocessor.should_run_regex("BBC One HD", ".*", "test"));
        
        // Test case where we should skip: pattern with significant literals not present in field
        assert!(!preprocessor.should_run_regex("BBC One HD", "channel.*sport.*name", "test"));
        
        // Test case where we should run: pattern with significant literals present in field
        assert!(preprocessor.should_run_regex("This is a sports channel name", "channel.*sport.*name", "test"));
    }

    #[test]
    fn test_extract_literal_strings() {
        let preprocessor = RegexPreprocessor::default();
        
        // Test simple pattern with literal strings
        let literals = preprocessor.extract_literal_strings_from_regex("channel.*sport.*name");
        assert!(literals.contains(&"channel".to_string()));
        assert!(literals.contains(&"sport".to_string()));
        assert!(literals.contains(&"name".to_string()));
        
        // Test pattern with no meaningful literals
        let literals2 = preprocessor.extract_literal_strings_from_regex(".*+.*");
        assert!(literals2.is_empty());
    }

    #[test]
    fn test_extract_special_chars() {
        let preprocessor = RegexPreprocessor::default();
        
        // Test pattern with explicit + character
        let special_chars = preprocessor.extract_special_chars_from_regex("test\\+[0-9]+");
        assert!(special_chars.contains(&'+'));
        
        // Test character class with special chars
        let special_chars2 = preprocessor.extract_special_chars_from_regex("[+-]");
        assert!(special_chars2.contains(&'+'));
        assert!(special_chars2.contains(&'-'));
    }

    #[test]
    fn test_preprocessing_disabled() {
        let config = RegexPreprocessorConfig {
            enable_first_pass_filtering: false,
            precheck_special_chars: "+-".to_string(),
            minimum_literal_length: 2,
            max_quantifier_limit: 100,
        };
        let preprocessor = RegexPreprocessor::new(config);
        
        // Should always run regex when preprocessing is disabled
        assert!(preprocessor.should_run_regex("BBC One HD", ".*complex.*regex.*", "test"));
    }
}