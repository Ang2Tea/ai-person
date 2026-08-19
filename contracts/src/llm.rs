use std::future::Future;

use crate::{ChatCompletion, ChatMessage, LlmError, ToolSpec};

/// Закрытый список назначений модели — единственный способ выбрать, какую
/// модель использовать для вызова. Вызывающий код (в т.ч. сторонние `Tool`)
/// выбирает роль, а не строку с именем модели: какая модель стоит за ролью,
/// решает только конфиг адаптера, собранный на старте приложения. Это не даёт
/// стороннему коду подставить непроверенную/дорогую модель через вызов.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LlmRole {
    /// Основная диалоговая модель — tool-calling цикл бота, извлечение и
    /// консолидация памяти.
    Primary,
    /// Модель эмбеддингов — поиск и дедуп в долгосрочной памяти.
    Embedding,
    /// Модель с поддержкой vision — описание фото и статичных изображений
    /// текстом для дальнейшей подстановки в обычный текстовый транскрипт.
    Vision,
}

pub trait Llm {
    fn chat(
        &self,
        role: LlmRole,
        messages: &[ChatMessage],
        tools: &[ToolSpec],
    ) -> impl Future<Output = Result<ChatCompletion, LlmError>> + Send;
    fn embed(
        &self,
        role: LlmRole,
        input: &str,
    ) -> impl Future<Output = Result<Vec<f32>, LlmError>> + Send;
    /// Разовое, вне общего tool-calling цикла, описание статичного
    /// изображения текстом — `instruction` задаёт вызывающий код (bot-core),
    /// не адаптер, по той же причине, по которой остальные промпты не живут
    /// в `llm-timeweb`.
    fn describe_image(
        &self,
        role: LlmRole,
        image_bytes: &[u8],
        mime_type: &str,
        instruction: &str,
    ) -> impl Future<Output = Result<String, LlmError>> + Send;
}
