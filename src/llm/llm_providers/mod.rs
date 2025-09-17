pub mod claude;
pub mod groq;
use crate::database::SessionContext;
use crate::llm::{LLMError, LLMOrchestrator, Query};
use async_trait::async_trait;
pub use claude::Claude;
pub use groq::Groq;

use crate::llm::LLMProvider;

pub enum LLM {
    Claude(Claude),
    Groq(Groq),
}

#[async_trait]
impl LLMProvider for LLM {
    async fn try_parse(
        &self,
        query: &str,
        context: &SessionContext,
        llm_orchestrator: &LLMOrchestrator,
    ) -> Result<Query, LLMError> {
        match self {
            LLM::Claude(claude) => claude.try_parse(query, context, llm_orchestrator).await,
            LLM::Groq(groq) => groq.try_parse(query, context, llm_orchestrator).await,
        }
    }
}
