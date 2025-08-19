use crate::configuration::PdfPriceListConfig;
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
    pub tags: Vec<String>,
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
                tags: config.tags,
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

    pub fn find_pricelist(&self, brand: &str, tags: &[String]) -> Option<String> {
        self.pricelists_by_brand
            .get(&brand.to_lowercase())?
            .iter()
            .find(|entry| {
                tags.iter().any(|tag| {
                    entry
                        .tags
                        .iter()
                        .any(|entry_tag| entry_tag.eq_ignore_ascii_case(tag))
                })
            })
            .map(|entry| entry.pdf_path.clone())
    }
}
