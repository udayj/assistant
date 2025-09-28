use crate::{
    configuration::PriceListConfig,
    prices::item_prices::{Description, PriceList, PricingSystem, Product},
};

use std::collections::HashMap;
use std::fs;
use thiserror::Error;
use tracing::info;

mod types;
pub use types::*;

#[derive(Debug, Error)]
pub enum QuotationError {
    #[error("Error reading pricelist file")]
    FileReadError,

    #[error("Error parsing pricelist file")]
    PricelistParseError,
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
        const TAX_RATE:f32 = 0.18;
        for item in request.items {
            info!(item = ?item, "Processing quotation item");

            let mut price = if let Some(user_price) = item.user_base_price {
                // User provided price - apply only markup, skip all lookups/loadings/discounts
                info!(user_price = %user_price, "Using user-provided price");
                match item.markup {
                    Some(markup) => {
                        info!(markup = %markup, "Applying markup to user price");
                        user_price * (1.0 + markup)
                    }
                    None => user_price,
                }
            } else {
                // Existing price lookup logic with loadings/discounts
                // If price is not found, then we skip creating the quotation and return None
                let listed_price = self.get_price(&item.product, &item.brand, &item.tag)?;
                info!(price = %listed_price, "Found item price");
                listed_price
                    * (1.0 - item.discount)
                    * (1.0 + item.loading_frls)
                    * (1.0 + item.loading_pvc)
            };

            // round prices to 2 decimal places
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
        let taxes = total_with_delivery * TAX_RATE;
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
            // Round prices to 2 decimal places
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
            "Above price is Ex-Godown Kolkata",
            "Qty. Tolerance: +/-5%",
            "Payment: Full payment against proforma invoice",
            "Delivery: Ready stock subject to prior sale",
            "Validity: 3 days from quotation date",
        ]
        .iter()
        .map(|x| x.to_string())
        .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::prices::item_prices::{Cable, Conductor, Flexible, FlexibleType, LT, PowerControl};
    use std::collections::HashMap;

    // Test helper: create a mock PricingSystem from JSON
    fn create_mock_pricing_system() -> PricingSystem {
        let json_data = r#"{
            "tags": ["latest"],
            "prices": [{
                "product": {
                    "Cable": {
                        "PowerControl": {
                            "LT": {
                                "conductor": "Copper",
                                "core_size": "3",
                                "sqmm": "2.5",
                                "armoured": false
                            }
                        }
                    }
                },
                "price": 100.0
            }]
        }"#;

        let price_list: PriceList = serde_json::from_str(json_data)
            .expect("Failed to create test price list");

        PricingSystem::from_price_list(price_list)
    }

    // Test helper: create a mock QuotationService
    fn create_mock_service() -> QuotationService {
        let mut pricelists = HashMap::new();
        pricelists.insert("kei".to_string(), vec![create_mock_pricing_system()]);

        QuotationService { pricelists }
    }

    // Test helper: create a test QuoteItem
    fn create_test_quote_item() -> QuoteItem {
        QuoteItem {
            product: Product::Cable(Cable::PowerControl(PowerControl::LT(LT {
                conductor: Conductor::Copper,
                core_size: "3".to_string(),
                sqmm: "2.5".to_string(),
                armoured: false,
            }))),
            brand: "kei".to_string(),
            tag: "latest".to_string(),
            discount: 0.0,
            loading_frls: 0.0,
            loading_pvc: 0.0,
            quantity: 1.0,
            user_base_price: None,
            markup: None,
        }
    }

    #[test]
    fn test_new_service_with_invalid_file_path() {
        let config = PriceListConfig {
            brand: "test".to_string(),
            pricelist: "/nonexistent/file.json".to_string(),
        };

        let result = QuotationService::new(vec![config]);
        assert!(matches!(result, Err(QuotationError::FileReadError)));
    }

    #[test]
    fn test_generate_quotation_returns_none_for_missing_product() {
        let service = create_mock_service();
        let mut item = create_test_quote_item();
        item.brand = "nonexistent_brand".to_string();

        let request = QuotationRequest {
            items: vec![item],
            delivery_charges: 0.0,
            to: None,
            terms_and_conditions: None,
        };

        let result = service.generate_quotation(request);
        assert!(result.is_none());
    }

    #[test]
    fn test_price_calculation_with_discount_and_loadings() {
        let service = create_mock_service();
        let mut item = create_test_quote_item();
        item.discount = 0.1; // 10% discount
        item.loading_frls = 0.03; // 3% FRLS loading
        item.loading_pvc = 0.05; // 5% PVC loading
        item.quantity = 2.0;

        let request = QuotationRequest {
            items: vec![item],
            delivery_charges: 0.0,
            to: None,
            terms_and_conditions: None,
        };

        let result = service.generate_quotation(request).unwrap();

        // Expected: 100.0 * (1-0.1) * (1+0.03) * (1+0.05) = 100.0 * 0.9 * 1.03 * 1.05 = 97.335
        // Rounded: 97.33 (due to f32 precision)
        let expected_price = 97.33;
        let expected_amount = expected_price * 2.0; // 194.66

        assert_eq!(result.items[0].price, expected_price);
        assert_eq!(result.items[0].amount, expected_amount);
        assert_eq!(result.basic_total, expected_amount);
    }

    #[test]
    fn test_user_base_price_with_markup() {
        let service = create_mock_service();
        let mut item = create_test_quote_item();
        item.user_base_price = Some(200.0);
        item.markup = Some(0.1); // 10% markup
        item.discount = 0.5; // Should be ignored when user_base_price is provided

        let request = QuotationRequest {
            items: vec![item],
            delivery_charges: 0.0,
            to: None,
            terms_and_conditions: None,
        };

        let result = service.generate_quotation(request).unwrap();

        // Expected: 200.0 * (1 + 0.1) = 220.0
        assert_eq!(result.items[0].price, 220.0);
    }

    #[test]
    fn test_user_base_price_without_markup() {
        let service = create_mock_service();
        let mut item = create_test_quote_item();
        item.user_base_price = Some(150.0);
        item.markup = None;

        let request = QuotationRequest {
            items: vec![item],
            delivery_charges: 0.0,
            to: None,
            terms_and_conditions: None,
        };

        let result = service.generate_quotation(request).unwrap();

        assert_eq!(result.items[0].price, 150.0);
    }

    #[test]
    fn test_tax_and_delivery_calculation() {
        let service = create_mock_service();
        let item = create_test_quote_item();

        let request = QuotationRequest {
            items: vec![item],
            delivery_charges: 50.0,
            to: None,
            terms_and_conditions: None,
        };

        let result = service.generate_quotation(request).unwrap();

        let expected_total_with_delivery = 100.0_f32 + 50.0; // basic_total + delivery
        let expected_taxes = expected_total_with_delivery * 0.18; // 27.0
        let expected_grand_total = (expected_total_with_delivery + expected_taxes).round(); // 177.0

        assert_eq!(result.total_with_delivery, expected_total_with_delivery);
        assert_eq!(result.taxes, expected_taxes);
        assert_eq!(result.grand_total, expected_grand_total);
    }

    #[test]
    fn test_price_rounding() {
        let service = create_mock_service();
        let mut item = create_test_quote_item();
        item.discount = 0.333; // Creates a price that needs rounding: 100 * 0.667 = 66.7

        let request = QuotationRequest {
            items: vec![item],
            delivery_charges: 0.0,
            to: None,
            terms_and_conditions: None,
        };

        let result = service.generate_quotation(request).unwrap();

        // Should be rounded to 2 decimal places
        assert_eq!(result.items[0].price, 66.7);
    }

    #[test]
    fn test_hundred_percent_discount() {
        let service = create_mock_service();
        let mut item = create_test_quote_item();
        item.discount = 1.0; // 100% discount

        let request = QuotationRequest {
            items: vec![item],
            delivery_charges: 0.0,
            to: None,
            terms_and_conditions: None,
        };

        let result = service.generate_quotation(request).unwrap();

        assert_eq!(result.items[0].price, 0.0);
        assert_eq!(result.basic_total, 0.0);
    }

    #[test]
    fn test_zero_quantity() {
        let service = create_mock_service();
        let mut item = create_test_quote_item();
        item.quantity = 0.0;

        let request = QuotationRequest {
            items: vec![item],
            delivery_charges: 0.0,
            to: None,
            terms_and_conditions: None,
        };

        let result = service.generate_quotation(request).unwrap();

        assert_eq!(result.items[0].amount, 0.0);
        assert_eq!(result.basic_total, 0.0);
    }

    #[test]
    fn test_empty_items_list() {
        let service = create_mock_service();
        let request = QuotationRequest {
            items: vec![],
            delivery_charges: 25.0,
            to: None,
            terms_and_conditions: None,
        };

        let result = service.generate_quotation(request).unwrap();

        assert_eq!(result.items.len(), 0);
        assert_eq!(result.basic_total, 0.0);
        assert_eq!(result.total_with_delivery, 25.0); // Only delivery charges
    }

    #[test]
    fn test_process_terms_standard() {
        let service = create_mock_service();
        let terms = Some(vec!["standard".to_string()]);

        let result = service.process_terms_and_conditions(terms);
        let standard_terms = service.get_standard_terms();

        assert_eq!(result, Some(standard_terms));
    }

    #[test]
    fn test_process_terms_custom() {
        let service = create_mock_service();
        let custom_terms = vec!["Custom term 1".to_string(), "Custom term 2".to_string()];
        let terms = Some(custom_terms.clone());

        let result = service.process_terms_and_conditions(terms);

        assert_eq!(result, Some(custom_terms));
    }

    #[test]
    fn test_process_terms_none() {
        let service = create_mock_service();

        let result = service.process_terms_and_conditions(None);

        assert_eq!(result, None);
    }

    #[test]
    fn test_get_prices_only_skips_missing_items() {
        let service = create_mock_service();

        let valid_item = PriceOnlyItem {
            product: Product::Cable(Cable::PowerControl(PowerControl::LT(LT {
                conductor: Conductor::Copper,
                core_size: "3".to_string(),
                sqmm: "2.5".to_string(),
                armoured: false,
            }))),
            brand: "kei".to_string(),
            tag: "latest".to_string(),
            discount: 0.0,
            quantity: Some(1.0),
            loading_frls: 0.0,
            loading_pvc: 0.0,
        };

        let invalid_item = PriceOnlyItem {
            product: Product::Cable(Cable::PowerControl(PowerControl::Flexible(Flexible {
                core_size: "3".to_string(),
                sqmm: "1.5".to_string(),
                flexible_type: FlexibleType::FR,
            }))),
            brand: "nonexistent".to_string(),
            tag: "latest".to_string(),
            discount: 0.0,
            quantity: Some(1.0),
            loading_frls: 0.0,
            loading_pvc: 0.0,
        };

        let request = PriceOnlyRequest {
            items: vec![valid_item, invalid_item],
        };

        let result = service.get_prices_only(request).unwrap();

        // Should only include the valid item
        assert_eq!(result.items.len(), 1);
        assert_eq!(result.items[0].price, 100.0);
    }

    #[test]
    fn test_extreme_loading_percentages() {
        let service = create_mock_service();
        let mut item = create_test_quote_item();
        item.loading_frls = 1.0; // 100% loading
        item.loading_pvc = 0.5;  // 50% loading

        let request = QuotationRequest {
            items: vec![item],
            delivery_charges: 0.0,
            to: None,
            terms_and_conditions: None,
        };

        let result = service.generate_quotation(request).unwrap();

        // Expected: 100.0 * (1+1.0) * (1+0.5) = 100.0 * 2.0 * 1.5 = 300.0
        assert_eq!(result.items[0].price, 300.0);
    }
}
