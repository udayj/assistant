use crate::communication::session_helpers::{
    create_session_or_error, create_whatsapp_session_context,
};
use crate::communication::websocket::websocket_handler;
use crate::configuration::Context;
use crate::core::http::RetryableClient;
use crate::core::service_manager::{Error as ServiceManagerError, ServiceWithErrorSender};
use crate::database::DatabaseService;
use crate::database::{SessionContext, User};
use crate::query::QueryFulfilment;
use crate::stock::StockService;
use async_trait::async_trait;
use axum::extract::WebSocketUpgrade;
use axum::{
    extract::{Form, State},
    http::StatusCode,
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
use uuid::Uuid;

mod file_serve;
pub mod message_sender;
mod whatsapp_helpers;

use file_serve::{serve_assets_file, serve_file};
use message_sender::send_text_response;
use whatsapp_helpers::{
    convert_whatsapp_error_to_query_error, process_query_response, QueryProcessingParams,
};

#[derive(Debug, Error)]
pub enum WhatsAppError {
    #[error("Error initializing query fulfilment service: {0}")]
    QueryFulfilmentInitError(String),
    #[error("Image processing error: {0}")]
    ImageProcessingError(String),
}

#[derive(Clone)]
pub struct AppState {
    pub query_fulfilment: Arc<QueryFulfilment>,
    pub error_sender: mpsc::Sender<String>,
    pub file_base_url: String,
    pub twilio_account_sid: String,
    pub twilio_auth_token: String,
    pub http_client: RetryableClient,
    pub database: Arc<DatabaseService>,
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

        let query_text = format!("Image query: {}", body);
        let media_url_copy = media_url.clone();
        let user_text = body.clone();
        let params = QueryProcessingParams {
            state: state.clone(),
            from: from.clone(),
            query_text: query_text.clone(),
            context: context.clone(),
            start_time,
        };

        tokio::spawn(async move {
            let result = download_and_process_image(
                &params.state,
                &media_url_copy,
                &user_text,
                &mut params.context.clone(),
                &params.state.error_sender,
            )
            .await
            .map_err(convert_whatsapp_error_to_query_error);

            process_query_response(params, result).await;
        });

        send_text_response("Processing your request...please wait ⏳", &state, &context).await
    } else {
        let params = QueryProcessingParams {
            state: state.clone(),
            from: from.clone(),
            query_text: body.clone(),
            context: context.clone(),
            start_time,
        };

        tokio::spawn(async move {
            let result = params
                .state
                .query_fulfilment
                .fulfil_query(
                    &params.query_text,
                    &mut params.context.clone(),
                    &params.state.error_sender,
                )
                .await;
            process_query_response(params, result).await;
        });

        send_text_response("Processing your request...please wait ⏳", &state, &context).await
    }
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
        .map(|p| p.to_string())
        .unwrap_or_default();

    let context = create_whatsapp_session_context(user, &phone);
    let _ = create_session_or_error(
        &state.database,
        &context,
        query_text,
        query_type,
        &state.error_sender,
    )
    .await;
    context
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
