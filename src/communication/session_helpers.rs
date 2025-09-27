use crate::communication::telegram::Response;
use crate::database::{DatabaseService, SessionContext, SessionResult, User};
use crate::query::QueryError;
use std::sync::Arc;
use tokio::sync::mpsc;

pub fn create_session_context(user: &User, telegram_id: &str) -> SessionContext {
    SessionContext::new(user.id, "telegram").with_telegram_id(telegram_id.to_string())
}

pub async fn create_session_or_error(
    database: &Arc<DatabaseService>,
    context: &SessionContext,
    query_text: &str,
    query_type: &str,
    error_sender: &mpsc::Sender<String>,
) -> Result<(), ()> {
    if database
        .create_session_with_context(context, query_text, query_type)
        .await
        .is_err()
    {
        let _ = error_sender
            .send("Failed to create session".to_string())
            .await;
        return Err(());
    }
    Ok(())
}

pub async fn complete_session_with_error(
    database: &Arc<DatabaseService>,
    context: &SessionContext,
    error: &QueryError,
    query_text: &str,
    start_time: std::time::Instant,
    error_sender: &mpsc::Sender<String>,
) {
    let error_msg = format!("❌ Query Failed\n\nQuery: {}\nError: {}", query_text, error);
    let _ = error_sender.send(error_msg).await;

    let result = SessionResult {
        success: false,
        error_message: Some(error.to_string()),
        processing_time_ms: start_time.elapsed().as_millis() as i32,
        query_metadata: None,
    };

    let _ = database
        .complete_session_with_notification(context, result, query_text, error_sender)
        .await;
}

pub async fn complete_session_with_success(
    database: &Arc<DatabaseService>,
    context: &SessionContext,
    response: &Response,
    query_text: &str,
    start_time: std::time::Instant,
    error_sender: &mpsc::Sender<String>,
) {
    let result = SessionResult {
        success: true,
        error_message: None,
        processing_time_ms: start_time.elapsed().as_millis() as i32,
        query_metadata: response.query_metadata.clone(),
    };

    let _ = database
        .complete_session_with_notification(context, result, query_text, error_sender)
        .await;
}
