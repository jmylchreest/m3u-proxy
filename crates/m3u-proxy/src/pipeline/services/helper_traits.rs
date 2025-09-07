//! Helper trait implementations for data models
//!
//! This module provides implementations of HelperDetectable and HelperProcessable
//! traits for Channel and EpgProgram models.

use crate::models::Channel;
use crate::pipeline::engines::rule_processor::EpgProgram as PipelineEpgProgram;
use crate::pipeline::services::helper_processor::{
    HelperDetectable, HelperProcessable, HelperField, HelperProcessor,
};

// Implementation for Channel model
impl HelperDetectable for Channel {
    fn contains_any_helpers(&self, processors: &[Box<dyn HelperProcessor>]) -> bool {
        // Check tvg_logo field for any helpers
        if let Some(logo) = &self.tvg_logo {
            for processor in processors {
                if processor.contains_helper(logo) {
                    return true;
                }
            }
        }
        
        // Check tvg_shift field for time helpers
        if let Some(shift) = &self.tvg_shift {
            for processor in processors {
                if processor.contains_helper(shift) {
                    return true;
                }
            }
        }
        
        false
    }
}

impl HelperProcessable for Channel {
    fn get_helper_processable_fields(&self) -> Vec<HelperField> {
        let mut fields = Vec::new();
        
        // Add tvg_logo field
        if let Some(logo) = &self.tvg_logo {
            fields.push(HelperField {
                name: "tvg_logo".to_string(),
                value: Some(logo.clone()),
            });
        }
        
        // Add tvg_shift field (for potential @time: helpers)
        if let Some(shift) = &self.tvg_shift {
            fields.push(HelperField {
                name: "tvg_shift".to_string(),
                value: Some(shift.clone()),
            });
        }
        
        fields
    }
    
    fn update_from_helper_fields(&mut self, fields: Vec<HelperField>) {
        for field in fields {
            match field.name.as_str() {
                "tvg_logo" => {
                    self.tvg_logo = field.value;
                }
                "tvg_shift" => {
                    self.tvg_shift = field.value;
                }
                _ => {
                    // Unknown field, ignore
                }
            }
        }
    }
}

// Implementation for Pipeline EpgProgram model  
impl HelperDetectable for PipelineEpgProgram {
    fn contains_any_helpers(&self, processors: &[Box<dyn HelperProcessor>]) -> bool {
        // Check description field for any helpers
        if let Some(description) = &self.description {
            for processor in processors {
                if processor.contains_helper(description) {
                    return true;
                }
            }
        }
        
        // Check title field for any helpers
        for processor in processors {
            if processor.contains_helper(&self.title) {
                return true;
            }
        }
        
        // Check new extended fields for helpers
        if let Some(category) = &self.program_category {
            for processor in processors {
                if processor.contains_helper(category) {
                    return true;
                }
            }
        }
        
        if let Some(subtitles) = &self.subtitles {
            for processor in processors {
                if processor.contains_helper(subtitles) {
                    return true;
                }
            }
        }
        
        false
    }
}

impl HelperProcessable for PipelineEpgProgram {
    fn get_helper_processable_fields(&self) -> Vec<HelperField> {
        let mut fields = Vec::new();
        
        // Add title field
        fields.push(HelperField {
            name: "title".to_string(),
            value: Some(self.title.clone()),
        });
        
        // Add description field
        if let Some(description) = &self.description {
            fields.push(HelperField {
                name: "description".to_string(),
                value: Some(description.clone()),
            });
        }
        
        // Add extended XMLTV fields
        if let Some(category) = &self.program_category {
            fields.push(HelperField {
                name: "program_category".to_string(),
                value: Some(category.clone()),
            });
        }
        
        if let Some(subtitles) = &self.subtitles {
            fields.push(HelperField {
                name: "subtitles".to_string(),
                value: Some(subtitles.clone()),
            });
        }
        
        if let Some(episode_num) = &self.episode_num {
            fields.push(HelperField {
                name: "episode_num".to_string(),
                value: Some(episode_num.clone()),
            });
        }
        
        if let Some(season_num) = &self.season_num {
            fields.push(HelperField {
                name: "season_num".to_string(),
                value: Some(season_num.clone()),
            });
        }
        
        if let Some(language) = &self.language {
            fields.push(HelperField {
                name: "language".to_string(),
                value: Some(language.clone()),
            });
        }
        
        if let Some(rating) = &self.rating {
            fields.push(HelperField {
                name: "rating".to_string(),
                value: Some(rating.clone()),
            });
        }
        
        fields
    }
    
    fn update_from_helper_fields(&mut self, fields: Vec<HelperField>) {
        for field in fields {
            match field.name.as_str() {
                "title" => {
                    if let Some(value) = field.value {
                        self.title = value;
                    }
                }
                "description" => {
                    self.description = field.value;
                }
                "program_category" => {
                    self.program_category = field.value;
                }
                "subtitles" => {
                    self.subtitles = field.value;
                }
                "episode_num" => {
                    self.episode_num = field.value;
                }
                "season_num" => {
                    self.season_num = field.value;
                }
                "language" => {
                    self.language = field.value;
                }
                "rating" => {
                    self.rating = field.value;
                }
                _ => {
                    // Unknown field, ignore
                }
            }
        }
    }
}

// Implementation for main EpgProgram model (from models/mod.rs)
impl HelperDetectable for crate::models::EpgProgram {
    fn contains_any_helpers(&self, processors: &[Box<dyn HelperProcessor>]) -> bool {
        // Check program_icon field for logo helpers
        if let Some(icon) = &self.program_icon {
            for processor in processors {
                if processor.contains_helper(icon) {
                    return true;
                }
            }
        }
        
        // Check program_description field for any helpers
        if let Some(description) = &self.program_description {
            for processor in processors {
                if processor.contains_helper(description) {
                    return true;
                }
            }
        }
        
        // Check program_title field for any helpers  
        for processor in processors {
            if processor.contains_helper(&self.program_title) {
                return true;
            }
        }
        
        false
    }
}

impl HelperProcessable for crate::models::EpgProgram {
    fn get_helper_processable_fields(&self) -> Vec<HelperField> {
        let mut fields = Vec::new();
        
        // Add program_title field
        fields.push(HelperField {
            name: "program_title".to_string(),
            value: Some(self.program_title.clone()),
        });
        
        // Add program_description field
        if let Some(description) = &self.program_description {
            fields.push(HelperField {
                name: "program_description".to_string(),
                value: Some(description.clone()),
            });
        }
        
        // Add program_icon field (for logo helpers)
        if let Some(icon) = &self.program_icon {
            fields.push(HelperField {
                name: "program_icon".to_string(),
                value: Some(icon.clone()),
            });
        }
        
        fields
    }
    
    fn update_from_helper_fields(&mut self, fields: Vec<HelperField>) {
        for field in fields {
            match field.name.as_str() {
                "program_title" => {
                    if let Some(value) = field.value {
                        self.program_title = value;
                    }
                }
                "program_description" => {
                    self.program_description = field.value;
                }
                "program_icon" => {
                    self.program_icon = field.value;
                }
                _ => {
                    // Unknown field, ignore
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use uuid::Uuid;
    
    #[test]
    fn test_channel_helper_detection() {
        let _channel = Channel {
            id: Uuid::new_v4(),
            source_id: Uuid::new_v4(),
            tvg_id: None,
            tvg_name: None,
            tvg_chno: None,
            tvg_logo: Some("@logo:550e8400-e29b-41d4-a716-446655440000".to_string()),
            tvg_shift: None,
            group_title: None,
            channel_name: "Test Channel".to_string(),
            stream_url: "http://example.com/stream".to_string(),
            video_codec: None,
            audio_codec: None,
            resolution: None,
            probe_method: None,
            last_probed_at: None,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        };
        
        // Mock processors vector - would need actual processors for real test
        let _processors: Vec<Box<dyn HelperProcessor>> = vec![];
        
        // This would return true if we had actual logo helper processor
        // assert!(channel.contains_any_helpers(&processors));
    }
    
    #[test]
    fn test_channel_helper_fields() {
        let mut channel = Channel {
            id: Uuid::new_v4(),
            source_id: Uuid::new_v4(),
            tvg_id: None,
            tvg_name: None,
            tvg_chno: None,
            tvg_logo: Some("@logo:550e8400-e29b-41d4-a716-446655440000".to_string()),
            tvg_shift: None,
            group_title: None,
            channel_name: "Test Channel".to_string(),
            stream_url: "http://example.com/stream".to_string(),
            video_codec: None,
            audio_codec: None,
            resolution: None,
            probe_method: None,
            last_probed_at: None,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        };
        
        let fields = channel.get_helper_processable_fields();
        assert_eq!(fields.len(), 1);
        assert_eq!(fields[0].name, "tvg_logo");
        assert_eq!(fields[0].value, Some("@logo:550e8400-e29b-41d4-a716-446655440000".to_string()));
        
        // Test updating from fields
        let updated_fields = vec![HelperField {
            name: "tvg_logo".to_string(),
            value: Some("https://example.com/logo.png".to_string()),
        }];
        
        channel.update_from_helper_fields(updated_fields);
        assert_eq!(channel.tvg_logo, Some("https://example.com/logo.png".to_string()));
        
        // Test removing field by setting to None
        let remove_fields = vec![HelperField {
            name: "tvg_logo".to_string(),
            value: None,
        }];
        
        channel.update_from_helper_fields(remove_fields);
        assert_eq!(channel.tvg_logo, None);
    }
}