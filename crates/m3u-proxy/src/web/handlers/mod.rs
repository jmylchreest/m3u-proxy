//! Web handlers module
//!
//! This module contains HTTP request handlers organized by domain.
//! Each handler module focuses on a specific domain area and uses
//! the service layer for business logic.

pub mod channels;
pub mod circuit_breaker;
pub mod epg;
pub mod epg_sources;
pub mod features;
pub mod health;
pub mod index;
pub mod proxies;
pub mod static_assets;
pub mod stream_sources;

// Re-export common handler utilities
pub use crate::web::extractors::*;
pub use crate::web::responses::*;
pub use crate::web::utils::*;
