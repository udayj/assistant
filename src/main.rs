use std::collections::HashMap;

use assistant::claude::Query;
use assistant::core::{Service, ServiceManager};
use assistant::prices::item_prices::{
    Cable, Conductor, PowerControl, PriceList, PricingSystem, Product, HT, LT, Flexible, FlexibleType
};
use assistant::prices::PriceService;
use assistant::query::get_query;
use assistant::quotation::QuoteItem;
use assistant::quotation::{QuotationRequest, QuotationService};
use assistant::pdf::create_quotation_pdf;
use chrono::{Datelike,Local};
use rand::prelude::*;

#[tokio::main]
async fn main() {
    let mut service_manager = ServiceManager::new();
    service_manager.spawn::<PriceService>();
    let price_service = PriceService::new().await;
    //price_service.fetch_price("copper").await.unwrap();
    let json_data = std::fs::read_to_string("pricelist.json").unwrap();
    let price_list: PriceList = serde_json::from_str(&json_data).unwrap();

    //println!("price list:{:#?}", price_list);
    println!("Deserialized successfully");
    //return;
    // Create pricing system
    let pricing_system = PricingSystem::from_price_list(price_list);

    // Query examples
    let mut quotation_service = QuotationService {
        pricelists: HashMap::new(),
    };

    quotation_service
        .pricelists
        .insert("kei".to_string(), vec![pricing_system]);

    let quotation_request = QuotationRequest {
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
                product: Product::Cable(Cable::PowerControl(PowerControl::Flexible(Flexible {
                    core_size: "4".to_string(),
                    sqmm: "2.5".to_string(),
                    flexible_type: FlexibleType::FR,
                }))),
                brand: "kei".to_string(),
                tag: "latest".to_string(),
                discount: 0.10,     // 15% discount
                loading_frls: 0.0, // 7% FRLS loading
                loading_pvc: 0.0,  // 2% PVC loading
                quantity: 50.0,     // 50 meters
            },
        ],
        delivery_charges: 500.0,
    };
    let file_content = std::fs::read_to_string("sample_response.json").unwrap();
    let date = Local::now().date_naive();
    let formatted_date = date.format("%Y%m%d").to_string();
    let quotation_response = serde_json::from_str(&file_content).unwrap();
    let mut random_gen = rand::rng();
    let random_q_num = random_gen.random_range(1000..=9999);
    let quotation_number = format!("Ref: Q-{}-{}",formatted_date, random_q_num);
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
    let _ = create_quotation_pdf(&quotation_number, &quotation_date, &quotation_response, "quotation.pdf").unwrap();
    return;
    let quotation_response = quotation_service
                .generate_quotation(&quotation_number, &quotation_date, quotation_request)
                .expect("Failed to generate quotation");
    println!("quotation response:{:#?}", quotation_response);

    let query_request =
        get_query("provide quotes for 3 C x 2.5 cu armd 50 m, 4 c x 2.5 fr cu flex 100 M")
            .await
            .unwrap();
    println!("query:{:#?}", query_request);


    /*let file_content = std::fs::read_to_string("sample_query.json").unwrap();
    let query_request = serde_json::from_str(&file_content).unwrap();
    println!("query:{:#?}", query_request);*/
    match query_request {
        Query::GetQuotation(quotation_request) => {
            let quotation_response = quotation_service
                .generate_quotation(&quotation_number, &quotation_date, quotation_request)
                .expect("Failed to generate quotation");
            println!("quotation response:{:#?}", quotation_response);
        },
        _ => println!("todo")
    }
    // Generate quotation

    /*if let Some(price) = pricing_system.get_price(&copper_lt, "latest") {
        println!("Copper LT cable (kei): Rs.{:.2}", price);
    }*/
}
