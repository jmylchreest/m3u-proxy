//! Web handlers module
//!
//! This module contains HTTP request handlers organized by domain.
//! Each handler module focuses on a specific domain area and uses
//! the service layer for business logic.

pub mod stream_sources;
pub mod epg_sources;
pub mod proxies;
pub mod channels;
pub mod epg;
pub mod health;
pub mod features;
pub mod index;
pub mod static_assets;
pub mod circuit_breaker;

// Re-export common handler utilities
pub use crate::web::utils::*;
pub use crate::web::responses::*;
pub use crate::web::extractors::*;