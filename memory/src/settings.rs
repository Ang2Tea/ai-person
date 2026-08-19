use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub struct PersonalityFiles {
    pub system_prompt: String,
    /// Ключ, под которым канал хранит буфер переписки (`ChannelHistory`) —
    /// сама память в этом поле не нуждается, оно только транзитом
    /// прочитывается вызывающим (`app`) и передаётся конструктору канала.
    pub working_memory: String,
    pub diary_dir: String,
    pub insights: String,
    /// Один общий файл на всю личность, не по чату — см. `CommitmentsStore`.
    pub commitments: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PersonalitySettings {
    pub path: String,
    pub files: PersonalityFiles,
}

#[derive(Debug, Clone, Deserialize)]
pub struct MemorySettings {
    pub token_threshold: u32,
    pub dedup_similarity_threshold: f32,
    pub auto_retrieval_similarity_threshold: f32,
    pub auto_retrieval_limit: usize,
    pub min_fact_length: usize,
    pub keep_last_messages: usize,
    pub stale_after_days: i64,
    pub idle_extraction_after_minutes: i64,
}
