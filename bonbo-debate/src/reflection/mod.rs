//! Self-Reflection Service — TradingGroup-inspired

pub mod reflector;
pub mod patterns;

pub use reflector::TradeReflector;
pub use patterns::PatternMatcher;

use bonbo_llm_types::journal::{TradeDecision, TradeOutcome, TradeReflection};
use bonbo_llm_types::RiskLevel;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Reflection input — what the reflector needs
#[derive(Debug, Clone)]
pub struct ReflectionInput {
    pub decision: TradeDecision,
    pub outcome: TradeOutcome,
}

/// Reflection output — generated assessment
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReflectionOutput {
    pub decision_id: String,
    pub ticker: String,
    pub what_went_well: Vec<String>,
    pub improvements: Vec<String>,
    pub lesson: String,
    pub analogous_situations: Vec<String>,
    pub would_repeat: bool,
    pub confidence: rust_decimal::Decimal,
    pub similar_past_decisions: Vec<String>,
    pub timestamp: DateTime<Utc>,
}

/// Pattern match result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PatternMatch {
    pub pattern_name: String,
    pub description: String,
    pub historical_win_rate: f64,
    pub sample_size: u32,
    pub advice: String,
}
