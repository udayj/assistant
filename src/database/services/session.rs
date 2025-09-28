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

        let response = self
            .client
            .from("conversation_messages")
            .insert(message.to_string())
            .execute()
            .await
            .map_err(|e| DatabaseError::QueryError(e.to_string()))?;

        if !response.status().is_success() {
            return Err(DatabaseError::QueryError(format!(
                "Conversation message creation failed for id {} with status: {}",
                conversation_id,
                response.status()
            )));
        }
        // Update conversation last_activity_at
        let update_data = serde_json::json!({
            "last_activity_at": Utc::now()
        });

        let response = self
            .client
            .from("conversations")
            .update(update_data.to_string())
            .eq("id", &conversation_id.to_string())
            .execute()
            .await
            .map_err(|e| DatabaseError::QueryError(e.to_string()))?;

            if !response.status().is_success() {
            return Err(DatabaseError::QueryError(format!(
                "Conversation id {} update failed with status: {}",
                conversation_id,
                response.status()
            )));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::types::{QuerySession, StructuredResponse};
    use chrono::Utc;
    use mockito::ServerGuard;
    use uuid::Uuid;

    fn create_test_session_context() -> SessionContext {
        SessionContext {
            user_id: Uuid::new_v4(),
            session_id: Uuid::new_v4(),
            platform: "test_platform".to_string(),
            user_phone: Some("+1234567890".to_string()),
            telegram_id: Some("test_user".to_string()),
            last_model_used: None,
            conversation_id: None,
        }
    }

    fn create_mock_database_service(server: &ServerGuard) -> DatabaseService {
        let client = postgrest::Postgrest::new(&server.url())
            .insert_header("apikey", "test_key")
            .insert_header("Authorization", "Bearer test_key");

        DatabaseService {
            client,
            admin_telegram_id: "test_admin".to_string(),
        }
    }

    fn create_test_query_session(context: &SessionContext) -> QuerySession {
        QuerySession {
            id: context.session_id,
            user_id: context.user_id,
            query_text: "test query".to_string(),
            query_type: "test_type".to_string(),
            response_type: "processing".to_string(),
            error_message: None,
            total_cost: 0.0,
            processing_time_ms: None,
            platform: context.platform.clone(),
            created_at: Utc::now(),
        }
    }


    #[tokio::test]
    async fn test_create_session_network_error() {
        let server = mockito::Server::new_async().await;
        // Don't create any mocks - this will cause a network error
        let db = create_mock_database_service(&server);
        let context = create_test_session_context();
        let session = create_test_query_session(&context);

        let result = db.create_session(session).await;

        assert!(result.is_err());
        if let Err(DatabaseError::QueryError(msg)) = result {
            // The actual error we get is related to JSON parsing because mockito returns empty body
            assert!(msg.contains("error decoding") || msg.contains("connection") || msg.contains("network") || msg.contains("EOF"));
        } else {
            panic!("Expected DatabaseError::QueryError for network error");
        }
    }

    #[tokio::test]
    async fn test_get_session_total_cost_multiple_events() {
        let mut server = mockito::Server::new_async().await;
        let context = create_test_session_context();
        let session_id = context.session_id;

        // Mock multiple cost events for this session
        let cost_events_data = r#"[
            {"cost_amount": 0.05},
            {"cost_amount": 0.03},
            {"cost_amount": 0.02},
            {"cost_amount": 0.01}
        ]"#;

        let _mock = server
            .mock("GET", "/cost_events")
            .match_query(mockito::Matcher::AllOf(vec![
                mockito::Matcher::UrlEncoded("select".into(), "cost_amount".into()),
                mockito::Matcher::UrlEncoded("query_session_id".into(), format!("eq.{}", session_id)),
            ]))
            .with_status(200)
            .with_body(&cost_events_data)
            .create_async()
            .await;

        let db = create_mock_database_service(&server);
        let result = db.get_session_total_cost(session_id).await;

        assert!(result.is_ok());
        let total_cost = result.unwrap();
        assert_eq!(total_cost, 0.11); // 0.05 + 0.03 + 0.02 + 0.01
    }

    #[tokio::test]
    async fn test_update_session_result_error() {
        let mut server = mockito::Server::new_async().await;
        let context = create_test_session_context();
        let session_id = context.session_id;

        // Mock server error response
        let _mock = server
            .mock("PATCH", "/query_sessions")
            .match_query(mockito::Matcher::UrlEncoded("id".into(), format!("eq.{}", session_id)))
            .with_status(500)
            .with_body("Internal Server Error")
            .create_async()
            .await;

        let db = create_mock_database_service(&server);
        let result = db.update_session_result(
            session_id,
            "error",
            Some("test error".to_string()),
            0.1,
            1000,
            None,
        ).await;

        assert!(result.is_err());
        if let Err(DatabaseError::QueryError(msg)) = result {
            assert!(msg.contains("Update failed with status"));
        } else {
            panic!("Expected DatabaseError::QueryError for update error");
        }
    }

    #[tokio::test]
    async fn test_get_recent_conversation_multiple_conversations() {
        let mut server = mockito::Server::new_async().await;
        let context = create_test_session_context();
        let user_id = context.user_id;
        let conversation_id = Uuid::new_v4();

        // Mock the conversation query - should return most recent conversation
        let conversations_data = format!(
            r#"[{{"id": "{}"}}]"#,
            conversation_id
        );

        let _conv_mock = server
            .mock("GET", "/conversations")
            .match_query(mockito::Matcher::Regex(r".*user_id=eq\..*".to_string()))
            .with_status(200)
            .with_body(&conversations_data)
            .create_async()
            .await;

        // Mock the messages query for this conversation
        let messages_data = r#"[
            {"user_query": "first message", "structured_response": {"response_text": "first response", "response_metadata": null, "timestamp": "2024-01-01T10:00:00Z"}},
            {"user_query": "second message", "structured_response": {"response_text": "second response", "response_metadata": "{\"test\": \"data\"}", "timestamp": "2024-01-01T10:05:00Z"}}
        ]"#;

        let _msg_mock = server
            .mock("GET", "/conversation_messages")
            .match_query(mockito::Matcher::Regex(r".*conversation_id=eq\..*".to_string()))
            .with_status(200)
            .with_body(&messages_data)
            .create_async()
            .await;

        let db = create_mock_database_service(&server);
        let result = db.get_recent_conversation(user_id).await;

        assert!(result.is_ok());
        let conversation = result.unwrap();
        assert!(conversation.is_some());

        let conv = conversation.unwrap();
        assert_eq!(conv.conversation_id, conversation_id);
        assert_eq!(conv.messages.len(), 2);
        assert_eq!(conv.messages[0].user_query, "first message");
        assert_eq!(conv.messages[1].user_query, "second message");

        // Verify structured responses are parsed correctly
        assert!(conv.messages[0].structured_response.is_some());
        assert!(conv.messages[1].structured_response.is_some());
        assert_eq!(conv.messages[0].structured_response.as_ref().unwrap().response_text, "first response");
        assert_eq!(conv.messages[1].structured_response.as_ref().unwrap().response_text, "second response");
    }

    #[tokio::test]
    async fn test_get_recent_conversation_none_within_24_hours() {
        let mut server = mockito::Server::new_async().await;
        let context = create_test_session_context();
        let user_id = context.user_id;

        // Mock empty response (no conversations within 24 hours)
        let _mock = server
            .mock("GET", "/conversations")
            .match_query(mockito::Matcher::Regex(r".*user_id=eq\..*".to_string()))
            .with_status(200)
            .with_body("[]")
            .create_async()
            .await;

        let db = create_mock_database_service(&server);
        let result = db.get_recent_conversation(user_id).await;

        assert!(result.is_ok());
        let conversation = result.unwrap();
        assert!(conversation.is_none());
    }

    #[tokio::test]
    async fn test_save_conversation_message_error() {
        let mut server = mockito::Server::new_async().await;
        let context = create_test_session_context();
        let conversation_id = Uuid::new_v4();
        let session_id = context.session_id;

        let structured_response = StructuredResponse {
            response_text: "test response".to_string(),
            response_metadata: Some("test metadata".to_string()),
            timestamp: "2024-01-01T10:00:00Z".to_string(),
        };

        // Mock server error for message insertion
        let _mock = server
            .mock("POST", "/conversation_messages")
            .with_status(500)
            .with_body("Internal Server Error")
            .create_async()
            .await;

        let db = create_mock_database_service(&server);
        let result = db.save_conversation_message(
            conversation_id,
            session_id,
            "test query",
            Some(structured_response),
        ).await;

        assert!(result.is_err());
        if let Err(DatabaseError::QueryError(msg)) = result {
            assert!(msg.contains("Conversation message creation failed"));
        } else {
            panic!("Expected DatabaseError::QueryError for message save error");
        }
    }

    #[tokio::test]
    async fn test_save_conversation_message_conversation_update_error() {
        let mut server = mockito::Server::new_async().await;
        let context = create_test_session_context();
        let conversation_id = Uuid::new_v4();
        let session_id = context.session_id;

        // Mock successful message insertion
        let _msg_mock = server
            .mock("POST", "/conversation_messages")
            .with_status(201)
            .create_async()
            .await;

        // Mock error for conversation update
        let _conv_mock = server
            .mock("PATCH", "/conversations")
            .match_query(mockito::Matcher::UrlEncoded("id".into(), format!("eq.{}", conversation_id)))
            .with_status(500)
            .with_body("Update failed")
            .create_async()
            .await;

        let db = create_mock_database_service(&server);
        let result = db.save_conversation_message(
            conversation_id,
            session_id,
            "test query",
            None,
        ).await;

        assert!(result.is_err());
        if let Err(DatabaseError::QueryError(msg)) = result {
            assert!(msg.contains("Conversation id") && msg.contains("update failed"));
        } else {
            panic!("Expected DatabaseError::QueryError for conversation update error");
        }
    }

    #[tokio::test]
    async fn test_create_conversation_error() {
        let server = mockito::Server::new_async().await;
        // Don't create any mocks - this will cause a network error
        let db = create_mock_database_service(&server);
        let user_id = Uuid::new_v4();

        let result = db.create_conversation(user_id).await;

        assert!(result.is_err());
        if let Err(DatabaseError::QueryError(msg)) = result {
            // Similar to create_session, we get JSON parsing errors with empty mock responses
            assert!(msg.contains("error decoding") || msg.contains("connection") || msg.contains("network") || msg.contains("EOF"));
        } else {
            panic!("Expected DatabaseError::QueryError for network error");
        }
    }

    #[tokio::test]
    async fn test_create_conversation_invalid_response() {
        let mut server = mockito::Server::new_async().await;
        let user_id = Uuid::new_v4();

        // Mock response without id field
        let _mock = server
            .mock("POST", "/conversations")
            .with_status(201)
            .with_body(r#"[{"invalid": "response"}]"#)
            .create_async()
            .await;

        let db = create_mock_database_service(&server);
        let result = db.create_conversation(user_id).await;

        assert!(result.is_err());
        if let Err(DatabaseError::QueryError(msg)) = result {
            println!("Invalid response error: {}", msg);
            assert!(msg.contains("No conversation ID returned") || msg.contains("error decoding") || msg.contains("EOF"));
        } else {
            panic!("Expected DatabaseError::QueryError for missing conversation ID");
        }
    }
}
