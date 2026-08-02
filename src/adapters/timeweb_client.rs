mod embedding_models;
mod models;
mod timeweb_models;

use serde_json::Value;

use crate::{
    adapters::timeweb_client::{
        embedding_models::{EmbeddingRequest, EmbeddingResponse},
        models::ChatRequest,
        timeweb_models::ChatResponse,
    },
    contracts::{ChatCompletion, ChatMessage, Usage},
    errors::LlmError,
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
            .build()?;

        Ok(Self {
            http,
            api_key: api_key.into(),
            endpoint: "https://api.timeweb.ai/v1".to_string(),
        })
    }

    pub async fn chat(
        &self,
        model: &str,
        messages: &[ChatMessage],
        tools: &[Value],
    ) -> Result<ChatCompletion, LlmError> {
        let req = ChatRequest {
            model,
            messages,
            tools,
            temperature: TEMPERATURE,
        };

        let resp = self
            .http
            .post(format!("{}/chat/completions", self.endpoint))
            .header("Authorization", format!("Bearer {}", self.api_key))
            .json(&req)
            .send()
            .await?
            .error_for_status()?
            .json::<ChatResponse>()
            .await?;

        let usage = Usage {
            prompt_tokens: resp.usage.prompt_tokens,
            completion_tokens: resp.usage.completion_tokens,
            total_tokens: resp.usage.total_tokens,
        };

        resp.choices
            .into_iter()
            .next()
            .map(|c| ChatCompletion {
                message: ChatMessage {
                    role: c.message.role,
                    content: c.message.content,
                    tool_calls: c.message.tool_calls,
                    tool_call_id: None,
                },
                usage,
            })
            .ok_or(LlmError::EmptyResponse)
    }

    pub async fn embed(&self, model: &str, input: &str) -> Result<Vec<f32>, LlmError> {
        let req = EmbeddingRequest { model, input };

        let resp = self
            .http
            .post(format!("{}/embeddings", self.endpoint))
            .header("Authorization", format!("Bearer {}", self.api_key))
            .json(&req)
            .send()
            .await?
            .error_for_status()?
            .json::<EmbeddingResponse>()
            .await?;

        resp.data
            .into_iter()
            .next()
            .map(|d| d.embedding)
            .ok_or(LlmError::EmptyResponse)
    }
}
