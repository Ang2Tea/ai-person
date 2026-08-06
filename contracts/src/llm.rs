use std::future::Future;

use crate::{ChatCompletion, ChatMessage, LlmError, ToolSpec};

pub trait Llm {
    fn chat(
        &self,
        model: &str,
        messages: &[ChatMessage],
        tools: &[ToolSpec],
    ) -> impl Future<Output = Result<ChatCompletion, LlmError>> + Send;
    fn embed(
        &self,
        model: &str,
        input: &str,
    ) -> impl Future<Output = Result<Vec<f32>, LlmError>> + Send;
}
