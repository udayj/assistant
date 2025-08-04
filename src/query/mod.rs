use serde::Deserialize;
use crate::prices::item_prices::PriceList;
use crate::quotation::QuotationRequest;
use thiserror::Error;
use crate::claude::{Query,parse_query};

#[derive(Error, Debug)]
pub enum QueryError {
    #[error("Failed to understand query: {0}")]
    LLMError(String)
}

pub async fn get_query(query: &str) -> Result<Query, QueryError> {
    let sample_response = r#"{
  "GetQuotation": {
    "items": [
      {
        "product": {
          "Cable": {
            "PowerControl": {
              "LT": {
                "conductor": "Copper",
                "core_size": "4",
                "sqmm": "95",
                "armoured": true
              }
            }
          }
        },
        "brand": "KEI",
        "tag": "latest",
        "discount": 0,
        "loading_frls": 0,
        "loading_pvc": 0,
        "quantity": 100
      }
    ],
    "delivery_charges": 0
  }
}"#;
    let system_prompt = "You are a query understanding agent for electrical items' related queries. User queries can be of 3 types as per following Rust type
    #[derive(Debug, Deserialize)]
    enum Query {
        MetalPricing, // eg. send metal prices or send copper prices or find aluminum prices, get current mcx prices etc.
        GetPriceList(PriceList), // eg. send current price list, give armoured cable price list
        GetQuotation(QuotationRequest),
        UnsupportedQuery
    }

    #[derive(Debug, Deserialize)]
    pub struct QuoteItem {
        pub product: Product,
        pub brand: String, // default kei
        pub tag: String, // default latest
        pub discount: f32,     // in percentage eg. 0.70 means 70%, default 0
        pub loading_frls: f32, // in percentage eg. 0.05 means 5%, default 0
        pub loading_pvc: f32,  // in percentage eg. 0.05 means 5%, default 0
        pub quantity: f32,
    }

    #[derive(Debug, Deserialize)]
    pub struct QuotationRequest {
        pub items: Vec<QuoteItem>,
        pub delivery_charges: f32, // default 0
    }

    #[derive(Debug, Deserialize)]
    pub struct PriceList {
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
    }

    #[derive(Eq, Hash, PartialEq, Deserialize, Clone, Debug)]
    pub enum Conductor {
        Copper,
        Aluminium,
    }

Quotation requests for armoured and unarmoured cables can include insulation type which can be either pvc or xlpe
for armoured and unarmoured cables, default is xlpe and loading would be 0 in this case, if insulation is of type pvc then loading_pvc would be 5% (given as 0.05)
for armoured and unarmoured cables, if cable is of type frls then loading_frls would be 3% represented as 0.03
loading_pvc and loading_frls is ONLY applicable for armoured and unarmoured cables of types LT and HT- 
User can either ask for metal prices, or ask for price lists or ask for quotations for electrical items. 
You need to understand what the user wants and return your response as a JSON string that can be deserialized into the Query type. Do not return anything else in the response.
If you cannot understand the request then use Unsupported query type
Return ONLY the raw JSON object without any markdown formatting, code blocks, or explanations - Do not wrap the response in ```json blocks or any other formatting -
Your entire response should be valid JSON that starts with { and ends with }
Your response must be exactly one valid JSON object with no additional text, formatting, or explanations
eg. ";
    let prompt_with_sample = format!("{}{}", system_prompt, sample_response);
    let query: Query = parse_query(&prompt_with_sample, query).await.map_err(|e| QueryError::LLMError(e.to_string()))?;
    println!("parsed query successfully");
    Ok(query)
}

// TODO - update prompt to include proper specific tags for each item type