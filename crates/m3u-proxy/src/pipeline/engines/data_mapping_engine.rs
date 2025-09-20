use super::rule_processor::{
    EpgProgram, EpgRuleProcessor, RuleProcessor, RuleResult, StreamRuleProcessor,
};
use crate::models::Channel;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::Duration;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EngineResult<T> {
    pub processed_records: Vec<T>,
    pub total_processed: usize,
    pub total_modified: usize,
    pub total_condition_matches: usize,
    pub rule_results: HashMap<String, Vec<RuleResult>>,
    pub execution_time: Duration,
}

pub trait DataMappingEngine<T> {
    type RuleProcessor: RuleProcessor<T>;

    fn new(source_id: Uuid) -> Self;
    fn add_rule_processor(&mut self, processor: Self::RuleProcessor);
    fn process_records(
        &mut self,
        records: Vec<T>,
    ) -> Result<EngineResult<T>, Box<dyn std::error::Error>>;
    fn get_source_id(&self) -> Uuid;
    fn destroy(self);
}

pub struct ChannelDataMappingEngine {
    source_id: Uuid,
    rule_processors: Vec<StreamRuleProcessor>,
}

impl DataMappingEngine<Channel> for ChannelDataMappingEngine {
    type RuleProcessor = StreamRuleProcessor;

    fn new(source_id: Uuid) -> Self {
        Self {
            source_id,
            rule_processors: Vec::new(),
        }
    }

    fn add_rule_processor(&mut self, processor: Self::RuleProcessor) {
        self.rule_processors.push(processor);
    }

    fn process_records(
        &mut self,
        records: Vec<Channel>,
    ) -> Result<EngineResult<Channel>, Box<dyn std::error::Error>> {
        let start = std::time::Instant::now();
        let mut processed_records = Vec::with_capacity(records.len());
        let mut rule_results: HashMap<String, Vec<RuleResult>> = HashMap::new();
        let mut total_modified = 0;
        let mut total_condition_matches = 0;

        for record in records {
            let mut current_record = record;

            // Process through each rule processor in order
            for rule_processor in &mut self.rule_processors {
                let (updated_record, result) = rule_processor.process_record(current_record)?;
                current_record = updated_record;

                if result.rule_applied {
                    total_modified += 1;
                }
                if result.condition_matched {
                    total_condition_matches += 1;
                }

                rule_results
                    .entry(rule_processor.get_rule_id().to_string())
                    .or_default()
                    .push(result);
            }

            processed_records.push(current_record);
        }

        Ok(EngineResult {
            total_processed: processed_records.len(),
            total_modified,
            total_condition_matches,
            processed_records,
            rule_results,
            execution_time: start.elapsed(),
        })
    }

    fn get_source_id(&self) -> Uuid {
        self.source_id
    }

    fn destroy(self) {
        // Cleanup resources if needed
        drop(self);
    }
}

pub struct ProgramDataMappingEngine {
    source_id: Uuid,
    rule_processors: Vec<EpgRuleProcessor>,
}

impl DataMappingEngine<EpgProgram> for ProgramDataMappingEngine {
    type RuleProcessor = EpgRuleProcessor;

    fn new(source_id: Uuid) -> Self {
        Self {
            source_id,
            rule_processors: Vec::new(),
        }
    }

    fn add_rule_processor(&mut self, processor: Self::RuleProcessor) {
        self.rule_processors.push(processor);
    }

    fn process_records(
        &mut self,
        records: Vec<EpgProgram>,
    ) -> Result<EngineResult<EpgProgram>, Box<dyn std::error::Error>> {
        let start = std::time::Instant::now();
        let mut processed_records = Vec::with_capacity(records.len());
        let mut rule_results: HashMap<String, Vec<RuleResult>> = HashMap::new();
        let mut total_modified = 0;
        let mut total_condition_matches = 0;

        for record in records {
            let mut current_record = record;

            // Process through each rule processor in order
            for rule_processor in &mut self.rule_processors {
                tracing::trace!(
                    "[EPG_RULE_ENGINE] applying rule_id={} name='{}' program_id={} title='{}'",
                    rule_processor.get_rule_id(),
                    rule_processor.get_rule_name(),
                    current_record.id,
                    current_record.title
                );
                let (updated_record, result) = rule_processor.process_record(current_record)?;
                current_record = updated_record;
                tracing::trace!(
                    "[EPG_RULE_ENGINE_RESULT] rule_id={} matched={} applied={} mods={} title='{}'",
                    rule_processor.get_rule_id(),
                    result.condition_matched,
                    result.rule_applied,
                    result.field_modifications.len(),
                    current_record.title
                );

                if result.rule_applied {
                    total_modified += 1;
                }
                if result.condition_matched {
                    total_condition_matches += 1;
                }

                rule_results
                    .entry(rule_processor.get_rule_id().to_string())
                    .or_default()
                    .push(result);
            }

            processed_records.push(current_record);
        }

        Ok(EngineResult {
            total_processed: processed_records.len(),
            total_modified,
            total_condition_matches,
            processed_records,
            rule_results,
            execution_time: start.elapsed(),
        })
    }

    fn get_source_id(&self) -> Uuid {
        self.source_id
    }

    fn destroy(self) {
        // Cleanup resources if needed
        drop(self);
    }
}
