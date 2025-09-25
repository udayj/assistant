use crate::communication::websocket::websocket_handler;
use crate::configuration::Context;
use crate::core::http::RetryableClient;
use crate::core::service_manager::{Error as ServiceManagerError, ServiceWithErrorSender};
use crate::database::DatabaseService;
use crate::database::{SessionContext, SessionResult, User};
use crate::query::QueryFulfilment;
use crate::stock::StockService;
use async_trait::async_trait;
use axum::extract::WebSocketUpgrade;
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
use tokio::sync::mpsc::Sender;
use tower_http::cors::CorsLayer;
use tracing::{error, info};
use urlencoding::{decode, encode};
use uuid::Uuid;

#[derive(Debug, Error)]
pub enum WhatsAppError {
    #[error("Error initializing query fulfilment service: {0}")]
    QueryFulfilmentInitError(String),
    #[error("Image processing error: {0}")]
    ImageProcessingError(String),
}

#[derive(Clone)]
pub struct AppState {
    query_fulfilment: Arc<QueryFulfilment>,
    error_sender: mpsc::Sender<String>,
    file_base_url: String,
    twilio_account_sid: String,
    twilio_auth_token: String,
    http_client: RetryableClient,
    database: Arc<DatabaseService>,
    pub stock_service: Arc<StockService>,
}

pub struct WhatsAppService {
    port: u16,
    query_fulfilment: QueryFulfilment,
    error_sender: mpsc::Sender<String>,
    file_base_url: String,
    twilio_account_sid: String,
    twilio_auth_token: String,
    http_client: RetryableClient,
    database: Arc<DatabaseService>,
    stock_service: Arc<StockService>,
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
            database: context.database.clone(),
            stock_service: context.stock_service.clone(),
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
            database: self.database,
            stock_service: self.stock_service.clone(),
        };

        let app = Router::new()
            .route("/health", get(health_check))
            .route("/webhook", post(webhook_handler))
            .route("/artifacts/{*filename}", get(serve_file))
            .route("/assets/pricelists/{*filename}", get(serve_assets_file))
            .route("/ws", get(whatsapp_websocket_handler))
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

async fn whatsapp_websocket_handler(
    ws: WebSocketUpgrade,
    State(app_state): State<AppState>,
) -> Response {
    let stock_service = app_state.stock_service.as_ref().clone();
    websocket_handler(ws, axum::extract::State(stock_service)).await
}

async fn health_check() -> (StatusCode, &'static str) {
    (StatusCode::OK, "OK")
}

// Main whatsapp webhook
// This is also the end-point which gets pinged with an error payload from twilio
async fn webhook_handler(
    State(state): State<AppState>,
    Form(payload): Form<HashMap<String, String>>,
) -> Response<String> {
    info!("Webhook payload: {:?}", payload);

    let from = payload.get("From").unwrap_or(&"".to_string()).clone();
    let body = payload.get("Body").unwrap_or(&"".to_string()).clone();

    let phone = from.strip_prefix("whatsapp:").unwrap_or(&from);

    // Define default IDs for unauthorized users
    let default_user_id = Uuid::parse_str("00000000-0000-0000-0000-000000000000").unwrap();
    let default_context =
        SessionContext::new(default_user_id, "whatsapp").with_phone(phone.to_string());
    let user = match state.database.get_user_by_phone(phone).await {
        Ok(Some(user)) => {
            if !state.database.is_user_authorized(&user).await {
                // Log cost for unauthorized user
                let _ = state
                    .database
                    .create_session_with_context(&default_context, &body, "unauthorized")
                    .await;
                let _ = state
                    .database
                    .log_whatsapp_message(&default_context, false, body.len(), false)
                    .await;

                return send_text_response("Access denied", &state, &default_context).await;
            }
            user
        }
        Ok(None) => {
            // Log cost for unknown user

            let _ = state
                .database
                .create_session_with_context(&default_context, &body, "unknown_user")
                .await;
            let _ = state
                .database
                .log_whatsapp_message(&default_context, false, body.len(), false)
                .await;

            return send_text_response("Access denied", &state, &default_context).await;
        }
        Err(_) => {
            return send_text_response("System error", &state, &default_context).await;
        }
    };

    let start_time = std::time::Instant::now();
    let context = create_session_context(&state, &user, &body, "text_query").await;
    let _ = state
        .database
        .log_whatsapp_message(&context, false, body.len(), false)
        .await;

    if body.trim() == "/help" || body.trim() == "help" {
        return send_text_response(&QueryFulfilment::get_help_text(), &state, &context).await;
    }

    if let Some(media_url) = payload.get("MediaUrl0") {
        let no_media_type = "".to_string();
        let media_type = payload.get("MediaContentType0").unwrap_or(&no_media_type);

        if !media_type.starts_with("image/") || media_type.is_empty() {
            return send_text_response(
                "Please send only images with your request",
                &state,
                &context,
            )
            .await;
        }

        // Process all image queries asynchronously
        let state_clone = state.clone();
        let from_clone = from.clone();
        let media_url_clone = media_url.clone();
        let body_clone = body.clone();
        let mut context_clone = context.clone();

        tokio::spawn(async move {
            match download_and_process_image(
                &state_clone,
                &media_url_clone,
                &body_clone,
                &mut context_clone,
                &state_clone.error_sender,
            )
            .await
            {
                Ok(response) => {
                    if let Some(file_path) = response.file {
                        let parts: Vec<&str> = file_path.split('/').collect();
                        let encoded_parts: Vec<String> =
                            parts.iter().map(|part| encode(part).to_string()).collect();
                        let encoded_path = encoded_parts.join("/");
                        let file_url = format!("{}/{}", state_clone.file_base_url, encoded_path);
                        let _ = send_whatsapp_message_with_media(
                            &state_clone,
                            &from_clone,
                            &file_url,
                            &context_clone,
                        )
                        .await;
                    } else {
                        let _ = send_whatsapp_message(
                            &state_clone,
                            &from_clone,
                            &response.text,
                            &context_clone,
                        )
                        .await;
                    }
                    let result = SessionResult {
                        success: true,
                        error_message: None,
                        processing_time_ms: start_time.elapsed().as_millis() as i32,
                        query_metadata: response.query_metadata,
                    };
                    let query_text = format!("Image query: {}", body_clone);
                    let _ = state_clone
                        .database
                        .complete_session_with_notification(
                            &context_clone,
                            result,
                            &query_text,
                            &state_clone.error_sender,
                        )
                        .await;
                }
                Err(e) => {
                    let error_msg = format!(
                        "❌ WhatsApp Image Query Failed\nText: {}\nError: {}",
                        body_clone, e
                    );
                    let result = SessionResult {
                        success: false,
                        error_message: Some(e.to_string()),
                        processing_time_ms: start_time.elapsed().as_millis() as i32,
                        query_metadata: None,
                    };
                    let query_text = format!("Image query: {}", body_clone);
                    let _ = state_clone
                        .database
                        .complete_session_with_notification(
                            &context_clone,
                            result,
                            &query_text,
                            &state_clone.error_sender,
                        )
                        .await;
                    let _ = state_clone.error_sender.try_send(error_msg);
                    let _ = send_whatsapp_message(
                        &state_clone,
                        &from_clone,
                        "Could not process image - please try again with clearer image and text",
                        &context_clone,
                    )
                    .await;
                }
            }
        });

        send_text_response("Processing your request...please wait ⏳", &state, &context).await
    } else {
        // Process all text queries asynchronously
        let state_clone = state.clone();
        let from_clone = from.clone();
        let body_clone = body.clone();
        let mut context_clone = context.clone();

        tokio::spawn(async move {
            match state_clone
                .query_fulfilment
                .fulfil_query(&body_clone, &mut context_clone, &state_clone.error_sender)
                .await
            {
                Ok(response) => {
                    if let Some(file_path) = response.file {
                        let parts: Vec<&str> = file_path.split('/').collect();
                        let encoded_parts: Vec<String> =
                            parts.iter().map(|part| encode(part).to_string()).collect();
                        let encoded_path = encoded_parts.join("/");
                        let file_url = format!("{}/{}", state_clone.file_base_url, encoded_path);
                        info!(file_url, %file_url, "File url");
                        let _ = send_whatsapp_message_with_media(
                            &state_clone,
                            &from_clone,
                            &file_url,
                            &context_clone,
                        )
                        .await;
                    } else {
                        let _ = send_whatsapp_message(
                            &state_clone,
                            &from_clone,
                            &response.text,
                            &context_clone,
                        )
                        .await;
                    }
                    let result = SessionResult {
                        success: true,
                        error_message: None,
                        processing_time_ms: start_time.elapsed().as_millis() as i32,
                        query_metadata: response.query_metadata,
                    };
                    let _ = state_clone
                        .database
                        .complete_session_with_notification(
                            &context_clone,
                            result,
                            &body_clone,
                            &state_clone.error_sender,
                        )
                        .await;
                }
                Err(e) => {
                    let error_msg = format!(
                        "❌ Background Processing Failed\nQuery: {}\nError: {}",
                        body_clone, e
                    );
                    let result = SessionResult {
                        success: false,
                        error_message: Some(e.to_string()),
                        processing_time_ms: start_time.elapsed().as_millis() as i32,
                        query_metadata: None,
                    };
                    let _ = state_clone
                        .database
                        .complete_session_with_notification(
                            &context_clone,
                            result,
                            &body_clone,
                            &state_clone.error_sender,
                        )
                        .await;
                    let _ = state_clone.error_sender.try_send(error_msg);
                    let error_response = match e {
                        crate::query::QueryError::MetalPricingError(_) =>
                            "Could not fetch metal prices - please try again later",
                        crate::query::QueryError::QuotationServiceError =>
                            "Error generating quotation - please check whether items are valid",
                        crate::query::QueryError::LLMError(_) =>
                            "Unable to understand query correctly",
                        _ => "Could not service request - please try again later",
                    };
                    let _ = send_whatsapp_message(
                        &state_clone,
                        &from_clone,
                        error_response,
                        &context_clone,
                    )
                    .await;
                }
            }
        });

        send_text_response("Processing your request...please wait ⏳", &state, &context).await
    }
}

async fn serve_file(
    State(state): State<AppState>,
    Path(filename): Path<String>,
) -> Result<Response<Body>, StatusCode> {
    let decoded_filename = decode(&filename).map_err(|_| StatusCode::BAD_REQUEST)?;
    let file_path = format!("artifacts/{}", decoded_filename);
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

async fn serve_assets_file(
    State(state): State<AppState>,
    Path(filename): Path<String>,
) -> Result<Response<Body>, StatusCode> {
    let decoded_filename = decode(&filename).map_err(|_| StatusCode::BAD_REQUEST)?;
    let file_path = format!("assets/pricelists/{}", decoded_filename);
    info!(file_path, %file_path, "File path");
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

async fn create_session_context(
    state: &AppState,
    user: &User,
    query_text: &str,
    query_type: &str,
) -> SessionContext {
    let phone = user
        .phone_number
        .as_ref()
        .and_then(|p| p.strip_prefix("whatsapp:"))
        .map(|p| p.to_string());

    let context = SessionContext::new(user.id, "whatsapp").with_phone(phone.unwrap_or_default());

    let _ = state
        .database
        .create_session_with_context(&context, query_text, query_type)
        .await;

    context
}

async fn send_text_response(
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

async fn download_and_process_image(
    state: &AppState,
    media_url: &str,
    user_text: &str,
    context: &mut SessionContext,
    error_sender: &Sender<String>,
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
        .fulfil_image_query(&image_data, user_text, context, error_sender)
        .await
        .map_err(|e| WhatsAppError::QueryFulfilmentInitError(e.to_string()))
}

async fn send_whatsapp_message_with_media(
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
async fn send_whatsapp_message(
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
