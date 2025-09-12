use crate::{
    configuration::PriceListConfig,
    prices::item_prices::{Description, PriceList, PricingSystem, Product},
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use thiserror::Error;
use tracing::info;

#[derive(Debug, Error)]
pub enum QuotationError {
    #[error("Error reading pricelist file")]
    FileReadError,

    #[error("Error parsing pricelist file")]
    PricelistParseError,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct QuoteItem {
    pub product: Product,
    pub brand: String,
    pub tag: String,
    pub discount: f32,     // in percentage eg. 0.70 means 70%
    pub loading_frls: f32, // in percentage eg. 0.05 means 5%
    pub loading_pvc: f32,  // in percentage eg. 0.05 means 5%
    pub quantity: f32,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct QuotationRequest {
    pub items: Vec<QuoteItem>,
    pub delivery_charges: f32,
    pub to: Option<Vec<String>>,
    pub terms_and_conditions: Option<Vec<String>>,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct PriceOnlyRequest {
    pub items: Vec<PriceOnlyItem>,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct PriceOnlyItem {
    pub product: Product,
    #[serde(default = "default_brand")]
    pub brand: String,
    #[serde(default = "default_tag")]
    pub tag: String,
    #[serde(default)]
    pub discount: f32,
    pub quantity: Option<f32>,
    #[serde(default)]
    pub loading_frls: f32,
    #[serde(default)]
    pub loading_pvc: f32,
}

fn default_brand() -> String {
    "kei".to_string()
}

fn default_tag() -> String {
    "latest".to_string()
}

#[derive(Debug, Deserialize)]
pub struct QuotedItem {
    pub product: Product,
    pub brand: String,
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
    pub to: Option<Vec<String>>,
    pub terms_and_conditions: Option<Vec<String>>,
}

#[derive(Debug)]
pub struct PriceOnlyResponse {
    pub items: Vec<PriceOnlyResponseItem>,
}

#[derive(Debug)]
pub struct PriceOnlyResponseItem {
    pub description: String,
    pub price: f32,
    pub quantity: Option<f32>,
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
            info!(item = ?item, "Processing quotation item");
            let listed_price = self.get_price(&item.product, &item.brand, &item.tag)?;
            info!(price = %listed_price, "Found item price");
            let mut price = listed_price
                * (1.0 - item.discount)
                * (1.0 + item.loading_frls)
                * (1.0 + item.loading_pvc);
            price = (price * 100.0).round() / 100.0;

            let amount = price * item.quantity;
            basic_total += amount;

            quoted_items.push(QuotedItem {
                product: item.product,
                brand: item.brand,
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
            to: request.to,
            terms_and_conditions: self.process_terms_and_conditions(request.terms_and_conditions),
        })
    }

    pub fn get_prices_only(&self, request: PriceOnlyRequest) -> Option<PriceOnlyResponse> {
        let mut response_items = Vec::new();

        for item in request.items {
            let listed_price = self.get_price(&item.product, &item.brand, &item.tag);
            if listed_price.is_none() {
                continue;
            }
            let listed_price = listed_price.unwrap();

            let mut price = listed_price
                * (1.0 - item.discount)
                * (1.0 + item.loading_frls)
                * (1.0 + item.loading_pvc);
            price = (price * 100.0).round() / 100.0;

            // Use existing Description trait but make it brief
            let mut extras = Vec::new();
            if item.loading_frls > 0.0 {
                extras.push("frls".to_string());
            }
            if item.loading_pvc > 0.0 {
                extras.push("pvc".to_string());
            }

            let description = format!("{}", item.product.get_brief_description(extras));

            response_items.push(PriceOnlyResponseItem {
                description,
                price,
                quantity: item.quantity,
            });
        }

        Some(PriceOnlyResponse {
            items: response_items,
        })
    }

    fn get_price(&self, product: &Product, brand: &str, tag: &str) -> Option<f32> {
        self.pricelists
            .get(&brand.to_lowercase())?
            .iter()
            .find_map(|pricing_system| pricing_system.get_price(product, tag))
    }

    fn process_terms_and_conditions(&self, terms: Option<Vec<String>>) -> Option<Vec<String>> {
        match terms {
            Some(terms_vec) if terms_vec.len() == 1 => match terms_vec[0].to_lowercase().as_str() {
                "standard" => Some(self.get_standard_terms()),
                _ => Some(terms_vec),
            },
            other => other,
        }
    }

    fn get_standard_terms(&self) -> Vec<String> {
        vec![
            "Qty. Tolerance: +/-5%",
            "Payment: Full payment against proforma invoice",
            "Delivery: Ready stock subject to prior sale",
            "GST: 18% extra as applicable",
            "Validity: 3 days from quotation date",
        ]
        .iter()
        .map(|x| x.to_string())
        .collect()
    }
}
