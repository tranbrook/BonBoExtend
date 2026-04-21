//! BonBo Extend — Plugin framework for BonBo AI Agent.
//!
//! Provides:
//! - `ToolPlugin` trait for creating new AI tools
//! - `PluginRegistry` for managing plugins
//! - Pre-built tools (trading, market data, notifications)
//! - Background service framework

mod error;
pub mod integration;
pub mod plugin;
pub mod registry;
pub mod services;
pub mod tools;

pub use error::{ExtendError, ExtendResult};
pub use plugin::{PluginContext, PluginMetadata, ServicePlugin, ToolPlugin};
pub use registry::PluginRegistry;
