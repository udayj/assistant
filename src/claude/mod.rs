use reqwest::Client;
use serde_json::json;
use std::env;
use thiserror::Error;
use crate::quotation::QuotationRequest;
use serde::Deserialize;
use crate::prices::item_prices::PriceList;
use dotenvy::dotenv;

#[derive(Debug, Deserialize)]
pub enum Query {
    MetalPricing,
    GetPriceList(PriceList),
    GetQuotation(QuotationRequest),
    UnsupportedQuery
}


#[derive(Error, Debug)]
pub enum LLMError {
    #[error("Cannot parse and deserialize llm response")]
    ParseError,
    #[error("Cannot find api key in env")]
    EnvError,
    #[error("Claude client error")]
    ClientError
}

pub async fn parse_query(system_prompt: &str, query: &str) -> Result<Query, LLMError> {
    let client = Client::new();
    dotenv().ok();
    let api_key = env::var("ANTHROPIC_API_KEY").map_err(|_| LLMError::EnvError)?;
    println!("API KEY:{}", api_key);
    let response = client
        .post("https://api.anthropic.com/v1/messages")
        .header("x-api-key", api_key)
        .header("anthropic-version", "2023-06-01")
        .json(&json!({
            "model": "claude-sonnet-4-20250514",
            "system": system_prompt,
            "max_tokens": 10240,
            "messages": [{
                "role": "user",
                "content": query
            }]
        }))
        .send()
        .await.map_err(|_| LLMError::ClientError)?
        .json::<serde_json::Value>()
        .await.map_err(|_| LLMError::ClientError)?;
    println!("raw response:{:#?}", response);
    println!("{}", response["content"][0]["text"].clone());
    let actual_query: Query =
        serde_json::from_str(response["content"][0]["text"].as_str().unwrap()).map_err(|e| {println!("error:{:#?}",e.to_string());LLMError::ParseError})?;
    Ok(actual_query)
}
