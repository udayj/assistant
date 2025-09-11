use crate::core::http::RetryableClient;
use crate::database::CostEventBuilder;
use crate::database::DatabaseService;
use crate::database::SessionContext;
use crate::query::RuntimeConfig;
use crate::quotation::{PriceOnlyRequest, QuotationRequest};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::env;
use std::fs;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use thiserror::Error;
use tracing::{error, info};

#[derive(Debug, Deserialize, Serialize)]
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
    GetStock {
        query: String,
    },
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
    database: Arc<DatabaseService>,
    runtime_config: Arc<Mutex<RuntimeConfig>>,
}

impl ClaudeAI {
    pub fn new(
        system_prompt_file: &str,
        database: Arc<DatabaseService>,
        runtime_config: Arc<Mutex<RuntimeConfig>>,
    ) -> Result<Self, LLMError> {
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
            database,
            runtime_config,
        })
    }

    pub async fn parse_query(
        &self,
        query: &str,
        context: &SessionContext,
    ) -> Result<Query, LLMError> {
        let primary_model = {
            let config = self.runtime_config.lock().unwrap();
            config.primary_llm.clone()
        };

        match primary_model.as_str() {
            "claude" => match self.try_claude(query, context).await {
                Ok(result) => Ok(result),
                Err(e) => {
                    error!("Claude failed with error: {}, trying Groq fallback", e);
                    self.try_groq(query, context).await
                }
            },
            "groq" => match self.try_groq(query, context).await {
                Ok(result) => Ok(result),
                Err(e) => {
                    error!("Groq failed with error: {}, trying Claude fallback", e);
                    self.try_claude(query, context).await
                }
            },
            _ => self.try_claude(query, context).await, // Default fallback
        }
    }

    pub async fn try_claude(
        &self,
        query: &str,
        context: &SessionContext,
    ) -> Result<Query, LLMError> {
        let mut parse_retry_attempted = false;

        // Try once with potential parse retry
        loop {
            let query_text = if parse_retry_attempted {
                format!("Your previous response failed JSON parsing. Return ONLY valid JSON matching the exact schema. Original query: {}", query)
            } else {
                query.to_string()
            };

            match self.make_api_request(&query_text, context).await {
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

    async fn make_api_request(
        &self,
        query: &str,
        context: &SessionContext,
    ) -> Result<serde_json::Value, LLMError> {
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

        // Extract exact token counts from response
        let usage = json_response.get("usage");
        let input_tokens = usage
            .and_then(|u| u.get("input_tokens"))
            .and_then(|t| t.as_i64())
            .unwrap_or(0) as i32;

        let cache_read_tokens = usage
            .and_then(|u| u.get("cache_read_input_tokens"))
            .and_then(|t| t.as_i64())
            .unwrap_or(0) as i32;

        let cache_write_tokens = usage
            .and_then(|u| u.get("cache_creation"))
            .and_then(|u| u.get("ephemeral_1h_input_tokens"))
            .and_then(|t| t.as_i64())
            .unwrap_or(0) as i32;

        let output_tokens = usage
            .and_then(|u| u.get("output_tokens"))
            .and_then(|t| t.as_i64())
            .unwrap_or(0) as i32;

        // Get rates from database

        let _ = self
            .database
            .log_claude_api_call(
                context,
                input_tokens,
                cache_read_tokens,
                cache_write_tokens,
                output_tokens,
                "claude-sonnet-4-20250514",
            )
            .await;

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
}

impl ClaudeAI {
    async fn try_groq(&self, query: &str, context: &SessionContext) -> Result<Query, LLMError> {
        match self.understand_using_groq(query, context).await {
            Ok(response) => self.parse_response(&response),
            Err(e) => {
                error!("Error:{:#?}", e.to_string());
                Err(e)
            },
        }
    }

    async fn understand_using_groq(
        &self,
        query: &str,
        context: &SessionContext,
    ) -> Result<serde_json::Value, LLMError> {
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

        let usage = json_response.get("usage");
        let prompt_tokens = usage
            .and_then(|u| u.get("prompt_tokens"))
            .and_then(|t| t.as_i64())
            .unwrap_or(0) as i32;

        let completion_tokens = usage
            .and_then(|u| u.get("completion_tokens"))
            .and_then(|t| t.as_i64())
            .unwrap_or(0) as i32;

        // Get rates from database
        let rates = self.database.get_groq_rates().await.unwrap_or_default();

        // Extract content from Groq's response format
        if let Some(choices) = json_response.get("choices").and_then(|c| c.as_array()) {
            if let Some(first_choice) = choices.first() {
                if let Some(message) = first_choice.get("message") {
                    if let Some(content) = message.get("content").and_then(|c| c.as_str()) {
                        let input_cost = (prompt_tokens as f64 * rates.input_token) / 1_000_000.0;
                        let output_cost =
                            (completion_tokens as f64 * rates.output_token) / 1_000_000.0;
                        let total_cost = input_cost + output_cost;

                        let metadata = serde_json::json!({
                            "model": "openai/gpt-oss-20b",
                            "prompt_tokens": prompt_tokens,
                            "completion_tokens": completion_tokens,
                            "input_cost": input_cost,
                            "output_cost": output_cost,
                        });

                        CostEventBuilder::new(context.clone(), "groq_api")
                            .with_cost(
                                total_cost,
                                "per_1m_tokens",
                                prompt_tokens + completion_tokens,
                            )
                            .with_metadata(metadata)
                            .log_total_cost(&self.database)
                            .await
                            .map_err(|_| LLMError::GroqError("Failed to log cost".to_string()))?;
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
