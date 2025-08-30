use crate::configuration::Context;
use crate::core::http::RetryableClient;
use crate::core::service_manager::{Error as ServiceManagerError, ServiceWithErrorSender};
use crate::query::QueryFulfilment;
use async_trait::async_trait;
use axum::{
    body::Body,
    extract::{Form, Path, State},
    http::{header::CONTENT_TYPE, StatusCode},
    response::Response,
    routing::{get, post},
    Router,
};
use std::collections::HashMap;
use std::sync::Arc;
use thiserror::Error;
use tokio::net::TcpListener;
use tokio::sync::mpsc;
use tower_http::cors::CorsLayer;
use tracing::{error, info};

#[derive(Debug, Error)]
pub enum WhatsAppError {
    #[error("Error initializing query fulfilment service: {0}")]
    QueryFulfilmentInitError(String),
    #[error("Image processing error: {0}")]
    ImageProcessingError(String),
}

#[derive(Clone)]
struct AppState {
    query_fulfilment: Arc<QueryFulfilment>,
    error_sender: mpsc::Sender<String>,
    file_base_url: String,
    twilio_account_sid: String,
    twilio_auth_token: String,
    http_client: RetryableClient,
}

pub struct WhatsAppService {
    port: u16,
    query_fulfilment: QueryFulfilment,
    error_sender: mpsc::Sender<String>,
    file_base_url: String,
    twilio_account_sid: String,
    twilio_auth_token: String,
    http_client: RetryableClient,
}

#[async_trait]
impl ServiceWithErrorSender for WhatsAppService {
    type Context = Context;

    async fn new(context: Context, error_sender: mpsc::Sender<String>) -> Self {
        let query_fulfilment = QueryFulfilment::new(context.clone()).await.unwrap();
        let twilio_account_sid = std::env::var("TWILIO_ACCOUNT_SID").unwrap();
        let twilio_auth_token = std::env::var("TWILIO_AUTH_TOKEN").unwrap();
        Self {
            port: context.config.whatsapp.webhook_port,
            query_fulfilment,
            error_sender,
            file_base_url: context.config.whatsapp.file_base_url,
            twilio_account_sid,
            twilio_auth_token,
            http_client: RetryableClient::new(),
        }
    }

    async fn run(self) -> Result<(), ServiceManagerError> {
        let state = AppState {
            query_fulfilment: Arc::new(self.query_fulfilment),
            error_sender: self.error_sender,
            file_base_url: self.file_base_url,
            twilio_account_sid: self.twilio_account_sid,
            twilio_auth_token: self.twilio_auth_token,
            http_client: self.http_client,
        };

        let app = Router::new()
            .route("/health", get(health_check))
            .route("/webhook", post(webhook_handler))
            .route("/artifacts/{*filename}", get(serve_file))
            .layer(CorsLayer::permissive())
            .with_state(state);

        let listener = TcpListener::bind(format!("0.0.0.0:{}", self.port))
            .await
            .map_err(|e| ServiceManagerError::new(&format!("Failed to bind port: {}", e)))?;

        info!("WhatsApp HTTP server running on port {}", self.port);

        axum::serve(listener, app)
            .await
            .map_err(|e| ServiceManagerError::new(&format!("HTTP server error: {}", e)))
    }
}

async fn health_check() -> (StatusCode, &'static str) {
    (StatusCode::OK, "OK")
}

async fn webhook_handler(
    State(state): State<AppState>,
    Form(payload): Form<HashMap<String, String>>,
) -> Response<String> {
    info!("Webhook payload: {:?}", payload);

    let from = payload.get("From").unwrap_or(&"".to_string()).clone();
    let body = payload.get("Body").unwrap_or(&"".to_string()).clone();

    if from.is_empty() {
        return send_text_response(
            "Error in whatsapp request - please contact admin@avantgardelabs.in",
        );
    }

    if body.trim() == "/help" || body.trim() == "help" {
        return send_text_response(&QueryFulfilment::get_help_text());
    }

    if let Some(media_url) = payload.get("MediaUrl0") {
        let no_media_type = "".to_string();
        let media_type = payload.get("MediaContentType0").unwrap_or(&no_media_type);

        if !media_type.starts_with("image/") || media_type.is_empty() {
            return send_text_response("Please send only images with your request");
        }

        // Process all image queries asynchronously
        let state_clone = state.clone();
        let from_clone = from.clone();
        let media_url_clone = media_url.clone();
        let body_clone = body.clone();

        tokio::spawn(async move {
            match download_and_process_image(&state_clone, &media_url_clone, &body_clone).await {
                Ok(response) => {
                    if let Some(file_path) = response.file {
                        let file_url = format!("{}/{}", state_clone.file_base_url, file_path);
                        let _ =
                            send_whatsapp_message_with_media(&state_clone, &from_clone, &file_url)
                                .await;
                    } else {
                        let _ =
                            send_whatsapp_message(&state_clone, &from_clone, &response.text).await;
                    }
                }
                Err(e) => {
                    let error_msg = format!(
                        "❌ WhatsApp Image Query Failed\nText: {}\nError: {}",
                        body_clone, e
                    );
                    let _ = state_clone.error_sender.try_send(error_msg);
                    let _ = send_whatsapp_message(
                        &state_clone,
                        &from_clone,
                        "Could not process image - please try again with clearer image and text",
                    )
                    .await;
                }
            }
        });

        send_text_response("Processing your request...please wait ⏳")
    } else {
        // Process all text queries asynchronously
        let state_clone = state.clone();
        let from_clone = from.clone();
        let body_clone = body.clone();

        tokio::spawn(async move {
            match state_clone.query_fulfilment.fulfil_query(&body_clone).await {
                Ok(response) => {
                    if let Some(file_path) = response.file {
                        let file_url = format!("{}/{}", state_clone.file_base_url, file_path);
                        info!(file_url, %file_url, "File url");
                        let _ =
                            send_whatsapp_message_with_media(&state_clone, &from_clone, &file_url)
                                .await;
                    } else {
                        let _ =
                            send_whatsapp_message(&state_clone, &from_clone, &response.text).await;
                    }
                }
                Err(e) => {
                    let error_msg = format!(
                        "❌ Background Processing Failed\nQuery: {}\nError: {}",
                        body_clone, e
                    );
                    let _ = state_clone.error_sender.try_send(error_msg);
                    let _ = send_whatsapp_message(
                        &state_clone,
                        &from_clone,
                        "Sorry, couldn't process your request. Please try again later.",
                    )
                    .await;
                }
            }
        });

        send_text_response("Processing your request...please wait ⏳")
    }
}

async fn serve_file(
    State(state): State<AppState>,
    Path(filename): Path<String>,
) -> Result<Response<Body>, StatusCode> {
    let file_path = format!("artifacts/{}", filename);
    info!(file_path, %file_path, "File path");
    match tokio::fs::read(&file_path).await {
        Ok(contents) => Ok(Response::builder()
            .status(StatusCode::OK)
            .header("content-type", "application/pdf")
            //.header("content-type", "image/jpeg")
            .body(Body::from(contents))
            .unwrap()),
        Err(e) => {
            let error_msg = format!("❌ File Serve Error\n\nFile: {}\nError: {}", file_path, e);
            let _ = state.error_sender.try_send(error_msg);
            Err(StatusCode::NOT_FOUND)
        }
    }
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

fn send_text_response(message: &str) -> Response<String> {
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

async fn download_and_process_image(
    state: &AppState,
    media_url: &str,
    user_text: &str,
) -> Result<crate::communication::telegram::Response, WhatsAppError> {
    // Download image from Twilio media URL
    let response = state
        .http_client
        .execute_with_retry(
            state
                .http_client
                .get(media_url)
                .basic_auth(&state.twilio_account_sid, Some(&state.twilio_auth_token)),
        )
        .await
        .map_err(|e| WhatsAppError::ImageProcessingError(e.to_string()))?;

    if !response.status().is_success() {
        return Err(WhatsAppError::ImageProcessingError(
            format!("Failed to download media: {}", response.status()).into(),
        ));
    }

    let image_data = response
        .bytes()
        .await
        .map_err(|e| WhatsAppError::ImageProcessingError(e.to_string()))?;

    // Process through existing query fulfilment
    state
        .query_fulfilment
        .fulfil_image_query(&image_data, user_text)
        .await
        .map_err(|e| WhatsAppError::QueryFulfilmentInitError(e.to_string()))
}

async fn send_whatsapp_message_with_media(
    state: &AppState,
    to: &str,
    media_url: &str,
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

    Ok(())
}

// Function to send WhatsApp message via Twilio REST API
async fn send_whatsapp_message(
    state: &AppState,
    to: &str,
    message: &str,
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

    Ok(())
}
