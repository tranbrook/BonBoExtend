use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidationMetrics {
    pub cpcv_mean_sharpe: f64,
    pub cpcv_sharpe_std: f64,
    pub deflated_sharpe_ratio: f64,
    pub dsr_p_value: f64,
    pub number_of_trials: u32,
    pub pbo: f64,
    pub original_sharpe: f64,
    pub haircut_sharpe: f64,
    pub is_statistically_significant: bool,
    pub minimum_track_record: u32,
}

impl Default for ValidationMetrics {
    fn default() -> Self {
        Self {
            cpcv_mean_sharpe: 0.0,
            cpcv_sharpe_std: 1.0,
            deflated_sharpe_ratio: 0.0,
            dsr_p_value: 1.0,
            number_of_trials: 1,
            pbo: 0.0,
            original_sharpe: 0.0,
            haircut_sharpe: 0.0,
            is_statistically_significant: false,
            minimum_track_record: 30,
        }
    }
}
