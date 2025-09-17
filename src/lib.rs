pub mod communication;
pub mod configuration;
pub mod core;
pub mod database;
pub mod llm;
pub mod ocr;
pub mod pdf;
pub mod prices;
pub mod query;
pub mod quotation;
pub mod stock;
pub mod transcription;

use thiserror::Error;

#[derive(Debug, Error)]
pub enum AppError {
    #[error("Config Error:{0}")]
    ConfigError(String),

    #[error("Service error")]
    ServiceError,
}
