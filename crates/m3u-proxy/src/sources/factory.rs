//! Source handler factory
//!
//! This module implements the Factory Pattern for creating appropriate source handlers
//! based on source type. This design follows the Open/Closed Principle - new source
//! types can be added by extending the factory without modifying existing code.

use std::sync::Arc;

use crate::errors::AppResult;
use crate::models::{StreamSourceType, EpgSourceType};
use super::traits::{SourceHandler, FullSourceHandler, EpgSourceHandler, FullEpgSourceHandler, EpgSourceHandlerSummary};
use super::m3u::M3uSourceHandler;
use super::xtream::XtreamSourceHandler;
use super::xmltv_epg::XmltvEpgHandler;
use super::xtream_epg::XtreamEpgHandler;

/// Factory for creating source handlers
///
/// This factory encapsulates the creation logic for source handlers, allowing
/// the system to remain open for extension (new source types) while closed
/// for modification (existing code doesn't need changes).
///
/// # Examples
///
/// ```rust
/// use crate::sources::SourceHandlerFactory;
/// use crate::models::StreamSourceType;
///
/// async fn example() -> Result<(), Box<dyn std::error::Error>> {
///     // Create handler for M3U source
///     let m3u_handler = SourceHandlerFactory::create_handler(&StreamSourceType::M3u)?;
///     
///     // Create handler for Xtream source  
///     let xtream_handler = SourceHandlerFactory::create_handler(&StreamSourceType::Xtream)?;
///     
///     Ok(())
/// }
/// ```
pub struct SourceHandlerFactory;

impl SourceHandlerFactory {
    /// Create a source handler for the specified source type
    ///
    /// # Arguments
    /// * `source_type` - The type of source to create a handler for
    ///
    /// # Returns
    /// A boxed source handler that implements all relevant traits
    ///
    /// # Errors
    /// Returns an error if the source type is not supported
    pub fn create_handler(source_type: &StreamSourceType) -> AppResult<Arc<dyn FullSourceHandler>> {
        match source_type {
            StreamSourceType::M3u => {
                let handler = M3uSourceHandler::new();
                Ok(Arc::new(handler))
            }
            StreamSourceType::Xtream => {
                let handler = XtreamSourceHandler::new();
                Ok(Arc::new(handler))
            }
        }
    }

    /// Create a basic source handler (without full functionality)
    ///
    /// This method creates handlers that only implement the core SourceHandler trait,
    /// useful for validation and capability checking without full ingestion support.
    pub fn create_basic_handler(source_type: &StreamSourceType) -> AppResult<Arc<dyn SourceHandler>> {
        match source_type {
            StreamSourceType::M3u => {
                let handler = M3uSourceHandler::new();
                Ok(Arc::new(handler))
            }
            StreamSourceType::Xtream => {
                let handler = XtreamSourceHandler::new();
                Ok(Arc::new(handler))
            }
        }
    }

    /// Get all supported source types
    ///
    /// This method returns a list of all source types that have registered handlers.
    /// Useful for UI generation and capability discovery.
    pub fn get_supported_types() -> Vec<StreamSourceType> {
        vec![
            StreamSourceType::M3u,
            StreamSourceType::Xtream,
        ]
    }

    /// Check if a source type is supported
    ///
    /// # Arguments  
    /// * `source_type` - The source type to check
    ///
    /// # Returns
    /// True if the source type has a registered handler, false otherwise
    pub fn is_supported(source_type: &StreamSourceType) -> bool {
        matches!(source_type, StreamSourceType::M3u | StreamSourceType::Xtream)
    }

    /// Get handler capabilities summary for a source type
    ///
    /// This method provides information about what capabilities a handler supports
    /// without actually creating the handler instance.
    pub fn get_handler_capabilities(source_type: &StreamSourceType) -> AppResult<super::traits::SourceHandlerSummary> {
        match source_type {
            StreamSourceType::M3u => Ok(super::traits::SourceHandlerSummary {
                source_type: StreamSourceType::M3u,
                supports_ingestion: true,
                supports_health_check: true,
                supports_url_generation: true,
                supports_authentication: false,
            }),
            StreamSourceType::Xtream => Ok(super::traits::SourceHandlerSummary {
                source_type: StreamSourceType::Xtream,
                supports_ingestion: true,
                supports_health_check: true,
                supports_url_generation: true,
                supports_authentication: true,
            }),
        }
    }

    // ============================================================================
    // EPG Source Factory Methods
    // ============================================================================

    /// Create an EPG source handler for the specified EPG source type
    ///
    /// # Arguments
    /// * `epg_source_type` - The type of EPG source to create a handler for
    ///
    /// # Returns
    /// A boxed EPG source handler that implements all relevant EPG traits
    ///
    /// # Errors
    /// Returns an error if the EPG source type is not supported
    pub fn create_epg_handler(epg_source_type: &EpgSourceType) -> AppResult<Arc<dyn FullEpgSourceHandler>> {
        match epg_source_type {
            EpgSourceType::Xmltv => {
                let handler = XmltvEpgHandler::new();
                Ok(Arc::new(handler))
            }
            EpgSourceType::Xtream => {
                let handler = XtreamEpgHandler::new();
                Ok(Arc::new(handler))
            }
        }
    }

    /// Create a basic EPG source handler (without full functionality)
    ///
    /// This method creates EPG handlers that only implement the core EpgSourceHandler trait,
    /// useful for validation and capability checking without full ingestion support.
    pub fn create_basic_epg_handler(epg_source_type: &EpgSourceType) -> AppResult<Arc<dyn EpgSourceHandler>> {
        match epg_source_type {
            EpgSourceType::Xmltv => {
                let handler = XmltvEpgHandler::new();
                Ok(Arc::new(handler))
            }
            EpgSourceType::Xtream => {
                let handler = XtreamEpgHandler::new();
                Ok(Arc::new(handler))
            }
        }
    }

    /// Get all supported EPG source types
    ///
    /// This method returns a list of all EPG source types that have registered handlers.
    /// Useful for UI generation and capability discovery.
    pub fn get_supported_epg_types() -> Vec<EpgSourceType> {
        vec![
            EpgSourceType::Xmltv,
            EpgSourceType::Xtream,
        ]
    }

    /// Check if an EPG source type is supported
    ///
    /// # Arguments  
    /// * `epg_source_type` - The EPG source type to check
    ///
    /// # Returns
    /// True if the EPG source type has a registered handler, false otherwise
    pub fn is_epg_supported(epg_source_type: &EpgSourceType) -> bool {
        matches!(epg_source_type, EpgSourceType::Xmltv | EpgSourceType::Xtream)
    }

    /// Get EPG handler capabilities summary for an EPG source type
    ///
    /// This method provides information about what capabilities an EPG handler supports
    /// without actually creating the handler instance.
    pub fn get_epg_handler_capabilities(epg_source_type: &EpgSourceType) -> AppResult<EpgSourceHandlerSummary> {
        match epg_source_type {
            EpgSourceType::Xmltv => Ok(EpgSourceHandlerSummary {
                epg_source_type: EpgSourceType::Xmltv,
                supports_program_ingestion: true,
                supports_authentication: false,
            }),
            EpgSourceType::Xtream => Ok(EpgSourceHandlerSummary {
                epg_source_type: EpgSourceType::Xtream,
                supports_program_ingestion: true,
                supports_authentication: true,
            }),
        }
    }
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_factory_supports_all_types() {
        for source_type in [StreamSourceType::M3u, StreamSourceType::Xtream] {
            assert!(SourceHandlerFactory::is_supported(&source_type));
            assert!(SourceHandlerFactory::create_handler(&source_type).is_ok());
        }
    }

    #[test]
    fn test_registry_registration() {
        let mut registry = SourceHandlerRegistry::new();
        assert!(registry.create_handler(&StreamSourceType::M3u).is_err());
        
        registry.register_handler(StreamSourceType::M3u, || {
            Arc::new(M3uSourceHandler::new())
        });
        
        assert!(registry.create_handler(&StreamSourceType::M3u).is_ok());
    }
}