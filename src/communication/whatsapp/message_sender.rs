use super::AppState;
use crate::database::SessionContext;
use axum::{
    http::{header::CONTENT_TYPE, StatusCode},
    response::Response,
};
use tracing::error;

pub async fn send_whatsapp_message_with_media(
    state: &AppState,
    to: &str,
    media_url: &str,
    context: &SessionContext,
) -> Result<(), Box<dyn std::error::Error>> {
    let url = format!(
        "https://api.twilio.com/2010-04-01/Accounts/{}/Messages.json",
        state.twilio_account_sid
    );

    let params = [
        ("From", "whatsapp:+17246175462"), // Your Twilio WhatsApp number
        ("To", to),
        ("MediaUrl", media_url),
    ];

    let response = state
        .http_client
        .execute_with_retry(
            state
                .http_client
                .post(&url)
                .basic_auth(&state.twilio_account_sid, Some(&state.twilio_auth_token))
                .form(&params),
        )
        .await?;

    if !response.status().is_success() {
        error!(
            "Failed to send WhatsApp message with media: {}",
            response.status()
        );
        let error_msg = format!(
            "❌ Error sending whatsapp message with media : {}, to:{}",
            media_url, to
        );
        let _ = state.error_sender.try_send(error_msg);
    }

    let _ = state
        .database
        .log_whatsapp_message(context, true, 0, true)
        .await;

    Ok(())
}

// Function to send WhatsApp message via Twilio REST API
pub async fn send_whatsapp_message(
    state: &AppState,
    to: &str,
    message: &str,
    context: &SessionContext,
) -> Result<(), Box<dyn std::error::Error>> {
    let url = format!(
        "https://api.twilio.com/2010-04-01/Accounts/{}/Messages.json",
        state.twilio_account_sid
    );

    let params = [
        ("From", "whatsapp:+17246175462"), // Your Twilio WhatsApp number
        ("To", to),
        ("Body", message),
    ];

    let response = state
        .http_client
        .execute_with_retry(
            state
                .http_client
                .post(&url)
                .basic_auth(&state.twilio_account_sid, Some(&state.twilio_auth_token))
                .form(&params),
        )
        .await?;

    if !response.status().is_success() {
        let error_msg = format!(
            "❌ Error sending whatsapp message - response state : {}, to:{}",
            response.status(),
            to
        );
        let _ = state.error_sender.try_send(error_msg);
        error!("Failed to send WhatsApp message: {}", response.status());
    }

    let _ = state
        .database
        .log_whatsapp_message(context, true, message.len(), false)
        .await;
    Ok(())
}

pub async fn send_text_response(
    message: &str,
    state: &AppState,
    context: &SessionContext,
) -> Response<String> {
    // Log cost
    let _ = state
        .database
        .log_whatsapp_message(context, true, message.len(), false)
        .await;

    let twiml = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
        <Response>
            <Message>
                <Body>{}</Body>
            </Message>
        </Response>"#,
        message
    );

    Response::builder()
        .status(StatusCode::OK)
        .header(CONTENT_TYPE, "text/xml")
        .body(twiml)
        .unwrap()
}

async fn _send_pdf_response(
    pdf_path: &str,
    message: &str,
    base_url: &str,
) -> Result<Response<String>, Box<dyn std::error::Error>> {
    let file_url = format!("{}/{}", base_url, pdf_path);

    let twiml = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
        <Response>
            <Message>
                <Body>{}</Body>
                <Media>{}</Media>
            </Message>
        </Response>"#,
        message, file_url
    );

    Ok(Response::builder()
        .status(StatusCode::OK)
        .header(CONTENT_TYPE, "text/xml")
        .body(twiml)
        .unwrap())
}
