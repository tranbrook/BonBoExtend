use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DmaModel {
    pub name: String,
    pub predictions: usize,
    pub correct: usize,
    pub recent_accuracy: f64,
    pub cumulative_log_score: f64,
}

impl DmaModel {
    pub fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
            predictions: 0,
            correct: 0,
            recent_accuracy: 0.5,
            cumulative_log_score: 0.0,
        }
    }

    pub fn accuracy(&self) -> f64 {
        if self.predictions == 0 {
            0.5
        } else {
            self.correct as f64 / self.predictions as f64
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WeightSnapshot {
    pub timestamp: i64,
    pub weights: Vec<(String, f64)>,
    pub trigger: String,
    pub regime: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LearningState {
    pub total_updates: u32,
    pub last_update_timestamp: i64,
    pub dma_alpha: f64,
    pub dma_lambda: f64,
    pub models: Vec<DmaModel>,
    pub weight_history: Vec<WeightSnapshot>,
}

impl Default for LearningState {
    fn default() -> Self {
        Self {
            total_updates: 0,
            last_update_timestamp: 0,
            dma_alpha: 0.99,
            dma_lambda: 0.99,
            models: Vec::new(),
            weight_history: Vec::new(),
        }
    }
}
