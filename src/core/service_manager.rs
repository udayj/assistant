use async_trait::async_trait;
use std::sync::Arc;
use thiserror::Error;
use tokio::sync::{mpsc, Mutex};
use tokio::task::JoinSet;

#[derive(Error, Debug)]
#[error("{0}")]
pub struct Error(String);

impl Error {
    pub fn new(s: &str) -> Error {
        Error(s.to_string())
    }

    pub fn from<E: std::error::Error>(e: E) -> Self {
        Self(e.to_string())
    }
}

#[async_trait]
pub trait Service {
    type Context: Clone + Send;
    async fn new(context: Self::Context) -> Self;
    async fn run(self) -> Result<(), Error>;
}

#[async_trait]
pub trait ServiceWithSender {
    type Context: Clone + Send;
    async fn new(context: Self::Context, price_channel: Option<mpsc::Sender<String>>) -> Self;
    async fn run(self) -> Result<(), Error>;
}

#[async_trait]
pub trait ServiceWithReceiver {
    type Context: Clone + Send;
    async fn new(
        context: Self::Context,
        receiver: Option<Arc<Mutex<mpsc::Receiver<String>>>>,
    ) -> Self;
    async fn run(self) -> Result<(), Error>;
}

pub struct ServiceManager<C> {
    context: C,
    services: JoinSet<()>,
}

impl<C> ServiceManager<C>
where
    C: 'static + Clone + Send,
{
    pub fn new(context: C) -> Self {
        Self {
            context,
            services: JoinSet::new(),
        }
    }

    pub fn spawn<T: Service<Context = C>>(&mut self) {
        let context = self.context.clone();
        self.services.spawn(async move {
            loop {
                let service = T::new(context.clone()).await;
                if let Err(_) = service.run().await {
                    continue;
                }
            }
        });
    }

    pub fn spawn_with_price_sender<T: ServiceWithSender<Context = C>>(
        &mut self,
        sender: mpsc::Sender<String>,
    ) {
        let context = self.context.clone();
        self.services.spawn(async move {
            loop {
                let service = T::new(context.clone(), Some(sender.clone())).await;
                if let Err(_) = service.run().await {
                    continue;
                }
            }
        });
    }

    pub fn spawn_with_price_receiver<T: ServiceWithReceiver<Context = C>>(
        &mut self,
        receiver: Arc<Mutex<mpsc::Receiver<String>>>,
    ) {
        let context = self.context.clone();
        self.services.spawn(async move {
            loop {
                let service = T::new(context.clone(), Some(receiver.clone())).await;
                if let Err(e) = service.run().await {
                    println!("Error:{}", e);
                    break;
                }
            }
        });
    }

    pub async fn wait(&mut self) -> Result<(), Error> {
        if self.services.join_next().await.is_some() {
            return Err(Error::new("Internal Service Error"));
        }
        Ok(())
    }
}
