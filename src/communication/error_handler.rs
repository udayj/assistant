use crate::communication::telegram::Response;
use crate::query::QueryError;

pub fn map_query_error_to_user_message(error: &QueryError) -> String {
    match error {
        QueryError::MetalPricingError(_) => {
            "Could not fetch metal prices - please try again later".to_string()
        }
        QueryError::QuotationServiceError => {
            "Error generating quotation - please check whether items are valid".to_string()
        }
        QueryError::LLMError(_) => "Unable to understand query correctly".to_string(),
        QueryError::OcrError(_) => "Could not process image - please try again with clearer image".to_string(),
        QueryError::TranscriptionError(_) => "Could not process audio - please try again with clearer audio".to_string(),
        _ => "Could not service request - please try again later".to_string(),
    }
}

pub fn create_error_response(error: &QueryError) -> Response {
    Response {
        text: map_query_error_to_user_message(error),
        file: None,
        query_metadata: None,
    }
}