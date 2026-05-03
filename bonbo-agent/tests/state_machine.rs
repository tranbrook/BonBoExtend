//! Tests for agent state machine.

use bonbo_agent::state_machine::AgentState;

#[test]
fn test_state_display() {
    assert_eq!(AgentState::Idle.to_string(), "IDLE");
    assert_eq!(AgentState::Scanning.to_string(), "SCANNING");
    assert_eq!(AgentState::Analyzing.to_string(), "ANALYZING");
    assert_eq!(AgentState::Signaling.to_string(), "SIGNALING");
    assert_eq!(AgentState::Executing.to_string(), "EXECUTING");
    assert_eq!(AgentState::Monitoring.to_string(), "MONITORING");
    assert_eq!(AgentState::Paused.to_string(), "PAUSED");
    assert_eq!(AgentState::Stopped.to_string(), "STOPPED");
}

#[test]
fn test_state_equality() {
    assert_eq!(AgentState::Idle, AgentState::Idle);
    assert_ne!(AgentState::Idle, AgentState::Scanning);
}

#[test]
fn test_state_serialization() {
    let state = AgentState::Executing;
    let json = serde_json::to_string(&state).unwrap();
    assert!(json.contains("Executing"));
    let decoded: AgentState = serde_json::from_str(&json).unwrap();
    assert_eq!(decoded, AgentState::Executing);
}

#[test]
fn test_all_states_serializable() {
    let states = [
        AgentState::Idle,
        AgentState::Scanning,
        AgentState::Analyzing,
        AgentState::Signaling,
        AgentState::Executing,
        AgentState::Monitoring,
        AgentState::Paused,
        AgentState::Stopped,
    ];
    for state in &states {
        let json = serde_json::to_string(state).unwrap();
        let back: AgentState = serde_json::from_str(&json).unwrap();
        assert_eq!(*state, back);
    }
}
