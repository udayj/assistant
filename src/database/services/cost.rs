use super::super::types::{ClaudeRates, CostEvent, CostEventBuilder, GroqRates, SessionContext};
use super::DatabaseError;
use super::DatabaseService;
use tracing::error;
use uuid::Uuid;

impl DatabaseService {
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
