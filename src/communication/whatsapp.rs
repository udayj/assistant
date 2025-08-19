use crate::configuration::Context;
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
use tokio::net::TcpListener;
use tokio::sync::mpsc;
use tower_http::cors::CorsLayer;

#[derive(Clone)]
struct AppState {
    query_fulfilment: Arc<QueryFulfilment>,
    error_sender: mpsc::Sender<String>,
    file_base_url: String,
}

pub struct WhatsAppService {
    port: u16,
    query_fulfilment: QueryFulfilment,
    error_sender: mpsc::Sender<String>,
    file_base_url: String,
}

#[async_trait]
impl ServiceWithErrorSender for WhatsAppService {
    type Context = Context;

    async fn new(context: Context, error_sender: mpsc::Sender<String>) -> Self {
        let query_fulfilment = QueryFulfilment::new(context.clone()).await.unwrap();

        Self {
            port: context.config.whatsapp.webhook_port,
            query_fulfilment,
            error_sender,
            file_base_url: context.config.whatsapp.file_base_url,
        }
    }

    async fn run(self) -> Result<(), ServiceManagerError> {
        let state = AppState {
            query_fulfilment: Arc::new(self.query_fulfilment),
            error_sender: self.error_sender,
            file_base_url: self.file_base_url,
        };

        let app = Router::new()
            .route("/health", get(health_check))
            .route("/webhook", post(webhook_handler))
            .route("/{filename}", get(serve_file))
            .layer(CorsLayer::permissive())
            .with_state(state);

        let listener = TcpListener::bind(format!("0.0.0.0:{}", self.port))
            .await
            .map_err(|e| ServiceManagerError::new(&format!("Failed to bind port: {}", e)))?;

        println!("WhatsApp HTTP server running on port {}", self.port);

        axum::serve(listener, app)
            .await
            .map_err(|e| ServiceManagerError::new(&format!("HTTP server error: {}", e)))?;

        Ok(())
    }
}

async fn health_check() -> (StatusCode, &'static str) {
    (StatusCode::OK, "OK")
}

async fn webhook_handler(
    State(state): State<AppState>,
    Form(payload): Form<HashMap<String, String>>,
) -> Response<String> {
    println!("Webhook payload: {:?}", payload);

    let from = payload.get("From").unwrap_or(&"".to_string()).clone();
    let body = payload.get("Body").unwrap_or(&"".to_string()).clone();

    if from.is_empty() {
        return send_text_response(
            "Error in whatsapp request - please contact admin@avantgardelabs.in",
        );
    }

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
            let error_msg = format!("❌ WhatsApp Query Failed\n\nQuery: {}\nError: {}", body, e);
            let _ = state.error_sender.try_send(error_msg);
            send_text_response("Error during processing - please contact admin@avantgardelabs.in")
        }
    }
}

async fn serve_file(
    State(state): State<AppState>,
    Path(filename): Path<String>,
) -> Result<Response<Body>, StatusCode> {
    let file_path = format!("artifacts/{}", filename);
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
