mod embedding_models;
mod models;
mod timeweb_models;
mod wire;

use contracts::{ChatCompletion, ChatMessage, Llm, LlmError, ToolCall, ToolSpec, Usage};

use crate::{
    embedding_models::{EmbeddingRequest, EmbeddingResponse},
    models::ChatRequest,
    timeweb_models::ChatResponse,
    wire::{WireMessage, WireToolSpec},
};

#[derive(Clone)]
pub struct TimewebClient {
    http: reqwest::Client,
    api_key: String,
    endpoint: String,
}

const TEMPERATURE: f32 = 0.7;

impl TimewebClient {
    pub fn try_new(api_key: impl Into<String>) -> Result<Self, LlmError> {
        let http = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(60))
            .build()
            .map_err(|e| LlmError::Request(e.to_string()))?;

        Ok(Self {
            http,
            api_key: api_key.into(),
            endpoint: "https://api.timeweb.ai/v1".to_string(),
        })
    }
}

impl Llm for TimewebClient {
    #[tracing::instrument(skip(self, messages, tools), fields(model = %model, messages = messages.len(), tools = tools.len()))]
    async fn chat(
        &self,
        model: &str,
        messages: &[ChatMessage],
        tools: &[ToolSpec],
    ) -> Result<ChatCompletion, LlmError> {
        let req = ChatRequest {
            model,
            messages: messages.iter().map(WireMessage::from).collect(),
            tools: tools.iter().map(WireToolSpec::from).collect(),
            temperature: TEMPERATURE,
        };

        let resp = self
            .http
            .post(format!("{}/chat/completions", self.endpoint))
            .header("Authorization", format!("Bearer {}", self.api_key))
            .json(&req)
            .send()
            .await
            .map_err(|e| LlmError::Request(e.to_string()))?
            .error_for_status()
            .map_err(|e| LlmError::Request(e.to_string()))?
            .json::<ChatResponse>()
            .await
            .map_err(|e| LlmError::Request(e.to_string()))?;

        let usage = Usage {
            prompt_tokens: resp.usage.prompt_tokens,
            completion_tokens: resp.usage.completion_tokens,
            total_tokens: resp.usage.total_tokens,
        };
        tracing::debug!(
            prompt_tokens = usage.prompt_tokens,
            completion_tokens = usage.completion_tokens,
            "chat completion received"
        );

        resp.choices
            .into_iter()
            .next()
            .map(|c| ChatCompletion {
                message: ChatMessage {
                    role: c.message.role,
                    content: c.message.content,
                    tool_calls: c
                        .message
                        .tool_calls
                        .map(|calls| calls.into_iter().map(ToolCall::from).collect()),
                    tool_call_id: None,
                },
                usage,
            })
            .ok_or(LlmError::EmptyResponse)
    }

    #[tracing::instrument(skip(self, input), fields(model = %model, input_len = input.len()))]
    async fn embed(&self, model: &str, input: &str) -> Result<Vec<f32>, LlmError> {
        let req = EmbeddingRequest { model, input };

        let resp = self
            .http
            .post(format!("{}/embeddings", self.endpoint))
            .header("Authorization", format!("Bearer {}", self.api_key))
            .json(&req)
            .send()
            .await
            .map_err(|e| LlmError::Request(e.to_string()))?
            .error_for_status()
            .map_err(|e| LlmError::Request(e.to_string()))?
            .json::<EmbeddingResponse>()
            .await
            .map_err(|e| LlmError::Request(e.to_string()))?;

        resp.data
            .into_iter()
            .next()
            .map(|d| d.embedding)
            .ok_or(LlmError::EmptyResponse)
    }
}
