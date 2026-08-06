use serde::Serialize;
use serde_json::Value;

use crate::wire::WireMessage;

#[derive(Debug, Serialize)]
pub struct ChatRequest<'a> {
    pub model: &'a str,
    pub messages: Vec<WireMessage>,
    pub tools: &'a [Value],
    pub temperature: f32,
}
