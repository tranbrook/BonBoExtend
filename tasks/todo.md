# BonBoExtend — Task List

## ✅ Phase 1: Foundation (DONE)
- [x] Phân tích kiến trúc BonBo Core hiện tại
- [x] Phân tích BonBoTrade hiện tại
- [x] Thiết kế Plugin System architecture
- [x] Tạo workspace Cargo.toml
- [x] Tạo bonbo-extend crate (Plugin trait, Registry)
- [x] Tạo bonbo-extend-mcp crate (MCP Server)
- [x] Build thành công, 0 warnings, 6/6 tests pass
- [x] Tạo docs: ARCHITECTURE.md, UPGRADE_GUIDE.md
- [x] Tạo upgrade script
- [x] Git init + commit

## 📋 Phase 2: Integration
- [ ] Cấu hình BonBo MCP client để kết nối bonbo-extend-mcp
- [ ] Test end-to-end: BonBo → MCP → Extend tools
- [ ] Migrate BonBoTrade code sang plugin architecture

## 📋 Phase 3: Advanced
- [ ] Dynamic plugin loading (từ thư mục ~/.bonbo/plugins/)
- [ ] WebSocket transport cho MCP (thêm SSE)
- [ ] Plugin hot-reload
- [ ] Auto-discovery plugins

## 📋 Phase 4: New Tools
- [ ] Trading execution plugin (dùng code từ BonBoTrade)
- [ ] Portfolio tracker plugin
- [ ] News/sentiment analysis plugin
- [ ] On-chain data plugin
