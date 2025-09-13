//! SeaORM repository implementations
//!
//! This module provides repository implementations using SeaORM that work across
//! SQLite, PostgreSQL, and MySQL databases with database-specific optimizations.

pub mod channel;
pub mod data_mapping_rule;
pub mod epg_program;
pub mod epg_source;
pub mod filter;
pub mod last_known_codec;
pub mod relay;
pub mod stream_proxy;
pub mod stream_source;
pub mod traits;

// Re-export for convenience
pub use channel::ChannelSeaOrmRepository;
pub use data_mapping_rule::DataMappingRuleSeaOrmRepository;
pub use epg_program::EpgProgramSeaOrmRepository;
pub use epg_source::EpgSourceSeaOrmRepository;
pub use filter::FilterSeaOrmRepository;
pub use last_known_codec::LastKnownCodecSeaOrmRepository;
pub use relay::RelaySeaOrmRepository;
pub use stream_proxy::StreamProxySeaOrmRepository;
pub use stream_source::StreamSourceSeaOrmRepository;
