//! BonBo Extend — Plugin framework for BonBo AI Agent.
//!
//! Provides:
//! - `ToolPlugin` trait for creating new AI tools
//! - `PluginRegistry` for managing plugins
//! - Pre-built tools (trading, market data, notifications)
//! - Background service framework

pub mod plugin;
pub mod registry;
pub mod tools;
pub mod services;
mod error;

pub use error::{ExtendError, ExtendResult};
pub use plugin::{ToolPlugin, ServicePlugin, PluginMetadata, PluginContext};
pub use registry::PluginRegistry;
