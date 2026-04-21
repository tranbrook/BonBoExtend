# BonBoExtend — Knowledge Gaps Implementation

## Phase 1: WebSocket Integration Tests
- [ ] 1.1 Create integration test framework for WebSocket connections
- [ ] 1.2 Test market data stream (kline, mark price) with mock server
- [ ] 1.3 Test user data stream (account/order updates) with mock server
- [ ] 1.4 Test auto-reconnect behavior (disconnect → reconnect)
- [ ] 1.5 Test subscription management (subscribe/unsubscribe)
- [ ] 1.6 Test message parsing for all event types

## Phase 2: Connect Decision Loop to MCP Tools
- [ ] 2.1 Create McpClient trait for abstract tool calls
- [ ] 2.2 Implement scan_candidates() — calls scan_market MCP tool
- [ ] 2.3 Implement analyze_candidates() — calls analyze_indicators + get_trading_signals
- [ ] 2.4 Add regime check via detect_market_regime MCP tool
- [ ] 2.5 Add funding rate filter via funding MCP tool
- [ ] 2.6 Integration test: full decision cycle with mock MCP

## Phase 3: Refactor Executor to Trait-based Design
- [ ] 3.1 Create OrderExecutor trait (execute, cancel, query)
- [ ] 3.2 Implement LiveExecutor wrapping FuturesRestClient
- [ ] 3.3 Implement DryRunExecutor without FuturesRestClient dependency
- [ ] 3.4 Update SagaExecutor to use trait instead of concrete client
- [ ] 3.5 Update DecisionLoop to work with trait
- [ ] 3.6 Tests for both Live and DryRun paths

## Verification
- [ ] cargo test --workspace — 0 failures
- [ ] cargo clippy — 0 warnings  
- [ ] cargo build --release — clean
