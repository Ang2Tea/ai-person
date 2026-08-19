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
}
