use serde::Serialize;

use crate::wire::{WireMessage, WireToolSpec};

#[derive(Debug, Serialize)]
pub struct ChatRequest<'a> {
    pub model: &'a str,
    pub messages: Vec<WireMessage>,
    pub tools: Vec<WireToolSpec>,
    pub temperature: f32,
}
