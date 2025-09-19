use crate::database::{DatabaseService, SessionContext, StructuredResponse};
use crate::prices::price_list::{AvailablePricelists, PriceListService};
use crate::query::RuntimeConfig;
use crate::quotation::{PriceOnlyRequest, QuotationRequest};
use async_trait::async_trait;
use schemars::schema_for;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::env;
use std::fs;
use std::sync::{Arc, Mutex};
use thiserror::Error;
use tokio::sync::mpsc::Sender;
use tracing::{error, info};

pub mod llm_providers;
use llm_providers::claude::Claude;
use llm_providers::groq::Groq;
use llm_providers::LLM;

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

#[async_trait]
pub trait LLMProvider {
    async fn try_parse(
        &self,
        query: &str,
        context: &SessionContext,
        llm_orechestrator: &LLMOrchestrator,
    ) -> Result<Query, LLMError>;
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

pub struct LLMOrchestrator {
    claude: LLM,
    groq: LLM,
    runtime_config: Arc<Mutex<RuntimeConfig>>,
    pricelist_service: Option<Arc<PriceListService>>,
}

impl LLMOrchestrator {
    pub fn get_tool_definitions() -> serde_json::Value {
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

    pub fn new(
        system_prompt_file: &str,
        database: Arc<DatabaseService>,
        runtime_config: Arc<Mutex<RuntimeConfig>>,
    ) -> Result<Self, LLMError> {
        let prompt = fs::read_to_string(system_prompt_file)
            .map_err(|e| LLMError::SystemPromptError(e.to_string()))?;

        let api_key = env::var("ANTHROPIC_API_KEY").map_err(|_| LLMError::EnvError)?;
        let groq_api_key = env::var("GROQ_API_KEY").map_err(|_| LLMError::EnvError)?;
        let claude = Claude::new(prompt.as_str(), api_key.as_str(), Arc::clone(&database));
        let groq = Groq::new(
            prompt.as_str(),
            groq_api_key.as_str(),
            Arc::clone(&database),
        );
        Ok(Self {
            claude: LLM::Claude(claude),
            groq: LLM::Groq(groq),
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

        // Continue with the same model used for previous conversation

        match &context.last_model_used {
            Option::Some(model) => match model.as_str() {
                "claude" => Box::pin(self.claude.try_parse(&continued_query, context, self)).await,
                "groq" => Box::pin(self.groq.try_parse(&continued_query, context, self)).await,
                _ => Box::pin(self.claude.try_parse(&continued_query, context, self)).await,
            },
            Option::None => Box::pin(self.claude.try_parse(&continued_query, context, self)).await,
        }
    }

    pub async fn parse_query(
        &self,
        query: &str,
        context: &mut SessionContext,
        error_sender: &Sender<String>,
    ) -> Result<Query, LLMError> {
        // Handle conversation context first - this is LLM's responsibility
        let conversation_context = match self.handle_conversation_context(query, context).await {
            Ok(ctx) => ctx,
            Err(e) => {
                tracing::error!("Failed to handle conversation context: {}", e);
                let _ = error_sender
                    .send(format!("Conversation context error: {}", e))
                    .await;
                None // Continue with fresh query
            }
        };

        // Build query with conversation history if continuing conversation
        let query_with_context = if let Some(conv_context) = conversation_context {
            self.build_query_with_conversation_history(query, &conv_context)
        } else {
            query.to_string()
        };

        let primary_model = {
            let config = self.runtime_config.lock().unwrap();
            config.primary_llm.clone()
        };
        context.last_model_used = Some(primary_model.clone());
        match primary_model.as_str() {
            "claude" => match self
                .claude
                .try_parse(&query_with_context, context, self)
                .await
            {
                Ok(result) => Ok(result),
                Err(e) => {
                    context.last_model_used = Some("groq".to_string());
                    error!("Claude failed with error: {}, trying Groq fallback", e);
                    self.groq
                        .try_parse(&query_with_context, context, self)
                        .await
                }
            },
            "groq" => match self
                .groq
                .try_parse(&query_with_context, context, self)
                .await
            {
                Ok(result) => Ok(result),
                Err(e) => {
                    context.last_model_used = Some("claude".to_string());
                    error!("Groq failed with error: {}, trying Claude fallback", e);
                    self.claude
                        .try_parse(&query_with_context, context, self)
                        .await
                }
            },
            _ => {
                self.claude
                    .try_parse(&query_with_context, context, self)
                    .await
            } // Default fallback
        }
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

    // Handle conversation continuation - this is an LLM responsibility
    pub async fn handle_conversation_context(
        &self,
        query: &str,
        context: &mut SessionContext,
    ) -> Result<Option<crate::database::ConversationContext>, LLMError> {
        // Get database service from claude provider
        let database = match &self.claude {
            LLM::Claude(claude_provider) => &claude_provider.database,
            _ => return Err(LLMError::ClientError("Database not available".to_string())),
        };

        // Check for existing conversation
        let recent_conversation = database
            .get_recent_conversation(context.user_id)
            .await
            .map_err(|e| LLMError::ClientError(e.to_string()))?;

        let should_continue = if let Some(conv_context) = &recent_conversation {
            if !conv_context.messages.is_empty() {
                self.should_continue_conversation(query, &conv_context.messages, context)
                    .await?
            } else {
                false
            }
        } else {
            false
        };

        // Set or create conversation ID and return context if continuing
        if should_continue {
            if let Some(conv_context) = recent_conversation {
                context.conversation_id = Some(conv_context.conversation_id);
                Ok(Some(conv_context))
            } else {
                Ok(None)
            }
        } else {
            // Create new conversation
            let new_conv_id = database
                .create_conversation(context.user_id)
                .await
                .map_err(|e| LLMError::ClientError(e.to_string()))?;
            context.conversation_id = Some(new_conv_id);
            Ok(None)
        }
    }

    // Build query with conversation history for LLM context
    fn build_query_with_conversation_history(
        &self,
        current_query: &str,
        conversation: &crate::database::ConversationContext,
    ) -> String {
        let mut context_messages = Vec::new();

        // Add conversation history as context
        for msg in &conversation.messages {
            context_messages.push(format!("User query: {}", msg.user_query));

            // Add assistant response if available
            if let Some(response) = &msg.structured_response {
                context_messages.push(format!(
                    "What the assistant understood: {}",
                    response.get_metadata()
                ));
            }
        }

        // Add current query
        context_messages.push(format!("Current User Query: {}", current_query));

        format!(
            "Previous conversation:\n{}\n\nPlease respond to the latest user query considering the conversation context.",
            context_messages.join("\n")
        )
    }

    // Simple conversation decision using exact stored responses
    async fn should_continue_conversation(
        &self,
        current_query: &str,
        conversation_messages: &[crate::database::ConversationMessage],
        context: &SessionContext,
    ) -> Result<bool, LLMError> {
        if conversation_messages.is_empty() {
            return Ok(false);
        }

        // Build conversation history with exact stored responses
        let mut history_parts = Vec::new();
        for msg in conversation_messages {
            history_parts.push(format!("User query: {}", msg.user_query));

            if let Some(response) = &msg.structured_response {
                // Send exact stored response to LLM - no extraction/summarization
                history_parts.push(format!(
                    "What the Assistant understood: {}",
                    serde_json::to_string_pretty(&response.get_metadata())
                        .unwrap_or_else(|_| "No response".to_string())
                ));
            }
        }

        let decision_prompt = format!(
            "Previous conversation:\n{}\n\nNew query: \"{}\"\n\nIs this new query a refinement, correction, or follow-up to the previous conversation? Answer only: YES or NO - do not respond with anything else",
            history_parts.join("\n"),
            current_query
        );
        info!("Decision prompt:{:#?}", decision_prompt);
        let decision_system_prompt = "You are a conversation classifier. Determine if a new query continues a previous conversation or starts a new topic. Only answer YES if the new query directly relates to, refines, corrects, or follows up on the previous conversation. Answer NO for completely new topics or unrelated queries or if unsure.";

        // Use Groq decision call with same session context for accurate cost tracking
        let groq_provider = match &self.groq {
            LLM::Groq(groq) => groq,
            _ => return Ok(false),
        };

        let response = groq_provider
            .make_decision_call(decision_system_prompt, &decision_prompt, context)
            .await?;

        let decision_text = response
            .get("content")
            .and_then(|c| c.as_array())
            .and_then(|arr| arr.first())
            .and_then(|item| item.get("text"))
            .and_then(|text| text.as_str())
            .unwrap_or("NO");

        Ok(decision_text.trim().to_uppercase() == "YES")
    }

    // Create structured response for storage - no serde_json::Value
    pub fn create_structured_response_for_storage(
        &self,
        response_text: &str,
        response_metadata: Option<&serde_json::Value>,
    ) -> StructuredResponse {
        StructuredResponse {
            response_text: response_text.to_string(),
            response_metadata: response_metadata
                .map(|v| serde_json::to_string(v).unwrap_or_default()),
            timestamp: chrono::Utc::now().to_rfc3339(),
        }
    }
}

impl ToolExecutor for LLMOrchestrator {
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
