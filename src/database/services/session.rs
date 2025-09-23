use super::super::types::{
    ConversationContext, ConversationMessage, QuerySession, SessionContext, SessionResult,
    StructuredResponse,
};
use super::DatabaseError;
use super::DatabaseService;
use chrono::Utc;
use tokio::sync::mpsc;
use tracing::error;
use uuid::Uuid;

impl StructuredResponse {
    pub fn get_metadata(&self) -> String {
        let response = if let Some(metadata) = self.clone().response_metadata {
            metadata
        } else {
            "".to_string()
        };
        response
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
        query_metadata: Option<serde_json::Value>,
    ) -> Result<(), DatabaseError> {
        let update_data = if let Some(err_msg) = error_message {
            serde_json::json!({
                "response_type": response_type,
                "error_message": err_msg,
                "total_cost": total_cost,
                "processing_time_ms": processing_time,
                "metadata": query_metadata
            })
        } else {
            serde_json::json!({
                "response_type": response_type,
                "error_message": null,
                "total_cost": total_cost,
                "processing_time_ms": processing_time,
                "metadata": query_metadata
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

    pub async fn update_session_query_type(
        &self,
        session_id: Uuid,
        query_type: &str,
    ) -> Result<(), DatabaseError> {
        let update_data = serde_json::json!({
            "query_type": query_type
        });

        let response = self
            .client
            .from("query_sessions")
            .update(update_data.to_string())
            .eq("id", &session_id.to_string())
            .execute()
            .await
            .map_err(|e| DatabaseError::QueryError(e.to_string()))?;

        if !response.status().is_success() {
            return Err(DatabaseError::QueryError(format!(
                "Update query type failed with status: {}",
                response.status()
            )));
        }

        Ok(())
    }

    pub async fn create_session_with_context(
        &self,
        context: &SessionContext,
        query_text: &str,
        query_type: &str,
    ) -> Result<Uuid, DatabaseError> {
        let session = QuerySession {
            id: context.session_id,
            user_id: context.user_id,
            query_text: query_text.to_string(),
            query_type: query_type.to_string(),
            response_type: "processing".to_string(),
            error_message: None,
            total_cost: 0.0,
            processing_time_ms: None,
            platform: context.platform.clone(),
            created_at: Utc::now(),
        };

        self.create_session(session).await
    }

    pub async fn complete_session(
        &self,
        context: &SessionContext,
        result: SessionResult,
    ) -> Result<(), DatabaseError> {
        let total_cost = self
            .get_session_total_cost(context.session_id)
            .await
            .unwrap_or(0.0);

        let response_type = if result.success { "success" } else { "error" };

        self.update_session_result(
            context.session_id,
            response_type,
            result.error_message,
            total_cost,
            result.processing_time_ms,
            result.query_metadata,
        )
        .await
    }

    pub async fn complete_session_with_notification(
        &self,
        context: &SessionContext,
        result: SessionResult,
        query_text: &str,
        error_sender: &mpsc::Sender<String>,
    ) -> Result<(), DatabaseError> {
        let total_cost = self
            .get_session_total_cost(context.session_id)
            .await
            .unwrap_or(0.0);

        let response_type = if result.success { "success" } else { "error" };

        let update_result = self
            .update_session_result(
                context.session_id,
                response_type,
                result.error_message,
                total_cost,
                result.processing_time_ms,
                result.query_metadata,
            )
            .await;

        if update_result.is_ok() && result.success {
            let cost_message = self
                .create_cost_notification(
                    context,
                    query_text,
                    total_cost,
                    result.processing_time_ms,
                )
                .await;
            let _ = error_sender.send(cost_message).await;
        }

        update_result
    }

    // Conversation management methods
    pub async fn get_recent_conversation(
        &self,
        user_id: Uuid,
    ) -> Result<Option<ConversationContext>, DatabaseError> {
        let twenty_four_hours_ago = Utc::now() - chrono::Duration::hours(24);

        // First, get the most recent conversation for this user
        let conv_response = self
            .client
            .from("conversations")
            .select("id")
            .eq("user_id", &user_id.to_string())
            .gte("last_activity_at", twenty_four_hours_ago.to_rfc3339())
            .order("last_activity_at.desc")
            .limit(1)
            .execute()
            .await
            .map_err(|e| DatabaseError::QueryError(e.to_string()))?;

        if conv_response.status() == 406 {
            return Ok(None); // No recent conversation
        }

        let conversations: Vec<serde_json::Value> = conv_response
            .json()
            .await
            .map_err(|e| DatabaseError::QueryError(e.to_string()))?;

        if conversations.is_empty() {
            return Ok(None);
        }

        let conversation_id = Uuid::parse_str(conversations[0]["id"].as_str().ok_or(
            DatabaseError::QueryError("Invalid conversation ID".to_string()),
        )?)
        .map_err(|e| DatabaseError::QueryError(e.to_string()))?;

        // Get ALL messages for this conversation
        let messages_response = self
            .client
            .from("conversation_messages")
            .select("user_query,structured_response")
            .eq("conversation_id", &conversation_id.to_string())
            .order("created_at.asc")
            .execute()
            .await
            .map_err(|e| DatabaseError::QueryError(e.to_string()))?;

        let message_data: Vec<serde_json::Value> = messages_response
            .json()
            .await
            .map_err(|e| DatabaseError::QueryError(e.to_string()))?;

        let mut messages = Vec::new();
        for msg in message_data {
            let structured_response = if let Some(response_data) = msg.get("structured_response") {
                serde_json::from_value::<StructuredResponse>(response_data.clone()).ok()
            } else {
                None
            };
            messages.push(ConversationMessage {
                user_query: msg["user_query"].as_str().unwrap_or("").to_string(),
                structured_response,
            });
        }

        Ok(Some(ConversationContext {
            conversation_id,
            messages,
        }))
    }

    pub async fn create_conversation(&self, user_id: Uuid) -> Result<Uuid, DatabaseError> {
        let new_conversation = serde_json::json!({
            "user_id": user_id,
            "created_at": Utc::now(),
            "last_activity_at": Utc::now()
        });

        let response = self
            .client
            .from("conversations")
            .insert(new_conversation.to_string())
            .select("id")
            .execute()
            .await
            .map_err(|e| DatabaseError::QueryError(e.to_string()))?;

        let result: Vec<serde_json::Value> = response
            .json()
            .await
            .map_err(|e| DatabaseError::QueryError(e.to_string()))?;

        let conversation_id = result[0]["id"].as_str().ok_or(DatabaseError::QueryError(
            "No conversation ID returned".to_string(),
        ))?;

        Uuid::parse_str(conversation_id).map_err(|e| DatabaseError::QueryError(e.to_string()))
    }

    pub async fn save_conversation_message(
        &self,
        conversation_id: Uuid,
        session_id: Uuid,
        user_query: &str,
        structured_response: Option<StructuredResponse>,
    ) -> Result<(), DatabaseError> {
        let message = serde_json::json!({
            "conversation_id": conversation_id,
            "session_id": session_id,
            "user_query": user_query,
            "structured_response": structured_response,
            "created_at": Utc::now()
        });

        let _response = self
            .client
            .from("conversation_messages")
            .insert(message.to_string())
            .execute()
            .await
            .map_err(|e| DatabaseError::QueryError(e.to_string()))?;

        // Update conversation last_activity_at
        let update_data = serde_json::json!({
            "last_activity_at": Utc::now()
        });

        let _response = self
            .client
            .from("conversations")
            .update(update_data.to_string())
            .eq("id", &conversation_id.to_string())
            .execute()
            .await
            .map_err(|e| DatabaseError::QueryError(e.to_string()))?;

        Ok(())
    }
}
