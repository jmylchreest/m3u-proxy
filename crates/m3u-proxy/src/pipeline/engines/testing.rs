use super::{ChannelDataMappingEngine, DataMappingEngine, StreamRuleProcessor};
use super::rule_processor::RegexEvaluator;
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
            let processor = StreamRuleProcessor::new(rule_id, rule_name, expression, regex_evaluator);
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
                        execution_time_ms: rule_result.execution_time.as_millis(),
                        field_changes: rule_result.field_modifications.iter()
                            .map(|m| format!("{}: {} -> {}", 
                                m.field_name, 
                                m.old_value.as_deref().unwrap_or("None"),
                                m.new_value.as_deref().unwrap_or("None")
                            ))
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