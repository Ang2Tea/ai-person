use channel_telegram_bot::settings::ProactiveSettings;
use memory::{MemorySettings, PersonalitySettings};
use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub struct LlmSettings {
    pub model: String,
    pub embedding_model: String,
}

/// Только форма данных — как их загружать (файл, env, что угодно ещё)
/// bot-core не знает и не должен: это забота вызывающего бинарника
/// (см. `app::load_settings`).
#[derive(Debug, Clone, Deserialize)]
pub struct Settings {
    pub personality: PersonalitySettings,
    pub memory: MemorySettings,
    pub llm: LlmSettings,
    pub proactive: ProactiveSettings,
}
