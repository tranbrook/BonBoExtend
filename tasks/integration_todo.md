# BonBoExtend — TradingAgents Integration Tasks

## P0 — Foundation ✅ COMPLETE
- [x] Create `bonbo-llm-types` crate — shared types (serde + JSON compatible)
  - sentiment.rs, news.rs, debate.rs, signal.rs, risk.rs, journal.rs
  - Common types: TradeDirection, Timeframe, Rating (5-tier), SignalSource, RiskLevel
- [x] Implement Structured Decision Journal (bonbo-decision-journal)
  - SQLite persistence: store_decision, store_signal, update_outcome, add_reflection
  - Query helpers: count_decisions, win_rate, pending_decisions
- [x] Implement Rust Hard Guards wrapping all LLM outputs
  - HardGuardResult, GuardCheck, CircuitBreakerState, RiskAdvisory types
  - 10 guard types: MaxPositionSize, MaxDrawdown, VolatilitySpike, etc.

## P1 — Core Integration ✅ COMPLETE
- [x] Multi-agent debate engine (bonbo-debate)
  - engine.rs: DebateEngine with run_research_debate(), run_risk_debate()
  - config.rs: DebateConfig (max rounds, consensus threshold, early termination)
  - debators.rs: RuleBasedDebator with keyword-based signal extraction
  - Debator trait: async argue() + position() for Rust rule or LLM agents
  - Conservative veto: Rust rule engine can override LLM
  - 3 risk perspectives: Aggressive/Neutral/Conservative

## P2 — Enhancement ✅ COMPLETE
- [x] Crypto-specific Bull/Bear debate (on-chain, whale, funding)
  - CryptoDebateContext: derivatives_bias(), onchain_health()
  - OnChainDebator: network metrics analysis
  - WhaleDebator: exchange flow + whale transaction tracking
  - FundingDebator: funding rate + L/S ratio contrarian analysis
- [x] Self-Reflection Service (TradingGroup-inspired)
  - TradeReflector: generates lessons from trade outcomes
  - PatternMatcher: identifies 6 recurring trade patterns

## P3 — Advanced ✅ COMPLETE
- [x] Episodic Memory with keyword-based retrieval
  - Episode type with keyword extraction
  - MemoryStore: SQLite persistence with full CRUD
  - MemoryRetrieval: Jaccard-like similarity search, regime+strategy stats

## Summary
- **22 crates** in workspace
- **760 tests**, 0 failures
- **3 commits** pushed to GitHub
- v0.3.0 (P0-P3 TradingAgents integration)
