# BonBoExtend — Kiến trúc Plugin System

## 1. Vấn đề hiện tại

```
┌─────────────────────────────────────────────────┐
│              BonBoTrade (HIỆN TẠI)              │
│                                                  │
│  ┌──────────────────────────────────────────┐   │
│  │  BonBo Core (COPY - v2.3.0)             │   │
│  │  ├── src/tools/definitions.rs            │   │
│  │  ├── src/tools/executor.rs               │   │
│  │  ├── src/client/                         │   │
│  │  ├── src/telegram/                       │   │
│  │  └── ... 100+ files                      │   │
│  └──────────────────────────────────────────┘   │
│  +                                              │
│  ┌──────────────────────────────────────────┐   │
│  │  bonbo-trade/ (NEW CODE)                 │   │
│  │  ├── exchange/                           │   │
│  │  ├── strategy/                           │   │
│  │  └── risk/                               │   │
│  └──────────────────────────────────────────┘   │
│                                                  │
│  ❌ Khi BonBo core update → phải merge thủ công │
│  ❌ 100+ files trùng lặp                        │
│  ❌ Conflict khi git merge                      │
└─────────────────────────────────────────────────┘
```

## 2. Kiến trúc mới — Plugin System

```
┌─────────────────────────────────────────────────────────┐
│                  BonBoExtend (MỚI)                       │
│                                                          │
│  ┌────────────────────────────────────────────────────┐ │
│  │  bonbo-core/ → Git submodule / path dependency    │ │
│  │  (tham chiếu đến ~/bonbo/bonbo-rust)              │ │
│  │  ⬆️ git pull origin main → nâng cấp dễ dàng       │ │
│  └────────────────────────────────────────────────────┘ │
│                                                          │
│  ┌────────────────────────────────────────────────────┐ │
│  │  bonbo-extend/ (CRATE MỚI - Plugin Layer)         │ │
│  │  ├── src/plugin.rs          — Plugin trait         │ │
│  │  ├── src/registry.rs        — Plugin manager       │ │
│  │  ├── src/tools/             — Custom tools mới     │ │
│  │  │   ├── mod.rs                                    │ │
│  │  │   ├── trade_tools.rs     — Trading tools        │ │
│  │  │   ├── market_tools.rs    — Market data tools    │ │
│  │  │   └── notification_tools.rs — Alert tools       │ │
│  │  ├── src/services/          — Background services  │ │
│  │  │   ├── price_monitor.rs   — Real-time prices     │ │
│  │  │   ├── alert_engine.rs    — Alert system         │ │
│  │  │   └── scheduler.rs       — Task scheduler       │ │
│  │  └── src/hooks/             — Lifecycle hooks      │ │
│  │       └── mod.rs                                   │ │
│  └────────────────────────────────────────────────────┘ │
│                                                          │
│  ┌────────────────────────────────────────────────────┐ │
│  │  bonbo-extend-cli/ (BINARY MỚI)                   │ │
│  │  → Chạy: bonbo-extend                              │ │
│  │  → Khởi động BonBo core + load plugins            │ │
│  │  → Có thể dùng BonBo core binary + IPC             │ │
│  └────────────────────────────────────────────────────┘ │
└─────────────────────────────────────────────────────────┘
```

## 3. Cơ chế tách biệt — 3 Options

### Option A: Git Submodule + Workspace (RECOMMENDED)
```
BonBoExtend/
├── Cargo.toml              # Workspace root
├── bonbo-core/             # git submodule → ~/bonbo/bonbo-rust
├── bonbo-extend/           # crate mới - plugins
├── bonbo-trade/            # migrated from BonBoTrade
└── bonbo-extend-cli/       # binary wrapper
```

**Ưu điểm:**
- `git submodule update` → nâng cấp BonBo core tự động
- BonBo core là read-only, không sửa gì
- Compile 1 lần, tất cả optimize cùng nhau
- Type-safe: dùng trực tiếp Rust types từ core

**Nhược điểm:**
- Cần refactor BonBo core để expose plugin API
- Compile time lâu hơn (nhưng chỉ lần đầu)

### Option B: FFI / IPC Integration
```
BonBoExtend/
├── bonbo-core/             # pre-built binary
├── bonbo-extend/           # standalone crate
└── bonbo-extend-cli/       # runs bonbo as subprocess
```

**Ưu điểm:**
- Không cần sửa BonBo core
- Tách biệt hoàn toàn

**Nhược điểm:**
- IPC overhead
- Không type-safe
- Phức tạp hơn

### Option C: MCP Server (Simplest First Step)
```
BonBoExtend/
├── src/
│   ├── main.rs             # MCP server
│   └── tools/              # MCP tools
└── Cargo.toml
```

**Ưu điểm:**
- BonBo đã có MCP support
- Không cần sửa core
- Nâng cấp core hoàn toàn độc lập

**Nhược điểm:**
- Giới hạn ở tool-level integration
- Không thể extend UI, commands

## 4. RECOMMENDATION: Hybrid Approach (A + C)

**Phase 1 (Quick Win):** MCP Server → tools mới chạy độc lập
**Phase 2 (Deep Integration):** Git Submodule → full plugin system

```
Phase 1: BonBoExtend MCP Server
┌──────────────────┐    MCP Protocol    ┌──────────────────┐
│   BonBo Core     │◄──────────────────►│  BonBoExtend     │
│   (AI Engine)    │    stdio/SSE        │  (MCP Server)    │
│                  │                     │  - trade tools    │
│                  │                     │  - market tools   │
│                  │                     │  - alert tools    │
└──────────────────┘                     └──────────────────┘
  upgrade độc lập                       upgrade độc lập

Phase 2: Plugin System
┌──────────────────────────────────────────────────────┐
│  bonbo-extend-cli                                     │
│  ├── BonBo Core (submodule, read-only)               │
│  ├── bonbo-extend (plugin crate)                     │
│  │   └── implements BonBo Tool trait                 │
│  └── bonbo-trade (domain crate)                      │
└──────────────────────────────────────────────────────┘
```
