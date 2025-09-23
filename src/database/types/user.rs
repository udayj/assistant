use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct User {
    pub id: Uuid,
    pub phone_number: Option<String>,
    pub telegram_id: Option<String>,
    pub status: String,
    pub platform: String,
    pub created_at: DateTime<Utc>,
}
