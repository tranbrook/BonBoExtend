//! BonBo Workflow Demo — Full trading agents workflow
//!
//! Demonstrates all 9 steps:
//! 1. Fetch market data (Binance)
//! 2. Technical analysis (bonbo-ta)
//! 3. Regime detection (bonbo-regime)
//! 4. Debate Engine (bonbo-debate) — Bull vs Bear → Risk
//! 5. Hard Guards check
//! 6. Decision Journal — store decision
//! 7. (Simulated) Execute trade
//! 8. Update outcome
//! 9. Self-Reflection + Episodic Memory