use crate::prices::item_prices::Product;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct QuoteItem {
    /// Specific electrical product for which quotation is required
    pub product: Product,
    /// Brand name for product
    pub brand: String,
    /// Selects which pricelist to use for pricing the item
    pub tag: String,
    /// in percentage eg. 0.70 means 70%
    pub discount: f32,
    /// in percentage eg. 0.03 means 3% - applicable only for LT/HT cable types
    pub loading_frls: f32,
    /// in percentage eg. 0.05 means 5%, - applicable only for LT/HT cable types
    pub loading_pvc: f32,
    /// Quantity required
    pub quantity: f32,
    /// Final price that can optionally be provided by the user - If provided, skip price lookup
    pub user_base_price: Option<f32>,
    /// Optional - Apply markup/margin, if given, to user_base_price (eg. 0.015 means 1.5%)
    pub markup: Option<f32>,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct QuotationRequest {
    /// List of items for which quotation is required
    pub items: Vec<QuoteItem>,
    /// Delivery charges, if provided by user, defaults to 0
    pub delivery_charges: f32,
    /// Optional addressee for the quotation/proforma invoice
    pub to: Option<Vec<String>>,
    /// Optional terms and conditions for the quotation/proforma invoice
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
