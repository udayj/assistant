use crate::claude::{ClaudeAI, Query};
use crate::communication::telegram::Response;
use crate::communication::websocket::StockService;
use crate::configuration::Context;
use crate::core::Service;
use crate::ocr::OcrService;
use crate::pdf::{create_quotation_pdf, DocumentType};
use crate::prices::price_list::PriceListService;
use crate::prices::PriceService;
use crate::quotation::QuotationService;
use chrono::{Datelike, Local};
use rand::prelude::*;
use std::sync::Arc;
use thiserror::Error;
use tracing::info;
use uuid::Uuid;

#[derive(Error, Debug)]
pub enum QueryError {
    #[error("Failed to understand query: {0}")]
    LLMError(String),

    #[error("LLM initialization error: {0}")]
    LLMInitializationError(String),

    #[error("OCR Service initialization error")]
    OcrInitializationError,

    #[error("OCR Service error: {0}")]
    OcrError(String),

    #[error("Quotation Service Initialization Error: {0}")]
    QuotationServiceInitializationError(String),

    #[error("Error getting metal price: {0}")]
    MetalPricingError(String),

    #[error("Quotation Formation Error")]
    QuotationServiceError,

    #[error("PriceList Service Initialization Error: {0}")]
    PriceListServiceInitializationError(String),
}

pub struct QueryFulfilment {
    price_service: PriceService,
    llm_service: ClaudeAI,
    quotation_service: QuotationService,
    pricelist_service: PriceListService,
    ocr_service: OcrService,
    stock_service: Arc<StockService>,
}

impl QueryFulfilment {
    pub async fn new(context: Context) -> Result<Self, QueryError> {
        let price_service = PriceService::new(context.clone()).await;
        let claude_ai = ClaudeAI::new(
            &context.config.claude.system_prompt,
            context.database.clone(),
        )
        .map_err(|e| QueryError::LLMInitializationError(e.to_string()))?;
        let quotation_service = QuotationService::new(context.config.pricelists.clone())
            .map_err(|e| QueryError::QuotationServiceInitializationError(e.to_string()))?;
        let pricelist_service = PriceListService::new(context.config.pdf_pricelists)
            .map_err(|e| QueryError::PriceListServiceInitializationError(e.to_string()))?;
        let ocr_service = OcrService::new(context.database.clone())
            .await
            .map_err(|_| QueryError::OcrInitializationError)?;

        Ok(Self {
            price_service,
            llm_service: claude_ai,
            quotation_service,
            pricelist_service,
            ocr_service,
            stock_service: Arc::clone(&context.stock_service),
        })
    }

    pub fn get_help_text() -> String {
        std::fs::read_to_string("assets/help.txt")
            .unwrap_or_else(|_| "Could not understand query. Please rephrase".to_string())
    }

    pub async fn fulfil_image_query(
        &self,
        image_data: &[u8],
        user_text: &str,
        user_id: Uuid,
        session_id: Uuid,
    ) -> Result<Response, QueryError> {
        // Extract text from image
        let image_text = self
            .ocr_service
            .extract_text_from_image(image_data.to_vec(), user_id, session_id)
            .await
            .map_err(|e| QueryError::OcrError(e.to_string()))?;

        let combined_query =
            if image_text.trim().is_empty() || image_text.contains("No readable text found") {
                // Fallback to user text only
                user_text.to_string()
            } else {
                format!("{}\n{}", image_text.trim(), user_text.trim())
            };
        info!("formed combined query:{}", combined_query);
        // Use existing fulfillment logic
        self.fulfil_query(&combined_query, user_id, session_id)
            .await
    }

    pub async fn fulfil_query(
        &self,
        query: &str,
        user_id: Uuid,
        session_id: Uuid,
    ) -> Result<Response, QueryError> {
        let query = self.get_query_type(query, user_id, session_id).await?;
        let response = match query {
            Query::GetPriceList { brand, keywords } => {
                match self.pricelist_service.find_pricelist(&brand, &keywords) {
                    Some(pdf_path) => Response {
                        text: "Pricelist".to_string(),
                        file: Some(pdf_path),
                    },
                    None => Response {
                        text: "No matching pricelist found".to_string(),
                        file: None,
                    },
                }
            }

            Query::MetalPricing => {
                let response_text = self
                    .price_service
                    .fetch_formatted_prices()
                    .await
                    .map_err(|e| QueryError::MetalPricingError(e.to_string()))?;
                Response {
                    text: response_text,
                    file: None,
                }
            }

            Query::GetQuotation(quotation_request) => {
                let q_response = self.quotation_service.generate_quotation(quotation_request);
                if q_response.is_none() {
                    return Err(QueryError::QuotationServiceError);
                } else {
                    let (quotation_number, quotation_date, filename) =
                        self.generate_document_details(DocumentType::Quotation);

                    let _ = create_quotation_pdf(
                        &quotation_number,
                        &quotation_date,
                        &q_response.unwrap(),
                        &filename,
                        DocumentType::Quotation,
                    )
                    .unwrap();

                    Response {
                        text: "Quotation created for given enquiry".to_string(),
                        file: Some(format!("artifacts/{}", filename)),
                    }
                }
            }

            Query::GetProformaInvoice(quotation_request) => {
                let q_response = self.quotation_service.generate_quotation(quotation_request);
                if q_response.is_none() {
                    return Err(QueryError::QuotationServiceError);
                } else {
                    let (quotation_number, quotation_date, filename) =
                        self.generate_document_details(DocumentType::ProformaInvoice);

                    let _ = create_quotation_pdf(
                        &quotation_number,
                        &quotation_date,
                        &q_response.unwrap(),
                        &filename,
                        DocumentType::ProformaInvoice,
                    )
                    .unwrap();

                    Response {
                        text: "Proforma Invoice created for given enquiry".to_string(),
                        file: Some(format!("artifacts/{}", filename)),
                    }
                }
            }

            Query::GetPricesOnly(price_only_request) => {
                // NEW
                let price_response = self.quotation_service.get_prices_only(price_only_request);
                match price_response {
                    Some(response) if !response.items.is_empty() => {
                        Response {
                            text: self.format_price_only_response(response),
                            file: None,
                        }
                    }
                    _ => Response {
                        text: "No prices found for the requested items. Please check item/specifications".to_string(),
                        file: None,
                    }
                }
            }

            Query::GetStock { query } => match self.stock_service.request_stock(query).await {
                Ok(stock_info) => Response {
                    text: stock_info,
                    file: None,
                },
                Err(e) => Response {
                    text: format!("Stock check failed: {}", e),
                    file: None,
                },
            },
            _ => Response {
                text: "Cannot fulfil this request at the moment".to_string(),
                file: None,
            },
        };
        Ok(response)
    }

    pub async fn get_query_type(
        &self,
        query: &str,
        user_id: Uuid,
        session_id: Uuid,
    ) -> Result<Query, QueryError> {
        let query: Query = self
            .llm_service
            .parse_query(query, user_id, session_id)
            .await
            .map_err(|e| QueryError::LLMError(e.to_string()))?;
        info!("parsed query successfully");
        Ok(query)
    }

    fn format_price_only_response(&self, response: crate::quotation::PriceOnlyResponse) -> String {
        let mut lines = Vec::new();

        for item in response.items {
            let line = format!("{}: Rs.{:.2}/mtr", item.description, item.price);

            lines.push(line);
        }

        lines.join("\n")
    }

    fn generate_document_details(&self, document_type: DocumentType) -> (String, String, String) {
        let date = Local::now().date_naive();
        let formatted_date = date.format("%Y%m%d").to_string();
        let mut random_gen = rand::rng();
        let random_num = random_gen.random_range(1000..=9999);
        let prefix = document_type.get_ref_prefix();
        let quotation_number = format!("{}-{}-{}", prefix, formatted_date, random_num);

        let now = Local::now();
        let day = now.day();
        let month = now.format("%B");
        let year = now.year();

        let suffix = match day {
            1 | 21 | 31 => "st",
            2 | 22 => "nd",
            3 | 23 => "rd",
            _ => "th",
        };

        let quotation_date = format!("{}{} {}, {}", day, suffix, month, year);
        let filename = format!("{}.pdf", quotation_number);

        (quotation_number, quotation_date, filename)
    }
}
