use crate::core::http::RetryableClient;
use crate::database::DatabaseService;
use crate::database::SessionContext;
use crate::llm::LLMOrchestrator;
use crate::llm::LLMProvider;
use crate::llm::{LLMError, Query};
use async_trait::async_trait;
use serde_json::json;
use std::sync::Arc;
use std::time::Duration;
use tracing::{error, info};

pub struct Claude {
    system_prompt: String,
    api_key: String,
    client: RetryableClient,
    pub database: Arc<DatabaseService>,
}

#[async_trait]
impl LLMProvider for Claude {
    async fn try_parse(
        &self,
        query: &str,
        context: &SessionContext,
        llm_orchestrator: &LLMOrchestrator,
    ) -> Result<Query, LLMError> {
        let mut parse_retry_attempted = false;
        let mut parse_error: String = "".into();
        // Try once with potential parse retry
        loop {
            let query_text = if parse_retry_attempted {
                format!("Original query: {}\nYour response:{}\nYour previous response was not as per input schema. Return ONLY valid tool call with input matching the exact input schema.", query, parse_error)
            } else {
                query.to_string()
            };

            match self.make_api_request(&query_text, context, llm_orchestrator).await {
                Ok(response) => match llm_orchestrator
                    .parse_response_with_multistep(&response, query, context)
                    .await
                {
                    Ok(parsed_query) => return Ok(parsed_query),
                    Err(LLMError::ParseError(err)) if !parse_retry_attempted => {
                        error!("Parse error, will retry with enhanced prompt");
                        parse_retry_attempted = true;
                        parse_error = err;
                        continue;
                    }
                    Err(e) => return Err(e),
                },
                Err(LLMError::ParseError(err)) if !parse_retry_attempted => {
                    error!("API request ParseError, will retry with enhanced prompt");
                    parse_retry_attempted = true;
                    parse_error = err;
                    continue;
                }
                Err(e) => return Err(e),
            }
        }
    }
}

impl Claude {
    pub fn new(system_prompt: &str, api_key: &str, database: Arc<DatabaseService>) -> Self {
        let client = RetryableClient::new();
        Self {
            system_prompt: system_prompt.to_string(),
            api_key: api_key.to_string(),
            database,
            client,
        }
    }

    async fn make_api_request(
        &self,
        query: &str,
        context: &SessionContext,
        llm_orchestrator: &LLMOrchestrator,
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
                            }
                        ],
                        "max_tokens": 10240,
                        "tool_choice": {"type": "any"},
                        "tools": llm_orchestrator.get_tool_definitions(),
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
            } else if error_type == "invalid_request_error"
                && (error_message.contains("input_schema")
                    || error_message.contains("tool")
                    || error_message.contains("JSON schema"))
            {
                error!("Claude tool validation failed, returning ParseError for retry");
                return Err(LLMError::ParseError(
                    serde_json::to_string(&json_response)
                        .map_err(|_| LLMError::ParseError("".into()))?,
                ));
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
}
