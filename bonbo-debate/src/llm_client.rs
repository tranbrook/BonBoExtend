//! OpenAI-compatible LLM client for debate agents
//!
//! Supports OpenAI GPT-4o, GPT-4o-mini, and compatible endpoints
//! (e.g. Ollama, LM Studio, Groq, etc.)

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use tracing::{debug, info};

/// LLM provider configuration
#[derive(Debug, Clone)]
pub struct LlmConfig {
    /// API key (env OPENAI_API_KEY)
    pub api_key: String,
    /// Model name (e.g. "gpt-4o", "gpt-4o-mini")
    pub model: String,
    /// Base URL (default: "https://api.openai.com/v1")
    pub base_url: String,
    /// Max tokens for response
    pub max_tokens: u32,
    /// Temperature (0.0 = deterministic, 1.0 = creative)
    pub temperature: f32,
}

impl Default for LlmConfig {
    fn default() -> Self {
        Self {
            api_key: String::new(),
            model: "gpt-4o-mini".to_string(),
            base_url: "https://api.openai.com/v1".to_string(),
            max_tokens: 1024,
            temperature: 0.3, // Low temp for analytical tasks
        }
    }
}

impl LlmConfig {
    /// Create from environment variables
    pub fn from_env() -> Self {
        Self {
            api_key: std::env::var("OPENAI_API_KEY").unwrap_or_default(),
            model: std::env::var("OPENAI_MODEL").unwrap_or_else(|_| "gpt-4o-mini".to_string()),
            base_url: std::env::var("OPENAI_BASE_URL")
                .unwrap_or_else(|_| "https://api.openai.com/v1".to_string()),
            ..Default::default()
        }
    }

    /// Use GPT-4o
    pub fn gpt4o() -> Self {
        Self {
            model: "gpt-4o".to_string(),
            ..Self::from_env()
        }
    }

    /// Use GPT-4o-mini (cheaper, faster)
    pub fn gpt4o_mini() -> Self {
        Self {
            model: "gpt-4o-mini".to_string(),
            ..Self::from_env()
        }
    }

    /// Check if API key is configured
    pub fn is_configured(&self) -> bool {
        !self.api_key.is_empty()
    }
}

/// LLM HTTP client
pub struct LlmClient {
    client: reqwest::Client,
    config: LlmConfig,
}

impl LlmClient {
    pub fn new(config: LlmConfig) -> Self {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .expect("Failed to create HTTP client");
        Self { client, config }
    }

    /// Create from environment
    pub fn from_env() -> Self {
        Self::new(LlmConfig::from_env())
    }

    /// Send a chat completion request
    pub async fn chat(&self, system_prompt: &str, user_message: &str) -> Result<LlmResponse> {
        let url = format!("{}/chat/completions", self.config.base_url);

        let request = ChatRequest {
            model: self.config.model.clone(),
            messages: vec![
                Message {
                    role: "system".to_string(),
                    content: system_prompt.to_string(),
                },
                Message {
                    role: "user".to_string(),
                    content: user_message.to_string(),
                },
            ],
            max_tokens: self.config.max_tokens,
            temperature: self.config.temperature,
        };

        debug!("Sending LLM request to {} (model: {})", url, self.config.model);

        let response = self
            .client
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.config.api_key))
            .header("Content-Type", "application/json")
            .json(&request)
            .send()
            .await
            .with_context(|| format!("Failed to call LLM API at {}", url))?;

        let status = response.status();
        let body = response.text().await?;

        if !status.is_success() {
            anyhow::bail!("LLM API error ({}): {}", status, truncate(&body, 500));
        }

        let chat_response: ChatResponse =
            serde_json::from_str(&body).with_context(|| {
                format!("Failed to parse LLM response: {}", truncate(&body, 200))
            })?;

        let choice = chat_response
            .choices
            .into_iter()
            .next()
            .context("No response from LLM")?;

        let usage = chat_response.usage;
        info!(
            "LLM response: {} tokens (prompt: {}, completion: {})",
            usage.total_tokens, usage.prompt_tokens, usage.completion_tokens
        );

        Ok(LlmResponse {
            content: choice.message.content,
            model: chat_response.model,
            prompt_tokens: usage.prompt_tokens,
            completion_tokens: usage.completion_tokens,
        })
    }

    /// Get config reference
    pub fn config(&self) -> &LlmConfig {
        &self.config
    }
}

/// Parsed LLM response
#[derive(Debug, Clone)]
pub struct LlmResponse {
    pub content: String,
    pub model: String,
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
}

// ═══════════════════════════════════════════════════════════════════
// API Request/Response types
// ═══════════════════════════════════════════════════════════════════

#[derive(Debug, Serialize)]
struct ChatRequest {
    model: String,
    messages: Vec<Message>,
    max_tokens: u32,
    temperature: f32,
}

#[derive(Debug, Serialize, Deserialize)]
struct Message {
    role: String,
    content: String,
}

#[derive(Debug, Deserialize)]
struct ChatResponse {
    model: String,
    choices: Vec<Choice>,
    usage: Usage,
}

#[derive(Debug, Deserialize)]
struct Choice {
    message: Message,
    index: u32,
}

#[derive(Debug, Deserialize)]
struct Usage {
    prompt_tokens: u32,
    completion_tokens: u32,
    total_tokens: u32,
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        format!("{}...", &s[..max.saturating_sub(3)])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_default() {
        let config = LlmConfig::default();
        assert_eq!(config.model, "gpt-4o-mini");
        assert_eq!(config.temperature, 0.3);
    }

    #[test]
    fn test_config_from_env_no_key() {
        // Without env var, api_key should be empty
        let config = LlmConfig {
            api_key: String::new(),
            ..LlmConfig::default()
        };
        assert!(!config.is_configured());
    }

    #[test]
    fn test_serialize_chat_request() {
        let req = ChatRequest {
            model: "gpt-4o-mini".to_string(),
            messages: vec![
                Message { role: "system".to_string(), content: "You are helpful".to_string() },
                Message { role: "user".to_string(), content: "Hello".to_string() },
            ],
            max_tokens: 1024,
            temperature: 0.3,
        };
        let json = serde_json::to_string(&req).unwrap();
        assert!(json.contains("gpt-4o-mini"));
    }
}
