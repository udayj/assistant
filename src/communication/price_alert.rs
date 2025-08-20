use crate::configuration::Context;
use crate::core::service_manager::{Error as ServiceManagerError, ServiceWithReceiver};
use async_trait::async_trait;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::env;
use std::sync::Arc;
use teloxide::prelude::*;
use tokio::sync::{mpsc, Mutex};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PriceAlert {
    pub timestamp: String,
    pub copper_price: f64,
    pub aluminum_price: f64,
}

pub struct PriceAlertService {
    bot: Bot,
    receiver: Option<Arc<Mutex<mpsc::Receiver<String>>>>,
    telegram_subscribers: Vec<i64>,
    // WhatsApp fields
    whatsapp_client: Client,
    whatsapp_subscribers: Vec<String>,
    twilio_account_sid: String,
    twilio_auth_token: String,
    twilio_from_number: String,
    template_sid: String,
}

#[async_trait]
impl ServiceWithReceiver for PriceAlertService {
    type Context = Context;

    async fn new(context: Context, receiver: Option<Arc<Mutex<mpsc::Receiver<String>>>>) -> Self {
        let bot = Bot::from_env();
        let subscribers = context.config.telegram.price_alert_subscribers.clone();
        let whatsapp_config = &context.config.whatsapp;
        let twilio_account_sid = env::var("TWILIO_ACCOUNT_SID").unwrap();
        let twilio_auth_token = env::var("TWILIO_AUTH_TOKEN").unwrap();

        Self {
            bot,
            receiver,
            telegram_subscribers: subscribers,
            whatsapp_client: Client::new(),
            whatsapp_subscribers: whatsapp_config.price_alert_subscribers.clone(),
            twilio_account_sid,
            twilio_auth_token,
            twilio_from_number: whatsapp_config.twilio_from_number.clone(),
            template_sid: whatsapp_config.template_sid.clone(),
        }
    }

    async fn run(self) -> Result<(), ServiceManagerError> {
        if let Some(receiver) = &self.receiver {
            loop {
                let mut rx = receiver.lock().await;
                if let Some(message) = rx.recv().await {
                    drop(rx);

                    let alert: PriceAlert = serde_json::from_str(&message)
                        .map_err(|_| ServiceManagerError::new("Failed to parse price alert"))?;

                    // Send to Telegram subscribers
                    self.send_telegram_alerts(&alert).await;

                    // Send to WhatsApp subscribers
                    self.send_whatsapp_alerts(&alert).await;
                }
            }
        }
        Ok(())
    }
}

impl PriceAlertService {
    async fn send_telegram_alerts(&self, alert: &PriceAlert) {
        let message = format!(
            "🔔 Metal Price Update\n  {}\n\n🟤 Copper: Rs. {:.2}\n⚪ Aluminium: Rs. {:.2}",
            alert.timestamp, alert.copper_price, alert.aluminum_price
        );

        for &chat_id in &self.telegram_subscribers {
            if let Err(e) = self.bot.send_message(ChatId(chat_id), &message).await {
                println!("Failed to send Telegram alert to {}: {}", chat_id, e);
            }
        }
    }

    async fn send_whatsapp_alerts(&self, alert: &PriceAlert) {
        for subscriber in &self.whatsapp_subscribers {
            if let Err(e) = self.send_whatsapp_template(alert, subscriber).await {
                println!("Failed to send WhatsApp alert to {}: {}", subscriber, e);
            }
        }
    }

    async fn send_whatsapp_template(
        &self,
        alert: &PriceAlert,
        to: &str,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let url = format!(
            "https://api.twilio.com/2010-04-01/Accounts/{}/Messages.json",
            self.twilio_account_sid
        );

        let params = json!({
            "From": self.twilio_from_number,
            "To": to,
            "ContentSid": self.template_sid,
            "ContentVariables": json!({
                "1": alert.timestamp,
                "2": format!("Rs. {:.2}", alert.copper_price),
                "3": format!("Rs. {:.2}", alert.aluminum_price)
            }).to_string()
        });

        self.whatsapp_client
            .post(&url)
            .basic_auth(&self.twilio_account_sid, Some(&self.twilio_auth_token))
            .form(&params)
            .send()
            .await?;

        Ok(())
    }
}
