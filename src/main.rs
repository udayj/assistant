use assistant::{communication::error_alert::ErrorAlertService, stock::StockService};
use assistant::communication::price_alert::PriceAlertService;
use assistant::communication::telegram::TelegramService;
use assistant::communication::whatsapp::WhatsAppService;
use assistant::configuration::Context;
use assistant::core::ServiceManager;
use assistant::prices::PriceService;
use assistant::AppError;
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

#[cfg(test)]
mod tests {

    use assistant::pdf::create_quotation_pdf;
    use assistant::pdf::DocumentType;
    use assistant::prices::item_prices::{
        Cable, Conductor, Flexible, FlexibleType, PowerControl, Product, LT,
    };
    use assistant::quotation::{QuotationRequest, QuoteItem};
    use chrono::{Datelike, Local};
    use rand::prelude::*;

    #[ignore = "dummy"]
    #[tokio::test]
    async fn test_various() {
        let _quotation_request = QuotationRequest {
            items: vec![
                QuoteItem {
                    product: Product::Cable(Cable::PowerControl(PowerControl::LT(LT {
                        conductor: Conductor::Copper,
                        core_size: "3".to_string(),
                        sqmm: "2.5".to_string(),
                        armoured: true,
                    }))),
                    brand: "kei".to_string(),
                    tag: "latest".to_string(), // assuming this tag exists in your pricing_system
                    discount: 0.10,            // 10% discount
                    loading_frls: 0.05,        // 5% FRLS loading
                    loading_pvc: 0.03,         // 3% PVC loading
                    quantity: 100.0,           // 100 meters
                },
                QuoteItem {
                    product: Product::Cable(Cable::PowerControl(PowerControl::Flexible(
                        Flexible {
                            core_size: "4".to_string(),
                            sqmm: "2.5".to_string(),
                            flexible_type: FlexibleType::FR,
                        },
                    ))),
                    brand: "kei".to_string(),
                    tag: "latest".to_string(),
                    discount: 0.10,    // 15% discount
                    loading_frls: 0.0, // 7% FRLS loading
                    loading_pvc: 0.0,  // 2% PVC loading
                    quantity: 50.0,    // 50 meters
                },
            ],
            delivery_charges: 500.0,
            to: None,
            terms_and_conditions: None,
        };

        let file_content = std::fs::read_to_string("sample_response.json").unwrap();
        let date = Local::now().date_naive();
        let formatted_date = date.format("%Y%m%d").to_string();
        let quotation_response = serde_json::from_str(&file_content).unwrap();
        let mut random_gen = rand::rng();
        let random_q_num = random_gen.random_range(1000..=9999);
        let quotation_number = format!("Ref: Q-{}-{}", formatted_date, random_q_num);
        let now = Local::now();

        // Get day, month, and year
        let day = now.day();
        let month = now.format("%B"); // Full month name, e.g., "August"
        let year = now.year();

        // Determine the ordinal suffix for the day
        let suffix = match day {
            1 | 21 | 31 => "st",
            2 | 22 => "nd",
            3 | 23 => "rd",
            _ => "th",
        };

        // Format the date as a string
        let quotation_date = format!("{}{} {}, {}", day, suffix, month, year);
        let _ = create_quotation_pdf(
            &quotation_number,
            &quotation_date,
            &quotation_response,
            "quotation.pdf",
            DocumentType::ProformaInvoice,
        )
        .unwrap();
        return;
    }
}
