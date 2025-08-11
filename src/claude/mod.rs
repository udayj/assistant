use crate::prices::item_prices::PriceList;
use crate::quotation::QuotationRequest;
use dotenvy::dotenv;
use reqwest::Client;
use serde::Deserialize;
use serde_json::json;
use std::env;
use std::fs;
use thiserror::Error;

#[derive(Debug, Deserialize)]
pub enum Query {
    MetalPricing,
    GetPriceList(PriceList),
    GetQuotation(QuotationRequest),
    UnsupportedQuery,
}

#[derive(Error, Debug)]
pub enum LLMError {
    #[error("Cannot parse and deserialize llm response")]
    ParseError,
    #[error("Cannot find api key in env")]
    EnvError,
    #[error("Claude client error")]
    ClientError,
    #[error("System prompt construction error:{0}")]
    SystemPromptError(String),
}

#[derive(Debug)]
pub struct ClaudeAI {
    system_prompt: String,
    api_key: String
}

impl ClaudeAI {
    pub fn new(system_prompt_file: &str) -> Result<Self, LLMError> {
        let prompt = fs::read_to_string(system_prompt_file)
            .map_err(|e| LLMError::SystemPromptError(e.to_string()))?;
        dotenv().ok();
        let api_key = env::var("ANTHROPIC_API_KEY").map_err(|_| LLMError::EnvError)?;
        Ok(Self {
            system_prompt: prompt,
            api_key
        })
    }

    pub async fn parse_query(&self, query: &str) -> Result<Query, LLMError> {
        let client = Client::new();
        // Cost per quotation = Rs.0.5 input + Rs.0.8 output for text queries with 10 items
        // Effective total cost per quotation = Rs.1.30 for file quotation
        let response = client
            .post("https://api.anthropic.com/v1/messages")
            .header("x-api-key", self.api_key.clone())
            .header("anthropic-version", "2023-06-01")
            .json(&json!({
                "model": "claude-sonnet-4-20250514",
                "system": self.system_prompt.as_str(),
                "max_tokens": 10240,
                "messages": [{
                    "role": "user",
                    "content": query
                }]
            }))
            .send()
            .await
            .map_err(|_| LLMError::ClientError)?
            .json::<serde_json::Value>()
            .await
            .map_err(|_| LLMError::ClientError)?;
        println!("raw response:{:#?}", response);
        println!("{}", response["content"][0]["text"].clone());
        let actual_query: Query = serde_json::from_str(
            response["content"][0]["text"].as_str().unwrap(),
        )
        .map_err(|e| {
            println!("error:{:#?}", e.to_string());
            LLMError::ParseError
        })?;
        Ok(actual_query)
    }
}
