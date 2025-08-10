use crate::{
    configuration::PriceListConfig,
    prices::item_prices::{PriceList, PricingSystem, Product},
};
use serde::Deserialize;
use std::collections::HashMap;
use std::fs;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum QuotationError {
    #[error("Error reading pricelist file")]
    FileReadError,

    #[error("Error parsing pricelist file")]
    PricelistParseError,
}

#[derive(Debug, Deserialize)]
pub struct QuoteItem {
    pub product: Product,
    pub brand: String,
    pub tag: String,
    pub discount: f32,     // in percentage eg. 0.70 means 70%
    pub loading_frls: f32, // in percentage eg. 0.05 means 5%
    pub loading_pvc: f32,  // in percentage eg. 0.05 means 5%
    pub quantity: f32,
}

#[derive(Debug, Deserialize)]
pub struct QuotationRequest {
    pub items: Vec<QuoteItem>,
    pub delivery_charges: f32,
}

#[derive(Debug, Deserialize)]
pub struct QuotedItem {
    pub product: Product,
    pub quantity_mtrs: f32,
    pub price: f32, // price = listed_price*(1-discount)*(1+loading_frls)*(1+loading_pvc)
    pub amount: f32, // amount = price*qty
    pub loading_pvc: f32,
    pub loading_frls: f32,
}

#[derive(Debug, Deserialize)]
pub struct QuotationResponse {
    pub items: Vec<QuotedItem>,
    pub basic_total: f32,
    pub delivery_charges: f32,
    pub total_with_delivery: f32,
    pub taxes: f32,       //taxes = total_with_delivery*0.18
    pub grand_total: f32, // grand_total = total_with_delivery + taxes
}
pub struct QuotationService {
    pub pricelists: HashMap<String, Vec<PricingSystem>>,
}

impl QuotationService {
    pub fn new(pricelist_configs: Vec<PriceListConfig>) -> Result<Self, QuotationError> {
        let mut pricelists = HashMap::new();

        for pricelist_config in pricelist_configs {
            let json_pricelist = fs::read_to_string(pricelist_config.pricelist)
                .map_err(|_| QuotationError::FileReadError)?;
            let pricelist: PriceList = serde_json::from_str(&json_pricelist)
                .map_err(|_| QuotationError::PricelistParseError)?;
            let pricing_system = PricingSystem::from_price_list(pricelist);
            let key = pricelist_config.brand.to_lowercase().trim().to_string();
            let brand_pricing_systems = pricelists
                .entry(key)
                .or_insert_with(|| Vec::<PricingSystem>::new());
            brand_pricing_systems.push(pricing_system);
        }
        Ok(Self { pricelists })
    }
}

impl QuotationService {
    pub fn generate_quotation(&self, request: QuotationRequest) -> Option<QuotationResponse> {
        let mut quoted_items = Vec::new();
        let mut basic_total = 0.0;

        for item in request.items {
            println!("processing:{:#?}", item);
            let listed_price = self.get_price(&item.product, &item.brand, &item.tag)?;
            println!("found price:{} for item:{:#?}", listed_price, item);
            let mut price = listed_price
                * (1.0 - item.discount)
                * (1.0 + item.loading_frls)
                * (1.0 + item.loading_pvc);
            price = (price * 100.0).round() / 100.0;

            let amount = price * item.quantity;
            basic_total += amount;

            quoted_items.push(QuotedItem {
                product: item.product,
                quantity_mtrs: item.quantity,
                price,
                amount,
                loading_frls: item.loading_frls,
                loading_pvc: item.loading_pvc,
            });
        }

        let total_with_delivery = basic_total + request.delivery_charges;
        let taxes = total_with_delivery * 0.18;
        let grand_total = (total_with_delivery + taxes).round();

        Some(QuotationResponse {
            items: quoted_items,
            basic_total,
            delivery_charges: request.delivery_charges,
            total_with_delivery,
            taxes,
            grand_total,
        })
    }

    fn get_price(&self, product: &Product, brand: &str, tag: &str) -> Option<f32> {
        self.pricelists
            .get(&brand.to_lowercase())?
            .iter()
            .find_map(|pricing_system| pricing_system.get_price(product, tag))
    }
}
