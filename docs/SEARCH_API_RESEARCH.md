# 🔍 BÁO CÁO: CÔNG CỤ TÌM KIẾM TỐT NHẤT CHO AI AGENT — THAY THẾ SERPAPI

> **Nguồn:** 777+ sources, 6 AI agents, 21 iterations | **Confidence: 78%**

---

## 🏆 EXECUTIVE SUMMARY — KẾT LUẬN NGAY

| # | Solution | Cost | Rate Limit | Quality | Khuyến nghị |
|---|----------|------|------------|---------|-------------|
| 🥇 | **SearXNG (self-hosted)** | **$0** | **None** | Very Good | ✅ **BEST for AI agents** |
| 🥈 | **Forage (SearXNG + Tavily API)** | **$0** | **None** | Excellent | ✅ Drop-in Tavily replacement |
| 🥉 | **Serper.dev** | $0.30/1K | Unknown | Good | ✅ Cheapest commercial |
| 4 | **Brave Search API** | $3/1K | 1 QPS (free) | Very Good | ✅ Independent index |
| 5 | **Tavily** | $0 (1K/mo) | Separate limits | Best for RAG | ⚠️ Acquired by Nebius $275M |
| ❌ | **Whoogle** | — | — | **DEAD** | ❌ Non-functional since Jan 2025 |
| ❌ | **Google CSE** | $5/1K | 10K/day cap | Best | ❌ Too expensive, hard limit |
| ❌ | **Bing API** | $15/1K | Unknown | Good | ❌ 50× Serper's cost |

---

## PHẦN 1: SELF-HOSTED — GIẢI PHÁP TỐI ƯU

### 🥇 SearXNG — De Facto Standard

**Aggregate 251 search engines** (Google, Bing, DuckDuckGo, Brave, Yahoo, etc.)

**Ưu điểm:**
- ✅ **Zero marginal cost, zero rate limits**
- ✅ Docker-first: `docker run -d --name searxng -p 8080:8080 searxng/searxng`
- ✅ JSON API output (must enable in settings.yml)
- ✅ Multiple AI agent projects converge on SearXNG
- ✅ LangChain integration documented

**Cài đặt nhanh:**
```bash
# 1. Docker deploy
docker run -d --name searxng -p 127.0.0.1:8080:8080 searxng/searxng

# 2. Enable JSON output in settings.yml
# search:
#   formats:
#     - html
#     - json

# 3. Test API
curl "http://localhost:8080/search?q=rust+programming&format=json"
```

**⚠️ Lưu ý quan trọng:**
- Upstream engines (Google, Bing) có thể block → cần **proxy rotation**
- Cần `limiter.toml` cho rate governance
- uWSGI worker tuning cho production

---

### 🥈 Forage — Self-Hosted Tavily Drop-In Replacement

**Tavily-compatible API — Đổi chỉ TAVILY_BASE_URL, không sửa code!**

**Kiến trúc:**
- Queries SearXNG (web) + local SQLite FTS5 simultaneously
- Merges và re-ranks results
- Continuous scraping: 30+ sources (HN, Reddit, GitHub Trending, ProductHunt)
- 90-day data retention, Docker Compose deployment

```bash
# Deploy Forage
git clone https://github.com/Pottonwu/Forage.git
cd Forage && docker compose up -d

# Change only this in your code
TAVILY_BASE_URL=http://localhost:8000
TAVILY_API_KEY=anything  # Not checked
```

---

### 🥉 DDGS (duckduckgo-search) — Now Self-Hostable

- Renamed to "Dux Distributed Global Search"
- **NEW: FastAPI server with Docker** — self-hostable
- Free, no API key, 2.5K stars, active community
- ⚠️ Rate limiting concern in production (CrewAI issue)

---

## PHẦN 2: COMMERCIAL — KHI CẦN RELIABILITY

### 💰 Cost Comparison (500 searches/day)

| Provider | $/1K Queries | Cost/month | Free Tier |
|----------|-------------|-----------|-----------|
| **SearXNG (self-hosted)** | **$0** | **$0** | Unlimited |
| **Serper.dev** | **$0.30** | **~$4.50** | 2,500 one-time |
| **DDGS (self-hosted)** | **$0** | **$0** | Unlimited |
| ValueSERP | ~$0.50 | ~$7.50 | Unknown |
| Brave Search | $3.00 | ~$45 | 2,000/mo (1 QPS) |
| Tavily | Unknown | Unknown | 1,000/mo |
| Google CSE | $5.00 | ~$75 | 100/day |
| Bing API | $15.00 | ~$225 | Unknown |

### Serper.dev — Cheapest Commercial

- **$0.30/1K queries** — cheapest confirmed rate
- Free tier: 2,500 queries, no credit card
- 1-2 second response time
- ⚠️ Returns **raw SERP metadata only** — cần Firecrawl/BeautifulSoup cho full content

### Brave Search API — Independent Index

- **35+ billion page index** — only major independent search index
- 50M+ searches/day, <1s latency
- Free: 2,000/month, **1 QPS**
- Enterprise: Zero Data Retention (ZDR)

### Tavily — AI-Native (Acquired $275M by Nebius)

- Purpose-built for LLM: clean, ad-free, RAG-optimized
- Official Python SDK, LangChain integration
- ⚠️ Rate limits unknown, pricing page returns 429 errors
- ⚠️ Corporate dependency risk after Nebius acquisition

---

## PHẦN 3: KIẾN TRÚC ĐỀ XUẤT CHO BONBO

### Multi-Layer Search Architecture:

```
Layer 1: SearXNG (Primary — Self-hosted, unlimited)
  ├── 251 engines aggregated
  ├── JSON API
  └── Proxy rotation for resilience
  
Layer 2: Forage (Tavily-compatible wrapper)
  ├── SearXNG + SQLite FTS5 hybrid
  ├── Drop-in Tavily API replacement
  └── 30+ continuous scrape sources
  
Layer 3: Serper.dev (Fallback — $0.30/1K)
  ├── Cheapest commercial
  └── Use only when SearXNG fails

Layer 4: Brave Search (Secondary fallback — $3/1K)
  ├── Independent index
  └── Zero data retention option

Circuit Breaker:
  ├── SearXNG: 3 consecutive fails → switch to Serper
  ├── Serper: 3 fails → switch to Brave
  └── Auto-recovery: retry SearXNG every 5 minutes
```

### ⚠️ Tránh xa:
- ❌ **Whoogle** — Dead since Jan 2025 (Google requires JavaScript)
- ❌ **Google CSE** — 10K/day hard cap, $5/1K
- ❌ **Bing API** — $15/1K (50× Serper's cost)
- ❌ **Exa** — Pricing/docs entirely unknown (429 errors everywhere)

---

## PHẦN 4: RUST ECOSYSTEM

**Zero Rust crates exist** cho bất kỳ search API nào!

Workaround: `reqwest` + hand-rolled HTTP → SearXNG JSON API

```rust
// Simple SearXNG client in Rust
let client = reqwest::Client::new();
let resp = client
    .get("http://localhost:8080/search")
    .query(&[("q", "rust async programming"), ("format", "json")])
    .send()
    .await?;
let results: serde_json::Value = resp.json().await?;
```

---

> *Báo cáo bởi BonBo Deep Research — 6 agents, 777+ sources, 21 iterations*
