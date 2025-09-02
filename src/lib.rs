pub mod claude;
pub mod communication;
pub mod configuration;
pub mod core;
pub mod database;
pub mod ocr;
pub mod pdf;
pub mod prices;
pub mod query;
pub mod quotation;

use thiserror::Error;

#[derive(Debug, Error)]
pub enum AppError {
    #[error("Config Error:{0}")]
    ConfigError(String),

    #[error("Service error")]
    ServiceError,
}
