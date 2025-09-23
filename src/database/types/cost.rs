use super::SessionContext;
use crate::database::{DatabaseError, DatabaseService};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Serialize, Deserialize)]
pub struct CostEvent {
    pub user_id: Uuid,
    pub query_session_id: Uuid,
    pub event_type: String,
    pub unit_cost: f64,
    pub unit_type: String,
    pub units_consumed: i32,
    pub cost_amount: f64,
    pub metadata: Option<serde_json::Value>,
    pub platform: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ClaudeRates {
    pub input_token: f64,
    pub cache_hit_refresh: f64,
    pub output_token: f64,
    pub one_h_cache_writes: f64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct GroqRates {
    pub input_token: f64,
    pub output_token: f64,
}

impl Default for ClaudeRates {
    fn default() -> Self {
        Self {
            input_token: 3.0,
            cache_hit_refresh: 0.3,
            output_token: 15.0,
            one_h_cache_writes: 6.0,
        }
    }
}

impl Default for GroqRates {
    fn default() -> Self {
        Self {
            input_token: 1.0,
            output_token: 3.0,
        }
    }
}

pub struct CostEventBuilder {
    context: SessionContext,
    event_type: String,
    unit_cost: f64,
    unit_type: String,
    units_consumed: i32,
    metadata: Option<serde_json::Value>,
}

impl CostEventBuilder {
    pub fn new(context: SessionContext, event_type: &str) -> Self {
        Self {
            context,
            event_type: event_type.to_string(),
            unit_cost: 0.0,
            unit_type: "unit".to_string(),
            units_consumed: 1,
            metadata: None,
        }
    }

    pub fn with_cost(mut self, unit_cost: f64, unit_type: &str, units_consumed: i32) -> Self {
        self.unit_cost = unit_cost;
        self.unit_type = unit_type.to_string();
        self.units_consumed = units_consumed;
        self
    }

    pub fn with_metadata(mut self, metadata: serde_json::Value) -> Self {
        self.metadata = Some(metadata);
        self
    }

    pub async fn log(self, database: &DatabaseService) -> Result<(), DatabaseError> {
        database
            .log_cost_event(CostEvent {
                user_id: self.context.user_id,
                query_session_id: self.context.session_id,
                event_type: self.event_type,
                unit_cost: self.unit_cost,
                unit_type: self.unit_type,
                units_consumed: self.units_consumed,
                cost_amount: self.unit_cost * self.units_consumed as f64,
                metadata: self.metadata,
                platform: self.context.platform,
                created_at: Utc::now(),
            })
            .await
    }

    pub async fn log_total_cost(self, database: &DatabaseService) -> Result<(), DatabaseError> {
        database
            .log_cost_event(CostEvent {
                user_id: self.context.user_id,
                query_session_id: self.context.session_id,
                event_type: self.event_type,
                unit_cost: self.unit_cost,
                unit_type: self.unit_type,
                units_consumed: self.units_consumed,
                cost_amount: self.unit_cost,
                metadata: self.metadata,
                platform: self.context.platform,
                created_at: Utc::now(),
            })
            .await
    }
}
