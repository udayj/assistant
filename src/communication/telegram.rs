use crate::communication::error_handler::create_error_response;
use crate::communication::session_helpers::{
    complete_session_with_error, complete_session_with_success, create_session_context,
    create_session_or_error,
};
use crate::core::service_manager::{Error as ServiceManagerError, ServiceWithErrorSender};
use crate::database::DatabaseService;
use crate::database::SessionContext;
use crate::query::QueryError;
use crate::{configuration::Context, query::QueryFulfilment};
use async_trait::async_trait;
use std::fs;
use std::sync::Arc;
use teloxide::net::Download;
use teloxide::prelude::*;
use teloxide::types::InputFile;
use teloxide::types::PhotoSize;
use thiserror::Error;
use tokio::sync::mpsc;
use tokio::sync::mpsc::Sender;
use tracing::error;

#[derive(Debug, Error)]
pub enum TelegramError {
    #[error("Environment variable error")]
    EnvError,
    #[error("Error initializing query fulfilment service: {0}")]
    QueryFulfilmentInitError(String),
    #[error("Image processing error: {0}")]
    ImageProcessingError(String),
}

pub struct TelegramService {
    bot: Bot,
    query_fulfilment: QueryFulfilment,
    error_sender: mpsc::Sender<String>,
    database: Arc<DatabaseService>,
}

pub struct Response {
    pub text: String,
    pub file: Option<String>,
    pub query_metadata: Option<serde_json::Value>,
}

#[async_trait]
impl ServiceWithErrorSender for TelegramService {
    type Context = Context;

    async fn new(context: Context, error_sender: mpsc::Sender<String>) -> Self {
        let query_fulfilment = QueryFulfilment::new(context.clone()).await.unwrap();
        let bot = Bot::from_env();

        Self {
            bot,
            query_fulfilment,
            error_sender,
            database: context.database.clone(),
        }
    }

    async fn run(self) -> Result<(), ServiceManagerError> {
        let query_fulfilment = Arc::new(self.query_fulfilment);
        let error_sender = Arc::new(self.error_sender);
        let database = self.database;
        teloxide::repl(self.bot, move |bot: Bot, msg: Message| {
            let query_fulfilment = Arc::clone(&query_fulfilment);
            let error_sender = Arc::clone(&error_sender);
            let database = Arc::clone(&database);
            async move {
                tokio::spawn(Self::handle_message(
                    bot,
                    msg,
                    query_fulfilment,
                    error_sender,
                    database,
                ));
                respond(())
            }
        })
        .await;
        Ok(())
    }
}

impl TelegramService {
    async fn handle_message(
        bot: Bot,
        msg: Message,
        query_fulfilment: Arc<QueryFulfilment>,
        error_sender: Arc<mpsc::Sender<String>>,
        database: Arc<DatabaseService>,
    ) -> ResponseResult<()> {
        let chat_id = msg.chat.id;
        let telegram_id = chat_id.0.to_string();
        let user = match database.get_user_by_telegram(&telegram_id).await {
            Ok(Some(user)) => {
                if !database.is_user_authorized(&user).await {
                    let status_msg = match user.status.as_str() {
                        "pending_approval" => {
                            "Your account is pending approval. Please wait for admin confirmation."
                        }
                        "suspended" => "Your account has been suspended. Please contact admin.",
                        _ => "Access denied. Please contact admin.",
                    };
                    bot.send_message(chat_id, status_msg).await?;
                    return Ok(());
                }
                user
            }
            Ok(None) => {
                // Create pending user for Telegram
                if let Err(e) = database.create_pending_telegram_user(&telegram_id).await {
                    let _ = error_sender
                        .send(format!("Failed to create pending user: {}", e))
                        .await;
                }
                bot.send_message(
                    chat_id,
                    "Your account is pending approval. Admin has been notified.",
                )
                .await?;
                return Ok(());
            }
            Err(e) => {
                let _ = error_sender
                    .send(format!(
                        "Database error for telegram_id {}: {}",
                        telegram_id, e
                    ))
                    .await;
                bot.send_message(chat_id, "System error. Please try again later.")
                    .await?;
                return Ok(());
            }
        };

        if let Some(photo) = msg.photo() {
            let caption = msg.caption().unwrap_or("").trim();

            bot.send_message(chat_id, "Processing request... please wait ⏳")
                .await?;
            let start_time = std::time::Instant::now();
            let mut context = create_session_context(&user, &telegram_id);
            let query_text = format!("Image query + caption:{}", caption);
            if create_session_or_error(&database, &context, &query_text, "image", &error_sender)
                .await
                .is_err()
            {
                bot.send_message(chat_id, "System error").await?;
                return Ok(());
            }
            match Self::process_image_query(
                &bot,
                photo,
                caption,
                &query_fulfilment,
                &mut context,
                &error_sender,
            )
            .await
            {
                Ok(response) => {
                    complete_session_with_success(
                        &database,
                        &context,
                        &response,
                        &query_text,
                        start_time,
                        &error_sender,
                    )
                    .await;
                    bot.send_message(chat_id, response.text).await?;
                    if let Some(file_path) = response.file {
                        bot.send_document(chat_id, InputFile::file(&file_path))
                            .await?;
                        // Clean up generated files
                        if !file_path.contains("assets") {
                            let _ = fs::remove_file(&file_path);
                        }
                    }
                }
                Err(e) => {
                    // Convert TelegramError to QueryError for consistent error handling
                    let query_error = match e {
                        TelegramError::ImageProcessingError(_) => {
                            QueryError::OcrError(e.to_string())
                        }
                        _ => QueryError::LLMError(e.to_string()),
                    };
                    complete_session_with_error(
                        &database,
                        &context,
                        &query_error,
                        &query_text,
                        start_time,
                        &error_sender,
                    )
                    .await;
                    let error_response = create_error_response(&query_error);
                    bot.send_message(chat_id, error_response.text).await?;
                }
            }
            return Ok(());
        }

        if let Some(text) = msg.text() {
            let response = match text {
                "/start" => Response {
                    text:
                        "Hello! I'm your Price Assistant. Send me your price / quotation queries."
                            .to_string(),
                    file: None,
                    query_metadata: None,
                },
                "/help" => Response {
                    text: QueryFulfilment::get_help_text(),
                    file: None,
                    query_metadata: None,
                },
                text if text.starts_with("/approve_telegram ") => {
                    if database.is_admin(&telegram_id).await {
                        let target_id = text.strip_prefix("/approve_telegram ").unwrap().trim();
                        match database.approve_telegram_user(target_id).await {
                            Ok(true) => Response {
                                text: format!("✅ Approved user: {}", target_id),
                                file: None,
                                query_metadata: None,
                            },
                            Ok(false) => Response {
                                text: format!(
                                    "❌ User {} not found or already approved",
                                    target_id
                                ),
                                file: None,
                                query_metadata: None,
                            },
                            Err(e) => Response {
                                text: format!("❌ Error approving user: {}", e),
                                file: None,
                                query_metadata: None,
                            },
                        }
                    } else {
                        Response {
                            text: "❌ Admin access required".to_string(),
                            file: None,
                            query_metadata: None,
                        }
                    }
                }
                text if text.starts_with("/approve_whatsapp ") => {
                    if database.is_admin(&telegram_id).await {
                        let phone = text.strip_prefix("/approve_whatsapp ").unwrap().trim();
                        match database.approve_whatsapp_user(phone).await {
                            Ok(_) => Response {
                                text: format!("✅ Approved WhatsApp user: {}", phone),
                                file: None,
                                query_metadata: None,
                            },
                            Err(e) => Response {
                                text: format!("❌ Error approving WhatsApp user: {}", e),
                                file: None,
                                query_metadata: None,
                            },
                        }
                    } else {
                        Response {
                            text: "❌ Admin access required".to_string(),
                            file: None,
                            query_metadata: None,
                        }
                    }
                }
                "/pending" => {
                    if database.is_admin(&telegram_id).await {
                        match database.get_pending_users().await {
                            Ok(users) => {
                                if users.is_empty() {
                                    Response {
                                        text: "No pending approvals".to_string(),
                                        file: None,
                                        query_metadata: None,
                                    }
                                } else {
                                    let mut msg = "📋 Pending Approvals:\n\n".to_string();
                                    for user in users {
                                        if let Some(tid) = user.telegram_id {
                                            msg.push_str(&format!("Telegram: {}\n", tid));
                                        }
                                    }
                                    Response {
                                        text: msg,
                                        file: None,
                                        query_metadata: None,
                                    }
                                }
                            }
                            Err(e) => Response {
                                text: format!("❌ Error fetching pending users: {}", e),
                                file: None,
                                query_metadata: None,
                            },
                        }
                    } else {
                        Response {
                            text: "❌ Admin access required".to_string(),
                            file: None,
                            query_metadata: None,
                        }
                    }
                }

                text if text.starts_with("/llm ") => {
                    if database.is_admin(&telegram_id).await {
                        let model = text.strip_prefix("/llm ").unwrap().trim();
                        match model {
                            "claude" | "groq" => {
                                query_fulfilment.set_primary_model(model);
                                Response {
                                    text: format!("✅ Primary LLM switched to: {}", model),
                                    file: None,
                                    query_metadata: None,
                                }
                            }
                            _ => Response {
                                text: "❌ Invalid model. Use: /llm claude or /llm groq".to_string(),
                                file: None,
                                query_metadata: None,
                            },
                        }
                    } else {
                        Response {
                            text: "❌ Admin access required".to_string(),
                            file: None,
                            query_metadata: None,
                        }
                    }
                }

                text => {
                    let start_time = std::time::Instant::now();
                    let mut context = create_session_context(&user, &telegram_id);
                    if create_session_or_error(&database, &context, text, "text", &error_sender)
                        .await
                        .is_err()
                    {
                        bot.send_message(chat_id, "System error").await?;
                        return Ok(());
                    }
                    match query_fulfilment
                        .fulfil_query(text, &mut context, &error_sender)
                        .await
                    {
                        Ok(response) => {
                            complete_session_with_success(
                                &database,
                                &context,
                                &response,
                                text,
                                start_time,
                                &error_sender,
                            )
                            .await;
                            response
                        }
                        Err(e) => {
                            complete_session_with_error(
                                &database,
                                &context,
                                &e,
                                text,
                                start_time,
                                &error_sender,
                            )
                            .await;
                            create_error_response(&e)
                        }
                    }
                }
            };

            bot.send_message(chat_id, response.text).await?;
            if let Some(file_path) = response.file {
                bot.send_document(chat_id, InputFile::file(&file_path))
                    .await?;

                // Clean up the PDF file - only quotations - after successful send
                if !&file_path.contains("assets") {
                    if let Err(e) = fs::remove_file(&file_path) {
                        error!("Warning: Failed to delete PDF file {}: {}", file_path, e);
                    }
                }
            }
        } else if let Some(voice) = msg.voice() {
            bot.send_message(chat_id, "Processing audio... please wait ⏳")
                .await?;
            let start_time = std::time::Instant::now();
            let mut context = create_session_context(&user, &telegram_id);
            let query_text = "Audio query";
            if create_session_or_error(&database, &context, query_text, "audio", &error_sender)
                .await
                .is_err()
            {
                bot.send_message(chat_id, "System error").await?;
                return Ok(());
            }
            match Self::process_voice_query(
                &bot,
                voice,
                &query_fulfilment,
                &mut context,
                &error_sender,
            )
            .await
            {
                Ok(response) => {
                    complete_session_with_success(
                        &database,
                        &context,
                        &response,
                        query_text,
                        start_time,
                        &error_sender,
                    )
                    .await;
                    bot.send_message(chat_id, response.text).await?;
                    if let Some(file_path) = response.file {
                        bot.send_document(chat_id, InputFile::file(&file_path))
                            .await?;
                        if !file_path.contains("assets") {
                            let _ = fs::remove_file(&file_path);
                        }
                    }
                }
                Err(e) => {
                    // Convert TelegramError to QueryError for consistent error handling
                    let query_error = match e {
                        TelegramError::ImageProcessingError(_) => {
                            QueryError::TranscriptionError(e.to_string())
                        }
                        _ => QueryError::LLMError(e.to_string()),
                    };
                    complete_session_with_error(
                        &database,
                        &context,
                        &query_error,
                        query_text,
                        start_time,
                        &error_sender,
                    )
                    .await;
                    let error_response = create_error_response(&query_error);
                    bot.send_message(chat_id, error_response.text).await?;
                }
            }
            return Ok(());
        } else if msg.document().is_some() {
            bot.send_message(chat_id, "I received a document! 📄")
                .await?;
        } else {
            bot.send_message(
                chat_id,
                "I received something, but I'm not sure what it was! 🤔",
            )
            .await?;
        }
        Ok(())
    }

    async fn process_image_query(
        bot: &Bot,
        photos: &[PhotoSize],
        caption: &str,
        query_fulfilment: &QueryFulfilment,
        context: &mut SessionContext,
        error_sender: &Sender<String>,
    ) -> Result<Response, TelegramError> {
        // Get the largest photo size
        let photo = photos.iter().max_by_key(|p| p.width * p.height).ok_or(
            TelegramError::ImageProcessingError("No photo found".to_string()),
        )?;

        // Download image
        let file_info = bot.get_file(&photo.file.id).await.map_err(|e| {
            TelegramError::ImageProcessingError(format!("Failed to get image file info: {}", e))
        })?;

        let mut image_data = Vec::new();
        bot.download_file(&file_info.path, &mut image_data)
            .await
            .map_err(|e| {
                TelegramError::ImageProcessingError(format!("Failed to download image: {}", e))
            })?;

        // Process through existing query fulfilment with image support
        query_fulfilment
            .fulfil_image_query(&image_data, caption, context, error_sender)
            .await
            .map_err(|e| TelegramError::ImageProcessingError(e.to_string()))
    }

    async fn process_voice_query(
        bot: &Bot,
        voice: &teloxide::types::Voice,
        query_fulfilment: &QueryFulfilment,
        context: &mut SessionContext,
        error_sender: &Sender<String>,
    ) -> Result<Response, TelegramError> {
        // Use voice.file.id for download
        let file_info = bot.get_file(&voice.file.id).await.map_err(|e| {
            TelegramError::ImageProcessingError(format!("Failed to get voice file info: {}", e))
        })?;

        let mut audio_data = Vec::new();
        bot.download_file(&file_info.path, &mut audio_data)
            .await
            .map_err(|e| {
                TelegramError::ImageProcessingError(format!("Failed to download voice: {}", e))
            })?;

        query_fulfilment
            .fulfil_audio_query(&audio_data, context, error_sender)
            .await
            .map_err(|e| TelegramError::ImageProcessingError(e.to_string()))
    }
}
