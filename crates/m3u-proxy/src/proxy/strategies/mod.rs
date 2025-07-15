//! Implementation of various stage strategies for different memory conditions

pub mod source_loading;
pub mod data_mapping;
pub mod filtering;
pub mod channel_numbering;

// Re-export commonly used strategies
pub use source_loading::*;
pub use data_mapping::*;
pub use filtering::*;
pub use channel_numbering::*;