//! Validation report generation.

use crate::cpcv::CpcvValidator;
use crate::error::ValidationError;
use crate::models::ValidationMetrics;
use crate::walk_forward::WalkForwardValidator;

/// Generate a full validation report for a strategy.
pub struct ValidationReport;

impl ValidationReport {
    /// Generate full validation report.
    pub fn generate(
        returns: &[f64],
        number_of_trials: u32,
        skewness: f64,
        kurtosis: f64,
    ) -> Result<ValidationMetrics, ValidationError> {
        // CPCV
        let cpcv = CpcvValidator::new(6, 2, 2, 1);
        let cpcv_result = cpcv.validate(returns);

        let (cpcv_mean, cpcv_std) = match cpcv_result {
            Ok(r) => (r.mean_sharpe, r.sharpe_std),
            Err(_) => (0.0, 1.0),
        };

        // Walk-forward
        let wf = WalkForwardValidator::new(5, 2, 1);
        let wf_result = wf.validate(returns);

        let original_sharpe = match &wf_result {
            Ok(r) => r.avg_train_sharpe,
            Err(_) => 0.0,
        };

        // DSR
        let dsr = bonbo_learning::deflated_sharpe_ratio(
            original_sharpe,
            number_of_trials,
            returns.len() as u32,
            skewness,
            kurtosis,
        );

        // Haircut
        let haircut = bonbo_learning::haircut_sharpe(original_sharpe);

        // PBO (simplified — use train/test split)
        let pbo = if let Ok(wf) = &wf_result {
            if wf.avg_test_sharpe < wf.avg_train_sharpe * 0.5 {
                0.8 // Likely overfitted
            } else if wf.avg_test_sharpe > 0.0 {
                0.2 // Likely not overfitted
            } else {
                0.5 // Uncertain
            }
        } else {
            0.5
        };

        let is_significant = dsr > 0.95 && pbo < 0.3;

        // Minimum track record (simplified)
        let min_track = if original_sharpe > 0.0 {
            (2.0 / original_sharpe.powi(2)).ceil() as u32
        } else {
            100
        };

        Ok(ValidationMetrics {
            cpcv_mean_sharpe: cpcv_mean,
            cpcv_sharpe_std: cpcv_std,
            deflated_sharpe_ratio: dsr,
            dsr_p_value: 1.0 - dsr,
            number_of_trials,
            pbo,
            original_sharpe,
            haircut_sharpe: haircut,
            is_statistically_significant: is_significant,
            minimum_track_record: min_track,
        })
    }
}
