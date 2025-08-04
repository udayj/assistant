use crate::core::service_manager::Error as ServiceManagerError;
use crate::core::Service;
use async_trait::async_trait;
use chrono::{DateTime, TimeZone, Timelike, Utc};
use chrono_tz::Asia::Kolkata;
use reqwest;
use scraper::{Html, Selector};
use std::thread;
use std::time::Duration;
use thiserror::Error;

pub mod item_prices;

#[derive(Error, Debug)]
pub enum PriceError {
    #[error("Failed to get response:{0}")]
    GetUrlError(String),

    #[error("Failed to Build Client")]
    ClientError,

    #[error("Invalid metal type")]
    InvalidMetalType,

    #[error("HTML parsing error:{0}")]
    HTMLParseError(String),

    #[error("Price not found")]
    PriceNotFoundError,

    #[error("Failed to parse Price")]
    PriceParseError,
}
// read url, time to check for executing price fetching from config
// caching of previous prices in memory with timestamp - cache for 10 minutes
pub struct PriceService {
    pub url_al: String,
    pub url_cu: String,
}

#[async_trait]
impl Service for PriceService {
    async fn new() -> Self {
        Self {
            url_al: "https://www.5paisa.com/commodity-trading/mcx-aluminium-price".to_string(),
            url_cu: "https://www.5paisa.com/commodity-trading/mcx-copper-price".to_string(),
        }
    }

    async fn run(self) -> Result<(), ServiceManagerError> {
        loop {
            let now_ist = Utc::now().with_timezone(&Kolkata);
            let hour = now_ist.hour();
            let minute = now_ist.minute();
            println!("running service");
            if (hour == 9 && minute == 30) || (hour == 15 && minute == 30) {
                self.fetch_price("aluminium")
                    .await
                    .map_err(|e| ServiceManagerError::from(e))?;
                thread::sleep(Duration::from_secs(2));
                self.fetch_price("copper")
                    .await
                    .map_err(|e| ServiceManagerError::from(e))?;
            }

            thread::sleep(Duration::from_secs(600));
        }
    }
}

impl PriceService {
    pub async fn fetch_price(&self, metal: &str) -> Result<f64, PriceError> {
        let client = reqwest::Client::builder()
        .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/91.0.4472.124 Safari/537.36")
        .build()
        .map_err(|_| PriceError::ClientError)?;

        let url = match metal.to_lowercase().as_str() {
            "aluminium" => &self.url_al,
            "copper" => &self.url_cu,
            _ => return Err(PriceError::InvalidMetalType),
        };
        let response = client
            .get(url)
            .header("Accept", "text/html")
            .header("Accept-Language", "en-US,en;q=0.9")
            .send()
            .await
            .map_err(|e| PriceError::GetUrlError(e.to_string()))?
            .text()
            .await
            .map_err(|e| PriceError::GetUrlError(e.to_string()))?;

        let document = Html::parse_document(&response);
        // Updated selectors to match the actual HTML structure
        let value_selector = Selector::parse("div.commodity-page__value")
            .map_err(|e| PriceError::HTMLParseError(e.to_string()))?;

        // Extract the main price value
        let value_element = document
            .select(&value_selector)
            .next()
            .ok_or("Price value not found")
            .map_err(|_| PriceError::PriceNotFoundError)?;

        // Get the main price (before decimal)
        let main_price_text = value_element
            .text()
            .collect::<String>()
            .replace("₹", "")
            .trim()
            .to_string();

        // Parse the combined price string
        let price = main_price_text
            .as_str()
            .parse::<f64>()
            .map_err(|_| PriceError::PriceParseError)?;

        println!("{} price is:{}", metal, price);
        Ok(price)
    }
}


/*

each cable has a conductor which can be aluminium or copper
cable can be of type LT or HT
each cable has a core size and conductor size in sq. mm
LT cables can be of type armoured of flexible
flexible cables can be of types FR, HRFR or FRLS
there will be different price lists with different tags like date, brand, etc. so that quotation can be prepared
against multiple price points
now there are more variations like
telephone cables have the form pair_size x conductor dia in mm
eg. 2 P x 0.5 mm
coaxial cables have the form RG-6, RG-59 etc. without any core or sq. mm dimensions
submersible cables are same as flexible cables but dont have subtypes like FR, FRLS and HRFR
solar cables are also like flexible cables but are single core cables and dont have subtypes

// TODO - deserialize price lists into rust repr
// persist data into sqlite dbs
// test this data for queries
// refine and simplify query language
// create type for query
// test deserializing from claude json query str -> query type
// test full flow

enum based approach for deser - done
test deser and print - done
json query str to enum product
include dates, brands and price list versions - done
use ocr to convert price lists to the required format
multi level hashmaps for storing prices ensuring O(1) lookup for prices - done

3 brands - havells, kei, polycab

price list -> brand, date, tags, [prices]
for a query look in all prices
prices -> can have different products
keep all price lists in memory - at max we keep current and 2 previous


handle
discount
loading
delivery charges
quotation
proforma
just prices

webhook should never not return a response - even for an error - it should return some message

query fulfilment module
1. query understanding - understand whether image / audio / text -> convert to text - claude call
2. query understanding - text query -> query type - claude call

based on query type
1. for metal price -> get from metal price module
2. for price list -> get from price list files module
3. for quotation -> send to quotation service (pi is similar and just prices is also similar)

quotation service-> understand exact items through claude call
deserialize to types for querying - send to quotation tool
form quotation while handling delivery chgs, discount, loading etc.
return to quotation service in quotation Type

4. use output from quotation tool to also form table -> form text response as well as pdf response and return to fulfilment service
5. return to whatsapp requester

query fulfillment config should hold reference to query understanding and quotation service
quotation service would have reference to price module and price lists which were initialised during config building
so main has a config
that then creates configs for all other services with necessary data to perform the tasks

QuoteItem {
    product: Product,
    brand: String,
    tag: String,
    discount: f32,
    loading_frls: f32,
    loading_pvc: f32,
    quantity: f32
}

QuotationRequest {
    items: Vec<QuoteItem>,
    delivery_charges: f32
}

QuotedItem {
    product: Product,
    quantity_mtrs: f32,
    price: f32,
    amount: f32
}

QuotationResponse {
    items: Vec<QuotedItem>
    basic_total: f32,
    taxes: f32,
    delivery_charges: f32,
    total_with_delivery: f32,
    grand_total: f32
}

enum Query {
    MetalPricing,
    GetPriceList(PriceList),
    GetQuotation(QuotationRequest)
}

struct PriceList {
    brand: String,
    tag: String
}
*/