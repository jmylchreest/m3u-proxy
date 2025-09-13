use crate::models::data_mapping::DataMappingSourceType;
use crate::pipeline::engines::{
    DataMappingValidator, FilteringValidator, GenerationValidator, NumberingValidator,
    PipelineStageType, RuleValidationResult, ValidationFactory,
};

/// Centralized validation service for all pipeline stages
/// Provides a unified API for validating rules across different pipeline stages
pub struct PipelineValidationService;

impl PipelineValidationService {
    /// Validate a data mapping rule expression
    pub fn validate_data_mapping_rule(
        expression: &str,
        source_type: &DataMappingSourceType,
    ) -> RuleValidationResult {
        DataMappingValidator::validate_expression(expression, source_type)
    }

    /// Validate a filtering rule expression
    pub fn validate_filtering_rule(expression: &str) -> RuleValidationResult {
        FilteringValidator::validate_expression(expression)
    }

    /// Validate a numbering rule expression
    pub fn validate_numbering_rule(expression: &str) -> RuleValidationResult {
        NumberingValidator::validate_expression(expression)
    }

    /// Validate a generation rule expression
    pub fn validate_generation_rule(expression: &str) -> RuleValidationResult {
        GenerationValidator::validate_expression(expression)
    }

    /// Validate expression for any pipeline stage using the factory
    pub fn validate_for_stage(
        expression: &str,
        stage_type: PipelineStageType,
        source_type: Option<DataMappingSourceType>,
    ) -> RuleValidationResult {
        ValidationFactory::validate_for_stage(expression, stage_type, source_type)
    }

    /// Get available fields for a specific stage
    pub fn get_available_fields_for_stage(
        stage_type: PipelineStageType,
        source_type: Option<DataMappingSourceType>,
    ) -> Vec<String> {
        let validator = ValidationFactory::create_validator(stage_type, source_type);
        validator.get_available_fields()
    }

    /// Validate only syntax (no field context) for any expression
    pub fn validate_syntax_only(expression: &str) -> RuleValidationResult {
        if expression.trim().is_empty() {
            return RuleValidationResult {
                is_valid: false,
                error: Some("Expression cannot be empty".to_string()),
                parsed_successfully: false,
                field_errors: vec![],
            };
        }

        RuleValidationResult {
            is_valid: true,
            error: None,
            parsed_successfully: true,
            field_errors: vec![],
        }
    }
}

/// API-friendly validation service for web endpoints
/// Provides string-based stage identification for easier integration
pub struct ApiValidationService;

impl ApiValidationService {
    /// Validate expression by stage name (string)
    pub fn validate_by_stage_name(
        expression: &str,
        stage_name: &str,
        source_type: Option<&str>,
    ) -> Result<RuleValidationResult, String> {
        let stage_type = match stage_name {
            "data_mapping" => PipelineStageType::DataMapping,
            "filtering" => PipelineStageType::Filtering,
            "numbering" => PipelineStageType::Numbering,
            "generation" => PipelineStageType::Generation,
            _ => return Err(format!("Unknown stage: {stage_name}")),
        };

        let source_type_enum = if let Some(st) = source_type {
            match st {
                "stream" => Some(DataMappingSourceType::Stream),
                "epg" => Some(DataMappingSourceType::Epg),
                _ => return Err(format!("Unknown source type: {st}")),
            }
        } else {
            None
        };

        Ok(PipelineValidationService::validate_for_stage(
            expression,
            stage_type,
            source_type_enum,
        ))
    }

    /// Get available fields by stage name (string)
    pub fn get_fields_by_stage_name(
        stage_name: &str,
        source_type: Option<&str>,
    ) -> Result<Vec<String>, String> {
        let stage_type = match stage_name {
            "data_mapping" => PipelineStageType::DataMapping,
            "filtering" => PipelineStageType::Filtering,
            "numbering" => PipelineStageType::Numbering,
            "generation" => PipelineStageType::Generation,
            _ => return Err(format!("Unknown stage: {stage_name}")),
        };

        let source_type_enum = if let Some(st) = source_type {
            match st {
                "stream" => Some(DataMappingSourceType::Stream),
                "epg" => Some(DataMappingSourceType::Epg),
                _ => return Err(format!("Unknown source type: {st}")),
            }
        } else {
            None
        };

        Ok(PipelineValidationService::get_available_fields_for_stage(
            stage_type,
            source_type_enum,
        ))
    }
}
