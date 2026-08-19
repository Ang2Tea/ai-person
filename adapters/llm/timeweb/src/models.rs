use serde::Serialize;

use crate::wire::{WireMessage, WireToolSpec};

#[derive(Debug, Serialize)]
pub struct ChatRequest<'a> {
    pub model: &'a str,
    pub messages: Vec<WireMessage>,
    pub tools: Vec<WireToolSpec>,
    /// Часть моделей (например `gpt-5.6-luna`) считает function tools и
    /// reasoning несовместимыми и отвечает 400 на любой запрос с непустым
    /// `tools`, пока `reasoning_effort` явно не выставлен в `"none"`.
    pub reasoning_effort: &'a str,
}
