use super::super::types::User;
use super::DatabaseError;
use super::DatabaseService;

impl DatabaseService {
    // Find user based on whatsapp phone number
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

    // Find user based on telegram id
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

    // Function used to create a user from telegram.id for future approval
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

    pub async fn is_user_authorized(&self, user: &User) -> bool {
        user.status == "active"
    }

    pub async fn is_admin(&self, telegram_id: &str) -> bool {
        telegram_id == self.admin_telegram_id
    }

    // Approve pending telegram user
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

    // approva whatsapp user - no pending step for whatsapp users like it is for telegram users
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
