use axum::{
    body::Body,
    extract::{Path, State},
    http::StatusCode,
    response::Response,
};

use super::AppState;
use tracing::info;
use urlencoding::decode;

/// Validates that a filename is safe and doesn't contain path traversal sequences
fn is_safe_filename(filename: &str) -> bool {
    // Reject if contains path traversal sequences or dangerous characters
    let trimmed = filename.trim();
    !filename.contains("..") &&
    !filename.contains('/') &&
    !filename.contains('\\') &&
    !filename.starts_with('.') &&
    !filename.is_empty() &&
    !trimmed.is_empty() && // Reject whitespace-only filenames
    filename.len() <= 255 // Reasonable filename length limit
}

pub async fn serve_file(
    State(state): State<AppState>,
    Path(filename): Path<String>,
) -> Result<Response<Body>, StatusCode> {
    let decoded_filename = decode(&filename).map_err(|_| StatusCode::BAD_REQUEST)?;

    // Validate filename for path traversal protection
    if !is_safe_filename(&decoded_filename) {
        return Err(StatusCode::BAD_REQUEST);
    }

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

    // Validate filename for path traversal protection
    if !is_safe_filename(&decoded_filename) {
        return Err(StatusCode::BAD_REQUEST);
    }

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_safe_filename_validation() {
        // Valid filenames should pass
        assert!(is_safe_filename("document.pdf"));
        assert!(is_safe_filename("quotation_123.pdf"));
        assert!(is_safe_filename("price-list.pdf"));
        assert!(is_safe_filename("file_name.pdf"));

        // Path traversal attempts should fail
        assert!(!is_safe_filename("../etc/passwd"));
        assert!(!is_safe_filename("..\\windows\\system32"));
        assert!(!is_safe_filename("../../secret.txt"));
        assert!(!is_safe_filename("dir/../file.pdf"));
        assert!(!is_safe_filename("../../../etc/shadow"));

        // Directory separators should fail
        assert!(!is_safe_filename("dir/file.pdf"));
        assert!(!is_safe_filename("folder\\file.pdf"));
        assert!(!is_safe_filename("/etc/passwd"));
        assert!(!is_safe_filename("\\windows\\system32"));

        // Hidden files should fail
        assert!(!is_safe_filename(".env"));
        assert!(!is_safe_filename(".secret"));
        assert!(!is_safe_filename(".bashrc"));

        // Edge cases should fail
        assert!(!is_safe_filename(""));
        assert!(!is_safe_filename(" "));
        assert!(!is_safe_filename(".."));
        assert!(!is_safe_filename("."));

        // Very long filenames should fail
        let long_filename = "a".repeat(256);
        assert!(!is_safe_filename(&long_filename));
    }

    #[test]
    fn test_filename_length_limits() {
        // Test exactly at the limit
        let max_filename = "a".repeat(255);
        assert!(is_safe_filename(&max_filename));

        // Test over the limit
        let over_limit = "a".repeat(256);
        assert!(!is_safe_filename(&over_limit));
    }

    #[test]
    fn test_valid_pdf_filenames() {
        // Test realistic valid filenames for your use case
        assert!(is_safe_filename("quotation_2024_001.pdf"));
        assert!(is_safe_filename("price-list-kei.pdf"));
        assert!(is_safe_filename("invoice_12345.pdf"));
        assert!(is_safe_filename("document.pdf"));
        assert!(is_safe_filename("cable_prices.pdf"));
    }
}