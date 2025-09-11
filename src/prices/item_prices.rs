use crate::prices::utils::normalize_decimal;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

pub trait Description {
    fn get_description(&self, extras: Vec<String>) -> String;
    fn get_brief_description(&self, extras: Vec<String>) -> String;
}

#[derive(PartialEq, Eq, Hash, Deserialize, Clone, Debug, Serialize)]
pub enum Product {
    Cable(Cable),
}

impl Description for Product {
    fn get_description(&self, extras: Vec<String>) -> String {
        match self {
            Self::Cable(cable) => cable.get_description(extras),
        }
    }

    fn get_brief_description(&self, extras: Vec<String>) -> String {
        // NEW
        match self {
            Self::Cable(cable) => cable.get_brief_description(extras),
        }
    }
}

#[derive(PartialEq, Eq, Hash, Deserialize, Clone, Debug, Serialize)]
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

    fn get_brief_description(&self, extras: Vec<String>) -> String {
        match self {
            Self::Coaxial(coaxial_type) => {
                format!("{}", coaxial_type.get_brief_description(extras))
            }
            Self::Solar { solar_type, sqmm } => format!(
                "{}mm² Solar {}",
                sqmm,
                solar_type.get_brief_description(extras)
            ),
            Self::Submersible { core_size, sqmm } => {
                format!("{}C x {}mm² Flat Flex", core_size, sqmm)
            }
            Self::Telephone {
                pair_size,
                conductor_mm,
            } => format!("{}P x {}mm Tel", pair_size, conductor_mm),
            Self::PowerControl(cable) => cable.get_brief_description(extras),
        }
    }
}

#[derive(Eq, Hash, PartialEq, Deserialize, Clone, Debug, Serialize)]
pub enum SolarType {
    BS,
    EN,
}

impl Description for SolarType {
    fn get_description(&self, _extras: Vec<String>) -> String {
        match self {
            Self::BS => "BS".to_string(),
            Self::EN => "EN".to_string(),
        }
    }

    fn get_brief_description(&self, extras: Vec<String>) -> String {
        self.get_description(extras) // Same as full description
    }
}

#[derive(Eq, Hash, PartialEq, Deserialize, Clone, Debug, Serialize)]
pub enum CoaxialType {
    RG6,
    RG11,
    RG59,
}

impl Description for CoaxialType {
    fn get_description(&self, _extras: Vec<String>) -> String {
        match self {
            Self::RG6 => "RG6".to_string(),
            Self::RG11 => "RG11".to_string(),
            Self::RG59 => "RG59".to_string(),
        }
    }

    fn get_brief_description(&self, extras: Vec<String>) -> String {
        self.get_description(extras) // Same as full description
    }
}

#[derive(Eq, Hash, PartialEq, Deserialize, Clone, Debug, Serialize)]
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

    fn get_brief_description(&self, extras: Vec<String>) -> String {
        // NEW
        match self {
            Self::LT(lt_cable) => lt_cable.get_brief_description(extras),
            Self::HT(ht_cable) => ht_cable.get_brief_description(extras),
            Self::Flexible(flexible_cable) => flexible_cable.get_brief_description(extras),
        }
    }
}
#[derive(Eq, Hash, PartialEq, Deserialize, Clone, Debug, Serialize)]
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
                    "{} C x {} sq. mm PVC Insulated, PVC Sheathed Unarmoured {} Cable",
                    self.core_size,
                    self.sqmm,
                    self.conductor.get_description(extras)
                );
            }
            if extras.contains(&pvc) && extras.contains(&frls) {
                return format!(
                    "{} C x {} sq. mm PVC Insulated, FRLS PVC Sheathed Unarmoured {} Cable",
                    self.core_size,
                    self.sqmm,
                    self.conductor.get_description(extras)
                );
            }

            if !extras.contains(&pvc) && extras.contains(&frls) {
                return format!(
                    "{} C x {} sq. mm XLPE Insulated, FRLS PVC Sheathed Unarmoured {} Cable",
                    self.core_size,
                    self.sqmm,
                    self.conductor.get_description(extras)
                );
            } else {
                return format!(
                    "{} C x {} sq. mm XLPE Insulated, PVC Sheathed Unarmoured {} Cable",
                    self.core_size,
                    self.sqmm,
                    self.conductor.get_description(extras)
                );
            }
        }
    }

    fn get_brief_description(&self, extras: Vec<String>) -> String {
        // NEW
        let insulation = if extras.contains(&"pvc".to_string()) {
            "PVC"
        } else {
            "XLPE"
        };
        let sheath = if extras.contains(&"frls".to_string()) {
            "FRLS"
        } else {
            ""
        };
        let armor = if self.armoured { "Armd" } else { "UnArm" };

        format!(
            "{}C x {}mm² {} {} {} {}",
            self.core_size,
            self.sqmm,
            self.conductor.get_brief_description(vec![]),
            insulation,
            sheath,
            armor
        )
    }
}
#[derive(Eq, Hash, PartialEq, Deserialize, Clone, Debug, Serialize)]
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

    fn get_brief_description(&self, _extras: Vec<String>) -> String {
        // NEW
        format!(
            "{}C x {}mm² {} {}kV Arm",
            self.core_size,
            self.sqmm,
            self.conductor.get_brief_description(vec![]),
            self.voltage_grade
        )
    }
}

#[derive(Eq, Hash, PartialEq, Deserialize, Clone, Debug, Serialize)]
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

    fn get_brief_description(&self, extras: Vec<String>) -> String {
        // NEW
        format!(
            "{}C x {}mm² Cu Flex {}",
            self.core_size,
            self.sqmm,
            self.flexible_type.get_brief_description(extras)
        )
    }
}
#[derive(Eq, Hash, PartialEq, Deserialize, Clone, Debug, Serialize)]
pub enum FlexibleType {
    FR,
    FRLSH,
    HRFR,
    ZHFR,
}

impl Description for FlexibleType {
    fn get_description(&self, _extras: Vec<String>) -> String {
        match self {
            Self::FR => "FR".to_string(),
            Self::FRLSH => "FRLSH".to_string(),
            Self::HRFR => "HRFR".to_string(),
            Self::ZHFR => "ZHFR".to_string(),
        }
    }

    fn get_brief_description(&self, extras: Vec<String>) -> String {
        // NEW
        self.get_description(extras) // Same as full description
    }
}

#[derive(Eq, Hash, PartialEq, Deserialize, Clone, Debug, Serialize)]
pub enum Conductor {
    Copper,
    Aluminium,
}

impl Description for Conductor {
    fn get_description(&self, _extras: Vec<String>) -> String {
        match self {
            Self::Aluminium => "Aluminium".to_string(),
            Self::Copper => "Copper".to_string(),
        }
    }

    fn get_brief_description(&self, _extras: Vec<String>) -> String {
        // NEW
        match self {
            Self::Aluminium => "Al".to_string(),
            Self::Copper => "Cu".to_string(),
        }
    }
}

impl Product {
    fn normalize(&self) -> Self {
        match self {
            Product::Cable(cable) => Product::Cable(cable.normalize()),
        }
    }
}

impl Cable {
    fn normalize(&self) -> Self {
        match self {
            Cable::PowerControl(pc) => Cable::PowerControl(pc.normalize()),
            Cable::Telephone {
                pair_size,
                conductor_mm,
            } => Cable::Telephone {
                pair_size: normalize_decimal(pair_size),
                conductor_mm: normalize_decimal(conductor_mm),
            },
            Cable::Submersible { core_size, sqmm } => Cable::Submersible {
                core_size: normalize_decimal(core_size),
                sqmm: normalize_decimal(sqmm),
            },
            Cable::Solar { solar_type, sqmm } => Cable::Solar {
                solar_type: solar_type.clone(),
                sqmm: normalize_decimal(sqmm),
            },
            Cable::Coaxial(coaxial_type) => Cable::Coaxial(coaxial_type.clone()),
        }
    }
}

impl PowerControl {
    fn normalize(&self) -> Self {
        match self {
            PowerControl::LT(lt) => PowerControl::LT(lt.normalize()),
            PowerControl::HT(ht) => PowerControl::HT(ht.normalize()),
            PowerControl::Flexible(flex) => PowerControl::Flexible(flex.normalize()),
        }
    }
}

impl LT {
    fn normalize(&self) -> Self {
        LT {
            conductor: self.conductor.clone(),
            core_size: normalize_decimal(&self.core_size),
            sqmm: normalize_decimal(&self.sqmm),
            armoured: self.armoured,
        }
    }
}

impl HT {
    fn normalize(&self) -> Self {
        HT {
            conductor: self.conductor.clone(),
            voltage_grade: self.voltage_grade.clone(),
            core_size: normalize_decimal(&self.core_size),
            sqmm: normalize_decimal(&self.sqmm),
        }
    }
}

impl Flexible {
    fn normalize(&self) -> Self {
        Flexible {
            core_size: normalize_decimal(&self.core_size),
            sqmm: normalize_decimal(&self.sqmm),
            flexible_type: self.flexible_type.clone(),
        }
    }
}

#[derive(Deserialize, Clone, Debug)]
pub struct PriceList {
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
    prices: HashMap<Product, f32>,
}

// for a given user query we try to find the price list corresponding to required brand and matching tags
// then we iterate through the price lists

impl PricingSystem {
    pub fn from_price_list(price_list: PriceList) -> Self {
        let mut prices = HashMap::new();

        for price_entry in price_list.prices {
            prices.insert(price_entry.product.normalize(), price_entry.price);
        }

        PricingSystem {
            tags: price_list
                .tags
                .iter()
                .map(|tag| tag.trim().to_lowercase())
                .collect(),
            prices,
        }
    }

    pub fn get_price(&self, product: &Product, tag: &str) -> Option<f32> {
        if self.tags.contains(&tag.to_string().trim().to_lowercase()) {
            self.prices.get(&product.normalize()).copied()
        } else {
            None
        }
    }
}

#[cfg(test)]
mod pricelist_tests {
    use crate::prices::item_prices::PriceList;
    use std::fs;
    use std::path::Path;

    #[test]
    fn test_pricelist_deserialization() {
        let test_cases = vec!["assets/processed_pricelists/polycab_armoured_lt.json"];
        for pricelist_path in test_cases {
            let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(pricelist_path);
            let json_content = fs::read_to_string(path)
                .unwrap_or_else(|_| panic!("Failed to read file: {}", pricelist_path));

            let pricelist: PriceList = serde_json::from_str(&json_content)
                .unwrap_or_else(|e| panic!("Failed to deserialize {}: {}", pricelist_path, e));

            println!("✅ Successfully deserialized: {}", pricelist_path);

            // Basic validation
            assert!(!pricelist.tags.is_empty(), "Tags should not be empty");
            assert!(!pricelist.prices.is_empty(), "Prices should not be empty");
        }
    }
}
