//! Agent state machine.

use serde::{Deserialize, Serialize};

/// Agent states.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AgentState {
    /// Idle — waiting for next scan cycle.
    Idle,
    /// Scanning market for opportunities.
    Scanning,
    /// Analyzing candidates with MTF analysis.
    Analyzing,
    /// Generating trade signal.
    Signaling,
    /// Executing trade (Saga pattern).
    Executing,
    /// Monitoring open positions.
    Monitoring,
    /// Agent paused (risk limit hit, manual pause).
    Paused,
    /// Agent stopped (kill switch, shutdown).
    Stopped,
}

impl std::fmt::Display for AgentState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AgentState::Idle => write!(f, "IDLE"),
            AgentState::Scanning => write!(f, "SCANNING"),
            AgentState::Analyzing => write!(f, "ANALYZING"),
            AgentState::Signaling => write!(f, "SIGNALING"),
            AgentState::Executing => write!(f, "EXECUTING"),
            AgentState::Monitoring => write!(f, "MONITORING"),
            AgentState::Paused => write!(f, "PAUSED"),
            AgentState::Stopped => write!(f, "STOPPED"),
        }
    }
}

impl AgentState {
    /// Check if the agent can accept new trades.
    pub fn can_trade(&self) -> bool {
        matches!(self, AgentState::Idle | AgentState::Monitoring)
    }

    /// Check if the agent is in an active (non-stopped) state.
    pub fn is_active(&self) -> bool {
        !matches!(self, AgentState::Stopped)
    }

    /// Get emoji for state.
    pub fn emoji(&self) -> &str {
        match self {
            AgentState::Idle => "⏸️",
            AgentState::Scanning => "🔍",
            AgentState::Analyzing => "📊",
            AgentState::Signaling => "📡",
            AgentState::Executing => "⚡",
            AgentState::Monitoring => "👁️",
            AgentState::Paused => "⏸️",
            AgentState::Stopped => "🛑",
        }
    }
}
