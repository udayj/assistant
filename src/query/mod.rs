use crate::claude::{ClaudeAI, Query};
use crate::communication::telegram::Response;
use crate::configuration::{Context};
use crate::core::Service;
use crate::prices::PriceService;
use crate::quotation::{QuotationService};
use thiserror::Error;
use crate::pdf::create_quotation_pdf;
use chrono::{Datelike,Local};
use rand::prelude::*;

#[derive(Error, Debug)]
pub enum QueryError {
    #[error("Failed to understand query: {0}")]
    LLMError(String),

    #[error("LLM initialization error: {0}")]
    LLMInitializationError(String),

    #[error("Quotation Service Initialization Error: {0}")]
    QuotationServiceInitializationError(String),

    #[error("Error getting metal price: {0}")]
    MetalPricingError(String),
}

pub struct QueryFulfilment {
    price_service: PriceService,
    llm_service: ClaudeAI,
    quotation_service: QuotationService,
}

impl QueryFulfilment {
    pub async fn new(context: Context) -> Result<Self, QueryError> {
        let price_service = PriceService::new(context.clone()).await;
        let claude_ai = ClaudeAI::new(&context.config.claude.system_prompt)
            .map_err(|e| QueryError::LLMInitializationError(e.to_string()))?;
        let quotation_service = QuotationService::new(context.config.pricelists.clone())
            .map_err(|e| QueryError::QuotationServiceInitializationError(e.to_string()))?;
        Ok(Self {
            price_service,
            llm_service: claude_ai,
            quotation_service,
        })
    }

    pub async fn fulfil_query(&self, query: &str) -> Result<Response, QueryError> {
        let query = self.get_query_type(query).await?;
        let response = match query {
            Query::MetalPricing => {
                let price_cu = self
                    .price_service
                    .fetch_price("copper")
                    .await
                    .map_err(|e| QueryError::MetalPricingError(e.to_string()))?;
                let price_al = self
                    .price_service
                    .fetch_price("aluminium")
                    .await
                    .map_err(|e| QueryError::MetalPricingError(e.to_string()))?;
                let response_text_cu = format!("Current Copper price : {:.2}", price_cu);
                let response_text_al = format!("Current Aluminium price : {:.2}", price_al);
                let response_text = format!("{}\n{}", response_text_cu, response_text_al);
                Response {
                    text: response_text,
                    file: None,
                }
            }
            Query::GetQuotation(quotation_request) => {
                let q_response = self.quotation_service.generate_quotation(quotation_request);
                if q_response.is_none() {
                    Response {
                        text: "Cannot form quotation for this request".to_string(),
                        file: None,
                    }
                } else {
                    let date = Local::now().date_naive();
                    let formatted_date = date.format("%Y%m%d").to_string();
                    let quotation_response = q_response.unwrap();
                    let mut random_gen = rand::rng();
                    let random_q_num = random_gen.random_range(1000..=9999);
                    let quotation_number = format!("Ref: Q-{}-{}", formatted_date, random_q_num);
                    let now = Local::now();

                    // Get day, month, and year
                    let day = now.day();
                    let month = now.format("%B"); // Full month name, e.g., "August"
                    let year = now.year();

                    // Determine the ordinal suffix for the day
                    let suffix = match day {
                        1 | 21 | 31 => "st",
                        2 | 22 => "nd",
                        3 | 23 => "rd",
                        _ => "th",
                    };

                    // Format the date as a string
                    let quotation_date = format!("{}{} {}, {}", day, suffix, month, year);
                    let _ = create_quotation_pdf(
                        &quotation_number,
                        &quotation_date,
                        &quotation_response,
                        format!("{}.pdf",quotation_number).as_str(),
                    )
                    .unwrap();
                    Response {
                        text: "Quotation created for given enquiry".to_string(),
                        file: Some(format!("{}.pdf",quotation_number))
                    }
                }
            }
            _ => Response {
                text: "Cannot fulfil this request at the moment".to_string(),
                file: None,
            },
        };
        Ok(response)
    }

    pub async fn get_query_type(&self, query: &str) -> Result<Query, QueryError> {
        let query: Query = self
            .llm_service
            .parse_query(query)
            .await
            .map_err(|e| QueryError::LLMError(e.to_string()))?;
        println!("parsed query successfully");
        Ok(query)
    }
}

// insert accepted tags - the tags act like selectors for price lists - for instance, user could specify
// give price for 3 c x 2.5 cu armd 100 M and 4 core x 2.5 cu armd 200 m - use feb 2025 price list and give disc 70%
// then feb 2025 becomes the tag because it will help idetify which price list to use to get the base price internally
// acceptable tags are given as follows
// the sizes of cables should not have trailing 0s in the value for instance, 3.5 Core should not be written as 3.50
// similarly 4 core should not be written as 4.0
// however, 0.75 sq. mm can be written with 1 leading decimal
// use claude itself to make the prompt clear
