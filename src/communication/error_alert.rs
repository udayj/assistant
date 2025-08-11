use crate::configuration::Context;
use crate::core::service_manager::{Error as ServiceManagerError, ServiceWithReceiver};
use async_trait::async_trait;
use std::sync::Arc;
use teloxide::prelude::*;
use tokio::sync::{mpsc, Mutex};
use dotenvy::dotenv;
use std::env;

pub struct ErrorAlertService {
    bot: Bot,
    receiver: Option<Arc<Mutex<mpsc::Receiver<String>>>>,
    channel_id: i64,
}

#[async_trait]
impl ServiceWithReceiver for ErrorAlertService {
    type Context = Context;

    async fn new(context: Context, receiver: Option<Arc<Mutex<mpsc::Receiver<String>>>>) -> Self {
        dotenv().ok();
        let error_bot_token = env::var("ERROR_BOT_TOKEN").expect("ERROR_BOT_TOKEN not found");
        let bot = Bot::new(error_bot_token);
        let channel_id = context.config.telegram.error_channel_id;
        
        Self {
            bot,
            receiver,
            channel_id,
        }
    }

    async fn run(self) -> Result<(), ServiceManagerError> {
        if let Some(receiver) = &self.receiver {
            loop {
                let mut rx = receiver.lock().await;
                if let Some(error_message) = rx.recv().await {
                    drop(rx);
                    if let Err(e) = self.bot.send_message(ChatId(self.channel_id), &error_message).await {
                        println!("Failed to send error alert: {}", e);
                    }
                }
            }
        }
        Ok(())
    }
}
