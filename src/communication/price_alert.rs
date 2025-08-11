use crate::configuration::Context;
use crate::core::service_manager::{Error as ServiceManagerError, ServiceWithReceiver};
use async_trait::async_trait;
use std::sync::Arc;
use teloxide::prelude::*;
use tokio::sync::{mpsc, Mutex};

pub struct PriceAlertService {
    bot: Bot,
    receiver: Option<Arc<Mutex<mpsc::Receiver<String>>>>,
    subscribers: Vec<i64>,
}

#[async_trait]
impl ServiceWithReceiver for PriceAlertService {
    type Context = Context;

    async fn new(context: Context, receiver: Option<Arc<Mutex<mpsc::Receiver<String>>>>) -> Self {
        let bot = Bot::from_env();
        let subscribers = context.config.telegram.price_alert_subscribers.clone();
        Self {
            bot,
            receiver,
            subscribers,
        }
    }

    async fn run(self) -> Result<(), ServiceManagerError> {
        println!("Task running on thread: {:?}", std::thread::current().id());
        if let Some(receiver) = &self.receiver {
            loop {
                println!("starting loop");
                let mut rx = receiver.lock().await;
                println!("acquired mutex guard");
                if let Some(price_message) = rx.recv().await {
                    println!("before releasing locking mechanism");
                    drop(rx); // Release lock before sending messages
                    println!("Got message: {}", price_message);
                    for &chat_id in &self.subscribers {
                        if let Err(e) = self.bot.send_message(ChatId(chat_id), &price_message).await
                        {
                            println!("Failed to send price alert to {}: {}", chat_id, e);
                        }
                    }
                }
            }
        }
        Ok(())
    }
}
