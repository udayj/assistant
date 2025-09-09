use crate::core::http::RetryableClient;
use crate::database::{CostEventBuilder, DatabaseService, SessionContext};
use std::sync::Arc;
use thiserror::Error;
use tracing::info;

#[derive(Debug, Error)]
pub enum TranscriptionError {
    #[error("Audio processing error: {0}")]
    ProcessingError(String),
}

pub struct TranscriptionService {
    client: RetryableClient,
    groq_api_key: String,
    database: Arc<DatabaseService>,
}

impl TranscriptionService {
    pub fn new(groq_api_key: String, database: Arc<DatabaseService>) -> Self {
        Self {
            client: RetryableClient::new(),
            groq_api_key,
            database,
        }
    }

    pub async fn transcribe_audio(
        &self,
        audio_data: Vec<u8>,
        context: &SessionContext,
    ) -> Result<String, TranscriptionError> {
        let audio_size = audio_data.len();

        // Create multipart form data
        let form = reqwest::multipart::Form::new()
            .part(
                "file",
                reqwest::multipart::Part::bytes(audio_data).file_name("audio.ogg"),
            )
            .text("model", "whisper-large-v3-turbo")
            .text("language", "en")
            .text("response_format", "verbose_json");

        let response = self
            .client
            .post("https://api.groq.com/openai/v1/audio/transcriptions")
            .header("Authorization", format!("Bearer {}", self.groq_api_key))
            .multipart(form)
            .send()
            .await
            .map_err(|e| TranscriptionError::ProcessingError(e.to_string()))?;

        let json_response: serde_json::Value = response
            .json()
            .await
            .map_err(|e| TranscriptionError::ProcessingError(e.to_string()))?;

        let transcribed_text = json_response
            .get("text")
            .and_then(|t| t.as_str())
            .ok_or_else(|| {
                TranscriptionError::ProcessingError("No text in response".to_string())
            })?;

        // Log cost (Groq Whisper is typically $0.0001 per second)
        // Estimate duration: ~1 second per 16KB for typical voice messages
        let estimated_duration_seconds = (audio_size / 16000).max(10);
        CostEventBuilder::new(context.clone(), "groq_whisper")
            .with_cost(
                0.004 / 3600.0,
                "per_second",
                estimated_duration_seconds as i32,
            )
            .with_metadata(serde_json::json!({
                "audio_size_bytes": audio_size,
                "estimated_duration_seconds": estimated_duration_seconds,
                "model": "whisper-large-v3-turbo"
            }))
            .log(&self.database)
            .await
            .map_err(|_| TranscriptionError::ProcessingError("Failed to log cost".to_string()))?;

        if transcribed_text.trim().is_empty() {
            Ok("No speech detected".to_string())
        } else {
            info!("Transcribed text: {}", transcribed_text.trim().to_string());
            Ok(transcribed_text.trim().to_string())
        }
    }
}
