//! SQLite storage for trade decisions

use anyhow::{Context, Result};
use bonbo_llm_types::journal::TradeDecision;
use bonbo_llm_types::signal::TradingSignal;
use chrono::Utc;
use rusqlite::params;
use tracing::{debug, info};

use crate::{JournalConfig, StoreResult};

/// Structured decision journal backed by SQLite
pub struct DecisionJournal {
    conn: rusqlite::Connection,
}

impl DecisionJournal {
    /// Open or create the journal database
    pub fn open(config: &JournalConfig) -> Result<Self> {
        // Ensure parent directory exists
        if let Some(parent) = std::path::Path::new(&config.db_path).parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("Creating journal directory: {}", parent.display()))?;
        }

        let conn = rusqlite::Connection::open(&config.db_path)
            .with_context(|| format!("Opening journal database: {}", config.db_path))?;

        let journal = Self { conn };

        if config.auto_migrate {
            journal.migrate()?;
        }

        Ok(journal)
    }

    /// Open in-memory (for testing)
    pub fn open_in_memory() -> Result<Self> {
        let conn = rusqlite::Connection::open_in_memory()?;
        let journal = Self { conn };
        journal.migrate()?;
        Ok(journal)
    }

    /// Run database migrations
    fn migrate(&self) -> Result<()> {
        info!("Running decision journal migrations...");
        self.conn
            .execute_batch(
                "
CREATE TABLE IF NOT EXISTS decisions (
    decision_id      TEXT PRIMARY KEY,
    ticker           TEXT NOT NULL,
    action           TEXT NOT NULL,
    rating           TEXT NOT NULL,
    confidence       TEXT NOT NULL,
    reasoning        TEXT NOT NULL,
    entry_price      TEXT NOT NULL,
    stop_loss        TEXT NOT NULL,
    position_size    TEXT NOT NULL,
    position_size_pct TEXT NOT NULL,
    risk_reward_ratio TEXT NOT NULL,
    risk_level       TEXT NOT NULL,
    debate_rounds    INTEGER NOT NULL DEFAULT 0,
    regime           TEXT NOT NULL DEFAULT '',
    strategy         TEXT NOT NULL DEFAULT '',
    agent_votes      TEXT NOT NULL DEFAULT '[]',
    take_profits     TEXT NOT NULL DEFAULT '[]',
    market_context   TEXT NOT NULL DEFAULT '{}',
    guard_result     TEXT NOT NULL DEFAULT '{}',
    outcome          TEXT,
    reflection       TEXT,
    metadata         TEXT NOT NULL DEFAULT '{}',
    created_at       TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at       TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE INDEX IF NOT EXISTS idx_decisions_ticker ON decisions(ticker);
CREATE INDEX IF NOT EXISTS idx_decisions_action ON decisions(action);
CREATE INDEX IF NOT EXISTS idx_decisions_created_at ON decisions(created_at);
CREATE INDEX IF NOT EXISTS idx_decisions_regime ON decisions(regime);
CREATE INDEX IF NOT EXISTS idx_decisions_strategy ON decisions(strategy);

CREATE TABLE IF NOT EXISTS signals (
    signal_id        TEXT PRIMARY KEY,
    ticker           TEXT NOT NULL,
    direction        TEXT NOT NULL,
    rating           TEXT NOT NULL,
    confidence       TEXT NOT NULL,
    entry_price      TEXT,
    stop_loss        TEXT,
    take_profits     TEXT NOT NULL DEFAULT '[]',
    risk_reward_ratio TEXT,
    position_size_fraction TEXT NOT NULL,
    reasoning        TEXT NOT NULL,
    source           TEXT NOT NULL,
    debate_rounds    INTEGER NOT NULL DEFAULT 0,
    metadata         TEXT NOT NULL DEFAULT '{}',
    created_at       TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE INDEX IF NOT EXISTS idx_signals_ticker ON signals(ticker);
CREATE INDEX IF NOT EXISTS idx_signals_created_at ON signals(created_at);

CREATE TABLE IF NOT EXISTS reflections (
    id               INTEGER PRIMARY KEY AUTOINCREMENT,
    decision_id      TEXT NOT NULL REFERENCES decisions(decision_id),
    what_went_well   TEXT NOT NULL DEFAULT '[]',
    improvements     TEXT NOT NULL DEFAULT '[]',
    lesson           TEXT NOT NULL DEFAULT '',
    analogous_situations TEXT NOT NULL DEFAULT '[]',
    would_repeat     INTEGER NOT NULL DEFAULT 0,
    reflection_confidence TEXT NOT NULL DEFAULT '0',
    reflection_source TEXT NOT NULL DEFAULT 'rule_based',
    created_at       TEXT NOT NULL DEFAULT (datetime('now')),
    FOREIGN KEY (decision_id) REFERENCES decisions(decision_id)
);

CREATE INDEX IF NOT EXISTS idx_reflections_decision_id ON reflections(decision_id);
",
            )
            .context("Running journal migrations")?;

        info!("Decision journal migrations complete");
        Ok(())
    }

    /// Store a trade decision
    pub fn store_decision(&self, decision: &TradeDecision) -> Result<StoreResult> {
        let now = Utc::now();
        self.conn
            .execute(
                "INSERT OR REPLACE INTO decisions (
                    decision_id, ticker, action, rating, confidence, reasoning,
                    entry_price, stop_loss, position_size, position_size_pct,
                    risk_reward_ratio, risk_level, debate_rounds, regime, strategy,
                    agent_votes, take_profits, market_context, guard_result,
                    outcome, reflection, metadata, created_at, updated_at
                ) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19,?20,?21,?22,?23,?24)",
                params![
                    decision.decision_id,
                    decision.ticker,
                    serde_json::to_string(&decision.action)?,
                    serde_json::to_string(&decision.rating)?,
                    decision.confidence.to_string(),
                    decision.reasoning,
                    decision.entry_price.to_string(),
                    decision.stop_loss.to_string(),
                    decision.position_size.to_string(),
                    decision.position_size_pct.to_string(),
                    decision.risk_reward_ratio.to_string(),
                    serde_json::to_string(&decision.risk_level)?,
                    decision.debate_rounds,
                    decision.regime,
                    decision.strategy,
                    serde_json::to_string(&decision.agent_votes)?,
                    serde_json::to_string(&decision.take_profits)?,
                    serde_json::to_string(&decision.market_context)?,
                    serde_json::to_string(&decision.guard_result)?,
                    decision.outcome.as_ref().map(|o| serde_json::to_string(o)).transpose()?,
                    decision.reflection.as_ref().map(|r| serde_json::to_string(r)).transpose()?,
                    serde_json::json!({}).to_string(),
                    now.to_rfc3339(),
                    now.to_rfc3339(),
                ],
            )
            .with_context(|| format!("Storing decision {}", decision.decision_id))?;

        debug!("Stored decision {} for {}", decision.decision_id, decision.ticker);
        Ok(StoreResult {
            decision_id: decision.decision_id.clone(),
            stored_at: now,
        })
    }

    /// Store a trading signal
    pub fn store_signal(&self, signal: &TradingSignal) -> Result<StoreResult> {
        let now = Utc::now();
        self.conn
            .execute(
                "INSERT OR REPLACE INTO signals (
                    signal_id, ticker, direction, rating, confidence,
                    entry_price, stop_loss, take_profits, risk_reward_ratio,
                    position_size_fraction, reasoning, source, debate_rounds,
                    metadata, created_at
                ) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15)",
                params![
                    signal.signal_id,
                    signal.ticker,
                    serde_json::to_string(&signal.direction)?,
                    serde_json::to_string(&signal.rating)?,
                    signal.confidence.to_string(),
                    signal.entry_price.map(|p| p.to_string()),
                    signal.stop_loss.map(|p| p.to_string()),
                    serde_json::to_string(&signal.take_profits)?,
                    signal.risk_reward_ratio.map(|r| r.to_string()),
                    signal.position_size_fraction.to_string(),
                    signal.reasoning,
                    serde_json::to_string(&signal.source)?,
                    signal.debate_rounds,
                    serde_json::to_string(&signal.metadata)?,
                    now.to_rfc3339(),
                ],
            )
            .with_context(|| format!("Storing signal {}", signal.signal_id))?;

        Ok(StoreResult {
            decision_id: signal.signal_id.clone(),
            stored_at: now,
        })
    }

    /// Update a decision with trade outcome
    pub fn update_outcome(
        &self,
        decision_id: &str,
        outcome: &bonbo_llm_types::journal::TradeOutcome,
    ) -> Result<()> {
        self.conn
            .execute(
                "UPDATE decisions SET outcome = ?1, updated_at = ?2 WHERE decision_id = ?3",
                params![
                    serde_json::to_string(outcome)?,
                    Utc::now().to_rfc3339(),
                    decision_id,
                ],
            )
            .with_context(|| format!("Updating outcome for decision {}", decision_id))?;
        Ok(())
    }

    /// Add a reflection to a decision
    pub fn add_reflection(
        &self,
        decision_id: &str,
        reflection: &bonbo_llm_types::journal::TradeReflection,
    ) -> Result<()> {
        // Update the decision's reflection field
        self.conn
            .execute(
                "UPDATE decisions SET reflection = ?1, updated_at = ?2 WHERE decision_id = ?3",
                params![
                    serde_json::to_string(reflection)?,
                    Utc::now().to_rfc3339(),
                    decision_id,
                ],
            )
            .with_context(|| format!("Updating reflection for decision {}", decision_id))?;

        // Also store in reflections table for querying
        self.conn
            .execute(
                "INSERT INTO reflections (
                    decision_id, what_went_well, improvements, lesson,
                    analogous_situations, would_repeat, reflection_confidence,
                    reflection_source, created_at
                ) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9)",
                params![
                    decision_id,
                    serde_json::to_string(&reflection.what_went_well)?,
                    serde_json::to_string(&reflection.improvements)?,
                    reflection.lesson,
                    serde_json::to_string(&reflection.analogous_situations)?,
                    reflection.would_repeat as i32,
                    reflection.reflection_confidence.to_string(),
                    serde_json::to_string(&reflection.reflection_source)?,
                    Utc::now().to_rfc3339(),
                ],
            )
            .with_context(|| format!("Storing reflection for decision {}", decision_id))?;

        Ok(())
    }

    /// Get underlying connection (for advanced queries)
    pub fn connection(&self) -> &rusqlite::Connection {
        &self.conn
    }
}
