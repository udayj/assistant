use crate::prices::item_prices::{PricingSystem, Product};
use serde::Deserialize;
use std::collections::HashMap;

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
    pub price: f32,  // price = listed_price*(1-discount)*(1+loading_frls)*(1+loading_pvc)
    pub amount: f32, // amount = price*qty
    pub loading_pvc: f32,
    pub loading_frls: f32
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
    /*
       brand key-> multiple tags
       multiple tags-> brand
    */
    pub pricelists: HashMap<String, Vec<PricingSystem>>,
}

impl QuotationService {
    pub fn generate_quotation(&self, quotation_number:&str, date: &str, request: QuotationRequest) -> Option<QuotationResponse> {
        let mut quoted_items = Vec::new();
        let mut basic_total = 0.0;

        for item in request.items {
            println!("processing:{:#?}", item);
            let listed_price = self.get_price(&item.product, &item.brand, &item.tag)?;
            println!("found price:{} for item:{:#?}", listed_price, item);
            let price = listed_price
                * (1.0 - item.discount)
                * (1.0 + item.loading_frls)
                * (1.0 + item.loading_pvc);

            let amount = price * item.quantity;
            basic_total += amount;

            quoted_items.push(QuotedItem {
                product: item.product,
                quantity_mtrs: item.quantity,
                price,
                amount,
                loading_frls: item.loading_frls,
                loading_pvc: item.loading_pvc
            });
        }

        let total_with_delivery = basic_total + request.delivery_charges;
        let taxes = total_with_delivery * 0.18;
        let grand_total = total_with_delivery + taxes;

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

/*
    You are a query understanding agent for electrical items' related queries. User queries can be of 3 types as per following Rust type
    #[derive(Debug, Deserialize)]
    enum Query {
    MetalPricing,
    GetPriceList(PriceList),
    GetQuotation(QuotationRequest),
    UnsupportedQuery
}
#[derive(Debug, Deserialize)]
struct PriceList {
    brand: String,
    tag: String
}
#[derive(PartialEq, Eq, Hash, Deserialize, Clone, Debug)]
pub enum Product {
    Cable(Cable),
}

#[derive(PartialEq, Eq, Hash, Deserialize, Clone, Debug)]
pub enum Cable {
    PowerControl(PowerControl),

    Telephone {
        pair_size: String,
        conductor_mm: String,
    },
    Coaxial(CoaxialType),
    Submersible {
        core_size: String,
        sqmm: String,
    },
    Solar {
        solar_type: SolarType,
        sqmm: String,
    },
}

#[derive(Eq, Hash, PartialEq, Deserialize, Clone, Debug)]
enum SolarType {
    BS,
    EN,
}

#[derive(Eq, Hash, PartialEq, Deserialize, Clone, Debug)]
enum CoaxialType {
    RG6,
    RG11,
    RG59,
}

#[derive(Eq, Hash, PartialEq, Deserialize, Clone, Debug)]
pub enum PowerControl {
    LT(LT),
    HT(HT),
    Flexible(Flexible),
}

#[derive(Eq, Hash, PartialEq, Deserialize, Clone, Debug)]
pub struct LT {
    pub conductor: Conductor,
    pub core_size: String,
    pub sqmm: String,
    pub armoured: bool,
}

#[derive(Eq, Hash, PartialEq, Deserialize, Clone, Debug)]
pub struct HT {
    pub conductor: Conductor,
    pub voltage_grade: String,
    pub core_size: String,
    pub sqmm: String,
}

#[derive(Eq, Hash, PartialEq, Deserialize, Clone, Debug)]
struct Flexible {
    core_size: String,
    sqmm: String,
    flexible_type: FlexibleType,
}

#[derive(Eq, Hash, PartialEq, Deserialize, Clone, Debug)]
enum FlexibleType {
    FR,
    FRLSH,
    HRFR,
    ZHFR
}

#[derive(Eq, Hash, PartialEq, Deserialize, Clone, Debug)]
pub enum Conductor {
    Copper,
    Aluminium,
}

User can either ask for metal prices, or ask for price lists or ask for quotations for electrical items. 
You need to understand what the user wants and return your response as a JSON string that can be deserialized into the Query type. Do not return anything else in the response.

*/
