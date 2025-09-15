use crate::claude::{ClaudeAI, Query};
use crate::communication::telegram::Response;
use crate::configuration::Context;
use crate::core::Service;
use crate::database::{DatabaseService, SessionContext};
use crate::ocr::OcrService;
use crate::pdf::{create_quotation_pdf, DocumentType};
use crate::prices::price_list::PriceListService;
use crate::prices::PriceService;
use crate::quotation::QuotationService;
use crate::stock::StockService;
use crate::transcription::TranscriptionService;
use chrono::{Datelike, Local};
use rand::prelude::*;
use std::env;
use std::sync::{Arc, Mutex};
use thiserror::Error;
use tracing::info;

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

    #[error("Transcription Service Initialization Error: {0}")]
    TranscriptionServiceInitializationError(String),

    #[error("Audio transcription error: {0}")]
    TranscriptionError(String),
}

pub struct QueryFulfilment {
    price_service: PriceService,
    llm_service: ClaudeAI,
    quotation_service: QuotationService,
    pricelist_service: PriceListService,
    ocr_service: OcrService,
    stock_service: Arc<StockService>,
    database: Arc<DatabaseService>,
    transcription_service: TranscriptionService,
    runtime_config: Arc<Mutex<RuntimeConfig>>,
}

#[derive(Debug, Clone)]
pub struct RuntimeConfig {
    pub primary_llm: String,
}

impl Default for RuntimeConfig {
    fn default() -> Self {
        Self {
            primary_llm: "groq".to_string(),
        }
    }
}

impl QueryFulfilment {
    pub async fn new(context: Context) -> Result<Self, QueryError> {
        let runtime_config = Arc::new(Mutex::new(RuntimeConfig::default()));
        let price_service = PriceService::new(context.clone()).await;
        let claude_ai = ClaudeAI::new(
            &context.config.claude.system_prompt,
            context.database.clone(),
            runtime_config.clone(),
        )
        .map_err(|e| QueryError::LLMInitializationError(e.to_string()))?;
        let quotation_service = QuotationService::new(context.config.pricelists.clone())
            .map_err(|e| QueryError::QuotationServiceInitializationError(e.to_string()))?;
        let pricelist_service = PriceListService::new(context.config.pdf_pricelists)
            .map_err(|e| QueryError::PriceListServiceInitializationError(e.to_string()))?;
        let ocr_service = OcrService::new(context.database.clone())
            .await
            .map_err(|_| QueryError::OcrInitializationError)?;
        let groq_api_key = env::var("GROQ_API_KEY").map_err(|_| {
            QueryError::TranscriptionServiceInitializationError(
                "GROQ_API_KEY not found".to_string(),
            )
        })?;
        let transcription_service =
            TranscriptionService::new(groq_api_key, context.database.clone());
        Ok(Self {
            price_service,
            llm_service: claude_ai,
            quotation_service,
            pricelist_service,
            ocr_service,
            stock_service: Arc::clone(&context.stock_service),
            database: context.database.clone(),
            transcription_service,
            runtime_config,
        })
    }

    pub fn get_help_text() -> String {
        std::fs::read_to_string("assets/help.txt")
            .unwrap_or_else(|_| "Could not understand query. Please rephrase".to_string())
    }

    pub fn set_primary_model(&self, model: &str) {
        let mut config = self.runtime_config.lock().unwrap();
        config.primary_llm = model.to_string();
    }

    pub async fn fulfil_audio_query(
        &self,
        audio_data: &[u8],
        context: &SessionContext,
    ) -> Result<Response, QueryError> {
        // Transcribe audio to text
        let transcribed_text = self
            .transcription_service
            .transcribe_audio(audio_data.to_vec(), context)
            .await
            .map_err(|e| QueryError::TranscriptionError(e.to_string()))?;

        // Use existing text query flow
        self.fulfil_query(&transcribed_text, context).await
    }

    pub async fn fulfil_image_query(
        &self,
        image_data: &[u8],
        user_text: &str,
        context: &SessionContext,
    ) -> Result<Response, QueryError> {
        // Extract text from image
        let image_text = self
            .ocr_service
            .extract_text_from_image(image_data.to_vec(), context)
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
        self.fulfil_query(&combined_query, context).await
    }

    pub async fn fulfil_query(
        &self,
        query: &str,
        context: &SessionContext,
    ) -> Result<Response, QueryError> {
        let query = self.get_query_type(query, context).await?;
        let query_metadata = Some(serde_json::to_value(&query).unwrap_or(serde_json::Value::Null));
        let response = match query {
            Query::GetPriceList { brand, keywords } => {
                match self.pricelist_service.find_pricelist(&brand, &keywords) {
                    Some(pdf_path) => Response {
                        text: "Pricelist".to_string(),
                        file: Some(pdf_path),
                        query_metadata,
                    },
                    None => Response {
                        text: "No matching pricelist found".to_string(),
                        file: None,
                        query_metadata,
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
                    query_metadata,
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
                        query_metadata,
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
                        query_metadata,
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
                            query_metadata
                        }
                    }
                    _ => Response {
                        text: "No prices found for the requested items. Please check item/specifications".to_string(),
                        file: None,
                        query_metadata
                    }
                }
            }

            Query::GetStock { query } => match self.stock_service.request_stock(query).await {
                Ok(stock_info) => Response {
                    text: stock_info,
                    file: None,
                    query_metadata,
                },
                Err(e) => Response {
                    text: format!("Stock check failed: {}", e),
                    file: None,
                    query_metadata,
                },
            },
            _ => Response {
                text: "Cannot fulfil this request at the moment".to_string(),
                file: None,
                query_metadata,
            },
        };
        Ok(response)
    }

    pub async fn get_query_type(
        &self,
        query: &str,
        context: &SessionContext,
    ) -> Result<Query, QueryError> {
        let query: Query = self
            .llm_service
            .parse_query(query, context)
            .await
            .map_err(|e| QueryError::LLMError(e.to_string()))?;
        info!("Parsed query successfully");

        // Update the session with the actual query type
        let query_type = match &query {
            Query::MetalPricing => "MetalPricing",
            Query::GetPriceList { .. } => "GetPriceList",
            Query::GetQuotation(_) => "GetQuotation",
            Query::GetProformaInvoice(_) => "GetProformaInvoice",
            Query::GetPricesOnly(_) => "GetPricesOnly",
            Query::GetStock { .. } => "GetStock",
            Query::UnsupportedQuery => "UnsupportedQuery",
        };

        // Update the session with actual query type
        let response = self
            .database
            .update_session_query_type(context.session_id, query_type)
            .await;
        info!("Database query update response:{:#?}", response);
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
