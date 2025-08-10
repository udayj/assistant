use crate::core::service_manager::Error as ServiceManagerError;
use crate::core::Service;
use crate::{configuration::Context, query::QueryFulfilment};
use async_trait::async_trait;
use std::sync::Arc;
use teloxide::prelude::*;
use teloxide::types::InputFile;
use thiserror::Error;

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
}

pub struct Response {
    pub text: String,
    pub file: Option<String>,
}

#[async_trait]
impl Service for TelegramService {
    type Context = Context;

    async fn new(context: Context) -> Self {
        let query_fulfilment = QueryFulfilment::new(context.clone()).await.unwrap();
        let bot = Bot::from_env();
        Self {
            bot,
            query_fulfilment,
        }
    }

    async fn run(self) -> Result<(), ServiceManagerError> {
        let query_fulfilment = Arc::new(self.query_fulfilment);
        teloxide::repl(self.bot, move |bot: Bot, msg: Message| {
            let query_fulfilment = Arc::clone(&query_fulfilment);
            async move {
                tokio::spawn(Self::handle_message(bot, msg, query_fulfilment));
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
                        "Hello! I'm your Price Assistant. Send me your price / quotation queries."
                            .to_string(),
                    file: None,
                },
                text => {
                    let response = query_fulfilment.fulfil_query(text).await.unwrap();
                    response
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
