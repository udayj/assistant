use aws_config::BehaviorVersion;
use aws_sdk_textract::{types::Document, Client as AWSClient};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum OcrError {
    #[error("Image processing error: {0}")]
    ProcessingError(String),
}

pub struct OcrService {
    client: AWSClient,
}

impl OcrService {
    pub async fn new() -> Result<Self, OcrError> {
        let config = aws_config::load_defaults(BehaviorVersion::latest()).await;
        let client = AWSClient::new(&config);
        Ok(Self { client })
    }

    pub async fn extract_text_from_image(&self, image_data: Vec<u8>) -> Result<String, OcrError> {
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

        if extracted_text.trim().is_empty() {
            Ok("No readable text found".to_string())
        } else {
            Ok(extracted_text.trim().to_string())
        }
    }
}
