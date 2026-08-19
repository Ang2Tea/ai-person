use std::future::Future;
use std::pin::Pin;

use chrono::{DateTime, Utc};

/// Время последнего события в чате и число накопленных сообщений.
pub type Activity = (DateTime<Utc>, usize);

/// Opaque межканальный идентификатор чата — `id` не парсится как число,
/// потому что у разных каналов формат id разный (Telegram — числовой,
/// у будущих каналов может быть иным); `channel` разделяет id разных
/// каналов, чтобы они не путались между собой при сравнении/хранении.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ChannelId {
    pub channel: &'static str,
    pub id: String,
}

/// История переписки конкретного канала связи — то, что раньше было
/// Telegram-специфичным `BufferStore`. Компенсирует ограничение Bot API
/// (нет доступа к полной истории чата), поэтому это забота канала, не
/// `bot-core`: у будущего канала с прямым доступом к истории (userbot)
/// реализация была бы иной, а то и вовсе не нужна.
pub trait ChannelHistory: Send + Sync {
    /// Транскрипт последних сообщений чата, готовый для передачи модели.
    fn transcript<'a>(
        &'a self,
        chat: &'a ChannelId,
    ) -> Pin<Box<dyn Future<Output = String> + Send + 'a>>;

    /// Обрезать историю чата, оставив только последние `n` сообщений —
    /// вызывается после того, как более старые сообщения уже извлечены в
    /// долгосрочную память.
    fn truncate_keep_last<'a>(
        &'a self,
        chat: &'a ChannelId,
        n: usize,
    ) -> Pin<Box<dyn Future<Output = ()> + Send + 'a>>;

    /// Все чаты, с которыми канал уже когда-либо взаимодействовал — для
    /// воркера извлечения по простою и проактивного воркера.
    fn known_chats<'a>(&'a self) -> Pin<Box<dyn Future<Output = Vec<ChannelId>> + Send + 'a>>;

    /// Время последнего события в чате и число накопленных сообщений.
    fn activity<'a>(
        &'a self,
        chat: &'a ChannelId,
    ) -> Pin<Box<dyn Future<Output = Option<Activity>> + Send + 'a>>;
}
