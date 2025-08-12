use crate::configuration::Context;
use crate::core::cache::ExpirableCache;
use crate::core::service_manager::Error as ServiceManagerError;
use crate::core::{service_manager::ServiceWithSender, Service};
use async_trait::async_trait;
use chrono::{Timelike, Utc, DateTime};
use chrono_tz::Asia::Kolkata;
use reqwest;
use scraper::{Html, Selector};
use std::time::Duration;
use thiserror::Error;
use tokio::sync::mpsc;

pub mod item_prices;

#[derive(Error, Debug)]
pub enum PriceError {
    #[error("Failed to get response:{0}")]
    GetUrlError(String),

    #[error("Failed to Build Client")]
    ClientError,

    #[error("Invalid metal type")]
    InvalidMetalType,

    #[error("HTML parsing error:{0}")]
    HTMLParseError(String),

    #[error("Price not found")]
    PriceNotFoundError,

    #[error("Failed to parse Price")]
    PriceParseError,
}

pub struct PriceService {
    pub url_al: String,
    pub url_cu: String,
    pub price_channel: Option<mpsc::Sender<String>>,
    pub price_cache: ExpirableCache<String, f64>,
    pub last_alert_hour: Option<u32>
}

#[async_trait]
impl Service for PriceService {
    type Context = Context;
    async fn new(context: Context) -> Self {
        Self {
            url_al: context.config.metal_pricing.al_url.to_string(),
            url_cu: context.config.metal_pricing.cu_url.to_string(),
            price_channel: None,
            price_cache: ExpirableCache::new(2, Duration::from_secs(300)),
            last_alert_hour: None
        }
    }

    async fn run(mut self) -> Result<(), ServiceManagerError> {
        loop {
            let now_ist = Utc::now().with_timezone(&Kolkata);
            let hour = now_ist.hour();
            let minute = now_ist.minute();
            
            // Check if we're in a valid time window and haven't sent alert this hour
            let should_send_alert = match hour {
                11 if minute >= 50 && minute <= 52 => {
                    self.last_alert_hour != Some(11)
                }
                15 if minute <= 10 => {
                    self.last_alert_hour != Some(15)
                }
                _ => false,
            };

            if should_send_alert {
                match self.send_price_alert(now_ist).await {
                    Ok(_) => {
                        self.last_alert_hour = Some(hour);
                        println!("Price alert sent successfully at {}:{:02}", hour, minute);
                    }
                    Err(e) => {
                        println!("Failed to send price alert: {}", e);
                        // Continue running even if alert fails
                    }
                }
            }

            tokio::time::sleep(Duration::from_secs(60)).await;
        }
    }
}

#[async_trait]
impl ServiceWithSender for PriceService {
    type Context = Context;

    async fn new(context: Context, price_channel: Option<mpsc::Sender<String>>) -> Self {
        Self {
            url_al: context.config.metal_pricing.al_url.to_string(),
            url_cu: context.config.metal_pricing.cu_url.to_string(),
            price_channel,
            price_cache: ExpirableCache::new(2, Duration::from_secs(300)),
            last_alert_hour: None
        }
    }

    async fn run(self) -> Result<(), ServiceManagerError> {
        <Self as Service>::run(self).await
    }
}

impl PriceService {

    async fn send_price_alert(&self, now_ist: DateTime<chrono_tz::Tz>) -> Result<(), ServiceManagerError> {
        let price_al = self
            .fetch_price("aluminium")
            .await
            .map_err(|e| ServiceManagerError::from(e))?;

        tokio::time::sleep(Duration::from_secs(2)).await;

        let price_cu = self
            .fetch_price("copper")
            .await
            .map_err(|e| ServiceManagerError::from(e))?;

        if let Some(sender) = &self.price_channel {
            let timestamp = now_ist.format("%d/%m/%Y %I:%M %p IST");
            let message = format!(
                "🔔 Metal Price Update\n📅 {}\n\n🟤 Copper: Rs. {:.2}\n⚪ Aluminium: Rs. {:.2}", 
                timestamp, price_cu, price_al
            );
            sender.send(message).await
                .map_err(|e| ServiceManagerError::new(&format!("Failed to send price alert: {}", e)))?;
        }
        Ok(())
    }

    pub async fn fetch_price(&self, metal: &str) -> Result<f64, PriceError> {
        let price = self.price_cache.get(&metal.to_string());
        if price.is_some() {
            return Ok(price.unwrap());
        }

        let client = reqwest::Client::builder()
        .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/91.0.4472.124 Safari/537.36")
        .build()
        .map_err(|_| PriceError::ClientError)?;

        let url = match metal.to_lowercase().as_str() {
            "aluminium" => &self.url_al,
            "copper" => &self.url_cu,
            _ => return Err(PriceError::InvalidMetalType),
        };
        let response = client
            .get(url)
            .header("Accept", "text/html")
            .header("Accept-Language", "en-US,en;q=0.9")
            .send()
            .await
            .map_err(|e| PriceError::GetUrlError(e.to_string()))?
            .text()
            .await
            .map_err(|e| PriceError::GetUrlError(e.to_string()))?;

        let document = Html::parse_document(&response);
        // Updated selectors to match the actual HTML structure
        let value_selector = Selector::parse("div.commodity-page__value")
            .map_err(|e| PriceError::HTMLParseError(e.to_string()))?;

        // Extract the main price value
        let value_element = document
            .select(&value_selector)
            .next()
            .ok_or("Price value not found")
            .map_err(|_| PriceError::PriceNotFoundError)?;

        // Get the main price (before decimal)
        let main_price_text = value_element
            .text()
            .collect::<String>()
            .replace("₹", "")
            .trim()
            .to_string();

        // Parse the combined price string
        let price = main_price_text
            .as_str()
            .parse::<f64>()
            .map_err(|_| PriceError::PriceParseError)?;

        println!("{} price is:{}", metal, price);
        self.price_cache.insert(metal.to_string(), price);
        Ok(price)
    }

    pub async fn fetch_formatted_prices(&self) -> Result<String, PriceError> {
        let price_cu = self.fetch_price("copper").await?;
        tokio::time::sleep(Duration::from_secs(2)).await;
        let price_al = self.fetch_price("aluminium").await?;

        let now_ist = Utc::now().with_timezone(&Kolkata);
        let timestamp = now_ist.format("%d/%m/%Y %I:%M %p IST");
        let message = format!(
            "🔔 Metal Price Update\n📅 {}\n\n🟤 Copper: Rs. {:.2}\n⚪ Aluminium: Rs. {:.2}",
            timestamp, price_cu, price_al
        );
        Ok(message)
    }
}
