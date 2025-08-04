use async_trait::async_trait;
use thiserror::Error;
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
    async fn new() -> Self;
    async fn run(self) -> Result<(), Error>;
}

pub struct ServiceManager {
    services: JoinSet<()>,
}

impl ServiceManager {
    pub fn new() -> Self {
        Self {
            services: JoinSet::new(),
        }
    }

    pub fn spawn<T: Service>(&mut self) {
        self.services.spawn(async move {
            loop {
                let service = T::new().await;
                if let Err(err) = service.run().await {
                    continue;
                }
            }
        });
    }
}
