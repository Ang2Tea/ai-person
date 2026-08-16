use std::cmp::Ordering;
use std::sync::Arc;

use chrono::Utc;
use contracts::{Llm, Storage};

use crate::commitments::CommitmentsStore;
use crate::consolidation::{self, SharedInsights};
use crate::extraction::maybe_extract;
use crate::record::{MemoryRecord, NewFact, Visibility};
use crate::settings::MemorySettings;
use crate::store::MemoryStore;
use crate::write::save_fact;

/// Единственная реализация `contracts::Memory` — собирает вместе долгосрочный
/// дневник (`MemoryStore`), краткосрочные commitments (`CommitmentsStore`) и
/// "ночные" insights, скрывая их (и конкретный `Storage`-backend `S`) за
/// трейтом от `bot-core`.
#[derive(Clone)]
pub struct PersonalityMemory<L, S> {
    llm: L,
    store: MemoryStore<S>,
    commitments: CommitmentsStore<S>,
    insights: SharedInsights,
    system_prompt: Arc<str>,
    model: Arc<str>,
    embedding_model: Arc<str>,
    settings: MemorySettings,
    personality_storage: S,
    system_prompt_key: Arc<str>,
    insights_key: Arc<str>,
}

impl<L, S> PersonalityMemory<L, S>
where
    L: Llm + Clone + Send + Sync + 'static,
    S: Storage + Clone + Send + Sync + 'static,
{
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        llm: L,
        store: MemoryStore<S>,
        commitments: CommitmentsStore<S>,
        insights: SharedInsights,
        system_prompt: impl Into<Arc<str>>,
        model: impl Into<Arc<str>>,
        embedding_model: impl Into<Arc<str>>,
        settings: MemorySettings,
        personality_storage: S,
        system_prompt_key: impl Into<Arc<str>>,
        insights_key: impl Into<Arc<str>>,
    ) -> Self {
        Self {
            llm,
            store,
            commitments,
            insights,
            system_prompt: system_prompt.into(),
            model: model.into(),
            embedding_model: embedding_model.into(),
            settings,
            personality_storage,
            system_prompt_key: system_prompt_key.into(),
            insights_key: insights_key.into(),
        }
    }

    /// Автоматический поиск по долгосрочной памяти вместо инструмента, который
    /// модель должна была бы сама решить вызвать — иначе она не всегда
    /// догадывается спросить, и бот "не помнит" собеседника в другом чате.
    /// Пороги строже, чем были бы у ручного инструмента: срабатывает на
    /// каждое сообщение, значит должен быть придирчивее, чтобы не забивать
    /// контекст маловероятным. Любая ошибка — тихо `None`, не роняя ход.
    ///
    /// Поиск идёт по всему архиву без учёта `origin_chat_id`/`about_users` —
    /// как в kuni, где diary — единое семантическое пространство поверх всех
    /// чатов, а не изолированное по собеседнику. `chat_id`/`user_id` больше не
    /// фильтруют результат, только маркируют вызов в логах.
    #[tracing::instrument(skip(self, query))]
    async fn retrieve_relevant_facts(&self, chat_id: i64, user_id: i64, query: &str) -> Option<String> {
        let query_embedding = self
            .llm
            .embed(&self.embedding_model, query)
            .await
            .inspect_err(|err| tracing::debug!(%err, "auto-retrieval: embedding failed"))
            .ok()?;

        let records = self
            .store
            .list_all()
            .await
            .inspect_err(|err| tracing::debug!(%err, "auto-retrieval: listing records failed"))
            .ok()?;

        let mut matches: Vec<(f32, MemoryRecord)> = Vec::new();
        for record in records {
            let score = crate::similarity::cosine_similarity(&record.embedding, &query_embedding);
            if score >= self.settings.auto_retrieval_similarity_threshold {
                matches.push((score, record));
            }
        }

        if matches.is_empty() {
            return None;
        }

        matches.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(Ordering::Equal));
        matches.truncate(self.settings.auto_retrieval_limit);

        let mut result = String::new();
        for (_, mut record) in matches {
            result.push_str(&format!(
                "- [уверенность: {}] {}\n",
                record.confidence,
                record.text.trim()
            ));

            record.usage_count += 1;
            record.last_used = Some(Utc::now());
            if let Err(err) = self.store.touch(&record).await {
                tracing::warn!(%err, "auto-retrieval: failed to update memory record usage stats");
            }
        }

        Some(result.trim().to_owned())
    }
}

impl<L, S> contracts::Memory for PersonalityMemory<L, S>
where
    L: Llm + Clone + Send + Sync + 'static,
    S: Storage + Clone + Send + Sync + 'static,
{
    async fn system_prompt(&self) -> String {
        let mut prompt = self.system_prompt.to_string();

        let insights = self.insights.read().await.clone();
        if !insights.is_empty() {
            prompt.push_str("\n\nЗаметки о собеседниках (не про тебя самого):\n");
            prompt.push_str(&insights);
        }

        prompt
    }

    fn keep_last_messages(&self) -> usize {
        self.settings.keep_last_messages
    }

    async fn recall(&self, chat_id: i64, user_id: i64, query: &str) -> Option<String> {
        self.retrieve_relevant_facts(chat_id, user_id, query).await
    }

    async fn commitments(&self) -> Option<String> {
        self.commitments.get().await
    }

    #[tracing::instrument(skip(self, transcript), fields(transcript_len = transcript.len()))]
    async fn extract(&self, chat_id: i64, transcript: &str) {
        if let Err(err) = maybe_extract(
            &self.llm,
            &self.store,
            &self.commitments,
            chat_id,
            transcript,
            self.settings.dedup_similarity_threshold,
            self.settings.min_fact_length,
            &self.model,
            &self.embedding_model,
        )
        .await
        {
            tracing::error!(%err, "memory extraction failed");
        }
    }

    #[tracing::instrument(skip(self, text, about_users), fields(text_len = text.len()))]
    async fn remember(
        &self,
        chat_id: i64,
        text: &str,
        confidence: f32,
        visibility_public: bool,
        about_users: &[i64],
    ) -> bool {
        let fact = NewFact {
            text: text.to_owned(),
            confidence,
            visibility: if visibility_public {
                Visibility::Public
            } else {
                Visibility::Private
            },
            about_users: about_users.to_vec(),
            origin_chat_id: chat_id,
        };

        match save_fact(
            &self.llm,
            &self.store,
            fact,
            self.settings.dedup_similarity_threshold,
            &self.embedding_model,
        )
        .await
        {
            Ok(saved) => saved,
            Err(err) => {
                tracing::error!(%err, "remember: failed to save fact");
                false
            }
        }
    }

    #[tracing::instrument(skip(self))]
    async fn consolidate(&self) -> Result<(), String> {
        consolidation::run(
            &self.llm,
            &self.store,
            &self.model,
            &self.embedding_model,
            self.settings.dedup_similarity_threshold,
            self.settings.stale_after_days,
            &self.personality_storage,
            &self.system_prompt_key,
            &self.insights_key,
            &self.insights,
        )
        .await
        .map_err(|err| err.to_string())
    }
}
