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

// Handles websocket upgrade request forwarded by the webserver
// We cannot create a different webserver listening on port 8081 because DO app platform lets us use only 1 port
pub async fn websocket_handler(
    ws: WebSocketUpgrade,
    State(stock_service): State<StockService>,
) -> Response {
    ws.on_upgrade(move |socket| handle_connection(socket, stock_service))
}

pub async fn handle_connection(socket: WebSocket, stock_service: StockService) {
    let (ws_sender, mut ws_receiver) = socket.split();
    // Create mpsc channel - the sender will be provided to the stock service which will
    // use it to return stock query responses after talking to the tally client
    let (tx, mut rx) = mpsc::channel::<String>(100);
    let ws_sender = Arc::new(Mutex::new(ws_sender));
    // Register this connection as THE Tally client - whenever there is a new call to handle_connection
    // It means that either this is the first connection or previous connection got broken
    // hence we can overwrite the sender in the stock service
    *stock_service.tally_sender.lock().await = Some(tx);
    info!("Tally sender registered at:{}", get_local_time());
    // Handle outgoing messages to Tally
    let sender = Arc::clone(&ws_sender);
    tokio::spawn(async move {
        // The rx Receiver is used to receive async messages sent by the stock service
        // These message are actually stock queries that will be forwarded to the tally client to get the stock status
        // In effect the tally client actually behaves like a stock data server for practical purposes
        while let Some(msg) = rx.recv().await {
            if sender
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
            // Send tally client response to stock service for forwarding to query fulfilment
            stock_service.handle_tally_response(&text).await;
        } else {
            error!("Message:{:#?}", msg);
            info!("Connection disconnected at:{}", get_local_time());
            break;
        }
    }
}
