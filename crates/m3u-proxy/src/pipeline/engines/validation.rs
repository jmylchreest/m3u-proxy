use crate::expression_parser::ExpressionParser;
use crate::models::data_mapping::{DataMappingSourceType, DataMappingFieldInfo};

/// Rule validation result
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct RuleValidationResult {
    pub is_valid: bool,
    pub error: Option<String>,
    pub parsed_successfully: bool,
    pub field_errors: Vec<String>,
}

/// Pipeline stage types for validation
#[derive(Debug, Clone, PartialEq)]
pub enum PipelineStageType {
    DataMapping,
    Filtering,
    Numbering,
    Generation,
}

/// Base trait for all pipeline stage validators
pub trait StageValidator {
    fn validate_expression(&self, expression: &str) -> RuleValidationResult;
    fn validate_syntax(&self, expression: &str) -> RuleValidationResult;
    fn get_available_fields(&self) -> Vec<String>;
    fn get_stage_type(&self) -> PipelineStageType;
}

/// Data mapping validator for stream and EPG data mapping rules
pub struct DataMappingValidator {
    source_type: DataMappingSourceType,
}

impl DataMappingValidator {
    pub fn new(source_type: DataMappingSourceType) -> Self {
        Self { source_type }
    }
    
    /// Validate a data mapping rule expression
    pub fn validate_expression(
        expression: &str,
        source_type: &DataMappingSourceType,
    ) -> RuleValidationResult {
        let validator = Self::new(source_type.clone());
        validator.validate_expression_impl(expression)
    }
    
    fn validate_expression_impl(&self, expression: &str) -> RuleValidationResult {
        let available_fields = self.get_available_fields();
        let parser = ExpressionParser::for_data_mapping(available_fields.clone());
        
        match parser.parse_extended(expression) {
            Ok(_parsed_expression) => {
                RuleValidationResult {
                    is_valid: true,
                    error: None,
                    parsed_successfully: true,
                    field_errors: vec![],
                }
            }
            Err(parse_error) => {
                let error_message = parse_error.to_string();
                let mut field_errors = vec![];
                
                for field in &available_fields {
                    if error_message.contains(field) {
                        field_errors.push(format!("Issue with field: {}", field));
                    }
                }
                
                RuleValidationResult {
                    is_valid: false,
                    error: Some(error_message),
                    parsed_successfully: false,
                    field_errors,
                }
            }
        }
    }
    
    /// Get available fields for a data mapping source type
    pub fn get_available_fields_for_source(source_type: &DataMappingSourceType) -> Vec<DataMappingFieldInfo> {
        DataMappingFieldInfo::available_for_source_type(source_type)
    }
}

impl StageValidator for DataMappingValidator {
    fn validate_expression(&self, expression: &str) -> RuleValidationResult {
        self.validate_expression_impl(expression)
    }
    
    fn validate_syntax(&self, expression: &str) -> RuleValidationResult {
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
    
    fn get_available_fields(&self) -> Vec<String> {
        match self.source_type {
            DataMappingSourceType::Stream => vec![
                "tvg_id".to_string(),
                "tvg_name".to_string(),
                "tvg_logo".to_string(),
                "tvg_shift".to_string(),
                "group_title".to_string(),
                "channel_name".to_string(),
            ],
            DataMappingSourceType::Epg => vec![
                "channel_id".to_string(),
                "channel_name".to_string(),
                "channel_logo".to_string(),
                "channel_group".to_string(),
                "language".to_string(),
            ],
        }
    }
    
    fn get_stage_type(&self) -> PipelineStageType {
        PipelineStageType::DataMapping
    }
}

/// Filtering validator for channel filtering rules
pub struct FilteringValidator;

impl FilteringValidator {
    pub fn new() -> Self {
        Self
    }
    
    /// Validate a filtering rule expression
    pub fn validate_expression(expression: &str) -> RuleValidationResult {
        let validator = Self::new();
        validator.validate_expression_impl(expression)
    }
    
    fn validate_expression_impl(&self, expression: &str) -> RuleValidationResult {
        // First validate time functions
        if let Some(error) = self.validate_time_functions(expression) {
            return RuleValidationResult {
                is_valid: false,
                error: Some(error),
                parsed_successfully: false,
                field_errors: vec![],
            };
        }
        
        let available_fields = self.get_available_fields();
        let parser = ExpressionParser::for_data_mapping(available_fields.clone());
        
        match parser.parse_extended(expression) {
            Ok(_parsed_expression) => {
                RuleValidationResult {
                    is_valid: true,
                    error: None,
                    parsed_successfully: true,
                    field_errors: vec![],
                }
            }
            Err(parse_error) => {
                let error_message = parse_error.to_string();
                let mut field_errors = vec![];
                
                for field in &available_fields {
                    if error_message.contains(field) {
                        field_errors.push(format!("Issue with field: {}", field));
                    }
                }
                
                RuleValidationResult {
                    is_valid: false,
                    error: Some(error_message),
                    parsed_successfully: false,
                    field_errors,
                }
            }
        }
    }
    
    /// Validate time function syntax in expressions
    fn validate_time_functions(&self, expression: &str) -> Option<String> {
        crate::utils::time::validate_time_function_syntax(expression)
    }
}

impl StageValidator for FilteringValidator {
    fn validate_expression(&self, expression: &str) -> RuleValidationResult {
        self.validate_expression_impl(expression)
    }
    
    fn validate_syntax(&self, expression: &str) -> RuleValidationResult {
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
    
    fn get_available_fields(&self) -> Vec<String> {
        vec![
            "channel_name".to_string(),
            "tvg_id".to_string(),
            "tvg_name".to_string(),
            "group_title".to_string(),
            "tvg_logo".to_string(),
            "stream_url".to_string(),
        ]
    }
    
    fn get_stage_type(&self) -> PipelineStageType {
        PipelineStageType::Filtering
    }
}

/// Numbering validator for channel numbering rules
pub struct NumberingValidator;

impl NumberingValidator {
    pub fn new() -> Self {
        Self
    }
    
    /// Validate a numbering rule expression
    pub fn validate_expression(expression: &str) -> RuleValidationResult {
        let validator = Self::new();
        validator.validate_expression_impl(expression)
    }
    
    fn validate_expression_impl(&self, expression: &str) -> RuleValidationResult {
        let available_fields = self.get_available_fields();
        let parser = ExpressionParser::for_data_mapping(available_fields.clone());
        
        match parser.parse_extended(expression) {
            Ok(_parsed_expression) => {
                RuleValidationResult {
                    is_valid: true,
                    error: None,
                    parsed_successfully: true,
                    field_errors: vec![],
                }
            }
            Err(parse_error) => {
                let error_message = parse_error.to_string();
                let mut field_errors = vec![];
                
                for field in &available_fields {
                    if error_message.contains(field) {
                        field_errors.push(format!("Issue with field: {}", field));
                    }
                }
                
                RuleValidationResult {
                    is_valid: false,
                    error: Some(error_message),
                    parsed_successfully: false,
                    field_errors,
                }
            }
        }
    }
}

impl StageValidator for NumberingValidator {
    fn validate_expression(&self, expression: &str) -> RuleValidationResult {
        self.validate_expression_impl(expression)
    }
    
    fn validate_syntax(&self, expression: &str) -> RuleValidationResult {
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
    
    fn get_available_fields(&self) -> Vec<String> {
        vec![
            "channel_name".to_string(),
            "group_title".to_string(),
            "position".to_string(),
            "sort_order".to_string(),
        ]
    }
    
    fn get_stage_type(&self) -> PipelineStageType {
        PipelineStageType::Numbering
    }
}

/// Generation validator for output generation rules
pub struct GenerationValidator;

impl GenerationValidator {
    pub fn new() -> Self {
        Self
    }
    
    /// Validate a generation rule expression
    pub fn validate_expression(expression: &str) -> RuleValidationResult {
        let validator = Self::new();
        validator.validate_expression_impl(expression)
    }
    
    fn validate_expression_impl(&self, expression: &str) -> RuleValidationResult {
        let available_fields = self.get_available_fields();
        let parser = ExpressionParser::for_data_mapping(available_fields.clone());
        
        match parser.parse_extended(expression) {
            Ok(_parsed_expression) => {
                RuleValidationResult {
                    is_valid: true,
                    error: None,
                    parsed_successfully: true,
                    field_errors: vec![],
                }
            }
            Err(parse_error) => {
                let error_message = parse_error.to_string();
                let mut field_errors = vec![];
                
                for field in &available_fields {
                    if error_message.contains(field) {
                        field_errors.push(format!("Issue with field: {}", field));
                    }
                }
                
                RuleValidationResult {
                    is_valid: false,
                    error: Some(error_message),
                    parsed_successfully: false,
                    field_errors,
                }
            }
        }
    }
}

impl StageValidator for GenerationValidator {
    fn validate_expression(&self, expression: &str) -> RuleValidationResult {
        self.validate_expression_impl(expression)
    }
    
    fn validate_syntax(&self, expression: &str) -> RuleValidationResult {
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
    
    fn get_available_fields(&self) -> Vec<String> {
        vec![
            "channel_name".to_string(),
            "stream_url".to_string(),
            "tvg_id".to_string(),
            "tvg_name".to_string(),
            "group_title".to_string(),
            "format".to_string(),
        ]
    }
    
    fn get_stage_type(&self) -> PipelineStageType {
        PipelineStageType::Generation
    }
}

/// Factory for creating validators for different pipeline stages
pub struct ValidationFactory;

impl ValidationFactory {
    /// Create a validator for a specific pipeline stage
    pub fn create_validator(stage_type: PipelineStageType, source_type: Option<DataMappingSourceType>) -> Box<dyn StageValidator> {
        match stage_type {
            PipelineStageType::DataMapping => {
                let source_type = source_type.unwrap_or(DataMappingSourceType::Stream);
                Box::new(DataMappingValidator::new(source_type))
            }
            PipelineStageType::Filtering => Box::new(FilteringValidator::new()),
            PipelineStageType::Numbering => Box::new(NumberingValidator::new()),
            PipelineStageType::Generation => Box::new(GenerationValidator::new()),
        }
    }
    
    /// Validate an expression for a specific stage type
    pub fn validate_for_stage(
        expression: &str,
        stage_type: PipelineStageType,
        source_type: Option<DataMappingSourceType>,
    ) -> RuleValidationResult {
        let validator = Self::create_validator(stage_type, source_type);
        validator.validate_expression(expression)
    }
}

// Legacy compatibility - maintain the old RuleValidationService for existing code
pub struct RuleValidationService;

impl RuleValidationService {
    /// Validate a rule expression using the existing parser (legacy compatibility)
    pub fn validate_expression(
        expression: &str,
        source_type: &DataMappingSourceType,
    ) -> RuleValidationResult {
        DataMappingValidator::validate_expression(expression, source_type)
    }
    
    /// Validate expression syntax only (basic check)
    pub fn validate_syntax(expression: &str) -> RuleValidationResult {
        let validator = DataMappingValidator::new(DataMappingSourceType::Stream);
        validator.validate_syntax(expression)
    }
    
    /// Get available fields for a source type (for API clients)
    pub fn get_available_fields(source_type: &DataMappingSourceType) -> Vec<DataMappingFieldInfo> {
        DataMappingValidator::get_available_fields_for_source(source_type)
    }
}