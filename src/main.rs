use assistant::communication::price_alert::PriceAlertService;
use assistant::communication::telegram::TelegramService;
use assistant::communication::whatsapp::WhatsAppService;
use assistant::configuration::Context;
use assistant::core::ServiceManager;
use assistant::prices::PriceService;
use assistant::AppError;
use assistant::{communication::error_alert::ErrorAlertService, stock::StockService};
use dotenvy::dotenv;
use std::str::FromStr;
use std::sync::Arc;
use tokio::sync::{mpsc, Mutex};
use tracing::Level;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

#[tokio::main]
async fn main() -> Result<(), AppError> {
    dotenv().ok();
    let stock_service = StockService::new();
    let stock_service = Arc::new(stock_service);
    let context = Context::new("config.json", stock_service)
        .map_err(|e| AppError::ConfigError(e.to_string()))?;

    let log_level = Level::from_str(&context.config.log_level).unwrap_or(Level::INFO);
    tracing_subscriber::registry()
        .with(tracing_subscriber::fmt::layer())
        .with(tracing_subscriber::EnvFilter::new(log_level.to_string()))
        .init();
    tracing::info!("Starting Assistant Application");

    let mut service_manager = ServiceManager::new(context);
    let (sender, receiver) = mpsc::channel::<String>(100);
    let (error_sender, error_receiver) = mpsc::channel::<String>(100);
    let shared_receiver = Arc::new(Mutex::new(receiver));
    let shared_error_receiver = Arc::new(Mutex::new(error_receiver));

    service_manager.spawn_with_error_receiver::<ErrorAlertService>(shared_error_receiver);
    service_manager.spawn_with_error_sender::<WhatsAppService>(error_sender.clone());
    service_manager.spawn_with_error_sender::<TelegramService>(error_sender);
    service_manager.spawn_with_price_receiver::<PriceAlertService>(shared_receiver);
    service_manager.spawn_with_price_sender::<PriceService>(sender.clone());

    service_manager
        .wait()
        .await
        .map_err(|_| AppError::ServiceError)
}
