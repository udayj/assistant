use crate::core::http::RetryableClient;
use crate::database::DatabaseService;
use crate::quotation::{PriceOnlyRequest, QuotationRequest};
use chrono::Utc;
use serde::Deserialize;
use serde_json::json;
use uuid::Uuid;
use std::env;
use std::fs;
use std::time::Duration;
use thiserror::Error;
use tracing::{error, info};
use std::sync::Arc;
use crate::database::CostEvent;

#[derive(Debug, Deserialize)]
pub enum Query {
    MetalPricing,
    GetPriceList {
        #[serde(default = "default_brand")]
        brand: String,
        keywords: Vec<String>,
    },
    GetQuotation(QuotationRequest),
    GetProformaInvoice(QuotationRequest),
    GetPricesOnly(PriceOnlyRequest),
    UnsupportedQuery,
}

fn default_brand() -> String {
    "kei".to_string()
}

#[derive(Error, Debug)]
pub enum LLMError {
    #[error("Cannot parse and deserialize llm response")]
    ParseError,
    #[error("Cannot find api key in env")]
    EnvError,
    #[error("Claude client error: {0}")]
    ClientError(String),
    #[error("System prompt construction error:{0}")]
    SystemPromptError(String),
    #[error("API overloaded")]
    OverloadedError,
    #[error("Image processing error: {0}")]
    ImageProcessingError(String),
    #[error("Groq error: {0}")]
    GroqError(String),
}

pub struct ClaudeAI {
    system_prompt: String,
    api_key: String,
    client: RetryableClient,
    groq_api_key: Option<String>,
    database: Arc<DatabaseService>
}

impl ClaudeAI {
    pub fn new(system_prompt_file: &str, database: Arc<DatabaseService>) -> Result<Self, LLMError> {
        let prompt = fs::read_to_string(system_prompt_file)
            .map_err(|e| LLMError::SystemPromptError(e.to_string()))?;

        let api_key = env::var("ANTHROPIC_API_KEY").map_err(|_| LLMError::EnvError)?;
        let groq_api_key = env::var("GROQ_API_KEY").ok();
        let client = RetryableClient::new();
        Ok(Self {
            system_prompt: prompt,
            api_key,
            client,
            groq_api_key,
            database
        })
    }

    pub async fn parse_query(&self, query: &str, user_id: Uuid, session_id: Uuid) -> Result<Query, LLMError> {
        // Try Claude first with existing logic
        match self.try_claude(query, user_id, session_id).await {
            Ok(result) => Ok(result),
            Err(e) => {
                error!("Claude failed with error: {}, trying Groq fallback", e);
                self.try_groq(query, user_id, session_id).await
            }
        }
    }

    pub async fn try_claude(&self, query: &str, user_id: Uuid, session_id: Uuid) -> Result<Query, LLMError> {
        let mut parse_retry_attempted = false;

        // Try once with potential parse retry
        loop {
            let query_text = if parse_retry_attempted {
                format!("Your previous response failed JSON parsing. Return ONLY valid JSON matching the exact schema. Original query: {}", query)
            } else {
                query.to_string()
            };

            match self.make_api_request(&query_text, user_id, session_id).await {
                Ok(response) => match self.parse_response(&response) {
                    Ok(parsed_query) => return Ok(parsed_query),
                    Err(LLMError::ParseError) if !parse_retry_attempted => {
                        error!("Parse error, will retry with enhanced prompt");
                        parse_retry_attempted = true;
                        continue;
                    }
                    Err(e) => return Err(e),
                },
                Err(e) => return Err(e),
            }
        }
    }

    async fn make_api_request(&self, query: &str, user_id: Uuid, session_id: Uuid) -> Result<serde_json::Value, LLMError> {
        info!("About to make HTTP request to Claude API");
        let response = self
            .client
            .execute_with_retry(
                self.client
                    .post("https://api.anthropic.com/v1/messages")
                    .timeout(Duration::from_secs(45))
                    .header("x-api-key", &self.api_key)
                    .header("anthropic-version", "2023-06-01")
                    .json(&json!({
                        "model": "claude-sonnet-4-20250514",
                        "temperature": 0.0,
                        "system": [
                            {
                                "type" : "text",
                                "text" : self.system_prompt.as_str(),
                                "cache_control": {
                                    "type": "ephemeral",
                                    "ttl": "1h"
                                }
                            }
                        ],
                        "max_tokens": 10240,
                        "messages": [{
                            "role": "user",
                            "content": query
                        }]
                    })),
            )
            .await
            .map_err(|e| LLMError::ClientError(e.to_string()))?;

        info!("Received HTTP response, parsing JSON...");
        let json_response: serde_json::Value = response
            .json()
            .await
            .map_err(|e| LLMError::ClientError(e.to_string()))?;

        // Check for API errors
        if let Some(error) = json_response.get("error") {
            let error_type = error
                .get("type")
                .and_then(|t| t.as_str())
                .unwrap_or("unknown");
            let error_message = error
                .get("message")
                .and_then(|m| m.as_str())
                .unwrap_or("unknown");

            return if error_type == "overloaded_error" {
                Err(LLMError::OverloadedError)
            } else {
                Err(LLMError::ClientError(format!(
                    "{}: {}",
                    error_type, error_message
                )))
            };
        }

        let input_tokens = self.estimate_tokens(query);
        let output_tokens = self.estimate_output_tokens(&json_response);
        
        let input_cost = (input_tokens as f64 * 3.0) / 1_000_000.0;
        let output_cost = (output_tokens as f64 * 15.0) / 1_000_000.0;
        let _ = self.database.log_cost_event(CostEvent {
                user_id,
                query_session_id: session_id,
                event_type: "claude_input_tokens".to_string(),
                unit_cost: 3.0,
                unit_type: "per_1m_tokens".to_string(),
                units_consumed: input_tokens,
                cost_amount: input_cost,
                metadata: Some(serde_json::json!({
                    "model": "claude-sonnet-4-20250514",
                    "token_type": "input"
                })),
                platform: "telegram".to_string(),
                created_at: Utc::now()
            }).await;

            // Log output tokens
            let _ = self.database.log_cost_event(CostEvent {
                user_id,
                query_session_id: session_id,
                event_type: "claude_output_tokens".to_string(),
                unit_cost: 15.0,
                unit_type: "per_1m_tokens".to_string(),
                units_consumed: output_tokens,
                cost_amount: output_cost,
                metadata: Some(serde_json::json!({
                    "model": "claude-sonnet-4-20250514",
                    "token_type": "output"
                })),
                platform: "telegram".to_string(),
                created_at: Utc::now()
            }).await;
        Ok(json_response)
    }

    fn parse_response(&self, response: &serde_json::Value) -> Result<Query, LLMError> {
        info!(response = ?response, "raw response ");

        let content_text = response["content"][0]["text"]
            .as_str()
            .ok_or(LLMError::ParseError)?;

        info!(content = %content_text, "content");

        let actual_query: Query = serde_json::from_str(content_text).map_err(|e| {
            info!(error = ?e, "Error parsing ");
            LLMError::ParseError
        })?;

        Ok(actual_query)
    }

    fn estimate_tokens(&self, text: &str) -> i32 {
        (text.len() / 4) as i32 // Rough estimate: 4 chars = 1 token
    }

    fn estimate_output_tokens(&self, response: &serde_json::Value) -> i32 {
        if let Some(content) = response["content"][0]["text"].as_str() {
            (content.len() / 4) as i32
        } else {
            0
        }
    }

    fn estimate_groq_output_tokens(&self, response: &serde_json::Value) -> i32 {
        if let Some(content) = response["choices"][0]["message"]["content"].as_str() {
            (content.len() / 4) as i32
        } else {
            0
        }
    }
}

impl ClaudeAI {
    async fn try_groq(&self, query: &str, user_id: Uuid, session_id: Uuid) -> Result<Query, LLMError> {
        match self.understand_using_groq(query, user_id, session_id).await {
            Ok(response) => self.parse_response(&response),
            Err(e) => Err(e),
        }
    }

    async fn understand_using_groq(&self, query: &str, user_id: Uuid, session_id: Uuid) -> Result<serde_json::Value, LLMError> {
        let groq_key = self
            .groq_api_key
            .as_ref()
            .ok_or_else(|| LLMError::GroqError("GROQ_API_KEY not found".to_string()))?;

        info!("Attempting Groq fallback");

        let response = self
            .client
            .execute_with_retry(
                self.client
                    .post("https://api.groq.com/openai/v1/chat/completions")
                    .header("Authorization", format!("Bearer {}", groq_key))
                    .header("Content-Type", "application/json")
                    .json(&json!({
                        "model": "openai/gpt-oss-20b",
                        "messages": [
                            {
                                "role": "system",
                                "content": self.system_prompt.as_str()
                            },
                            {
                                "role": "user",
                                "content": query
                            }
                        ],
                        "temperature": 0.0,
                        "max_completion_tokens": 8192,
                        "include_reasoning": false
                    })),
            )
            .await
            .map_err(|e| LLMError::GroqError(e.to_string()))?;

        let json_response: serde_json::Value = response
            .json()
            .await
            .map_err(|e| LLMError::GroqError(e.to_string()))?;

        info!(json_response = ?json_response, "Raw groq response ");
        // Extract content from Groq's response format
        if let Some(choices) = json_response.get("choices").and_then(|c| c.as_array()) {
            if let Some(first_choice) = choices.first() {
                if let Some(message) = first_choice.get("message") {
                    if let Some(content) = message.get("content").and_then(|c| c.as_str()) {
                        let input_tokens = self.estimate_tokens(query);
                        let output_tokens = self.estimate_groq_output_tokens(&json_response);
                        
                        let input_cost = (input_tokens as f64 * 0.1) / 1_000_000.0;
                        let output_cost = (output_tokens as f64 * 0.5) / 1_000_000.0;
                        
                        let _ = self.database.log_cost_event(CostEvent {
                            user_id,
                            query_session_id: session_id,
                            event_type: "groq_api".to_string(),
                            unit_cost: 0.6, // Combined rate for simplicity
                            unit_type: "per_1m_tokens".to_string(),
                            units_consumed: input_tokens + output_tokens,
                            cost_amount: input_cost + output_cost,
                            metadata: Some(serde_json::json!({
                                "model": "openai/gpt-oss-20b",
                                "fallback_call": true
                            })),
                            platform: "telegram".to_string(),
                            created_at: Utc::now()
                        }).await;
                        return Ok(json!({ "content": [{ "text": content }] }));
                    }
                }
            }
        }

        Err(LLMError::GroqError(
            "Invalid Groq response format".to_string(),
        ))
    }
}
