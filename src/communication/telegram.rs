use crate::core::service_manager::{Error as ServiceManagerError, ServiceWithErrorSender};
use crate::{configuration::Context, query::QueryFulfilment};
use async_trait::async_trait;
use std::sync::Arc;
use teloxide::prelude::*;
use teloxide::types::InputFile;
use thiserror::Error;
use tokio::sync::mpsc;

#[derive(Debug, Error)]
pub enum TelegramError {
    #[error("Environment variable error")]
    EnvError,
    #[error("Error initializing query fulfilment service: {0}")]
    QueryFulfilmentInitError(String),
}

pub struct TelegramService {
    bot: Bot,
    query_fulfilment: QueryFulfilment,
    error_sender: mpsc::Sender<String>,
}

pub struct Response {
    pub text: String,
    pub file: Option<String>,
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
            error_sender
        }
    }

    async fn run(self) -> Result<(), ServiceManagerError> {
        let query_fulfilment = Arc::new(self.query_fulfilment);
        let error_sender = Arc::new(self.error_sender);
        teloxide::repl(self.bot, move |bot: Bot, msg: Message| {
            let query_fulfilment = Arc::clone(&query_fulfilment);
            let error_sender = Arc::clone(&error_sender);
            async move {
                tokio::spawn(Self::handle_message(bot, msg, query_fulfilment, error_sender));
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
        error_sender: Arc<mpsc::Sender<String>>
    ) -> ResponseResult<()> {
        let chat_id = msg.chat.id;
        if let Some(text) = msg.text() {
            let response = match text {
                "/start" => Response {
                    text:
                        "Hello! I'm your Price Assistant. Send me your price / quotation queries."
                            .to_string(),
                    file: None,
                },
                "/help" => Response {
                    text:
                        format!("Hello! I'm your Price Assistant. Send me your price / quotation queries.")
                            ,
                    file: None,
                },
                text => {
                    match query_fulfilment.fulfil_query(text).await {
                        Ok(response) => response,
                        Err(e) => {
                            let error_msg = format!("❌ Query Failed\n\nQuery: {}\nError: {}", text, e);
                            let _ = error_sender.send(error_msg).await;
                            Response {
                                text: "Faced error during request processing - please contact admin".to_string(),
                                file: None,
                            }
                        }
                    }
                }
            };

            bot.send_message(chat_id, response.text).await?;
            if response.file.is_some() {
                bot.send_document(chat_id, InputFile::file(response.file.unwrap()))
                    .await?;
            }
        } else if msg.photo().is_some() {
            bot.send_message(chat_id, "I am not able to process image requests right now")
                .await?;
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
}
