use serde::Deserialize;
use std::fs;
use std::sync::Arc;
use thiserror::Error;

use crate::database::DatabaseService;
use crate::stock::StockService;

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("File read error")]
    FileError,

    #[error("Deserialization error:{0}")]
    DeserializationError(String),
}

#[derive(Debug, Deserialize, Clone)]
pub struct Config {
    pub log_level: String,
    pub pricelists: Vec<PriceListConfig>,
    pub pdf_pricelists: Vec<PdfPriceListConfig>,
    pub metal_pricing: MetalPricingConfig,
    pub claude: ClaudeConfig,
    pub telegram: TelegramConfig,
    pub whatsapp: WhatsappConfig,
}

#[derive(Debug, Deserialize, Clone)]
pub struct MetalPricingConfig {
    pub al_url: String,
    pub cu_url: String,
}

#[derive(Debug, Deserialize, Clone)]
pub struct ClaudeConfig {
    pub system_prompt: String,
}

#[derive(Debug, Deserialize, Clone)]
pub struct PriceListConfig {
    pub pricelist: String,
    pub brand: String,
}

#[derive(Debug, Deserialize, Clone)]
pub struct PdfPriceListConfig {
    pub pdf_path: String,
    pub brand: String,
    pub keywords: Vec<String>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct TelegramConfig {
    pub price_alert_subscribers: Vec<i64>,
    pub error_channel_id: i64,
}

#[derive(Debug, Deserialize, Clone)]
pub struct WhatsappConfig {
    pub webhook_port: u16,
    pub file_base_url: String,
    pub price_alert_subscribers: Vec<String>,
    pub twilio_from_number: String,
    pub template_sid: String,
}

#[derive(Clone)]
pub struct Context {
    pub config: Config,
    pub database: Arc<DatabaseService>,
    pub stock_service: Arc<StockService>,
}

impl Context {
    pub fn new(config_file: &str, stock_service: Arc<StockService>) -> Result<Self, ConfigError> {
        let database = DatabaseService::new().map_err(|e| {
            ConfigError::DeserializationError(format!("Database init failed: {}", e))
        })?;
        Ok(Self {
            config: Config::new(config_file)?,
            database: Arc::new(database),
            stock_service,
        })
    }
}

impl Config {
    pub fn new(config_file: &str) -> Result<Self, ConfigError> {
        let config_str = fs::read_to_string(config_file).map_err(|_| ConfigError::FileError)?;
        let config: Config = serde_json::from_str(&config_str)
            .map_err(|e| ConfigError::DeserializationError(e.to_string()))?;
        Ok(config)
    }
}
