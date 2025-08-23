use crate::quotation::{QuotationRequest, PriceOnlyRequest};
use dotenvy::dotenv;
use reqwest::Client;
use serde::Deserialize;
use serde_json::json;
use std::env;
use std::fs;
use std::time::Duration;
use thiserror::Error;
use tokio::time::sleep;

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
    #[error("API overloaded - all retries exhausted")]
    OverloadedError,
}

#[derive(Debug)]
pub struct ClaudeAI {
    system_prompt: String,
    api_key: String,
    client: Client,
}

impl ClaudeAI {
    pub fn new(system_prompt_file: &str) -> Result<Self, LLMError> {
        let prompt = fs::read_to_string(system_prompt_file)
            .map_err(|e| LLMError::SystemPromptError(e.to_string()))?;
        dotenv().ok();
        let api_key = env::var("ANTHROPIC_API_KEY").map_err(|_| LLMError::EnvError)?;
        let client = Client::builder()
            .timeout(Duration::from_secs(30))
            .build()
            .map_err(|e| LLMError::ClientError(e.to_string()))?;
        Ok(Self {
            system_prompt: prompt,
            api_key,
            client,
        })
    }

    pub async fn parse_query(&self, query: &str) -> Result<Query, LLMError> {
        const MAX_RETRIES: u32 = 3;
        let mut last_error = None;
        let mut parse_retry_attempted = false;

        for attempt in 0..MAX_RETRIES {
            let query_text = if parse_retry_attempted {
                format!("Your previous response failed JSON parsing. Return ONLY valid JSON matching the exact schema. Original query: {}", query)
            } else {
                query.to_string()
            };

            match self.make_api_request(&query_text).await {
                Ok(response) => match self.parse_response(&response) {
                    Ok(parsed_query) => return Ok(parsed_query),
                    Err(LLMError::ParseError) if !parse_retry_attempted => {
                        println!(
                            "Parse error on attempt {}, will retry with enhanced prompt",
                            attempt + 1
                        );
                        parse_retry_attempted = true;
                        last_error = Some(LLMError::ParseError);
                        continue;
                    }
                    Err(e) => return Err(e),
                },
                Err(e) => {
                    last_error = Some(e);

                    if attempt < MAX_RETRIES - 1 {
                        let delay = Duration::from_millis(1000 * (2_u64.pow(attempt)));
                        println!(
                            "Claude API attempt {} failed, retrying in {:?}",
                            attempt + 1,
                            delay
                        );
                        sleep(delay).await;
                    }
                }
            }
        }

        // All retries exhausted
        match last_error {
            Some(LLMError::ParseError) => {
                println!("Parse error persisted after retry for: {}", query);
                Err(LLMError::ParseError)
            }
            Some(LLMError::OverloadedError) => Err(LLMError::OverloadedError),
            Some(e) => Err(e),
            None => Err(LLMError::ClientError("Unknown error".to_string())),
        }
    }

    async fn make_api_request(&self, query: &str) -> Result<serde_json::Value, LLMError> {
        let response = self
            .client
            .post("https://api.anthropic.com/v1/messages")
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", "2023-06-01") 
            .json(&json!({
                "model": "claude-sonnet-4-20250514",
                "temperature": 0.0,
                "system": [
                    {
                        "type" : "text",
                        "text" : self.system_prompt.as_str(),
                        "cache_control" : { "type" : "ephemeral"}
                    } 
                ],
                "max_tokens": 10240,
                "messages": [{
                    "role": "user",
                    "content": query
                }]
            }))
            .send()
            .await
            .map_err(|e| LLMError::ClientError(e.to_string()))?;

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

        Ok(json_response)
    }

    fn parse_response(&self, response: &serde_json::Value) -> Result<Query, LLMError> {
        println!("raw response:{:#?}", response);

        let content_text = response["content"][0]["text"]
            .as_str()
            .ok_or(LLMError::ParseError)?;

        println!("{}", content_text);

        let actual_query: Query = serde_json::from_str(content_text).map_err(|e| {
            println!("Parse error: {:#?}", e);
            LLMError::ParseError
        })?;

        Ok(actual_query)
    }
}
