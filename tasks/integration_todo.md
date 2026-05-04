# BonBoExtend — TradingAgents Integration Tasks

## P0 — Foundation
- [ ] Create `bonbo-llm-types` crate — shared types (serde + protobuf)
- [ ] Implement Structured Decision Journal in Rust (rusqlite) — replace flat markdown
- [ ] Implement Rust Hard Guards wrapping all LLM outputs

## P1 — Core Integration
- [ ] Setup gRPC Signal Bridge (Tonic) — Python signals → Rust execution
- [ ] Setup MCP Server (rmcp) — expose analysis tools to LLM agents
- [ ] Port Risk Debate pattern — hybrid Rust rule-agent + LLM debator

## P2 — Enhancement
- [ ] Crypto-specific Bull/Bear debate (on-chain, whale, funding)
- [ ] Self-Reflection Service (TradingGroup-inspired)

## P3 — Advanced
- [ ] Episodic Memory with embedding-based retrieval
- [ ] Fine-tuned small model optimization

## Completed
- [x] Push v0.3.0-pre to GitHub
- [x] Deep research TradingAgents integration possibilities
