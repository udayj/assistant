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

    // Serves user stock queries sent by query fulfilment
    pub async fn request_stock(&self, query: String) -> Result<String, String> {
        let request_id = Uuid::new_v4().to_string();
        // This one-shot channel is used for synchronising request response
        // Any new request is stored in pending_requests with reference to the sender part of this channel
        // When the response is received, tx.send is used to signal that a response was received - this is what
        // enables timeout on the getting the response
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

        // The tally_sender is set by the websocket handler whenever a connection is made or reconnected
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

        // Wait for response with timeout - and send response to query fulfilment
        match tokio::time::timeout(Duration::from_secs(10), rx).await {
            Ok(Ok(response)) => Ok(response),
            Ok(Err(_)) => Err("Request cancelled".to_string()),
            Err(_) => Err("Request timeout".to_string()),
        }
    }

    // This is called by the websocket module whenever it receives a response from tally client
    pub async fn handle_tally_response(&self, response_json: &str) {
        // Parse response
        // Finding pending request
        // Prepare response or error message
        // Send response to waiting thread using the oneshot channel established when sending the request
        if let Ok(response) = serde_json::from_str::<StockResponse>(response_json) {
            let mut pending = self.pending_requests.lock().await;
            if let Some(sender) = pending.remove(&response.id) {
                let result = response.error.unwrap_or(response.stock_info);
                // Calling sender.send actually signals to the tokio::time::timeout function waiting with the receiver
                // that a response was received - that response is then sent to query fulfilment
                // it also enables timeout based request processing
                let _ = sender.send(result);
            }
        }
    }
}
