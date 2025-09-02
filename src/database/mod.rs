use chrono::{DateTime, Utc};
use postgrest::Postgrest;
use serde::{Deserialize, Serialize};
use std::env;
use thiserror::Error;
use tracing::error;
use uuid::Uuid;

#[derive(Error, Debug)]
pub enum DatabaseError {
    #[error("Database connection error: {0}")]
    ConnectionError(String),
    #[error("Query error: {0}")]
    QueryError(String),
    #[error("User not found")]
    UserNotFound,
    #[error("User not authorized")]
    UserNotAuthorized,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct User {
    pub id: Uuid,
    pub phone_number: Option<String>,
    pub telegram_id: Option<String>,
    pub status: String,
    pub platform: String,
    pub created_at: DateTime<Utc>,
}

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
pub struct QuerySession {
    pub id: Uuid,
    pub user_id: Uuid,
    pub query_text: String,
    pub query_type: String,
    pub response_type: String,
    pub error_message: Option<String>,
    pub total_cost: f64,
    pub processing_time_ms: Option<i32>,
    pub platform: String,
    pub created_at: DateTime<Utc>,
}

pub struct DatabaseService {
    client: Postgrest,
}

impl DatabaseService {
    pub fn new() -> Result<Self, DatabaseError> {
        let url = env::var("SUPABASE_URL")
            .map_err(|_| DatabaseError::ConnectionError("SUPABASE_URL not found".to_string()))?;
        let service_key = env::var("SUPABASE_KEY")
            .map_err(|_| DatabaseError::ConnectionError("SUPABASE_KEY not found".to_string()))?;

        let rest_url = format!("{}/rest/v1", url);
        let client = Postgrest::new(&rest_url)
            .insert_header("apikey", &service_key)
            .insert_header("Authorization", &format!("Bearer {}", service_key));

        Ok(Self { client })
    }

    pub async fn get_user_by_phone(&self, phone: &str) -> Result<Option<User>, DatabaseError> {
        let response = self
            .client
            .from("users")
            .select("*")
            .eq("phone_number", format!("whatsapp:{}", phone))
            .single()
            .execute()
            .await
            .map_err(|e| DatabaseError::QueryError(e.to_string()))?;
        println!("response:{:#?}", response);
        if response.status() == 406 {
            // No rows found
            return Ok(None);
        }

        let user: User = response
            .json()
            .await
            .map_err(|e| DatabaseError::QueryError(e.to_string()))?;

        Ok(Some(user))
    }

    pub async fn get_user_by_telegram(
        &self,
        telegram_id: &str,
    ) -> Result<Option<User>, DatabaseError> {
        let response = self
            .client
            .from("users")
            .select("*")
            .eq("telegram_id", telegram_id)
            .single()
            .execute()
            .await
            .map_err(|e| DatabaseError::QueryError(e.to_string()))?;

        if response.status() == 406 {
            // No rows found
            return Ok(None);
        }

        let user: User = response
            .json()
            .await
            .map_err(|e| DatabaseError::QueryError(e.to_string()))?;

        Ok(Some(user))
    }

    pub async fn is_user_authorized(&self, user: &User) -> bool {
        user.status == "active"
    }

    pub async fn log_cost_event(&self, cost_event: CostEvent) -> Result<(), DatabaseError> {
        let _response = self
            .client
            .from("cost_events")
            .insert(serde_json::to_string(&cost_event).unwrap())
            .execute()
            .await
            .map_err(|e| DatabaseError::QueryError(e.to_string()))?;

        Ok(())
    }

    pub async fn create_pending_telegram_user(
        &self,
        telegram_id: &str,
    ) -> Result<(), DatabaseError> {
        let new_user = serde_json::json!({
            "telegram_id": telegram_id,
            "status": "pending_approval",
            "platform": "telegram"
        });

        let _response = self
            .client
            .from("users")
            .insert(new_user.to_string())
            .execute()
            .await
            .map_err(|e| DatabaseError::QueryError(e.to_string()))?;

        Ok(())
    }
}

impl DatabaseService {
    pub async fn is_admin(&self, telegram_id: &str) -> bool {
        telegram_id == "2050924196"
    }

    pub async fn approve_telegram_user(&self, telegram_id: &str) -> Result<bool, DatabaseError> {
        let response = self
            .client
            .from("users")
            .update(
                serde_json::json!({
                    "status": "active",
                    "approved_at": chrono::Utc::now()
                })
                .to_string(),
            )
            .eq("telegram_id", telegram_id)
            .eq("status", "pending_approval")
            .execute()
            .await
            .map_err(|e| DatabaseError::QueryError(e.to_string()))?;

        Ok(response.status().is_success())
    }

    pub async fn approve_whatsapp_user(&self, phone: &str) -> Result<(), DatabaseError> {
        let new_user = serde_json::json!({
            "phone_number": format!("whatsapp:{}",phone),
            "status": "active",
            "platform": "whatsapp",
            "approved_at": chrono::Utc::now()
        });

        let _response = self
            .client
            .from("users")
            .insert(new_user.to_string())
            .execute()
            .await
            .map_err(|e| DatabaseError::QueryError(e.to_string()))?;

        Ok(())
    }

    pub async fn get_pending_users(&self) -> Result<Vec<User>, DatabaseError> {
        let response = self
            .client
            .from("users")
            .select("*")
            .eq("status", "pending_approval")
            .execute()
            .await
            .map_err(|e| DatabaseError::QueryError(e.to_string()))?;

        let users: Vec<User> = response
            .json()
            .await
            .map_err(|e| DatabaseError::QueryError(e.to_string()))?;

        Ok(users)
    }
}

impl DatabaseService {
    pub async fn create_session(&self, session: QuerySession) -> Result<Uuid, DatabaseError> {
        let response = self
            .client
            .from("query_sessions")
            .insert(serde_json::to_string(&session).unwrap())
            .select("id")
            .execute()
            .await
            .map_err(|e| DatabaseError::QueryError(e.to_string()))?;

        let result: Vec<serde_json::Value> = response
            .json()
            .await
            .map_err(|e| DatabaseError::QueryError(e.to_string()))?;

        let session_id = result[0]["id"].as_str().ok_or(DatabaseError::QueryError(
            "No session ID returned".to_string(),
        ))?;

        Uuid::parse_str(session_id).map_err(|e| DatabaseError::QueryError(e.to_string()))
    }

    pub async fn update_session_result(
        &self,
        session_id: Uuid,
        response_type: &str,
        error_message: Option<String>,
        total_cost: f64,
        processing_time: i32,
    ) -> Result<(), DatabaseError> {
        let update_data = if let Some(err_msg) = error_message {
            serde_json::json!({
                "response_type": response_type,
                "error_message": err_msg,
                "total_cost": total_cost,
                "processing_time_ms": processing_time
            })
        } else {
            serde_json::json!({
                "response_type": response_type,
                "error_message": null,
                "total_cost": total_cost,
                "processing_time_ms": processing_time
            })
        };

        let response = self
            .client
            .from("query_sessions")
            .update(update_data.to_string())
            .eq("id", &session_id.to_string())
            .execute()
            .await
            .map_err(|e| DatabaseError::QueryError(e.to_string()))?;

        if !response.status().is_success() {
            error!(err= ?response.status(), "Error updating session id:{}, err ", session_id);
            let error_text = response
                .text()
                .await
                .unwrap_or_else(|_| "Unknown error".to_string());
            error!("Actual error:{}", error_text);
            return Err(DatabaseError::QueryError(format!(
                "Update failed with status: {}",
                error_text
            )));
        }

        Ok(())
    }

    pub async fn get_session_total_cost(&self, session_id: Uuid) -> Result<f64, DatabaseError> {
        let response = self
            .client
            .from("cost_events")
            .select("cost_amount")
            .eq("query_session_id", &session_id.to_string())
            .execute()
            .await
            .map_err(|e| DatabaseError::QueryError(e.to_string()))?;

        let costs: Vec<serde_json::Value> = response
            .json()
            .await
            .map_err(|e| DatabaseError::QueryError(e.to_string()))?;

        let total: f64 = costs.iter().filter_map(|c| c["cost_amount"].as_f64()).sum();

        Ok(total)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dotenvy::dotenv;

    #[tokio::test]
    async fn test_database_connection() {
        dotenv().ok();
        let db = DatabaseService::new().expect("Failed to create database service");

        // Test user lookup (should return None for non-existent user)
        let result = db.get_user_by_phone("+999999999999").await;
        println!("result:{:#?}", result);
        assert!(result.is_ok());
        assert!(result.unwrap().is_none());

        println!("✅ Database connection test passed");
    }
}
