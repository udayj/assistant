use axum::{
    body::Body,
    extract::{Path, State},
    http::StatusCode,
    response::Response,
};

use super::AppState;
use tracing::info;
use urlencoding::decode;

pub async fn serve_file(
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

pub async fn serve_assets_file(
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
