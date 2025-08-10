pub mod core;
pub mod prices;
pub mod quotation;
pub mod query;
pub mod claude;
pub mod pdf;
pub mod configuration;
pub mod communication;

use thiserror::Error;

#[derive(Debug, Error)]
pub enum AppError{

    #[error("Config Error:{0}")]
    ConfigError(String),

    #[error("Service error")]
    ServiceError
}