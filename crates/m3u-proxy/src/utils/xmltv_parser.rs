//! Quick-XML based XMLTV parser
//!
//! This module provides a streaming XML parser for XMLTV files using quick-xml.
//! It extracts only the fields we actually use, providing better memory efficiency
//! and performance compared to the full xmltv crate deserialization.

use quick_xml::events::{Event, BytesStart};
use quick_xml::Reader;
use std::collections::HashMap;
use crate::errors::{AppError, AppResult};

/// Simple program structure containing only the fields we actually use
#[derive(Debug, Clone)]
pub struct SimpleXmltvProgram {
    pub channel: String,
    pub start: String,
    pub stop: Option<String>,
    pub title: Option<String>,
    pub description: Option<String>,
    pub category: Option<String>,
    pub language: Option<String>,
    pub icon: Option<String>,
}

/// Parse XMLTV content using streaming quick-xml parser
pub fn parse_xmltv_programs(content: &str) -> AppResult<Vec<SimpleXmltvProgram>> {
    let mut reader = Reader::from_str(content);
    reader.config_mut().trim_text(true);
    
    let mut programs = Vec::new();
    
    let mut current_program: Option<SimpleXmltvProgram> = None;
    let mut current_element_stack = Vec::new();
    let mut current_text = String::new();
    
    loop {
        match reader.read_event() {
            Ok(Event::Start(ref e)) => {
                let name = std::str::from_utf8(e.name().as_ref())
                    .map_err(|e| AppError::source_error(format!("Invalid UTF-8 in XML element name: {e}")))?
                    .to_string();
                
                if name.as_str() == "programme" {
                    let attrs = parse_attributes(e);
                    current_program = Some(SimpleXmltvProgram {
                        channel: attrs.get("channel").cloned().unwrap_or_default(),
                        start: attrs.get("start").cloned().unwrap_or_default(),
                        stop: attrs.get("stop").cloned(),
                        title: None,
                        description: None,
                        category: None,
                        language: None,
                        icon: None,
                    });
                }
                
                current_element_stack.push(name);
                current_text.clear();
            }
            
            Ok(Event::End(ref e)) => {
                let name = std::str::from_utf8(e.name().as_ref())
                    .map_err(|e| AppError::source_error(format!("Invalid UTF-8 in XML element name: {e}")))?
                    .to_string();
                
                // Process the element we're closing
                if let Some(ref mut program) = current_program {
                    match name.as_str() {
                        "title" => {
                            if !current_text.trim().is_empty() {
                                program.title = Some(current_text.trim().to_string());
                            }
                        }
                        "desc" => {
                            if !current_text.trim().is_empty() {
                                program.description = Some(current_text.trim().to_string());
                            }
                        }
                        "category" => {
                            if !current_text.trim().is_empty() {
                                program.category = Some(current_text.trim().to_string());
                            }
                        }
                        "language" => {
                            if !current_text.trim().is_empty() {
                                program.language = Some(current_text.trim().to_string());
                            }
                        }
                        "programme" => {
                            // End of programme - add to results
                            if let Some(program) = current_program.take() {
                                programs.push(program);
                            }
                        }
                        _ => {}
                    }
                }
                
                current_element_stack.pop();
                current_text.clear();
            }
            
            Ok(Event::Empty(ref e)) => {
                let name = std::str::from_utf8(e.name().as_ref())
                    .map_err(|e| AppError::source_error(format!("Invalid UTF-8 in XML element name: {e}")))?
                    .to_string();
                
                // Handle self-closing elements
                if let Some(ref mut program) = current_program {
                    if name.as_str() == "icon" {
                        let attrs = parse_attributes(e);
                        if let Some(src) = attrs.get("src") {
                            program.icon = Some(src.clone());
                        }
                    }
                }
            }
            
            Ok(Event::Text(e)) => {
                let text = std::str::from_utf8(&e)
                    .map_err(|e| AppError::source_error(format!("Invalid UTF-8 in text: {e}")))?;
                current_text.push_str(text);
            }
            
            Ok(Event::CData(e)) => {
                let text = std::str::from_utf8(&e)
                    .map_err(|e| AppError::source_error(format!("Invalid UTF-8 in CDATA: {e}")))?;
                current_text.push_str(text);
            }
            
            Ok(Event::Eof) => break,
            
            Err(e) => {
                return Err(AppError::source_error(format!("XML parsing error: {e}")));
            }
            
            _ => {} // Ignore other events (comments, processing instructions, etc.)
        }
    }
    
    Ok(programs)
}

/// Parse XML attributes into a HashMap
fn parse_attributes(element: &BytesStart) -> HashMap<String, String> {
    let mut attrs = HashMap::new();
    
    for attr in element.attributes().flatten() {
        if let (Ok(key), Ok(value)) = (
            std::str::from_utf8(attr.key.as_ref()),
            std::str::from_utf8(&attr.value)
        ) {
            attrs.insert(key.to_string(), value.to_string());
        }
    }
    attrs
}