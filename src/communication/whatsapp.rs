use crate::configuration::Context;
use crate::core::service_manager::{Error as ServiceManagerError, ServiceWithErrorSender};
use crate::query::{QueryError, QueryFulfilment};
use async_trait::async_trait;
use axum::{
    body::Body,
    extract::{Form, Path, State},
    http::{header::CONTENT_TYPE, StatusCode},
    response::Response,
    routing::{get, post},
    Router,
};
use reqwest::Client;
use std::collections::HashMap;
use std::sync::Arc;
use thiserror::Error;
use tokio::net::TcpListener;
use tokio::sync::mpsc;
use tower_http::cors::CorsLayer;
use tracing::info;

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
    http_client: Client,
}

pub struct WhatsAppService {
    port: u16,
    query_fulfilment: QueryFulfilment,
    error_sender: mpsc::Sender<String>,
    file_base_url: String,
    twilio_account_sid: String,
    twilio_auth_token: String,
    http_client: Client,
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
            http_client: Client::new(),
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

    if body.trim() == "/help" {
        return send_text_response(&QueryFulfilment::get_help_text());
    }
    if let Some(media_url) = payload.get("MediaUrl0") {
        let no_media_type = "".to_string();
        let media_type = payload.get("MediaContentType0").unwrap_or(&no_media_type);

        if !media_type.starts_with("image/") || media_type.is_empty() {
            return send_text_response("Please send only images with your request");
        }

        if body.trim().is_empty() {
            return send_text_response(
                "Please send the image with a text message describing what you need",
            );
        }

        match download_and_process_image(&state, media_url, &body).await {
            Ok(response) => {
                if let Some(file_path) = response.file {
                    send_pdf_response(&file_path, &response.text, &state.file_base_url)
                        .await
                        .unwrap_or_else(|_| send_text_response("Error processing request"))
                } else {
                    send_text_response(&response.text)
                }
            }
            Err(e) => {
                let error_msg = format!(
                    "❌ WhatsApp Image Query Failed\n\nText: {}\nError: {}",
                    body, e
                );
                let _ = state.error_sender.try_send(error_msg);
                send_text_response(
                    "Could not process image - please try again with clearer image and text",
                )
            }
        }
    } else {
        match state.query_fulfilment.fulfil_query(&body).await {
            Ok(response) => {
                if let Some(file_path) = response.file {
                    send_pdf_response(&file_path, &response.text, &state.file_base_url)
                        .await
                        .unwrap_or_else(|_| {
                            send_text_response(
                                "Error during processing - please contact admin@avantgardelabs.in",
                            )
                        })
                } else {
                    send_text_response(&response.text)
                }
            }
            Err(e) => {
                let error_msg =
                    format!("❌ WhatsApp Query Failed\n\nQuery: {}\nError: {}", body, e);
                let _ = state.error_sender.try_send(error_msg);
                match e {
                    QueryError::MetalPricingError(_) => {
                        send_text_response("Could not fetch metal prices - please try again later")
                    }
                    QueryError::QuotationServiceError => send_text_response(
                        "Error generating quotation - please check whether items are valid",
                    ),
                    QueryError::LLMError(_) => {
                        send_text_response(&QueryFulfilment::get_help_text())
                    }
                    _ => send_text_response("Could not service request - please try again later"),
                }
            }
        }
    }
}

async fn serve_file(
    State(state): State<AppState>,
    Path(filename): Path<String>,
) -> Result<Response<Body>, StatusCode> {
    let file_path = format!("{}", filename);
    match tokio::fs::read(&file_path).await {
        Ok(contents) => Ok(Response::builder()
            .status(StatusCode::OK)
            .header("content-type", "application/pdf")
            .body(Body::from(contents))
            .unwrap()),
        Err(e) => {
            let error_msg = format!("❌ File Serve Error\n\nFile: {}\nError: {}", file_path, e);
            let _ = state.error_sender.try_send(error_msg);
            Err(StatusCode::NOT_FOUND)
        }
    }
}

async fn send_pdf_response(
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
        .get(media_url)
        .basic_auth(&state.twilio_account_sid, Some(&state.twilio_auth_token))
        .send()
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
