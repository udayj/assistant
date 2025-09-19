use chrono::{DateTime, Utc};
use postgrest::Postgrest;
use serde::{Deserialize, Serialize};
use std::env;
use thiserror::Error;
use tokio::sync::mpsc;
use tracing::{error};
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

#[derive(Debug, Clone)]
pub struct SessionContext {
    pub user_id: Uuid,
    pub session_id: Uuid,
    pub platform: String,
    pub user_phone: Option<String>,
    pub telegram_id: Option<String>,
    pub last_model_used: Option<String>,
    pub conversation_id: Option<Uuid>,
}

#[derive(Debug)]
pub struct SessionResult {
    pub success: bool,
    pub error_message: Option<String>,
    pub processing_time_ms: i32,
    pub query_metadata: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StructuredResponse {
    pub response_text: String,
    pub response_metadata: Option<String>, // JSON string
    pub timestamp: String,
}

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
}

impl DatabaseService {
    pub async fn get_claude_rates(&self) -> Result<ClaudeRates, DatabaseError> {
        let response = self
            .client
            .from("cost_rate_history")
            .select("cost_type,unit_cost")
            .eq("service_provider", "anthropic")
            .execute()
            .await;

        match response {
            Ok(resp) if resp.status() == 200 => {
                let rates: Vec<serde_json::Value> = resp
                    .json()
                    .await
                    .map_err(|e| DatabaseError::QueryError(e.to_string()))?;
                println!("resp:{:#?}", rates);
                let mut claude_rates = ClaudeRates::default();
                for rate in rates {
                    let cost_type = rate["cost_type"].as_str().unwrap_or("");
                    let unit_cost = rate["unit_cost"].as_f64().unwrap_or(0.0);

                    match cost_type {
                        "input_token" => claude_rates.input_token = unit_cost,
                        "output_token" => claude_rates.output_token = unit_cost,
                        "cache_hit_refresh" => claude_rates.cache_hit_refresh = unit_cost,
                        "1h_cache_writes" => claude_rates.one_h_cache_writes = unit_cost,
                        _ => {}
                    }
                }
                Ok(claude_rates)
            }
            _ => Ok(ClaudeRates::default()),
        }
    }

    pub async fn get_groq_rates(&self) -> Result<GroqRates, DatabaseError> {
        let response = self
            .client
            .from("cost_rate_history")
            .select("cost_type,unit_cost")
            .eq("service_provider", "groq_kimi_k2")
            .execute()
            .await;
        match response {
            Ok(resp) if resp.status() == 200 => {
                let rates: Vec<serde_json::Value> = resp
                    .json()
                    .await
                    .map_err(|e| DatabaseError::QueryError(e.to_string()))?;

                let mut groq_rates = GroqRates::default();
                for rate in rates {
                    let cost_type = rate["cost_type"].as_str().unwrap_or("");
                    let unit_cost = rate["unit_cost"].as_f64().unwrap_or(0.0);

                    match cost_type {
                        "input_token" => groq_rates.input_token = unit_cost,
                        "output_token" => groq_rates.output_token = unit_cost,
                        _ => {}
                    }
                }
                Ok(groq_rates)
            }
            _ => Ok(GroqRates::default()),
        }
    }
}

impl DatabaseService {
    async fn get_session_cost_events(
        &self,
        session_id: Uuid,
    ) -> Result<Vec<CostEvent>, DatabaseError> {
        let response = self
            .client
            .from("cost_events")
            .select("*")
            .eq("query_session_id", &session_id.to_string())
            .execute()
            .await
            .map_err(|e| DatabaseError::QueryError(e.to_string()))?;

        let events: Vec<CostEvent> = response
            .json()
            .await
            .map_err(|e| DatabaseError::QueryError(e.to_string()))?;

        Ok(events)
    }

    async fn create_cost_notification(
        &self,
        context: &SessionContext,
        query_text: &str,
        total_cost: f64,
        processing_time: i32,
    ) -> String {
        let forex_rate = 90.0; // rough Rs. per $
        let cost_events = match self.get_session_cost_events(context.session_id).await {
            Ok(events) => events,
            Err(e) => {
                error!(
                    "Failed to get cost events for session {}: {}",
                    context.session_id, e
                );
                return format!(
                    "💰 Query Cost Alert\n\nPlatform: {}\nQuery: {}\nTotal Cost: Rs.{:.3}\n\nBreakdown: Unable to retrieve details",
                    context.platform,
                    if query_text.len() > 100 { format!("{}...", &query_text[..97]) } else { query_text.to_string() },
                    total_cost * forex_rate
                );
            }
        };

        let truncated_query = if query_text.len() > 100 {
            format!("{}...", &query_text[..97])
        } else {
            query_text.to_string()
        };

        let mut breakdown = String::new();
        let mut claude_cost = 0.0;
        let mut groq_cost = 0.0;
        let mut groq_decision_cost = 0.0;
        let mut groq_whisper_cost = 0.0;
        let mut textract_cost = 0.0;
        let mut platform_cost = 0.0;

        for event in &cost_events {
            match event.event_type.as_str() {
                "claude_api" => claude_cost += event.cost_amount,
                "groq_api" => groq_cost += event.cost_amount,
                "groq_decision" => groq_decision_cost += event.cost_amount,
                "groq_whisper" => groq_whisper_cost += event.cost_amount,
                "textract_api" => textract_cost += event.cost_amount,
                t if t.contains("whatsapp") || t.contains("telegram") => {
                    platform_cost += event.cost_amount
                }
                _ => {}
            }
        }

        if claude_cost > 0.0 {
            breakdown.push_str(&format!(
                "• Claude API: Rs.{:.3}\n",
                claude_cost * forex_rate
            ));
        }
        if groq_cost > 0.0 {
            breakdown.push_str(&format!("• Groq API: Rs.{:.3}\n", groq_cost * forex_rate));
        }

        if groq_decision_cost > 0.0 {
            breakdown.push_str(&format!(
                "• Groq Decision API: Rs.{:.3}\n",
                groq_decision_cost * forex_rate
            ));
        }

        if groq_whisper_cost > 0.0 {
            breakdown.push_str(&format!(
                "• Groq Whisper: Rs.{:.3}\n",
                groq_whisper_cost * forex_rate
            ));
        }
        if textract_cost > 0.0 {
            breakdown.push_str(&format!(
                "• Textract: Rs.{:.3}\n",
                textract_cost * forex_rate
            ));
        }
        if platform_cost > 0.0 {
            let platform_name = context.platform.to_uppercase();
            breakdown.push_str(&format!(
                "• {}: Rs.{:.3}\n",
                platform_name,
                platform_cost * forex_rate
            ));
        }

        format!(
            "💰 Query Cost Alert\n\nPlatform: {}\nQuery: {}\nTotal Cost: Rs.{:.3}\nProcessing Time: {} ms\n\nBreakdown:\n{}",
            context.platform, truncated_query, total_cost * forex_rate, processing_time, breakdown
        )
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

    pub async fn log_whatsapp_message(
        &self,
        context: &SessionContext,
        outgoing: bool,
        message_len: usize,
        has_media: bool,
    ) -> Result<(), DatabaseError> {
        let event_type = if outgoing {
            "whatsapp_outgoing"
        } else {
            "whatsapp_incoming"
        };
        let metadata = serde_json::json!({
            "message_length": message_len,
            "has_media": has_media,
            "phone_number": context.user_phone
        });

        CostEventBuilder::new(context.clone(), event_type)
            .with_cost(0.005, "message", 1)
            .with_metadata(metadata)
            .log(self)
            .await
    }

    pub async fn log_claude_api_call(
        &self,
        context: &SessionContext,
        input_tokens: i32,
        cache_read_tokens: i32,
        cache_write_tokens: i32,
        output_tokens: i32,
        model: &str,
    ) -> Result<(), DatabaseError> {
        let rates = self.get_claude_rates().await.unwrap_or_default();
        let input_cost = (input_tokens as f64 * rates.input_token) / 1_000_000.0;
        let cache_read_cost = (cache_read_tokens as f64 * rates.cache_hit_refresh) / 1_000_000.0;
        let output_cost = (output_tokens as f64 * rates.output_token) / 1_000_000.0;
        let cache_write_cost = (cache_write_tokens as f64 * rates.one_h_cache_writes) / 1_000_000.0;

        let metadata = serde_json::json!({
            "model": model,
            "input_tokens": input_tokens,
            "cache_read_tokens": cache_read_tokens,
            "cache_write_tokens": cache_write_tokens,
            "output_tokens": output_tokens,
            "input_cost": input_cost,
            "cache_read_cost": cache_read_cost,
            "output_cost": output_cost,
            "cache_write_cost": cache_write_cost
        });

        let total_cost = input_cost + cache_read_cost + cache_write_cost + output_cost;

        let total_tokens = input_tokens + cache_read_tokens + cache_write_tokens + output_tokens;

        CostEventBuilder::new(context.clone(), "claude_api")
            .with_cost(total_cost, "per_1m_tokens", total_tokens)
            .with_metadata(metadata)
            .log_total_cost(self)
            .await
    }

    pub async fn log_textract_usage(
        &self,
        context: &SessionContext,
        image_size_bytes: usize,
    ) -> Result<(), DatabaseError> {
        let metadata = serde_json::json!({
            "image_size_bytes": image_size_bytes
        });

        CostEventBuilder::new(context.clone(), "textract_api")
            .with_cost(0.0015, "per_page", 1)
            .with_metadata(metadata)
            .log(self)
            .await
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
