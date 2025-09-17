use crate::core::http::RetryableClient;
use crate::database::CostEventBuilder;
use crate::database::DatabaseService;
use crate::database::SessionContext;
use crate::llm::LLMOrchestrator;
use crate::llm::LLMProvider;
use crate::llm::{LLMError, Query};
use async_trait::async_trait;
use serde_json::json;
use std::sync::Arc;
use tracing::{error, info};

pub struct Groq {
    system_prompt: String,
    api_key: String,
    client: RetryableClient,
    database: Arc<DatabaseService>,
}

#[async_trait]
impl LLMProvider for Groq {
    async fn try_parse(
        &self,
        query: &str,
        context: &SessionContext,
        llm_orchestrator: &LLMOrchestrator,
    ) -> Result<Query, LLMError> {
        let mut parse_retry_attempted = false;

        // Try once with potential parse retry
        loop {
            let query_text = if parse_retry_attempted {
                format!("Your previous response was not as per input schema. Return ONLY valid tool call with input matching the exact input schema. Original query: {}", query)
            } else {
                query.to_string()
            };

            match self.make_api_request(&query_text, context).await {
                Ok(response) => match llm_orchestrator
                    .parse_response_with_multistep(&response, query, context)
                    .await
                {
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
}

impl Groq {
    pub fn new(system_prompt: &str, api_key: &str, database: Arc<DatabaseService>) -> Self {
        let client = RetryableClient::new();
        Self {
            system_prompt: system_prompt.to_string(),
            api_key: api_key.to_string(),
            database,
            client,
        }
    }

    fn get_groq_tool_definitions(&self) -> serde_json::Value {
        let claude_tools = LLMOrchestrator::get_tool_definitions();
        let mut groq_tools = Vec::new();

        for tool in claude_tools.as_array().unwrap() {
            let groq_tool = json!({
                "type": "function",
                "function": {
                    "name": tool["name"],
                    "description": tool["description"],
                    "parameters": tool["input_schema"]
                }
            });
            groq_tools.push(groq_tool);
        }

        json!(groq_tools)
    }

    async fn make_api_request(
        &self,
        query: &str,
        context: &SessionContext,
    ) -> Result<serde_json::Value, LLMError> {
        info!("Attempting Groq API call");

        let tools = self.get_groq_tool_definitions();
        //info!(tools = ?tools, "Groq tool definitions");

        let response = self
            .client
            .execute_with_retry(
                self.client
                    .post("https://api.groq.com/openai/v1/chat/completions")
                    .header("Authorization", format!("Bearer {}", self.api_key))
                    .header("Content-Type", "application/json")
                    .json(&json!({
                        "model": "moonshotai/kimi-k2-instruct",
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
                        "tools": tools,
                        "tool_choice": "required",
                        "temperature": 0.0,
                        "max_completion_tokens": 8192
                    })),
            )
            .await
            .map_err(|e| LLMError::GroqError(e.to_string()))?;

        let json_response: serde_json::Value = response
            .json()
            .await
            .map_err(|e| LLMError::GroqError(e.to_string()))?;

        info!(json_response = ?json_response, "Raw groq response ");

        // Check for tool_use_failed errors and convert to ParseError for retry
        if let Some(error) = json_response.get("error") {
            if let Some(code) = error.get("code").and_then(|c| c.as_str()) {
                if code == "tool_use_failed" {
                    error!("Groq tool call validation failed, returning ParseError for retry");
                    return Err(LLMError::ParseError);
                }
            }
        }

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

        // Log costs first
        let input_cost = (prompt_tokens as f64 * rates.input_token) / 1_000_000.0;
        let output_cost = (completion_tokens as f64 * rates.output_token) / 1_000_000.0;
        let total_cost = input_cost + output_cost;

        let metadata = serde_json::json!({
            "model": "moonshotai/kimi-k2-instruct",
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

        // Extract response from Groq's format
        if let Some(choices) = json_response.get("choices").and_then(|c| c.as_array()) {
            if let Some(first_choice) = choices.first() {
                if let Some(message) = first_choice.get("message") {
                    // Check for tool calls first
                    if let Some(tool_calls) = message.get("tool_calls").and_then(|tc| tc.as_array())
                    {
                        if let Some(tool_call) = tool_calls.first() {
                            // Return tool call in Claude-compatible format
                            return Ok(json!({
                                "content": [{
                                    "type": "tool_use",
                                    "name": tool_call["function"]["name"],
                                    "input": serde_json::from_str::<serde_json::Value>(
                                        tool_call["function"]["arguments"].as_str().unwrap_or("{}")
                                    ).unwrap_or(json!({}))
                                }]
                            }));
                        }
                    }
                    // Fallback to text content if no tool calls
                    if let Some(content) = message.get("content").and_then(|c| c.as_str()) {
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
