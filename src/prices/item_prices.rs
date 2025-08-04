use serde::Deserialize;
use std::collections::HashMap;

pub trait Description {
    fn get_description(&self, extras: Vec<String>) -> String;
}

#[derive(PartialEq, Eq, Hash, Deserialize, Clone, Debug)]
pub enum Product {
    Cable(Cable),
}

impl Description for Product {
    fn get_description(&self, extras: Vec<String>) -> String {
        match self {
            Self::Cable(cable) => cable.get_description(extras),
        }
    }
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

impl Description for Cable {
    fn get_description(&self, extras: Vec<String>) -> String {
        match self {
            Self::Coaxial(coaxial_type) => coaxial_type.get_description(extras),
            Self::Solar { solar_type, sqmm } => format!(
                "1 C x {} sq. mm {}",
                sqmm,
                solar_type.get_description(extras)
            ),
            Self::Submersible { core_size, sqmm } => {
                format!("{} C x {} sq. mm Submersible cable", core_size, sqmm)
            }
            Self::Telephone {
                pair_size,
                conductor_mm,
            } => format!("{} P x {} mm Unarmoured Tel Cable", pair_size, conductor_mm),
            Self::PowerControl(cable) => cable.get_description(extras),
        }
    }
}

#[derive(Eq, Hash, PartialEq, Deserialize, Clone, Debug)]
pub enum SolarType {
    BS,
    EN,
}

impl Description for SolarType {
    fn get_description(&self, extras: Vec<String>) -> String {
        match self {
            Self::BS => "BS".to_string(),
            Self::EN => "EN".to_string(),
        }
    }
}

#[derive(Eq, Hash, PartialEq, Deserialize, Clone, Debug)]
pub enum CoaxialType {
    RG6,
    RG11,
    RG59,
}

impl Description for CoaxialType {
    fn get_description(&self, extras: Vec<String>) -> String {
        match self {
            Self::RG6 => "RG6".to_string(),
            Self::RG11 => "RG11".to_string(),
            Self::RG59 => "RG59".to_string(),
        }
    }
}

#[derive(Eq, Hash, PartialEq, Deserialize, Clone, Debug)]
pub enum PowerControl {
    LT(LT),
    HT(HT),
    Flexible(Flexible),
}

impl Description for PowerControl {
    fn get_description(&self, extras: Vec<String>) -> String {
        match self {
            Self::LT(lt_cable) => lt_cable.get_description(extras),
            Self::HT(ht_cable) => ht_cable.get_description(extras),
            Self::Flexible(flexible_cable) => flexible_cable.get_description(extras),
        }
    }
}
#[derive(Eq, Hash, PartialEq, Deserialize, Clone, Debug)]
pub struct LT {
    pub conductor: Conductor,
    pub core_size: String,
    pub sqmm: String,
    pub armoured: bool,
}

impl Description for LT {
    fn get_description(&self, extras: Vec<String>) -> String {
        let pvc = "pvc".to_string();
        let frls = "frls".to_string();
        if self.armoured {
            if extras.contains(&pvc) && !extras.contains(&frls) {
                return format!(
                    "{} C x {} sq. mm PVC Insulated, PVC Sheathed Armoured {} Cable",
                    self.core_size,
                    self.sqmm,
                    self.conductor.get_description(extras)
                );
            }
            if extras.contains(&pvc) && extras.contains(&frls) {
                return format!(
                    "{} C x {} sq. mm PVC Insulated, FRLS PVC Sheathed Armoured {} Cable",
                    self.core_size,
                    self.sqmm,
                    self.conductor.get_description(extras)
                );
            }

            if !extras.contains(&pvc) && extras.contains(&frls) {
                return format!(
                    "{} C x {} sq. mm XLPE Insulated, FRLS PVC Sheathed Armoured {} Cable",
                    self.core_size,
                    self.sqmm,
                    self.conductor.get_description(extras)
                );
            } else {
                return format!(
                    "{} C x {} sq. mm XLPE Insulated, PVC Sheathed Armoured {} Cable",
                    self.core_size,
                    self.sqmm,
                    self.conductor.get_description(extras)
                );
            }
        } else {
            if extras.contains(&pvc) && !extras.contains(&frls) {
                return format!(
                    "{} C x {} sq. mm PVC Insulated, PVC Sheathed Armoured {} Cable",
                    self.core_size,
                    self.sqmm,
                    self.conductor.get_description(extras)
                );
            }
            if extras.contains(&pvc) && extras.contains(&frls) {
                return format!(
                    "{} C x {} sq. mm PVC Insulated, FRLS PVC Sheathed Armoured {} Cable",
                    self.core_size,
                    self.sqmm,
                    self.conductor.get_description(extras)
                );
            }

            if !extras.contains(&pvc) && extras.contains(&frls) {
                return format!(
                    "{} C x {} sq. mm XLPE Insulated, FRLS PVC Sheathed Armoured {} Cable",
                    self.core_size,
                    self.sqmm,
                    self.conductor.get_description(extras)
                );
            } else {
                return format!(
                    "{} C x {} sq. mm XLPE Insulated, PVC Sheathed Armoured {} Cable",
                    self.core_size,
                    self.sqmm,
                    self.conductor.get_description(extras)
                );
            }
        }
    }
}
#[derive(Eq, Hash, PartialEq, Deserialize, Clone, Debug)]
pub struct HT {
    pub conductor: Conductor,
    pub voltage_grade: String,
    pub core_size: String,
    pub sqmm: String,
}

impl Description for HT {
    fn get_description(&self, extras: Vec<String>) -> String {
        format!(
            "{} C x {} sq. mm {} grade Armoured {} Cable ",
            self.core_size,
            self.sqmm,
            self.voltage_grade,
            self.conductor.get_description(extras)
        )
    }
}

#[derive(Eq, Hash, PartialEq, Deserialize, Clone, Debug)]
pub struct Flexible {
    pub core_size: String,
    pub sqmm: String,
    pub flexible_type: FlexibleType,
}

impl Description for Flexible {
    fn get_description(&self, extras: Vec<String>) -> String {
        format!(
            "{} C x {} sq. mm Copper Flex. {}",
            self.core_size,
            self.sqmm,
            self.flexible_type.get_description(extras)
        )
    }
}
#[derive(Eq, Hash, PartialEq, Deserialize, Clone, Debug)]
pub enum FlexibleType {
    FR,
    FRLSH,
    HRFR,
    ZHFR
}

impl Description for FlexibleType {
    fn get_description(&self, extras: Vec<String>) -> String {
        match self {
            Self::FR => "FR".to_string(),
            Self::FRLSH => "FRLSH".to_string(),
            Self::HRFR => "HRFR".to_string(),
            Self::ZHFR => "ZHFR".to_string(),
        }
    }
}

#[derive(Eq, Hash, PartialEq, Deserialize, Clone, Debug)]
pub enum Conductor {
    Copper,
    Aluminium,
}

impl Description for Conductor {
    fn get_description(&self, extras: Vec<String>) -> String {
        match self {
            Self::Aluminium => "Aluminium".to_string(),
            Self::Copper => "Copper".to_string(),
        }
    }
}

#[derive(Deserialize, Clone, Debug)]
pub struct PriceList {
    brand: String,
    tags: Vec<String>,
    prices: Vec<Prices>,
}

#[derive(Deserialize, Clone, Debug)]
pub struct Prices {
    product: Product,
    price: f32,
}

pub struct PricingSystem {
    tags: Vec<String>,
    prices: HashMap<(Product), f32>,
}

// for a given user query we try to find the price list corresponding to required brand and matching tags
// then we iterate through the price lists

impl PricingSystem {
    pub fn from_price_list(price_list: PriceList) -> Self {
        let mut prices = HashMap::new();

        for price_entry in price_list.prices {
            prices.insert(price_entry.product.clone(), price_entry.price);
        }

        PricingSystem {
            tags: price_list.tags,
            prices,
        }
    }

    pub fn get_price(&self, product: &Product, tag: &str) -> Option<f32> {
        if self.tags.contains(&tag.to_string()) {
            self.prices.get(&product.clone()).copied()
        } else {
            None
        }
    }
}
