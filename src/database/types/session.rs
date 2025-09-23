use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

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

#[derive(Debug)]
pub struct SessionResult {
    pub success: bool,
    pub error_message: Option<String>,
    pub processing_time_ms: i32,
    pub query_metadata: Option<serde_json::Value>,
}

// Holds session related information
#[derive(Debug, Clone)]
pub struct SessionContext {
    pub user_id: Uuid,
    pub session_id: Uuid,
    pub platform: String,
    pub user_phone: Option<String>,
    pub telegram_id: Option<String>,
    pub last_model_used: Option<String>,
    pub conversation_id: Option<Uuid>, // Used to handle conversation context
}

impl SessionContext {
    pub fn new(user_id: Uuid, platform: &str) -> Self {
        Self {
            user_id,
            session_id: Uuid::new_v4(),
            platform: platform.to_string(),
            user_phone: None,
            telegram_id: None,
            last_model_used: None,
            conversation_id: None,
        }
    }

    pub fn with_phone(mut self, phone: String) -> Self {
        self.user_phone = Some(phone);
        self
    }

    pub fn with_telegram_id(mut self, telegram_id: String) -> Self {
        self.telegram_id = Some(telegram_id);
        self
    }

    pub fn with_conversation_id(mut self, conversation_id: Uuid) -> Self {
        self.conversation_id = Some(conversation_id);
        self
    }
}

// This is actually holding the Query type which was inferred by the Query fulfilment module
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StructuredResponse {
    pub response_text: String,
    pub response_metadata: Option<String>, // JSON string
    pub timestamp: String,
}

// Holds all all user + assistant messages for the conversation
#[derive(Debug, Clone)]
pub struct ConversationContext {
    pub conversation_id: Uuid,
    pub messages: Vec<ConversationMessage>,
}

#[derive(Debug, Clone)]
pub struct ConversationMessage {
    pub user_query: String,
    pub structured_response: Option<StructuredResponse>,
}
