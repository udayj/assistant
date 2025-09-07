use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{mpsc, oneshot, Mutex};
use tracing::error;
use uuid::Uuid;

#[derive(Serialize, Deserialize)]
pub struct StockRequest {
    pub id: String,
    pub query: String,
}

#[derive(Serialize, Deserialize)]
pub struct StockResponse {
    pub id: String,
    pub stock_info: String,
    pub error: Option<String>,
}

#[derive(Clone)]
pub struct StockService {
    pub tally_sender: Arc<Mutex<Option<mpsc::Sender<String>>>>,
    pub pending_requests: Arc<Mutex<HashMap<String, oneshot::Sender<String>>>>,
}

impl StockService {
    pub fn new() -> Self {
        Self {
            tally_sender: Arc::new(Mutex::new(None)),
            pending_requests: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub async fn request_stock(&self, query: String) -> Result<String, String> {
        let request_id = Uuid::new_v4().to_string();
        let (tx, rx) = oneshot::channel();

        // Store pending request
        self.pending_requests
            .lock()
            .await
            .insert(request_id.clone(), tx);

        // Send request to Tally
        let request = StockRequest {
            id: request_id,
            query,
        };

        let sender = self.tally_sender.lock().await;
        if let Some(sender) = sender.as_ref() {
            sender
                .send(serde_json::to_string(&request).unwrap())
                .await
                .map_err(|_| "Failed to send request to Tally")?;
        } else {
            error!("Tally client not connected at the time of stock request");
            return Err("Tally client not connected".to_string());
        }
        drop(sender);

        // Wait for response with timeout
        match tokio::time::timeout(Duration::from_secs(10), rx).await {
            Ok(Ok(response)) => Ok(response),
            Ok(Err(_)) => Err("Request cancelled".to_string()),
            Err(_) => Err("Request timeout".to_string()),
        }
    }

    pub async fn handle_tally_response(&self, response_json: &str) {
        if let Ok(response) = serde_json::from_str::<StockResponse>(response_json) {
            let mut pending = self.pending_requests.lock().await;
            if let Some(sender) = pending.remove(&response.id) {
                let result = response.error.unwrap_or(response.stock_info);
                let _ = sender.send(result);
            }
        }
    }
}
