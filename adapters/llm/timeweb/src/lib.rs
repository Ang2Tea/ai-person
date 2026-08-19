mod embedding_models;
mod models;
mod timeweb_models;
mod vision;
mod wire;

use base64::Engine;
use contracts::{ChatCompletion, ChatMessage, Llm, LlmError, LlmRole, ToolCall, ToolSpec, Usage};

use crate::{
    embedding_models::{EmbeddingRequest, EmbeddingResponse},
    models::ChatRequest,
    timeweb_models::ChatResponse,
    vision::{ImageUrlData, VisionContentPart, VisionMessage, VisionRequest},
    wire::{WireMessage, WireToolSpec},
};

#[derive(Clone)]
pub struct TimewebClient {
    http: reqwest::Client,
    api_key: String,
    endpoint: String,
    primary_model: String,
    embedding_model: String,
    vision_model: String,
}

const REASONING_EFFORT_NONE: &str = "none";

impl TimewebClient {
    pub fn try_new(
        api_key: impl Into<String>,
        primary_model: impl Into<String>,
        embedding_model: impl Into<String>,
        vision_model: impl Into<String>,
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
            vision_model: vision_model.into(),
        })
    }

    /// Единственное место, где `LlmRole` превращается в конкретную строку
    /// модели — привязка задаётся один раз в `try_new` (из конфига), вызовы
    /// `chat`/`embed`/`describe_image` выбирают только роль.
    fn model_for(&self, role: LlmRole) -> &str {
        match role {
            LlmRole::Primary => &self.primary_model,
            LlmRole::Embedding => &self.embedding_model,
            LlmRole::Vision => &self.vision_model,
        }
    }

    /// Общий POST для всех трёх эндпоинтов Timeweb. Тело ответа читается
    /// целиком до проверки статуса (не `error_for_status`, который его
    /// отбрасывает) — на 4xx/5xx текст тела обычно и есть причина ("unknown
    /// model" и т.п.), без него ошибка вида "400 Bad Request" не даёт
    /// ничего для диагностики.
    async fn post_json<T: serde::de::DeserializeOwned>(
        &self,
        path: &str,
        body: &impl serde::Serialize,
    ) -> Result<T, LlmError> {
        let resp = self
            .http
            .post(format!("{}{path}", self.endpoint))
            .header("Authorization", format!("Bearer {}", self.api_key))
            .json(body)
            .send()
            .await
            .map_err(|e| LlmError::Request(e.to_string()))?;

        let status = resp.status();
        let text = resp
            .text()
            .await
            .map_err(|e| LlmError::Request(e.to_string()))?;

        if !status.is_success() {
            return Err(LlmError::Request(format!("HTTP {status}: {text}")));
        }

        serde_json::from_str(&text)
            .map_err(|e| LlmError::Request(format!("failed to parse response ({e}): {text}")))
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
            reasoning_effort: REASONING_EFFORT_NONE,
        };

        let resp: ChatResponse = self.post_json("/chat/completions", &req).await?;

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

        let resp: EmbeddingResponse = self.post_json("/embeddings", &req).await?;

        resp.data
            .into_iter()
            .next()
            .map(|d| d.embedding)
            .ok_or(LlmError::EmptyResponse)
    }

    #[tracing::instrument(level = "debug", skip(self, image_bytes, instruction), fields(?role, mime_type, image_bytes = image_bytes.len()))]
    async fn describe_image(
        &self,
        role: LlmRole,
        image_bytes: &[u8],
        mime_type: &str,
        instruction: &str,
    ) -> Result<String, LlmError> {
        let data_url = format!(
            "data:{mime_type};base64,{}",
            base64::engine::general_purpose::STANDARD.encode(image_bytes)
        );

        let req = VisionRequest {
            model: self.model_for(role),
            messages: vec![VisionMessage {
                role: "user",
                content: vec![
                    VisionContentPart::Text {
                        text: instruction.to_owned(),
                    },
                    VisionContentPart::ImageUrl {
                        image_url: ImageUrlData { url: data_url },
                    },
                ],
            }],
        };

        let resp: ChatResponse = self.post_json("/chat/completions", &req).await?;

        resp.choices
            .into_iter()
            .next()
            .and_then(|c| c.message.content)
            .ok_or(LlmError::EmptyResponse)
    }
}
