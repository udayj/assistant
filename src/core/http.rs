use reqwest::{Client, RequestBuilder, Response};
use std::ops::Deref;
use std::time::Duration;
use thiserror::Error;
use tracing::{error, warn};

#[derive(Debug, Error)]
pub enum RetryError {
    #[error("Request failed after all retries: {0}")]
    AllRetriesFailed(String),
    #[error("Non-retryable error: {0}")]
    NonRetryable(String),
}

#[derive(Clone, Debug)]
pub struct RetryableClient {
    client: Client,
    max_retries: u32,
}

impl RetryableClient {
    pub fn new() -> Self {
        Self {
            client: Client::builder()
                .timeout(Duration::from_secs(45))
                .build()
                .unwrap(),
            max_retries: 3,
        }
    }

    pub fn with_retries(client: Client, max_retries: u32) -> Self {
        Self {
            client,
            max_retries,
        }
    }

    pub async fn execute_with_retry(
        &self,
        request_builder: RequestBuilder,
    ) -> Result<Response, RetryError> {
        let mut last_error = None;

        for attempt in 0..self.max_retries {
            // Clone the request for retry
            let request = match request_builder.try_clone() {
                Some(req) => req,
                None => {
                    return Err(RetryError::NonRetryable(
                        "Request body not cloneable".to_string(),
                    ))
                }
            };

            match request.send().await {
                Ok(response) => {
                    if response.status().is_success() || !should_retry_status(response.status()) {
                        return Ok(response);
                    }
                    warn!(response = ?response, "Error response received on attempt {}; ", (attempt+1));
                    last_error = Some(format!("HTTP {}", response.status()));
                }
                Err(e) => {
                    if !should_retry_error(&e) {
                        return Err(RetryError::NonRetryable(e.to_string()));
                    }
                    warn!(error = ?e, "Error response received on attempt {}; ", (attempt+1));
                    last_error = Some(e.to_string());
                }
            }

            if attempt < self.max_retries - 1 {
                let delay = Duration::from_millis(1000 * (2_u64.pow(attempt + 1)));
                warn!(
                    "Request attempt {} failed, retrying in {:?}",
                    attempt + 1,
                    delay
                );
                tokio::time::sleep(delay).await;
            }
        }
        error!("All retires failed - check server");
        Err(RetryError::AllRetriesFailed(
            last_error.unwrap_or_else(|| "Unknown error".to_string()),
        ))
    }
}

impl Deref for RetryableClient {
    type Target = Client;

    fn deref(&self) -> &Self::Target {
        &self.client
    }
}

fn should_retry_status(status: reqwest::StatusCode) -> bool {
    status.is_server_error() || status == reqwest::StatusCode::TOO_MANY_REQUESTS
}

fn should_retry_error(error: &reqwest::Error) -> bool {
    error.is_timeout() || error.is_connect() || error.is_request()
}
