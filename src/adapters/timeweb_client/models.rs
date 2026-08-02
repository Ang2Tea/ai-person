use serde::Serialize;
use serde_json::Value;

use crate::contracts::ChatMessage;

#[derive(Debug, Serialize)]
pub struct ChatRequest<'a> {
    pub model: &'a str,
    pub messages: &'a [ChatMessage],
    pub tools: &'a [Value],
    pub temperature: f32,
}
