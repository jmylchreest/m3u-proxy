#![allow(clippy::multiple_crate_versions)]
// TODO: Unify transitive dependency versions (wasi, windows-link) and remove this allow.

pub mod assets;
pub mod config;
pub mod data_mapping;
pub mod database;
pub mod entities;
pub mod errors;
pub mod expression_parser;
pub mod field_registry;

// Expression system (new DRY abstractions)
// Provides: ExpressionDomain, ParsedExpression, build_parser_for(), preprocess_expression()
// Implemented in `expression` module hierarchy (to be added).
pub mod expression;
pub mod ingestor;
pub mod job_scheduling;
pub mod logo_assets;
pub mod models;
pub mod observability;
pub mod pipeline;
pub mod proxy;
pub mod repositories;
pub mod runtime_settings;
pub mod services;
pub mod sources;
pub mod streaming;
pub mod utils;
pub mod web;
