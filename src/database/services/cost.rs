use super::super::types::{ClaudeRates, CostEvent, CostEventBuilder, GroqRates, SessionContext};
use super::DatabaseError;
use super::DatabaseService;
use tracing::error;
use uuid::Uuid;

impl DatabaseService {
    pub async fn log_cost_event(&self, cost_event: CostEvent) -> Result<(), DatabaseError> {
        let response = self
            .client
            .from("cost_events")
            .insert(serde_json::to_string(&cost_event).unwrap())
            .execute()
            .await
            .map_err(|e| DatabaseError::QueryError(e.to_string()))?;

        if response.status() != 201 && response.status() != 204 {
            return Err(DatabaseError::QueryError(
                "Cost event insertion error".into(),
            ));
        }
        Ok(())
    }

    // Get current api costing for claude model
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

    // Get current api costing for groq models
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

    // Log claude api call with token and cost details for given session_id
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

    // Log Amazon textract api usage - for queries involving ocr
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

    // Get cost events associated with given session_id
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

    // Create notification of session cost - total + individual components for sending on telegram alert channel
    // Does not modify the db - just collects and summarises the data
    pub async fn create_cost_notification(
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::types::{CostEvent, SessionContext};
    use chrono::Utc;
    use mockito::ServerGuard;
    use serial_test::serial;
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

        DatabaseService { client }
    }

    #[tokio::test]
    #[serial]
    async fn test_log_cost_event_success() {
        let mut server = mockito::Server::new_async().await;
        let _mock = server
            .mock("POST", "/cost_events")
            .with_status(201)
            .with_body(r#"{"id": 1}"#)
            .create_async()
            .await;

        let db = create_mock_database_service(&server);
        let cost_event = CostEvent {
            user_id: Uuid::new_v4(),
            query_session_id: Uuid::new_v4(),
            event_type: "test_event".to_string(),
            unit_cost: 0.01,
            unit_type: "token".to_string(),
            units_consumed: 100,
            cost_amount: 1.0,
            metadata: None,
            platform: "test".to_string(),
            created_at: Utc::now(),
        };

        let result = db.log_cost_event(cost_event).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    #[serial]
    async fn test_log_cost_event_database_error() {
        let server = mockito::Server::new_async().await;
        // Don't mock anything - this will cause a connection error
        let db = create_mock_database_service(&server);
        let cost_event = CostEvent {
            user_id: Uuid::new_v4(),
            query_session_id: Uuid::new_v4(),
            event_type: "test_event".to_string(),
            unit_cost: 0.01,
            unit_type: "token".to_string(),
            units_consumed: 100,
            cost_amount: 1.0,
            metadata: None,
            platform: "test".to_string(),
            created_at: Utc::now(),
        };

        let result = db.log_cost_event(cost_event).await;

        assert!(result.is_err());
    }

    #[tokio::test]
    #[serial]
    async fn test_get_claude_rates_success() {
        let mut server = mockito::Server::new_async().await;
        let mock_data = r#"[
            {"cost_type": "input_token", "unit_cost": 2.5},
            {"cost_type": "output_token", "unit_cost": 12.0},
            {"cost_type": "cache_hit_refresh", "unit_cost": 0.25},
            {"cost_type": "1h_cache_writes", "unit_cost": 5.5}
        ]"#;

        let _mock = server
            .mock("GET", "/cost_rate_history")
            .match_query(mockito::Matcher::AllOf(vec![
                mockito::Matcher::UrlEncoded("select".into(), "cost_type,unit_cost".into()),
                mockito::Matcher::UrlEncoded("service_provider".into(), "eq.anthropic".into()),
            ]))
            .with_status(200)
            .with_body(mock_data)
            .create_async()
            .await;

        let db = create_mock_database_service(&server);
        let result = db.get_claude_rates().await;

        assert!(result.is_ok());
        let rates = result.unwrap();
        assert_eq!(rates.input_token, 2.5);
        assert_eq!(rates.output_token, 12.0);
        assert_eq!(rates.cache_hit_refresh, 0.25);
        assert_eq!(rates.one_h_cache_writes, 5.5);
    }

    #[tokio::test]
    #[serial]
    async fn test_get_claude_rates_partial_data() {
        let mut server = mockito::Server::new_async().await;
        let mock_data = r#"[
            {"cost_type": "input_token", "unit_cost": 2.5},
            {"cost_type": "output_token", "unit_cost": 12.0}
        ]"#;

        let _mock = server
            .mock("GET", "/cost_rate_history")
            .match_query(mockito::Matcher::AllOf(vec![
                mockito::Matcher::UrlEncoded("select".into(), "cost_type,unit_cost".into()),
                mockito::Matcher::UrlEncoded("service_provider".into(), "eq.anthropic".into()),
            ]))
            .with_status(200)
            .with_body(mock_data)
            .create_async()
            .await;

        let db = create_mock_database_service(&server);
        let result = db.get_claude_rates().await;

        assert!(result.is_ok());
        let rates = result.unwrap();
        assert_eq!(rates.input_token, 2.5);
        assert_eq!(rates.output_token, 12.0);
        // These should be defaults from ClaudeRates::default() since not provided
        assert_eq!(rates.cache_hit_refresh, 0.3);
        assert_eq!(rates.one_h_cache_writes, 6.0);
    }

    #[tokio::test]
    #[serial]
    async fn test_get_claude_rates_unknown_cost_types() {
        let mut server = mockito::Server::new_async().await;
        let mock_data = r#"[
            {"cost_type": "unknown_type", "unit_cost": 999.0},
            {"cost_type": "input_token", "unit_cost": 2.5}
        ]"#;

        let _mock = server
            .mock("GET", "/cost_rate_history")
            .match_query(mockito::Matcher::AllOf(vec![
                mockito::Matcher::UrlEncoded("select".into(), "cost_type,unit_cost".into()),
                mockito::Matcher::UrlEncoded("service_provider".into(), "eq.anthropic".into()),
            ]))
            .with_status(200)
            .with_body(mock_data)
            .create_async()
            .await;

        let db = create_mock_database_service(&server);
        let result = db.get_claude_rates().await;

        assert!(result.is_ok());
        let rates = result.unwrap();
        assert_eq!(rates.input_token, 2.5);
        // Unknown type should be ignored, default value from ClaudeRates::default()
        assert_eq!(rates.output_token, 15.0);
    }

    #[tokio::test]
    #[serial]
    async fn test_get_claude_rates_malformed_data() {
        let mut server = mockito::Server::new_async().await;
        let mock_data = r#"[
            {"cost_type": null, "unit_cost": "invalid"},
            {"cost_type": "input_token", "unit_cost": 2.5}
        ]"#;

        let _mock = server
            .mock("GET", "/cost_rate_history")
            .match_query(mockito::Matcher::AllOf(vec![
                mockito::Matcher::UrlEncoded("select".into(), "cost_type,unit_cost".into()),
                mockito::Matcher::UrlEncoded("service_provider".into(), "eq.anthropic".into()),
            ]))
            .with_status(200)
            .with_body(mock_data)
            .create_async()
            .await;

        let db = create_mock_database_service(&server);
        let result = db.get_claude_rates().await;

        assert!(result.is_ok());
        let rates = result.unwrap();
        assert_eq!(rates.input_token, 2.5);
        // Malformed entries should use defaults from ClaudeRates::default()
        assert_eq!(rates.output_token, 15.0);
    }

    #[tokio::test]
    #[serial]
    async fn test_get_claude_rates_database_error_returns_defaults() {
        let mut server = mockito::Server::new_async().await;
        let _mock = server
            .mock("GET", "/cost_rate_history")
            .with_status(500)
            .create_async()
            .await;

        let db = create_mock_database_service(&server);
        let result = db.get_claude_rates().await;

        assert!(result.is_ok());
        let rates = result.unwrap();
        // Should return defaults from ClaudeRates::default()
        assert_eq!(rates.input_token, 3.0);
        assert_eq!(rates.output_token, 15.0);
        assert_eq!(rates.cache_hit_refresh, 0.3);
        assert_eq!(rates.one_h_cache_writes, 6.0);
    }

    #[tokio::test]
    #[serial]
    async fn test_get_groq_rates_success() {
        let mut server = mockito::Server::new_async().await;
        let mock_data = r#"[
            {"cost_type": "input_token", "unit_cost": 0.8},
            {"cost_type": "output_token", "unit_cost": 2.5}
        ]"#;

        let _mock = server
            .mock("GET", "/cost_rate_history")
            .match_query(mockito::Matcher::AllOf(vec![
                mockito::Matcher::UrlEncoded("select".into(), "cost_type,unit_cost".into()),
                mockito::Matcher::UrlEncoded("service_provider".into(), "eq.groq_kimi_k2".into()),
            ]))
            .with_status(200)
            .with_body(mock_data)
            .create_async()
            .await;

        let db = create_mock_database_service(&server);
        let result = db.get_groq_rates().await;

        assert!(result.is_ok());
        let rates = result.unwrap();
        assert_eq!(rates.input_token, 0.8);
        assert_eq!(rates.output_token, 2.5);
    }

    #[tokio::test]
    #[serial]
    async fn test_log_whatsapp_message_outgoing() {
        let mut server = mockito::Server::new_async().await;
        let _mock = server
            .mock("POST", "/cost_events")
            .with_status(201)
            .create_async()
            .await;

        let db = create_mock_database_service(&server);
        let context = create_test_session_context();

        let result = db.log_whatsapp_message(&context, true, 150, true).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    #[serial]
    async fn test_log_claude_api_call_zero_tokens() {
        let mut server = mockito::Server::new_async().await;
        let rates_data = r#"[]"#;
        let _rates_mock = server
            .mock("GET", "/cost_rate_history")
            .with_status(200)
            .with_body(rates_data)
            .create_async()
            .await;
        let _cost_mock = server
            .mock("POST", "/cost_events")
            .with_status(201)
            .create_async()
            .await;

        let db = create_mock_database_service(&server);
        let context = create_test_session_context();

        let result = db.log_claude_api_call(&context, 0, 0, 0, 0, "test").await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    #[serial]
    async fn test_log_textract_usage_large_size() {
        let mut server = mockito::Server::new_async().await;
        let _mock = server
            .mock("POST", "/cost_events")
            .with_status(201)
            .create_async()
            .await;

        let db = create_mock_database_service(&server);
        let context = create_test_session_context();

        let result = db.log_textract_usage(&context, 50_000_000).await; // 50MB
        assert!(result.is_ok());
    }

    #[tokio::test]
    #[serial]
    async fn test_create_cost_notification_long_query_truncation() {
        let mut server = mockito::Server::new_async().await;
        let context = create_test_session_context();
        let cost_events_data = format!(
            r#"[
            {{"user_id": "{}", "query_session_id": "{}", "event_type": "claude_api", "unit_cost": 0.0005, "unit_type": "token", "units_consumed": 100, "cost_amount": 0.05, "metadata": null, "platform": "test_platform", "created_at": "2024-01-01T00:00:00Z"}},
            {{"user_id": "{}", "query_session_id": "{}", "event_type": "textract_api", "unit_cost": 0.0015, "unit_type": "page", "units_consumed": 1, "cost_amount": 0.0015, "metadata": null, "platform": "test_platform", "created_at": "2024-01-01T00:00:00Z"}}
        ]"#,
            context.user_id, context.session_id, context.user_id, context.session_id
        );
        let _mock = server
            .mock("GET", "/cost_events")
            .match_query(mockito::Matcher::AllOf(vec![
                mockito::Matcher::UrlEncoded("select".into(), "*".into()),
                mockito::Matcher::UrlEncoded(
                    "query_session_id".into(),
                    format!("eq.{}", context.session_id),
                ),
            ]))
            .with_status(200)
            .with_body(&cost_events_data)
            .create_async()
            .await;

        let db = create_mock_database_service(&server);
        let long_query = "This is a very long query that should be truncated because it exceeds the 100 character limit set in the function";

        let notification = db
            .create_cost_notification(&context, long_query, 0.1, 1500)
            .await;

        // Check for truncation - should be truncated to 97 chars + "..."
        assert!(notification.contains("This is a very long query that should be truncated because it exceeds the 100 character limit set..."));
        assert!(notification.contains("Rs.9.000")); // 0.1 * 90.0 forex rate
        assert!(notification.contains("1500 ms"));
    }

    #[tokio::test]
    #[serial]
    async fn test_create_cost_notification_database_error() {
        let mut server = mockito::Server::new_async().await;
        let _mock = server
            .mock("GET", "/cost_events")
            .with_status(500)
            .create_async()
            .await;

        let db = create_mock_database_service(&server);
        let context = create_test_session_context();

        let notification = db
            .create_cost_notification(&context, "test query", 0.05, 800)
            .await;

        assert!(notification.contains("Unable to retrieve details"));
        assert!(notification.contains("test query"));
        assert!(notification.contains("Rs.4.500")); // 0.05 * 90.0
    }

    #[tokio::test]
    #[serial]
    async fn test_create_cost_notification_cost_breakdown() {
        let mut server = mockito::Server::new_async().await;
        let context = create_test_session_context();
        let cost_events_data = format!(
            r#"[
            {{"user_id": "{}", "query_session_id": "{}", "event_type": "claude_api", "unit_cost": 0.0005, "unit_type": "token", "units_consumed": 100, "cost_amount": 0.05, "metadata": null, "platform": "test_platform", "created_at": "2024-01-01T00:00:00Z"}},
            {{"user_id": "{}", "query_session_id": "{}", "event_type": "groq_api", "unit_cost": 0.0002, "unit_type": "token", "units_consumed": 100, "cost_amount": 0.02, "metadata": null, "platform": "test_platform", "created_at": "2024-01-01T00:00:00Z"}},
            {{"user_id": "{}", "query_session_id": "{}", "event_type": "textract_api", "unit_cost": 0.0015, "unit_type": "page", "units_consumed": 1, "cost_amount": 0.0015, "metadata": null, "platform": "test_platform", "created_at": "2024-01-01T00:00:00Z"}},
            {{"user_id": "{}", "query_session_id": "{}", "event_type": "whatsapp_outgoing", "unit_cost": 0.005, "unit_type": "message", "units_consumed": 1, "cost_amount": 0.005, "metadata": null, "platform": "test_platform", "created_at": "2024-01-01T00:00:00Z"}},
            {{"user_id": "{}", "query_session_id": "{}", "event_type": "telegram_message", "unit_cost": 0.001, "unit_type": "message", "units_consumed": 1, "cost_amount": 0.001, "metadata": null, "platform": "test_platform", "created_at": "2024-01-01T00:00:00Z"}}
        ]"#,
            context.user_id,
            context.session_id,
            context.user_id,
            context.session_id,
            context.user_id,
            context.session_id,
            context.user_id,
            context.session_id,
            context.user_id,
            context.session_id
        );
        let _mock = server
            .mock("GET", "/cost_events")
            .match_query(mockito::Matcher::AllOf(vec![
                mockito::Matcher::UrlEncoded("select".into(), "*".into()),
                mockito::Matcher::UrlEncoded(
                    "query_session_id".into(),
                    format!("eq.{}", context.session_id),
                ),
            ]))
            .with_status(200)
            .with_body(&cost_events_data)
            .create_async()
            .await;

        let db = create_mock_database_service(&server);

        let notification = db
            .create_cost_notification(&context, "test", 0.0775, 1000)
            .await;

        assert!(notification.contains("• Claude API: Rs.4.500"));
        assert!(notification.contains("• Groq API: Rs.1.800"));
        assert!(notification.contains("• Textract: Rs.0.135"));
        // Platform cost is sum of whatsapp + telegram = 0.005 + 0.001 = 0.006
        assert!(notification.contains("• TEST_PLATFORM: Rs.0.540")); // 0.006 * 90.0 = 0.54
    }

    #[tokio::test]
    async fn test_cost_event_builder_fluent_interface() {
        let context = create_test_session_context();
        let mut server = mockito::Server::new_async().await;
        let _mock = server
            .mock("POST", "/cost_events")
            .with_status(201)
            .create_async()
            .await;
        let db = create_mock_database_service(&server);

        let metadata = serde_json::json!({"test": "value"});
        let builder = CostEventBuilder::new(context, "test_event")
            .with_cost(0.01, "token", 100)
            .with_metadata(metadata);

        let result = builder.log(&db).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_cost_event_builder_log_vs_log_total_cost() {
        let context1 = create_test_session_context();
        let context2 = context1.clone();
        let mut server = mockito::Server::new_async().await;

        // Mock for log() call - should have cost_amount = unit_cost * units_consumed (0.01 * 100 = 1.0)
        let mock1 = server
            .mock("POST", "/cost_events")
            .match_body(mockito::Matcher::PartialJson(serde_json::json!({
                "event_type": "test1",
                "unit_cost": 0.01,
                "units_consumed": 100,
                "cost_amount": 1.0  // This should be unit_cost * units_consumed
            })))
            .with_status(201)
            .expect(1)
            .create_async()
            .await;

        // Mock for log_total_cost() call - should have cost_amount = unit_cost (0.50)
        let mock2 = server
            .mock("POST", "/cost_events")
            .match_body(mockito::Matcher::PartialJson(serde_json::json!({
                "event_type": "test2",
                "unit_cost": 0.50,
                "units_consumed": 100,
                "cost_amount": 0.50  // This should be unit_cost directly
            })))
            .with_status(201)
            .expect(1)
            .create_async()
            .await;

        let db = create_mock_database_service(&server);

        // Test log() - should multiply unit_cost * units_consumed
        let builder1 = CostEventBuilder::new(context1, "test1").with_cost(0.01, "token", 100);
        let result1 = builder1.log(&db).await;
        assert!(result1.is_ok());

        // Test log_total_cost() - should use unit_cost directly as cost_amount
        let builder2 = CostEventBuilder::new(context2, "test2").with_cost(0.50, "total", 100); // This 0.50 should be used directly
        let result2 = builder2.log_total_cost(&db).await;
        assert!(result2.is_ok());

        // Verify that both mocks were called correctly
        mock1.assert();
        mock2.assert();
    }
}
