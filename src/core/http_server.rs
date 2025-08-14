use axum::{
    extract::Form,
    http::{StatusCode, header::CONTENT_TYPE},
    response::{Json, Response},
    routing::{get, post},
    Router,
    body::Body,
    extract::Path
};
use serde_json::{json, Value};
use std::collections::HashMap;
use tokio::net::TcpListener;
use tower_http::cors::CorsLayer;

pub struct HttpServer;

impl HttpServer {
    pub async fn start(port: u16) -> Result<(), Box<dyn std::error::Error>> {
        let app = Router::new()
            .route("/health", get(health_check))
            .route("/webhook", post(webhook_handler))
            .route("/files/{filename}", get(serve_file)) 
            .layer(CorsLayer::permissive());

        let listener = TcpListener::bind(format!("0.0.0.0:{}", port)).await?;
        println!("HTTP server running on port {}", port);

        axum::serve(listener, app).await?;
        Ok(())
    }
}

async fn health_check() -> (StatusCode, &'static str) {
    (StatusCode::OK, "OK")
}

async fn webhook_handler(Form(payload): Form<HashMap<String, String>>) -> Response<String> {
    println!("Webhook payload: {:?}", payload);
    
    let from = payload.get("From").unwrap_or(&"".to_string()).clone();
    let _body = payload.get("Body").unwrap_or(&"".to_string());

    if from.is_empty() {
        return Response::builder()
            .status(StatusCode::BAD_REQUEST)
            .body("Missing required fields".to_string())
            .unwrap();
    }

    // For testing - assume you have a PDF file ready
    let pdf_path = "assets/KEI Cable LP - Mar 25.pdf"; // Your PDF file
    
    match send_pdf_response(pdf_path).await {
        Ok(twiml) => Response::builder()
            .status(StatusCode::OK)
            .header(CONTENT_TYPE, "text/xml")
            .body(twiml)
            .unwrap(),
        Err(_) => send_text_response("Sorry, couldn't send the file. Please try again.")
    }
}

async fn send_pdf_response(pdf_path: &str) -> Result<String, Box<dyn std::error::Error>> {
    // Upload PDF to temporary public URL (using file.io for simplicity)
    let filename = std::path::Path::new(pdf_path).file_name()
        .unwrap().to_str().unwrap();
    let file_url = format!("https://e5344e41850c.ngrok-free.app/files/pricelist");
    
    let twiml = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<Response>
    <Message>
        <Body>Here's your price list:</Body>
        <Media>{}</Media>
    </Message>
</Response>"#,
        file_url
    );
    
    Ok(twiml)
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
async fn serve_file(Path(filename): Path<String>) -> Result<Response<Body>, StatusCode> {
    match tokio::fs::read("assets/KEI Cable LP - Mar 25.pdf").await {
        Ok(contents) => Ok(Response::builder()
            .status(StatusCode::OK)
            .header("content-type", "application/pdf")
            .body(Body::from(contents))
            .unwrap()),
        Err(_) => Err(StatusCode::NOT_FOUND),
    }
}