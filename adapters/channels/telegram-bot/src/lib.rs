pub mod dispatch;
mod errors;
pub mod history;
pub mod jobs;
mod list_known_chats;
mod read_chat_history;
mod send_message;
mod send_reaction;
pub mod settings;
#[cfg(feature = "strict-messaging")]
mod text;

pub use errors::DispatchError;
pub use jobs::proactive::ProactiveJob;
pub use list_known_chats::ListKnownChats;
pub use read_chat_history::ReadChatHistory;
pub use send_message::TelegramSendMessage;
pub use send_reaction::TelegramSendReaction;

use std::sync::Arc;

use contracts::{ChannelHistory, Storage, Tool};

use crate::history::BufferStore;

/// Инструменты, специфичные для Telegram Bot API — регистрируются в общий
/// `ToolRegistry` бота вместе с channel-агностичными из `bot_core::tools::tools`.
pub fn tools<S>(bot: teloxide::Bot, bot_user_id: i64, history: BufferStore<S>) -> Vec<Arc<dyn Tool>>
where
    S: Storage + Clone + Send + Sync + 'static,
{
    vec![
        Arc::new(TelegramSendMessage::new(bot.clone(), bot_user_id, history.clone())),
        Arc::new(TelegramSendReaction::new(bot)),
        Arc::new(ListKnownChats::new(history.clone())),
        Arc::new(ReadChatHistory::new(history)),
    ]
}

/// `ChannelHistory`, отдаваемая `bot-core` — конкретный тип `BufferStore<S>`
/// скрыт за трейтом, `bot-core` знает только про `ChannelId`/транскрипт.
pub fn channel_history<S>(history: BufferStore<S>) -> Arc<dyn ChannelHistory>
where
    S: Storage + Clone + Send + Sync + 'static,
{
    Arc::new(history)
}
