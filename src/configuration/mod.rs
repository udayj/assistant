use serde::Deserialize;
use std::fs;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("File read error")]
    FileError,

    #[error("Deserialization error:{0}")]
    DeserializationError(String),
}

#[derive(Debug, Deserialize, Clone)]
pub struct Config {
    pub pricelists: Vec<PriceListConfig>,
    pub metal_pricing: MetalPricingConfig,
    pub claude: ClaudeConfig,
    pub telegram: TelegramConfig
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
pub struct TelegramConfig {
    pub price_alert_subscribers: Vec<i64>,
    pub error_channel_id: i64,
}

#[derive(Debug, Clone)]
pub struct Context {
    pub config: Config,
}

impl Context {
    pub fn new(config_file: &str) -> Result<Self, ConfigError> {
        Ok(Self {
            config: Config::new(config_file)?,
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
