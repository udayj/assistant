use crate::prices::utils::get_local_time;
use crate::stock::StockService;
use axum::{
    extract::{
        ws::{Message, WebSocket},
        State, WebSocketUpgrade,
    },
    response::Response,
};
use futures_util::SinkExt;
use futures_util::StreamExt;
use std::sync::Arc;
use tokio::sync::{mpsc, Mutex};
use tracing::{error, info};

pub async fn websocket_handler(
    ws: WebSocketUpgrade,
    State(stock_service): State<StockService>,
) -> Response {
    ws.on_upgrade(move |socket| handle_connection(socket, stock_service))
}

pub async fn handle_connection(socket: WebSocket, stock_service: StockService) {
    let (ws_sender, mut ws_receiver) = socket.split();
    let (tx, mut rx) = mpsc::channel::<String>(100);
    let ws_sender = Arc::new(Mutex::new(ws_sender));
    // Register this connection as THE Tally client
    *stock_service.tally_sender.lock().await = Some(tx);
    info!("Tally sender registered at:{}", get_local_time());
    // Handle outgoing messages to Tally
    let sender_clone = Arc::clone(&ws_sender);
    tokio::spawn(async move {
        while let Some(msg) = rx.recv().await {
            if sender_clone
                .lock()
                .await
                .send(Message::Text(msg.into()))
                .await
                .is_err()
            {
                break;
            }
        }
    });

    // Handle incoming responses from tally client
    while let Some(msg) = ws_receiver.next().await {
        info!("Message received from tally at:{}", get_local_time());
        if let Ok(Message::Text(text)) = msg {
            if text == "PING" {
                info!("PING received from tally_client");
                if ws_sender
                    .lock()
                    .await
                    .send(Message::Text("PONG".into()))
                    .await
                    .is_err()
                {
                    break; // Connection broken
                }
                continue;
            }
            stock_service.handle_tally_response(&text).await;
        } else {
            error!("Message:{:#?}", msg);
            info!("Connection disconnected at:{}", get_local_time());
            break;
        }
    }
}
