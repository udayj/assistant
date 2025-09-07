use crate::database::SessionContext;
use aws_config::BehaviorVersion;
use aws_sdk_textract::{types::Document, Client as AWSClient};
use std::sync::Arc;
use thiserror::Error;

use crate::database::DatabaseService;

#[derive(Debug, Error)]
pub enum OcrError {
    #[error("Image processing error: {0}")]
    ProcessingError(String),
}

pub struct OcrService {
    client: AWSClient,
    database: Arc<DatabaseService>,
}

impl OcrService {
    pub async fn new(database: Arc<DatabaseService>) -> Result<Self, OcrError> {
        let config = aws_config::load_defaults(BehaviorVersion::latest()).await;
        let client = AWSClient::new(&config);
        Ok(Self { client, database })
    }

    pub async fn extract_text_from_image(
        &self,
        image_data: Vec<u8>,
        context: &SessionContext,
    ) -> Result<String, OcrError> {
        let image_data_len = image_data.len();
        let document = Document::builder()
            .bytes(aws_sdk_textract::primitives::Blob::new(image_data))
            .build();

        let response = self
            .client
            .detect_document_text()
            .document(document)
            .send()
            .await
            .map_err(|e| OcrError::ProcessingError(e.to_string()))?;

        let mut extracted_text = String::new();
        if let Some(blocks) = response.blocks {
            for block in blocks {
                if block.block_type() == Some(&aws_sdk_textract::types::BlockType::Line) {
                    if let Some(text) = block.text() {
                        extracted_text.push_str(text);
                        extracted_text.push('\n');
                    }
                }
            }
        }

        let _ = self
            .database
            .log_textract_usage(context, image_data_len)
            .await;
        if extracted_text.trim().is_empty() {
            Ok("No readable text found".to_string())
        } else {
            Ok(extracted_text.trim().to_string())
        }
    }
}
