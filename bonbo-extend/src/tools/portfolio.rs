//! Portfolio Analysis Plugin — correlation, concentration, and portfolio-level risk.

use crate::plugin::{ParameterSchema, PluginContext, PluginMetadata, ToolSchema, ToolPlugin};
use async_trait::async_trait;
use serde_json::{Value, json};

/// Portfolio Analysis Plugin — evaluates portfolio-level risk.
pub struct PortfolioPlugin {
    metadata: PluginMetadata,
}

impl Default for PortfolioPlugin {
    fn default() -> Self {
        Self::new()
    }
}

impl PortfolioPlugin {
    pub fn new() -> Self {
        Self {
            metadata: PluginMetadata {
                id: "portfolio".into(),
                name: "portfolio".into(),
                version: "0.1.0".into(),
                description: "Portfolio-level risk analysis".into(),
                author: "BonBo".into(),
                tags: vec!["portfolio".into(), "risk".into()],
            },
        }
    }

    fn compute_hhi(weights: &[f64]) -> f64 {
        let total: f64 = weights.iter().sum();
        if total <= 0.0 {
            return 1.0;
        }
        weights.iter().map(|w| (w / total).powi(2)).sum()
    }

    fn rolling_correlation(a: &[f64], b: &[f64], window: usize) -> Option<f64> {
        if a.len() < window || b.len() < window || a.len() != b.len() {
            return None;
        }
        let a_w = &a[a.len() - window..];
        let b_w = &b[b.len() - window..];
        let n = a_w.len() as f64;
        let mean_a: f64 = a_w.iter().sum::<f64>() / n;
        let mean_b: f64 = b_w.iter().sum::<f64>() / n;
        let mut cov = 0.0_f64;
        let mut var_a = 0.0_f64;
        let mut var_b = 0.0_f64;
        for i in 0..a_w.len() {
            let da = a_w[i] - mean_a;
            let db = b_w[i] - mean_b;
            cov += da * db;
            var_a += da * da;
            var_b += db * db;
        }
        if var_a <= 0.0 || var_b <= 0.0 {
            return None;
        }
        Some(cov / (var_a.sqrt() * var_b.sqrt()))
    }

    fn stress_test(positions: &[(String, f64, f64)], shock_pct: f64, equity: f64) -> f64 {
        positions.iter().map(|(_, qty, entry)| qty * entry * shock_pct / 100.0).sum::<f64>() / equity * 100.0
    }

    async fn do_portfolio_analysis(&self, args: &Value) -> anyhow::Result<String> {
        let equity = args.get("equity").and_then(|v| v.as_f64())
            .ok_or_else(|| anyhow::anyhow!("equity required"))?;

        let positions_val: Vec<Value> = args.get("positions")
            .and_then(|v| v.as_array()).cloned().unwrap_or_default();

        if positions_val.is_empty() {
            return Ok("📊 No positions to analyze.".to_string());
        }

        let mut parsed: Vec<(String, f64, f64)> = Vec::new();
        for p in &positions_val {
            let sym = p["symbol"].as_str().unwrap_or("").to_string();
            let qty = p["quantity"].as_f64().unwrap_or(0.0);
            let entry = p["entry_price"].as_f64().unwrap_or(0.0);
            if !sym.is_empty() && qty > 0.0 { parsed.push((sym, qty, entry)); }
        }

        if parsed.is_empty() {
            return Ok("📊 No valid positions.".to_string());
        }

        let mut r = "📊 **Portfolio Analysis**\n\n".to_string();
        let notional: Vec<f64> = parsed.iter().map(|(_, q, e)| q * e).collect();
        let total_n: f64 = notional.iter().sum();

        // Position breakdown
        r.push_str("### Position Breakdown\n| Symbol | Notional | Weight | Leverage |\n|--------|----------|--------|----------|\n");
        for (i, (sym, _, _)) in parsed.iter().enumerate() {
            let w = if total_n > 0.0 { notional[i] / total_n * 100.0 } else { 0.0 };
            r.push_str(&format!("| {} | ${:.2} | {:.1}% | {:.1}x |\n", sym, notional[i], w, notional[i] / equity));
        }

        // HHI
        let weights: Vec<f64> = notional.iter().map(|n| *n / total_n).collect();
        let hhi = Self::compute_hhi(&weights);
        let conc = if hhi < 0.15 { "✅ Diversified" } else if hhi < 0.25 { "⚠️ Moderate" } else { "🔴 Concentrated" };
        r.push_str(&format!("\n### Concentration\n- **HHI**: {:.3} — {}\n- Total leverage: {:.1}x\n", hhi, conc, total_n / equity));

        // Correlation matrix
        let symbols: Vec<&str> = parsed.iter().map(|(s, _, _)| s.as_str()).collect();
        if symbols.len() >= 2 {
            r.push_str("\n### Correlation Matrix (30d)\n| | ");
            for s in &symbols { r.push_str(&format!("{} | ", s)); }
            r.push_str("\n|---");
            for _ in &symbols { r.push_str("|---"); }
            r.push_str("\n");

            let fetcher = bonbo_data::fetcher::MarketDataFetcher::new();
            let mut prices: Vec<Vec<f64>> = Vec::new();
            for sym in &symbols {
                let klines = fetcher.fetch_klines(sym, "1d", Some(60)).await.unwrap_or_default();
                prices.push(klines.iter().map(|k| k.close).collect());
            }

            for (i, si) in symbols.iter().enumerate() {
                r.push_str(&format!("| {} | ", si));
                for (j, _) in symbols.iter().enumerate() {
                    if i == j { r.push_str("1.00 | "); continue; }
                    if prices[i].len() >= 31 && prices[j].len() >= 31 {
                        let ml = prices[i].len().min(prices[j].len());
                        let a = &prices[i][prices[i].len()-ml..];
                        let b = &prices[j][prices[j].len()-ml..];
                        let ra: Vec<f64> = a.windows(2).map(|w| (w[1]-w[0])/w[0]).collect();
                        let rb: Vec<f64> = b.windows(2).map(|w| (w[1]-w[0])/w[0]).collect();
                        if let Some(c) = Self::rolling_correlation(&ra, &rb, 30) {
                            let e = if c > 0.7 { "🔴" } else if c > 0.4 { "🟡" } else { "🟢" };
                            r.push_str(&format!("{}{:.2} | ", e, c));
                        } else { r.push_str("— | "); }
                    } else { r.push_str("— | "); }
                }
                r.push_str("\n");
            }
        }

        // Stress tests
        r.push_str("\n### Stress Tests\n| Scenario | Portfolio Loss |\n|----------|---------------|\n");
        for shock in [-5.0, -10.0, -20.0, -30.0] {
            let loss = Self::stress_test(&parsed, shock, equity);
            let e = if loss.abs() > 50.0 { "🔴" } else if loss.abs() > 20.0 { "🟡" } else { "🟢" };
            r.push_str(&format!("| {} -{}% | **{:.1}%** loss |\n", e, shock.abs() as i32, loss));
        }

        // Recommendations
        r.push_str("\n### 💡 Recommendations\n");
        if hhi > 0.25 { r.push_str("- 🔴 **Concentrated** — diversify\n"); }
        if total_n / equity > 5.0 { r.push_str("- 🔴 **High leverage** — reduce exposure\n"); }
        if hhi < 0.15 && total_n / equity < 3.0 { r.push_str("- ✅ **Well-balanced** portfolio\n"); }

        Ok(r)
    }
}

#[async_trait]
impl ToolPlugin for PortfolioPlugin {
    fn metadata(&self) -> &PluginMetadata { &self.metadata }

    fn tools(&self) -> Vec<ToolSchema> {
        vec![ToolSchema {
            name: "analyze_portfolio".into(),
            description: "Analyze portfolio risk: correlation, concentration (HHI), stress tests".into(),
            parameters: vec![
                ParameterSchema {
                    name: "equity".into(),
                    param_type: "number".into(),
                    description: "Account equity in USDT".into(),
                    required: true,
                    default: None,
                    r#enum: None,
                },
                ParameterSchema {
                    name: "positions".into(),
                    param_type: "array".into(),
                    description: "Array of {symbol, quantity, entry_price}".into(),
                    required: true,
                    default: None,
                    r#enum: None,
                },
            ],
        }]
    }

    async fn execute_tool(
        &self,
        tool_name: &str,
        arguments: &serde_json::Value,
        _context: &PluginContext,
    ) -> anyhow::Result<String> {
        match tool_name {
            "analyze_portfolio" => self.do_portfolio_analysis(arguments).await,
            _ => anyhow::bail!("Unknown tool: {}", tool_name),
        }
    }
}
