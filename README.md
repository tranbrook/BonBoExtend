# BonBoExtend

Hệ thống mở rộng cho BonBo AI Coding Agent, thiết kế plugin-first cho phép:
- ✅ Thêm tools mới mà không sửa BonBo core
- ✅ Nâng cấp BonBo core dễ dàng (git pull / update dependency)
- ✅ Chạy độc lập hoặc tích hợp sâu

## Kiến trúc

```
BonBoExtend/
├── Cargo.toml                 # Workspace
├── bonbo-extend/              # Core plugin crate
│   ├── src/
│   │   ├── lib.rs
│   │   ├── plugin.rs          # Plugin trait
│   │   ├── registry.rs        # Plugin manager
│   │   ├── tools/             # Custom tools
│   │   └── services/          # Background services
│   └── Cargo.toml
├── bonbo-extend-mcp/          # MCP Server (Phase 1 - Quick Start)
│   ├── src/
│   │   └── main.rs            # MCP server exposing extend tools
│   └── Cargo.toml
└── docs/
    ├── ARCHITECTURE.md         # Kiến trúc chi tiết
    └── activity.md
```

## Upgrade BonBo Core

```bash
# Cách 1: Git submodule update
cd BonBoExtend/bonbo-core && git pull origin main

# Cách 2: Path dependency (development)
# bonbo-core points to ~/bonbo/bonbo-rust
# Just rebuild: cargo build --release
```

## Sử dụng

### Phase 1: MCP Server (recommended)
```bash
# Chạy MCP server
cargo run -p bonbo-extend-mcp

# Trong BonBo, kết nối MCP server
# ~/.bonbo/mcp-config.toml
```

### Phase 2: Plugin System
```bash
# Build với plugins
cargo build --release -p bonbo-extend-cli
./target/release/bonbo-extend
```

## Development

```bash
# Test
cargo test --workspace

# Lint
cargo clippy --workspace

# Format
cargo fmt --all
```
