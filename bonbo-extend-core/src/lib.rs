//! BonBo Extend Core — Plugin framework traits and registry.
//!
//! This crate provides the foundational types for the BonBo plugin system:
//! - `ToolPlugin` trait for creating new AI tools
//! - `ServicePlugin` trait for background services
//! - `PluginRegistry` for managing plugins
//! - `PluginContext` for shared state
//!
//! This crate has **zero heavy dependencies** — it only defines traits and the registry.
//! Actual tool implementations live in `bonbo-extend` (which depends on all analysis crates).

mod error;
pub mod plugin;
pub mod registry;

pub use error::{ExtendError, ExtendResult};
pub use plugin::{
    ParameterSchema, PluginContext, PluginMetadata, ServicePlugin, ToolPlugin, ToolSchema,
};
pub use registry::PluginRegistry;
