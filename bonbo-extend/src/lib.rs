//! BonBo Extend — Plugin framework for BonBo AI Agent.
//!
//! Provides:
//! - Pre-built tools (trading, market data, notifications) — 15 plugins
//! - Background service implementations
//! - Integration utilities (Telegram alerts, PineScript export)
//!
//! Core traits and registry are defined in `bonbo-extend-core`.

// Re-export core types so downstream users only need bonbo-extend
pub use bonbo_extend_core::{
    ExtendError, ExtendResult, ParameterSchema, PluginContext, PluginMetadata, PluginRegistry,
    ServicePlugin, ToolPlugin, ToolSchema,
};

pub mod integration;
pub mod services;
pub mod tools;
