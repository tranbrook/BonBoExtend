# BonBoExtend — Hướng dẫn sử dụng

## 📦 Cài đặt

```bash
# Build release
cd ~/BonBoExtend
cargo build --release

# (Optional) Cài đặt vào PATH
sudo cp target/release/bonbo-extend-mcp /usr/local/bin/
```

---

## 🚀 3 Cách sử dụng

### Cách 1: Dùng trực tiếp từ BonBo (HTTP Mode) — RECOMMENDED

Bước 1: Khởi động MCP server
```bash
# Terminal 1: Chạy MCP server (HTTP mode, port 9876)
bonbo-extend-mcp --http --port 9876
```

Bước 2: Trong BonBo, gọi tools qua MCP
```bash
# Terminal 2: Test kết nối
curl -X POST http://localhost:9876/mcp \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","method":"initialize","id":1}'
```

Bước 3: Cấu hình BonBo auto-connect
```bash
# Tạo file MCP config
cat > ~/.bonbo/mcp-servers.toml << 'EOF'
[[mcp_servers]]
name = "bonbo-extend"
url = "http://localhost:9876/mcp"
auto_start = true

[mcp_servers.env]
BONBO_EXTEND_LOG = "info"
EOF
```

### Cách 2: Stdio Mode (CLI trực tiếp)

```bash
# Gọi trực tiếp qua pipe
echo '{"jsonrpc":"2.0","method":"tools/call","params":{"name":"get_crypto_price","arguments":{"symbol":"BTCUSDT"}},"id":1}' | bonbo-extend-mcp
```

### Cách 3: Systemd Service (chạy nền vĩnh viễn)

```bash
# Tạo systemd service
cat > /tmp/bonbo-extend-mcp.service << 'EOF'
[Unit]
Description=BonBo Extend MCP Server
After=network.target

[Service]
Type=simple
User=tranbrook
ExecStart=/usr/local/bin/bonbo-extend-mcp --http --port 9876
Restart=always
RestartSec=5
Environment=BONBO_EXTEND_LOG=info

[Install]
WantedBy=multi-user.target
EOF

sudo mv /tmp/bonbo-extend-mcp.service /etc/systemd/system/
sudo systemctl daemon-reload
sudo systemctl enable bonbo-extend-mcp
sudo systemctl start bonbo-extend-mcp

# Kiểm tra
sudo systemctl status bonbo-extend-mcp
```

---

## 🔧 10 Tools sẵn sàng

### 💰 Market Data (4 tools)

| Tool | Mô tả | Ví dụ |
|------|--------|-------|
| `get_crypto_price` | Giá crypto real-time | `{"symbol": "BTCUSDT"}` |
| `get_crypto_candles` | Biểu đồ nến OHLCV | `{"symbol": "ETHUSDT", "interval": "1h", "limit": 24}` |
| `get_crypto_orderbook` | Sổ lệnh mua/bán | `{"symbol": "BTCUSDT", "limit": 10}` |
| `get_top_crypto` | Top crypto theo volume | `{"limit": 10}` |

### 🔔 Price Alerts (3 tools)

| Tool | Mô tả | Ví dụ |
|------|--------|-------|
| `create_price_alert` | Tạo cảnh báo giá | `{"symbol": "BTCUSDT", "target_price": 100000, "direction": "above"}` |
| `list_price_alerts` | Xem tất cả alerts | `{}` |
| `delete_price_alert` | Xóa alert | `{"alert_id": "abc123"}` |

### 🖥️ System Monitor (3 tools)

| Tool | Mô tả | Ví dụ |
|------|--------|-------|
| `system_status` | CPU, RAM, uptime, load | `{}` |
| `check_port` | Kiểm tra port mở | `{"port": 8080, "host": "127.0.0.1"}` |
| `disk_usage` | Dung lượng ổ đĩa | `{}` |

---

## 📡 Ví dụ gọi qua HTTP

```bash
# 1. Lấy giá BTC
curl -s -X POST http://localhost:9876/mcp \
  -H "Content-Type: application/json" \
  -d '{
    "jsonrpc": "2.0",
    "method": "tools/call",
    "params": {
      "name": "get_crypto_price",
      "arguments": {"symbol": "BTCUSDT"}
    },
    "id": 1
  }'
# → 📈 **BTCUSDT** Price: $77,866.53 | 24h Change: 5.21%

# 2. Lấy nến ETH 1 giờ
curl -s -X POST http://localhost:9876/mcp \
  -H "Content-Type: application/json" \
  -d '{
    "jsonrpc": "2.0",
    "method": "tools/call",
    "params": {
      "name": "get_crypto_candles",
      "arguments": {"symbol": "ETHUSDT", "interval": "1h", "limit": 5}
    },
    "id": 2
  }'
# → 📊 **ETHUSDT Candles** (1h interval, last 5 candles)
#   | Time     | Open     | High     | Low      | Close    | Volume    |
#   |----------|----------|----------|----------|----------|-----------|
#   | 04-17 14:00 | 2445.12  | 2448.63  | 2440.50  | 2446.89  | 1234.56   |

# 3. Tạo alert khi BTC qua 100k
curl -s -X POST http://localhost:9876/mcp \
  -H "Content-Type: application/json" \
  -d '{
    "jsonrpc": "2.0",
    "method": "tools/call",
    "params": {
      "name": "create_price_alert",
      "arguments": {"symbol": "BTCUSDT", "target_price": 100000, "direction": "above"}
    },
    "id": 3
  }'
# → ✅ Alert #a1b2c3: BTCUSDT goes ABOVE $100000.00

# 4. Kiểm tra hệ thống
curl -s -X POST http://localhost:9876/mcp \
  -H "Content-Type: application/json" \
  -d '{
    "jsonrpc": "2.0",
    "method": "tools/call",
    "params": {"name": "system_status", "arguments": {}},
    "id": 4
  }'
# → 🖥️ **System Status**
#   ⏱️ Uptime: 21:55:12 up 38 min, 1 user, load average: 3.02, 1.30, 0.64
#   Mem: 5.6Gi total, 1.8Gi used, 1.5Gi free
#   ⚡ Load: 3.02 1.30 0.64
#   🔧 CPUs: 16

# 5. Top 5 crypto
curl -s -X POST http://localhost:9876/mcp \
  -H "Content-Type: application/json" \
  -d '{
    "jsonrpc": "2.0",
    "method": "tools/call",
    "params": {"name": "get_top_crypto", "arguments": {"limit": 5}},
    "id": 5
  }'
# → 🏆 **Top 5 Crypto by Volume**
#   | # | Symbol   | Price       | 24h %   | Volume          |
#   |---|----------|-------------|---------|-----------------|
#   | 1 | BTCUSDT  | $77,866     | 📈5.21% | $1,878,589,550  |
#   | 2 | ETHUSDT  | $2,448      | 📈6.09% | $1,073,872,873  |
```

---

## 🔄 Nâng cấp BonBo Core (không ảnh hưởng Extend)

```bash
# Chỉ 2 lệnh — BonBo Extend KHÔNG cần rebuild
cd ~/bonbo/bonbo-rust
git pull origin main
cargo build --release
sudo cp target/release/bonbo /usr/local/bin/
# → BonBo Core đã nâng cấp. Extend vẫn chạy bình thường.
```

---

## 🧪 Test

```bash
cd ~/BonBoExtend

# Unit tests
cargo test --workspace

# Test MCP server (stdio)
echo '{"jsonrpc":"2.0","method":"initialize","id":1}' | cargo run -p bonbo-extend-mcp

# Test MCP server (http)
cargo run -p bonbo-extend-mcp -- --http --port 9876 &
sleep 2
curl -s http://localhost:9876/mcp \
  -d '{"jsonrpc":"2.0","method":"tools/list","id":1}' \
  -H "Content-Type: application/json"
```

---

## ➕ Thêm Custom Plugin mới

### Ví dụ: Thêm "News Plugin"

**Bước 1:** Tạo file `bonbo-extend/src/tools/news.rs`:
```rust
use async_trait::async_trait;
use crate::plugin::*;

pub struct NewsPlugin { /* ... */ }

#[async_trait]
impl ToolPlugin for NewsPlugin {
    fn metadata(&self) -> &PluginMetadata { /* ... */ }
    fn tools(&self) -> Vec<ToolSchema> {
        vec![ToolSchema {
            name: "get_crypto_news".into(),
            description: "Get latest crypto news".into(),
            parameters: vec![/* ... */],
        }]
    }
    async fn execute_tool(&self, name: &str, args: &Value, ctx: &PluginContext) -> anyhow::Result<String> {
        // Fetch news from API...
        Ok("Latest crypto news...".into())
    }
}
```

**Bước 2:** Đăng ký trong `bonbo-extend/src/tools/mod.rs`:
```rust
pub mod news;
pub use news::NewsPlugin;
```

**Bước 3:** Đăng ký trong MCP server `bonbo-extend-mcp/src/main.rs`:
```rust
registry.register_tool_plugin(NewsPlugin::new())?;
```

**Bước 4:** Build & restart:
```bash
cargo build --release
# Nếu dùng systemd: sudo systemctl restart bonbo-extend-mcp
```

→ **Tool mới tự động xuất hiện cho BonBo AI.**
