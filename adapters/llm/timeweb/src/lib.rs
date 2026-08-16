mod embedding_models;
mod models;
mod timeweb_models;
mod wire;

use contracts::{ChatCompletion, ChatMessage, Llm, LlmError, LlmRole, ToolCall, ToolSpec, Usage};

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
    primary_model: String,
    embedding_model: String,
}

const TEMPERATURE: f32 = 0.7;

impl TimewebClient {
    pub fn try_new(
        api_key: impl Into<String>,
        primary_model: impl Into<String>,
        embedding_model: impl Into<String>,
    ) -> Result<Self, LlmError> {
        let http = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(60))
            .build()
            .map_err(|e| LlmError::Request(e.to_string()))?;

        Ok(Self {
            http,
            api_key: api_key.into(),
            endpoint: "https://api.timeweb.ai/v1".to_string(),
            primary_model: primary_model.into(),
            embedding_model: embedding_model.into(),
        })
    }

    /// Единственное место, где `LlmRole` превращается в конкретную строку
    /// модели — привязка задаётся один раз в `try_new` (из конфига), вызовы
    /// `chat`/`embed` выбирают только роль.
    fn model_for(&self, role: LlmRole) -> &str {
        match role {
            LlmRole::Primary => &self.primary_model,
            LlmRole::Embedding => &self.embedding_model,
        }
    }
}

impl Llm for TimewebClient {
    #[tracing::instrument(level = "debug", skip(self, messages, tools), fields(?role, messages = messages.len(), tools = tools.len()))]
    async fn chat(
        &self,
        role: LlmRole,
        messages: &[ChatMessage],
        tools: &[ToolSpec],
    ) -> Result<ChatCompletion, LlmError> {
        let req = ChatRequest {
            model: self.model_for(role),
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

    #[tracing::instrument(level = "debug", skip(self, input), fields(?role, input_len = input.len()))]
    async fn embed(&self, role: LlmRole, input: &str) -> Result<Vec<f32>, LlmError> {
        let req = EmbeddingRequest {
            model: self.model_for(role),
            input,
        };

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
