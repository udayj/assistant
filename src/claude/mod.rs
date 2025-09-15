use crate::core::http::RetryableClient;
use crate::database::CostEventBuilder;
use crate::database::DatabaseService;
use crate::database::SessionContext;
use crate::prices::price_list::{AvailablePricelists, PriceListService};
use crate::query::RuntimeConfig;
use crate::quotation::{PriceOnlyRequest, QuotationRequest};
use schemars::schema_for;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::env;
use std::fs;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use thiserror::Error;
use tracing::{error, info};

#[derive(Debug, Deserialize, Serialize)]
pub enum ToolCall {
    GetMetalPrices,
    GetStockInfo {
        query: String,
    },
    GenerateQuotation(QuotationRequest),
    GenerateProforma(QuotationRequest),
    GetPricesOnly(PriceOnlyRequest),
    FindPriceList {
        #[serde(default = "default_brand")]
        brand: String,
        keywords: Vec<String>,
    },
    ListAvailablePricelists {
        #[serde(default)]
        brand: Option<String>,
    },
}

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
    ListAvailablePricelists {
        #[serde(default)]
        brand: Option<String>,
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

#[derive(Debug, Serialize, Deserialize)]
pub enum ToolResult {
    AvailablePricelists(AvailablePricelists),
}

trait ToolExecutor {
    fn execute_tool(&self, tool_name: &str, input: &serde_json::Value) -> Option<ToolResult>;
}

pub struct ClaudeAI {
    system_prompt: String,
    api_key: String,
    client: RetryableClient,
    groq_api_key: Option<String>,
    database: Arc<DatabaseService>,
    runtime_config: Arc<Mutex<RuntimeConfig>>,
    pricelist_service: Option<Arc<PriceListService>>,
}

impl ClaudeAI {
    fn get_tool_definitions(&self) -> serde_json::Value {
        let quotation_schema = schema_for!(QuotationRequest);
        let price_only_schema = schema_for!(PriceOnlyRequest);

        json!([
            {
                "name": "get_metal_prices",
                "description": "Get current metal prices from MCX for copper and aluminum",
                "input_schema": {
                    "type": "object",
                    "properties": {},
                    "required": []
                }
            },
            {
                "name": "get_stock_info",
                "description": "Check stock availability for electrical items using Tally ERP",
                "input_schema": {
                    "type": "object",
                    "properties": {
                        "query": {
                            "type": "string",
                            "description": "Stock query string (e.g., '4 C x 2.5 2XWYL')"
                        }
                    },
                    "required": ["query"]
                }
            },
            {
                "name": "generate_quotation",
                "description": "Generate a PDF quotation for electrical items",
                "input_schema": serde_json::to_value(&quotation_schema).unwrap()
            },
            {
                "name": "generate_proforma",
                "description": "Generate a PDF proforma invoice for electrical items",
                "input_schema": serde_json::to_value(&quotation_schema).unwrap()
            },
            {
                "name": "get_prices_only",
                "description": "Get prices for electrical items without generating quotation PDF",
                "input_schema": serde_json::to_value(&price_only_schema).unwrap()
            },
            {
                "name": "find_price_list",
                "description": "Find and return PDF pricelists for specific brands and categories",
                "input_schema": {
                    "type": "object",
                    "properties": {
                        "brand": {
                            "type": "string",
                            "default": "kei",
                            "description": "Brand name (kei or polycab)"
                        },
                        "keywords": {
                            "type": "array",
                            "items": {"type": "string"},
                            "description": "Keywords to match pricelists (e.g., ['latest armoured', 'current cable'])"
                        }
                    },
                    "required": ["keywords"]
                }
            },
            {
                "name": "list_available_pricelists",
                "description": "List all available PDF pricelists with their keywords and metadata. Use this before find_price_list to see what's available.",
                "input_schema": {
                    "type": "object",
                    "properties": {
                        "brand": {
                            "type": "string",
                            "description": "Optional brand filter (kei, polycab). If not specified, shows all brands."
                        }
                    },
                    "required": []
                }
            }
        ])
    }

    fn get_groq_tool_definitions(&self) -> serde_json::Value {
        let claude_tools = self.get_tool_definitions();
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
            pricelist_service: None,
        })
    }

    pub fn set_pricelist_service(&mut self, pricelist_service: Arc<PriceListService>) {
        self.pricelist_service = Some(pricelist_service);
    }

    async fn continue_conversation_with_tool_result(
        &self,
        original_query: &str,
        tool_result: ToolResult,
        context: &SessionContext,
    ) -> Result<Query, LLMError> {
        let tool_result_text = match tool_result {
            ToolResult::AvailablePricelists(pricelists) => {
                serde_json::to_string_pretty(&pricelists)
                    .unwrap_or_else(|_| "Error serializing pricelists".to_string())
            }
        };

        let continued_query = format!(
            "Available pricelists: {}\n\nOriginal user query: {}\n\nNow use find_price_list with appropriate keywords based on the available pricelists above.",
            tool_result_text, original_query
        );

        // Continue with the same primary model preference
        let primary_model = {
            let config = self.runtime_config.lock().unwrap();
            config.primary_llm.clone()
        };

        match primary_model.as_str() {
            "claude" => Box::pin(self.try_claude(&continued_query, context)).await,
            "groq" => Box::pin(self.try_groq(&continued_query, context)).await,
            _ => Box::pin(self.try_claude(&continued_query, context)).await,
        }
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
                Ok(response) => match self
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
                        "tools": self.get_tool_definitions(),
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

    async fn parse_response_with_multistep(
        &self,
        response: &serde_json::Value,
        original_query: &str,
        context: &SessionContext,
    ) -> Result<Query, LLMError> {
        info!(response = ?response, "raw response ");

        let content_array = response["content"].as_array().ok_or(LLMError::ParseError)?;

        // Look for tool_use in content blocks
        for content_block in content_array {
            if let Some(content_type) = content_block.get("type").and_then(|t| t.as_str()) {
                if content_type == "tool_use" {
                    let tool_name = content_block["name"].as_str().ok_or(LLMError::ParseError)?;
                    let input = &content_block["input"];

                    // Check if this is an information tool that requires multi-step handling
                    if let Some(tool_result) = self.execute_tool(tool_name, input) {
                        // This is an information tool - continue conversation with result
                        return self
                            .continue_conversation_with_tool_result(
                                original_query,
                                tool_result,
                                context,
                            )
                            .await;
                    } else {
                        // This is an action tool - handle normally
                        return self.handle_tool_call(content_block);
                    }
                }
            }
        }

        // If no tool use found, it's an unsupported query
        Ok(Query::UnsupportedQuery)
    }

    fn handle_tool_call(&self, tool_content: &serde_json::Value) -> Result<Query, LLMError> {
        let tool_name = tool_content["name"].as_str().ok_or(LLMError::ParseError)?;
        let input = &tool_content["input"];

        match tool_name {
            "get_metal_prices" => Ok(Query::MetalPricing),
            "get_stock_info" => {
                let query = input["query"]
                    .as_str()
                    .ok_or(LLMError::ParseError)?
                    .to_string();
                Ok(Query::GetStock { query })
            }
            "generate_quotation" => {
                let quotation_request: QuotationRequest =
                    serde_json::from_value(input.clone()).map_err(|_| LLMError::ParseError)?;
                Ok(Query::GetQuotation(quotation_request))
            }
            "generate_proforma" => {
                let quotation_request: QuotationRequest =
                    serde_json::from_value(input.clone()).map_err(|_| LLMError::ParseError)?;
                Ok(Query::GetProformaInvoice(quotation_request))
            }
            "get_prices_only" => {
                let price_request: PriceOnlyRequest =
                    serde_json::from_value(input.clone()).map_err(|_| LLMError::ParseError)?;
                Ok(Query::GetPricesOnly(price_request))
            }
            "find_price_list" => {
                let brand = input["brand"].as_str().unwrap_or("kei").to_string();
                let keywords: Vec<String> = input["keywords"]
                    .as_array()
                    .ok_or(LLMError::ParseError)?
                    .iter()
                    .filter_map(|v| v.as_str().map(|s| s.to_string()))
                    .collect();
                Ok(Query::GetPriceList { brand, keywords })
            }
            "list_available_pricelists" => {
                let brand = input["brand"].as_str().map(|s| s.to_string());
                Ok(Query::ListAvailablePricelists { brand })
            }
            _ => Ok(Query::UnsupportedQuery),
        }
    }
}

impl ClaudeAI {
    async fn try_groq(&self, query: &str, context: &SessionContext) -> Result<Query, LLMError> {
        match self.understand_using_groq(query, context).await {
            Ok(response) => {
                self.parse_response_with_multistep(&response, query, context)
                    .await
            }
            Err(e) => {
                error!("Error:{:#?}", e.to_string());
                Err(e)
            }
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

        info!("Attempting Groq API call");

        let tools = self.get_groq_tool_definitions();
        //info!(tools = ?tools, "Groq tool definitions");

        let response = self
            .client
            .execute_with_retry(
                self.client
                    .post("https://api.groq.com/openai/v1/chat/completions")
                    .header("Authorization", format!("Bearer {}", groq_key))
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
                        "tool_choice": "auto",
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

impl ToolExecutor for ClaudeAI {
    fn execute_tool(&self, tool_name: &str, input: &serde_json::Value) -> Option<ToolResult> {
        match tool_name {
            "list_available_pricelists" => {
                if let Some(pricelist_service) = &self.pricelist_service {
                    let brand_filter = input["brand"].as_str();
                    let result = pricelist_service.list_available_pricelists(brand_filter);
                    info!("Available price lists:{:#?}", result);
                    Some(ToolResult::AvailablePricelists(result))
                } else {
                    None
                }
            }
            _ => None,
        }
    }
}
