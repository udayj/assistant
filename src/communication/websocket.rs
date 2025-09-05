use crate::prices::utils::get_local_time;
use axum::{
    extract::{
        ws::{Message, WebSocket},
        State, WebSocketUpgrade,
    },
    response::Response,
    routing::get,
    Router,
};
use futures_util::SinkExt;
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::net::TcpListener;
use tokio::sync::{mpsc, oneshot, Mutex};
use tracing::info;
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
    tally_sender: Arc<Mutex<Option<mpsc::Sender<String>>>>,
    pending_requests: Arc<Mutex<HashMap<String, oneshot::Sender<String>>>>,
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

    async fn handle_tally_response(&self, response_json: &str) {
        if let Ok(response) = serde_json::from_str::<StockResponse>(response_json) {
            let mut pending = self.pending_requests.lock().await;
            if let Some(sender) = pending.remove(&response.id) {
                let result = response.error.unwrap_or(response.stock_info);
                let _ = sender.send(result);
            }
        }
    }
}

pub struct WebSocketService {
    port: u16,
    stock_service: StockService,
}

impl WebSocketService {
    pub async fn new() -> Self {
        Self {
            port: 8081,
            stock_service: StockService::new(),
        }
    }

    pub fn get_stock_service(&self) -> StockService {
        self.stock_service.clone()
    }

    pub async fn run(self) -> Result<(), crate::core::service_manager::Error> {
        let app = Router::new()
            .route("/ws", get(websocket_handler))
            .with_state(self.stock_service);

        let listener = TcpListener::bind(format!("0.0.0.0:{}", self.port))
            .await
            .map_err(|e| {
                crate::core::service_manager::Error::new(&format!("Bind failed: {}", e))
            })?;
        println!("Listening on websocket port");
        axum::serve(listener, app)
            .await
            .map_err(|e| crate::core::service_manager::Error::new(&format!("Server error: {}", e)))
    }
}

pub async fn websocket_handler(
    ws: WebSocketUpgrade,
    State(stock_service): State<StockService>,
) -> Response {
    //let stock_service= (*state.stock_service).clone();
    ws.on_upgrade(move |socket| handle_tally_connection(socket, stock_service))
}

pub async fn handle_tally_connection(socket: WebSocket, stock_service: StockService) {
    let (mut ws_sender, mut ws_receiver) = socket.split();
    let (tx, mut rx) = mpsc::channel::<String>(100);

    // Register this connection as THE Tally client
    *stock_service.tally_sender.lock().await = Some(tx);
    info!("Tally sender registered at:{}", get_local_time());
    // Handle outgoing messages to Tally
    tokio::spawn(async move {
        while let Some(msg) = rx.recv().await {
            if ws_sender.send(Message::Text(msg.into())).await.is_err() {
                break;
            }
        }
    });

    // Handle incoming responses from Tally
    while let Some(msg) = ws_receiver.next().await {
        info!("Message received from tally at:{}", get_local_time());
        if let Ok(Message::Text(text)) = msg {
            stock_service.handle_tally_response(&text).await;
        } else {
            info!("Connection disconnected at:{}", get_local_time());
            break;
        }
    }

    // Clean up on disconnect
    info!("Tally sender cleaned at:{}", get_local_time());
    *stock_service.tally_sender.lock().await = None;
}
