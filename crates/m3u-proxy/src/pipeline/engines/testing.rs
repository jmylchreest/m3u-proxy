use super::rule_processor::{EpgProgram, RegexEvaluator};
use super::{
    ChannelDataMappingEngine, DataMappingEngine, EpgRuleProcessor, ProgramDataMappingEngine,
    StreamRuleProcessor,
};
use crate::models::Channel;
use crate::utils::regex_preprocessor::RegexPreprocessor;
use uuid::Uuid;

/// Test result for data mapping rule testing
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct DataMappingTestResult {
    pub test_id: Uuid,
    pub channels_processed: usize,
    pub channels_modified: usize,
    pub processing_time_ms: u128,
    pub results: Vec<ChannelTestResult>,
}

/// Individual channel test result
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ChannelTestResult {
    pub channel_id: Uuid,
    pub channel_name: String,
    pub was_modified: bool,
    pub rule_applications: Vec<RuleApplicationResult>,
    pub initial_channel: Channel,
    pub final_channel: Channel,
}

/// Result of applying a single rule to a channel
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct RuleApplicationResult {
    pub rule_id: String,
    pub rule_name: String,
    pub applied: bool,
    pub condition_matched: bool,
    pub execution_time_ms: u128,
    pub field_changes: Vec<String>, // Simple descriptions of changes
    pub error: Option<String>,
}

/// Service for testing data mapping rules against sample channels
pub struct DataMappingTestService;

impl DataMappingTestService {
    /// Test a set of rules against sample channels
    pub fn test_rules_against_channels(
        rules: Vec<(String, String, String)>, // (rule_id, rule_name, expression)
        channels: Vec<Channel>,
    ) -> Result<DataMappingTestResult, Box<dyn std::error::Error>> {
        let test_id = Uuid::new_v4();
        let start_time = std::time::Instant::now();

        // Create a test engine
        let mut engine = ChannelDataMappingEngine::new(Uuid::new_v4()); // dummy source_id

        // Initialize regex preprocessor for testing
        let regex_preprocessor = RegexPreprocessor::new(Default::default());

        // Add rule processors to the engine
        for (rule_id, rule_name, expression) in rules {
            let regex_evaluator = RegexEvaluator::new(regex_preprocessor.clone());
            let processor =
                StreamRuleProcessor::new(rule_id, rule_name, expression, regex_evaluator);
            engine.add_rule_processor(processor);
        }

        // Process channels
        let engine_result = engine.process_records(channels.clone())?;

        let processing_time_ms = start_time.elapsed().as_millis();

        // Convert engine results to API-friendly test results
        let mut results = Vec::new();

        for (i, final_channel) in engine_result.processed_records.iter().enumerate() {
            let original_channel = &channels[i];
            let was_modified = original_channel != final_channel;

            // Extract rule applications for this channel
            let mut rule_applications = Vec::new();
            for (rule_id, rule_results) in &engine_result.rule_results {
                if let Some(rule_result) = rule_results.get(i) {
                    let app_result = RuleApplicationResult {
                        rule_id: rule_id.clone(),
                        rule_name: "Test Rule".to_string(), // TODO: Get from processor
                        applied: rule_result.rule_applied,
                        condition_matched: rule_result.condition_matched,
                        execution_time_ms: rule_result.execution_time.as_millis(),
                        field_changes: rule_result
                            .field_modifications
                            .iter()
                            .map(|m| {
                                format!(
                                    "{}: {} -> {}",
                                    m.field_name,
                                    m.old_value.as_deref().unwrap_or("None"),
                                    m.new_value.as_deref().unwrap_or("None")
                                )
                            })
                            .collect(),
                        error: rule_result.error.clone(),
                    };
                    rule_applications.push(app_result);
                }
            }

            let channel_result = ChannelTestResult {
                channel_id: final_channel.id,
                channel_name: final_channel.channel_name.clone(),
                was_modified,
                rule_applications,
                initial_channel: original_channel.clone(),
                final_channel: final_channel.clone(),
            };

            results.push(channel_result);
        }

        // Clean up engine
        engine.destroy();

        Ok(DataMappingTestResult {
            test_id,
            channels_processed: results.len(),
            channels_modified: results.iter().filter(|r| r.was_modified).count(),
            processing_time_ms,
            results,
        })
    }

    /// Test a single rule expression against sample channels
    pub fn test_single_rule(
        rule_expression: String,
        channels: Vec<Channel>,
    ) -> Result<DataMappingTestResult, Box<dyn std::error::Error>> {
        let rules = vec![(
            Uuid::new_v4().to_string(),
            "Test Rule".to_string(),
            rule_expression,
        )];

        Self::test_rules_against_channels(rules, channels)
    }
}

/// Test result for EPG data mapping rule testing
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct EpgDataMappingTestResult {
    pub test_id: Uuid,
    pub programs_processed: usize,
    pub programs_modified: usize,
    pub processing_time_ms: u128,
    pub results: Vec<EpgProgramTestResult>,
}

/// Individual EPG program test result
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct EpgProgramTestResult {
    pub program_id: String,
    pub program_title: String,
    pub channel_id: String,
    pub was_modified: bool,
    pub rule_applications: Vec<RuleApplicationResult>,
    pub initial_program: EpgProgram,
    pub final_program: EpgProgram,
}

/// Service for testing EPG data mapping rules against sample programs
pub struct EpgDataMappingTestService;

impl EpgDataMappingTestService {
    /// Test a set of EPG rules against sample programs
    pub fn test_rules_against_programs(
        rules: Vec<(String, String, String)>, // (rule_id, rule_name, expression)
        programs: Vec<EpgProgram>,
    ) -> Result<EpgDataMappingTestResult, Box<dyn std::error::Error>> {
        let test_id = Uuid::new_v4();
        let start_time = std::time::Instant::now();

        // Create a test engine
        let mut engine = ProgramDataMappingEngine::new(Uuid::new_v4()); // dummy source_id

        // Initialize regex preprocessor for testing
        let regex_preprocessor = RegexPreprocessor::new(Default::default());

        // Add rule processors to the engine
        for (rule_id, rule_name, expression) in rules {
            let _regex_evaluator = RegexEvaluator::new(regex_preprocessor.clone());
            let processor = EpgRuleProcessor::new(rule_id, rule_name, expression);
            if let Some(err) = &processor.parse_error {
                println!(
                    "(TEST_DEBUG) EPG rule parse_error: id={} name='{}' error={}",
                    processor.rule_id, processor.rule_name, err
                );
            }
            engine.add_rule_processor(processor);
        }

        // Process programs
        let engine_result = engine.process_records(programs.clone())?;

        let processing_time_ms = start_time.elapsed().as_millis();

        // Convert engine results to API-friendly test results
        let mut results = Vec::new();

        for (i, final_program) in engine_result.processed_records.iter().enumerate() {
            let original_program = &programs[i];
            let was_modified = original_program != final_program;

            // Extract rule applications for this program
            let mut rule_applications = Vec::new();
            for (rule_id, rule_results) in &engine_result.rule_results {
                if let Some(rule_result) = rule_results.get(i) {
                    let app_result = RuleApplicationResult {
                        rule_id: rule_id.clone(),
                        rule_name: "Test EPG Rule".to_string(), // TODO: Get from processor
                        applied: rule_result.rule_applied,
                        condition_matched: rule_result.condition_matched,
                        execution_time_ms: rule_result.execution_time.as_millis(),
                        field_changes: rule_result
                            .field_modifications
                            .iter()
                            .map(|m| {
                                format!(
                                    "{}: {} -> {}",
                                    m.field_name,
                                    m.old_value.as_deref().unwrap_or("None"),
                                    m.new_value.as_deref().unwrap_or("None")
                                )
                            })
                            .collect(),
                        error: rule_result.error.clone(),
                    };
                    rule_applications.push(app_result);
                }
            }

            let program_result = EpgProgramTestResult {
                program_id: final_program.id.clone(),
                program_title: final_program.title.clone(),
                channel_id: final_program.channel_id.clone(),
                was_modified,
                rule_applications,
                initial_program: original_program.clone(),
                final_program: final_program.clone(),
            };

            results.push(program_result);
        }

        // Clean up engine
        engine.destroy();

        Ok(EpgDataMappingTestResult {
            test_id,
            programs_processed: results.len(),
            programs_modified: results.iter().filter(|r| r.was_modified).count(),
            processing_time_ms,
            results,
        })
    }

    /// Test a single EPG rule expression against sample programs
    pub fn test_single_epg_rule(
        rule_expression: String,
        programs: Vec<EpgProgram>,
    ) -> Result<EpgDataMappingTestResult, Box<dyn std::error::Error>> {
        let rules = vec![(
            Uuid::new_v4().to_string(),
            "Test EPG Rule".to_string(),
            rule_expression,
        )];

        Self::test_rules_against_programs(rules, programs)
    }

    /// Create sample EPG programs for testing
    pub fn create_sample_epg_programs() -> Vec<EpgProgram> {
        use chrono::{DateTime, Utc};

        vec![
            EpgProgram {
                id: "prog_1".to_string(),
                channel_id: "channel_1".to_string(),
                channel_name: "News Channel".to_string(),
                title: "Breaking News".to_string(),
                description: Some("Latest breaking news coverage".to_string()),
                program_icon: None,
                start_time: DateTime::parse_from_rfc3339("2024-01-01T10:00:00Z")
                    .unwrap()
                    .with_timezone(&Utc),
                end_time: DateTime::parse_from_rfc3339("2024-01-01T11:00:00Z")
                    .unwrap()
                    .with_timezone(&Utc),
                program_category: Some("News".to_string()),
                subtitles: Some("English subtitles available".to_string()),
                episode_num: Some("1".to_string()),
                season_num: Some("1".to_string()),
                language: Some("en".to_string()),
                rating: Some("TV-G".to_string()),
                aspect_ratio: Some("16:9".to_string()),
            },
            EpgProgram {
                id: "prog_2".to_string(),
                channel_id: "channel_2".to_string(),
                channel_name: "Movie Channel".to_string(),
                title: "Movie Night: Action Hero".to_string(),
                description: Some("Explosive action movie".to_string()),
                program_icon: None,
                start_time: DateTime::parse_from_rfc3339("2024-01-01T20:00:00Z")
                    .unwrap()
                    .with_timezone(&Utc),
                end_time: DateTime::parse_from_rfc3339("2024-01-01T22:00:00Z")
                    .unwrap()
                    .with_timezone(&Utc),
                program_category: Some("Movies".to_string()),
                subtitles: None,
                episode_num: None,
                season_num: None,
                language: Some("en".to_string()),
                rating: Some("TV-14".to_string()),
                aspect_ratio: Some("21:9".to_string()),
            },
            EpgProgram {
                id: "prog_3".to_string(),
                channel_id: "channel_1".to_string(),
                channel_name: "News Channel".to_string(),
                title: "Sports Tonight".to_string(),
                description: Some("Latest sports highlights and analysis".to_string()),
                program_icon: None,
                start_time: DateTime::parse_from_rfc3339("2024-01-01T23:00:00Z")
                    .unwrap()
                    .with_timezone(&Utc),
                end_time: DateTime::parse_from_rfc3339("2024-01-02T00:00:00Z")
                    .unwrap()
                    .with_timezone(&Utc),
                program_category: Some("Sports".to_string()),
                subtitles: Some("Multiple languages available".to_string()),
                episode_num: Some("15".to_string()),
                season_num: Some("2024".to_string()),
                language: Some("en".to_string()),
                rating: Some("TV-PG".to_string()),
                aspect_ratio: Some("16:9".to_string()),
            },
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_epg_rule_category_filtering() {
        let programs = EpgDataMappingTestService::create_sample_epg_programs();

        // Test category filtering - should modify programs with "Movies" category
        let rule_expression =
            r#"program_category equals "Movies" SET program_title = "MOVIE: Action Hero""#;

        let result =
            EpgDataMappingTestService::test_single_epg_rule(rule_expression.to_string(), programs)
                .expect("EPG rule test should succeed");

        // Should process 3 programs, modify 1 (the movie)
        assert_eq!(result.programs_processed, 3);
        assert_eq!(result.programs_modified, 1);

        // Find the modified movie program
        let modified_movie = result
            .results
            .iter()
            .find(|r| r.initial_program.program_category == Some("Movies".to_string()))
            .expect("Should find movie program");

        assert!(modified_movie.was_modified);
        assert_eq!(modified_movie.final_program.title, "MOVIE: Action Hero");
    }

    #[test]
    fn test_epg_rule_channel_based_filtering() {
        let programs = EpgDataMappingTestService::create_sample_epg_programs();

        // Test channel-based rule - should only affect channel_1 programs
        let rule_expression = r#"channel_id equals "channel_1" SET language = "en-US""#;

        let result =
            EpgDataMappingTestService::test_single_epg_rule(rule_expression.to_string(), programs)
                .expect("EPG rule test should succeed");

        // Should modify 2 programs (both channel_1 programs)
        assert_eq!(result.programs_modified, 2);

        // Verify channel_1 programs were modified
        for test_result in &result.results {
            if test_result.initial_program.channel_id == "channel_1" {
                assert!(test_result.was_modified);
                assert_eq!(
                    test_result.final_program.language,
                    Some("en-US".to_string())
                );
            } else {
                assert!(!test_result.was_modified);
            }
        }
    }

    #[test]
    fn test_epg_rule_title_transformation() {
        let programs = EpgDataMappingTestService::create_sample_epg_programs();

        // Test regex-based title transformation
        let rule_expression =
            r#"program_title matches "^(.+): (.+)$" SET program_title = "$2 ($1)""#;

        let result =
            EpgDataMappingTestService::test_single_epg_rule(rule_expression.to_string(), programs)
                .expect("EPG rule test should succeed");

        // Should modify the "Movie Night: Action Hero" program
        let modified_program = result
            .results
            .iter()
            .find(|r| r.initial_program.title.contains(": "))
            .expect("Should find program with colon in title");

        assert!(modified_program.was_modified);
        assert_eq!(
            modified_program.final_program.title,
            "Action Hero (Movie Night)"
        );
    }

    #[test]
    fn test_epg_rule_conditional_assignment() {
        let mut programs = EpgDataMappingTestService::create_sample_epg_programs();

        // Remove subtitles from one program to test conditional assignment
        programs[1].subtitles = None;

        let rule_expression =
            r#"program_category equals "Movies" SET subtitles ?= "No subtitles available""#;

        let result =
            EpgDataMappingTestService::test_single_epg_rule(rule_expression.to_string(), programs)
                .expect("EPG rule test should succeed");

        // Should modify 1 program (the one without subtitles)
        assert_eq!(result.programs_modified, 1);

        // Find the program that had no subtitles
        let modified_program = result
            .results
            .iter()
            .find(|r| r.initial_program.subtitles.is_none())
            .expect("Should find program without subtitles");

        assert!(modified_program.was_modified);
        assert_eq!(
            modified_program.final_program.subtitles,
            Some("No subtitles available".to_string())
        );
    }

    #[test]
    fn test_epg_rule_alias_action_program_title() {
        // Verify that using program_title (alias) in the action updates the canonical programme_title
        let programs = EpgDataMappingTestService::create_sample_epg_programs();

        let rule_expression =
            r#"program_category equals "Movies" SET program_title = "ALIased Movie Title""#;

        let result =
            EpgDataMappingTestService::test_single_epg_rule(rule_expression.to_string(), programs)
                .expect("EPG rule alias action test should succeed");

        // Exactly one movie category program should be modified
        assert_eq!(
            result.programs_modified, 1,
            "Expected exactly one modified program (movie)"
        );

        let modified_movie = result
            .results
            .iter()
            .find(|r| r.initial_program.program_category.as_deref() == Some("Movies"))
            .expect("Should find movie program result");

        assert!(
            modified_movie.was_modified,
            "Alias action should mark program as modified"
        );
        assert_eq!(
            modified_movie.final_program.title, "ALIased Movie Title",
            "Title should be updated via alias action field"
        );
    }
}
