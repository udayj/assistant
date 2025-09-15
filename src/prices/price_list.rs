use crate::configuration::PdfPriceListConfig;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use thiserror::Error;

#[derive(Debug, Error)]
pub enum PriceListError {
    #[error("Error creating price list service")]
    ServiceCreationError,
}

#[derive(Debug, Clone)]
pub struct PdfPriceListEntry {
    pub pdf_path: String,
    pub keywords: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct PriceListInfo {
    pub brand: String,
    pub pdf_path: String,
    pub keywords: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct AvailablePricelists {
    pub pricelists: Vec<PriceListInfo>,
}

pub struct PriceListService {
    pricelists_by_brand: HashMap<String, Vec<PdfPriceListEntry>>,
}

impl PriceListService {
    pub fn new(pdf_configs: Vec<PdfPriceListConfig>) -> Result<Self, PriceListError> {
        let mut pricelists_by_brand = HashMap::new();

        for config in pdf_configs {
            let entry = PdfPriceListEntry {
                pdf_path: config.pdf_path,
                keywords: config.keywords,
            };

            pricelists_by_brand
                .entry(config.brand.to_lowercase())
                .or_insert_with(Vec::new)
                .push(entry);
        }

        Ok(Self {
            pricelists_by_brand,
        })
    }

    pub fn find_pricelist(&self, brand: &str, keywords: &[String]) -> Option<String> {
        self.pricelists_by_brand
            .get(&brand.to_lowercase())?
            .iter()
            .find(|entry| {
                keywords.iter().any(|keyword| {
                    entry
                        .keywords
                        .iter()
                        .any(|entry_keyword| entry_keyword.eq_ignore_ascii_case(keyword))
                })
            })
            .map(|entry| entry.pdf_path.clone())
    }

    pub fn list_available_pricelists(&self, brand_filter: Option<&str>) -> AvailablePricelists {
        let mut pricelists = Vec::new();

        for (brand, entries) in &self.pricelists_by_brand {
            // Apply brand filter if specified
            if let Some(filter) = brand_filter {
                if !brand.eq_ignore_ascii_case(filter) {
                    continue;
                }
            }

            for entry in entries {
                pricelists.push(PriceListInfo {
                    brand: brand.clone(),
                    pdf_path: entry.pdf_path.clone(),
                    keywords: entry.keywords.clone(),
                });
            }
        }

        AvailablePricelists { pricelists }
    }
}
