//! Scan Scheduler — manages periodic scan schedules.

use crate::error::ScannerError;
use crate::models::*;

/// Manages scheduled scans.
pub struct ScanScheduler {
    scans: Vec<ScheduledScan>,
}

impl Default for ScanScheduler {
    fn default() -> Self {
        Self::new()
    }
}

impl ScanScheduler {
    pub fn new() -> Self {
        let mut scheduler = Self { scans: Vec::new() };
        scheduler.add_default_scans();
        scheduler
    }

    fn add_default_scans(&mut self) {
        self.scans.push(ScheduledScan {
            id: "market_scan_4h".to_string(),
            name: "Market Scan (4h)".to_string(),
            interval_hours: 4,
            config: ScanConfig::default(),
            last_run: None,
            next_run: None,
            enabled: true,
        });

        self.scans.push(ScheduledScan {
            id: "learning_review_daily".to_string(),
            name: "Learning Review (Daily)".to_string(),
            interval_hours: 24,
            config: ScanConfig {
                symbols: vec!["BTCUSDT".to_string(), "ETHUSDT".to_string()],
                min_score: 0.0,
                max_results: 20,
                include_backtest: false,
            },
            last_run: None,
            next_run: None,
            enabled: true,
        });

        self.scans.push(ScheduledScan {
            id: "strategy_discovery_weekly".to_string(),
            name: "Strategy Discovery (Weekly)".to_string(),
            interval_hours: 168, // 7 days
            config: ScanConfig::default(),
            last_run: None,
            next_run: None,
            enabled: true,
        });
    }

    /// Get all scheduled scans.
    pub fn list_scans(&self) -> &[ScheduledScan] {
        &self.scans
    }

    /// Get scans that are due for execution.
    pub fn get_due_scans(&self) -> Vec<&ScheduledScan> {
        let now = chrono::Utc::now().timestamp();
        self.scans
            .iter()
            .filter(|s| s.enabled)
            .filter(|s| {
                match s.next_run {
                    Some(next) => now >= next,
                    None => true, // Never run → due now
                }
            })
            .collect()
    }

    /// Mark a scan as completed and schedule next run.
    pub fn mark_completed(&mut self, scan_id: &str) -> Result<(), ScannerError> {
        let now = chrono::Utc::now().timestamp();
        let scan = self
            .scans
            .iter_mut()
            .find(|s| s.id == scan_id)
            .ok_or_else(|| ScannerError::Schedule(format!("Scan not found: {}", scan_id)))?;

        scan.last_run = Some(now);
        scan.next_run = Some(now + (scan.interval_hours as i64) * 3600);
        Ok(())
    }

    /// Add a custom scan schedule.
    pub fn add_scan(&mut self, scan: ScheduledScan) {
        self.scans.push(scan);
    }

    /// Toggle a scan on/off.
    pub fn toggle_scan(&mut self, scan_id: &str, enabled: bool) -> Result<(), ScannerError> {
        let scan = self
            .scans
            .iter_mut()
            .find(|s| s.id == scan_id)
            .ok_or_else(|| ScannerError::Schedule(format!("Scan not found: {}", scan_id)))?;
        scan.enabled = enabled;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_scheduler_default_scans() {
        let scheduler = ScanScheduler::new();
        let scans = scheduler.list_scans();
        assert_eq!(scans.len(), 3);
        assert!(scans.iter().any(|s| s.id == "market_scan_4h"));
    }

    #[test]
    fn test_scheduler_due_scans() {
        let scheduler = ScanScheduler::new();
        // All scans should be due initially
        let due = scheduler.get_due_scans();
        assert_eq!(due.len(), 3);
    }

    #[test]
    fn test_scheduler_mark_completed() {
        let mut scheduler = ScanScheduler::new();
        scheduler.mark_completed("market_scan_4h").unwrap();
        let scan = scheduler
            .list_scans()
            .iter()
            .find(|s| s.id == "market_scan_4h")
            .unwrap();
        assert!(scan.last_run.is_some());
        assert!(scan.next_run.is_some());
        // Should not be due anymore
        let due = scheduler.get_due_scans();
        assert!(due.iter().all(|s| s.id != "market_scan_4h"));
    }

    #[test]
    fn test_scheduler_toggle() {
        let mut scheduler = ScanScheduler::new();
        scheduler.toggle_scan("market_scan_4h", false).unwrap();
        let due = scheduler.get_due_scans();
        assert!(due.iter().all(|s| s.id != "market_scan_4h"));
    }
}
