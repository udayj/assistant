use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(PartialEq, Eq, Hash, Deserialize, Clone, Debug, Serialize, JsonSchema)]
pub enum Product {
    Cable(Cable),
}

#[derive(PartialEq, Eq, Hash, Deserialize, Clone, Debug, Serialize, JsonSchema)]
pub enum Cable {
    /// Use this variant for armoured / unarmoured / flexible cables
    PowerControl(PowerControl),
    /// Use this variant for telephone cables
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

#[derive(Eq, Hash, PartialEq, Deserialize, Clone, Debug, Serialize, JsonSchema)]
pub enum SolarType {
    BS,
    EN,
}

#[derive(Eq, Hash, PartialEq, Deserialize, Clone, Debug, Serialize, JsonSchema)]
pub enum CoaxialType {
    /// RG6 type cables
    RG6,
    /// RG11 type cables
    RG11,
    /// RG59 type cables
    RG59,
}

#[derive(Eq, Hash, PartialEq, Deserialize, Clone, Debug, Serialize, JsonSchema)]
pub enum PowerControl {
    /// For armoured/unarmoured cables - use this variant
    LT(LT),
    /// For HT cables with voltages more than 1.1 KV use this variant
    HT(HT),
    /// For all kinds of flexible cables use this
    Flexible(Flexible),
}

#[derive(Eq, Hash, PartialEq, Deserialize, Clone, Debug, Serialize, JsonSchema)]
pub struct LT {
    pub conductor: Conductor,
    pub core_size: String,
    pub sqmm: String,
    pub armoured: bool,
}

#[derive(Eq, Hash, PartialEq, Deserialize, Clone, Debug, Serialize, JsonSchema)]
pub struct HT {
    pub conductor: Conductor,
    pub voltage_grade: String,
    pub core_size: String,
    pub sqmm: String,
}

#[derive(Eq, Hash, PartialEq, Deserialize, Clone, Debug, Serialize, JsonSchema)]
pub struct Flexible {
    /// Core size eg. "3"
    pub core_size: String,
    pub sqmm: String,
    /// Type of flexible cable eg. "FR" / "FRLSH" - do not apply any loading for flexible cables
    pub flexible_type: FlexibleType,
}

#[derive(Eq, Hash, PartialEq, Deserialize, Clone, Debug, Serialize, JsonSchema)]
pub enum FlexibleType {
    FR,
    FRLSH,
    HRFR,
    ZHFR,
}

#[derive(Eq, Hash, PartialEq, Deserialize, Clone, Debug, Serialize, JsonSchema)]
pub enum Conductor {
    /// Type of conductor to be used for eg. "Cu" / "Copper"
    Copper,
    /// Type of conductor to be used for eg. "Al" / "Aluminium"
    Aluminium,
}

#[derive(Deserialize, Clone, Debug)]
pub struct PriceList {
    pub tags: Vec<String>,
    pub prices: Vec<Prices>,
}

#[derive(Deserialize, Clone, Debug)]
pub struct Prices {
    pub product: Product,
    pub price: f32,
}

pub struct PricingSystem {
    pub tags: Vec<String>,
    pub prices: HashMap<Product, f32>,
}
