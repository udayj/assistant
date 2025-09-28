use crate::communication::error_handler::map_query_error_to_user_message;
use crate::communication::session_helpers::{complete_session_with_error, complete_session_with_success};
use crate::communication::telegram::Response;
use crate::communication::whatsapp::message_sender::{send_whatsapp_message, send_whatsapp_message_with_media};
use crate::communication::whatsapp::AppState;
use crate::database::SessionContext;
use crate::query::QueryError;
use urlencoding::encode;

pub struct QueryProcessingParams {
    pub state: AppState,
    pub from: String,
    pub query_text: String,
    pub context: SessionContext,
    pub start_time: std::time::Instant,
}

pub async fn process_query_response(
    params: QueryProcessingParams,
    result: Result<Response, QueryError>,
) {
    let QueryProcessingParams { state, from, query_text, context, start_time } = params;

    match result {
        Ok(response) => {
            complete_session_with_success(&state.database, &context, &response, &query_text, start_time, &state.error_sender).await;

            if let Some(file_path) = response.file {
                let parts: Vec<&str> = file_path.split('/').collect();
                let encoded_parts: Vec<String> = parts.iter().map(|part| encode(part).to_string()).collect();
                let encoded_path = encoded_parts.join("/");
                let file_url = format!("{}/{}", state.file_base_url, encoded_path);
                let _ = send_whatsapp_message_with_media(&state, &from, &file_url, &context).await;
            } else {
                let _ = send_whatsapp_message(&state, &from, &response.text, &context).await;
            }
        }
        Err(e) => {
            complete_session_with_error(&state.database, &context, &e, &query_text, start_time, &state.error_sender).await;
            let error_response = map_query_error_to_user_message(&e);
            let _ = send_whatsapp_message(&state, &from, &error_response, &context).await;
        }
    }
}

pub fn convert_whatsapp_error_to_query_error(error: crate::communication::whatsapp::WhatsAppError) -> QueryError {
    match error {
        crate::communication::whatsapp::WhatsAppError::ImageProcessingError(_) => QueryError::OcrError(error.to_string()),
        _ => QueryError::LLMError(error.to_string()),
    }
}