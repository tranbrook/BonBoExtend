# BonBo Extend — Upgrade & Integration Guide

## 🔄 Nâng cấp BonBo Core (Không ảnh hưởng Extend)

### Cách 1: Script tự động (recommended)
```bash
cd ~/BonBoExtend
./scripts/upgrade-core.sh          # Pull + build
./scripts/upgrade-core.sh --install # Pull + build + install
```

### Cách 2: Thủ công
```bash
# 1. Cập nhật BonBo Core
cd ~/bonbo/bonbo-rust
git pull origin main
cargo build --release

# 2. Cài đặt binary mới
sudo cp target/release/bonbo /usr/local/bin/bonbo

# 3. Rebuild BonBo Extend (nếu cần)
cd ~/BonBoExtend
cargo build --release
```

### Cách 3: Git Submodule (nếu dùng submodule)
```bash
cd ~/BonBoExtend
git submodule update --remote bonbo-core
cargo build --release
```

## 🔗 Tích hợp với BonBo

### Phase 1: MCP Server (Không cần sửa BonBo Core)

**Bước 1: Build MCP Server**
```bash
cd ~/BonBoExtend
cargo build --release -p bonbo-extend-mcp
```

**Bước 2: Cấu hình BonBo MCP Client**

Tạo hoặc sửa file `~/.bonbo/mcp-config.json`:
```json
{
  "mcpServers": {
    "bonbo-extend": {
      "command": "/path/to/BonBoExtend/target/release/bonbo-extend-mcp",
      "args": [],
      "env": {}
    }
  }
}
```

**Bước 3: Khởi động BonBo**
```bash
bonbo
# BonBo sẽ tự động discover MCP tools
# 10 tools mới sẽ sẵn sàng:
# - get_crypto_price
# - get_crypto_candles
# - get_crypto_orderbook
# - get_top_crypto
# - create_price_alert
# - list_price_alerts
# - delete_price_alert
# - system_status
# - check_port
# - disk_usage
```

### Phase 2: Plugin System (Deep Integration)

**Yêu cầu:** BonBo Core cần expose Plugin API

```toml
# Trong BonBoExtend/bonbo-extend/Cargo.toml
[dependencies]
# BonBo core as path dependency (read-only!)
bonbo = { path = "../../bonbo/bonbo-rust" }
```

## 📦 Thêm Plugin mới

### Tạo Custom Plugin

```rust
// 1. Tạo file mới: bonbo-extend/src/tools/my_plugin.rs
use async_trait::async_trait;
use crate::plugin::*;

pub struct MyPlugin;

#[async_trait]
impl ToolPlugin for MyPlugin {
    fn metadata(&self) -> &PluginMetadata {
        // ...
    }

    fn tools(&self) -> Vec<ToolSchema> {
        vec![ToolSchema {
            name: "my_custom_tool".to_string(),
            description: "My custom tool description".to_string(),
            parameters: vec![/* ... */],
        }]
    }

    async fn execute_tool(
        &self,
        tool_name: &str,
        arguments: &serde_json::Value,
        context: &PluginContext,
    ) -> anyhow::Result<String> {
        match tool_name {
            "my_custom_tool" => Ok("Result".to_string()),
            _ => Err(anyhow::anyhow!("Unknown tool")),
        }
    }
}
```

```rust
// 2. Đăng ký trong bonbo-extend/src/tools/mod.rs
pub mod my_plugin;
pub use my_plugin::MyPlugin;
```

```rust
// 3. Đăng ký trong MCP Server (bonbo-extend-mcp/src/main.rs)
registry.register_tool_plugin(MyPlugin::new())?;
```

### External Plugin (từ thư mục)

```bash
# Tạo thư mục plugins
mkdir -p ~/.bonbo/plugins

# Build plugin thành .so hoặc binary
# (sẽ hỗ trợ dynamic loading trong Phase 3)
```

## 🧪 Test

```bash
# Test plugin registry
cargo test -p bonbo-extend

# Test MCP server thủ công
echo '{"jsonrpc":"2.0","method":"initialize","id":1}' | \
  cargo run -p bonbo-extend-mcp

# List tools
echo '{"jsonrpc":"2.0","method":"tools/list","id":2}' | \
  cargo run -p bonbo-extend-mcp

# Call a tool
echo '{"jsonrpc":"2.0","method":"tools/call","params":{"name":"get_crypto_price","arguments":{"symbol":"BTCUSDT"}},"id":3}' | \
  cargo run -p bonbo-extend-mcp
```

## 📊 Kiến trúc tổng thể

```
┌──────────────────────────────────────────────────────────┐
│                     User                                  │
│                    ↓ ↓ ↓                                  │
│              ┌──────────┐                                 │
│              │  BonBo   │  ← AI Engine (upgrade riêng)    │
│              │  Core    │                                 │
│              └────┬─────┘                                 │
│                   │ MCP Protocol                          │
│              ┌────┴─────┐                                 │
│              │ BonBo    │  ← Plugin Layer (upgrade riêng) │
│              │ Extend   │                                 │
│              │ MCP Srv  │                                 │
│              └────┬─────┘                                 │
│           ┌───────┼───────┐                               │
│      ┌────┴──┐ ┌──┴────┐ ┌┴────────┐                     │
│      │Market │ │Price  │ │System   │  ← Custom Plugins    │
│      │Data   │ │Alert  │ │Monitor  │    (thêm dễ dàng)    │
│      └───────┘ └───────┘ └─────────┘                     │
│                                                           │
│  Binance API    In-Memory    Linux                        │
└──────────────────────────────────────────────────────────┘
```

## 🔑 Điểm quan trọng

1. **BonBo Core KHÔNG BAO GIỜ bị sửa** → upgrade an toàn
2. **BonBo Extend là independent** → thêm tools không ảnh hưởng core
3. **MCP Protocol** → giao tiếp chuẩn, không coupling
4. **Plugin Trait** → dễ tạo plugin mới
5. **Upgrade chỉ cần:** `git pull` + `cargo build`
