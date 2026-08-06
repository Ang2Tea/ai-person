mod send_message;
mod send_reaction;

pub use send_message::TelegramSendMessage;
pub use send_reaction::TelegramSendReaction;

use std::sync::Arc;

use bot_core::buffer::BufferStore;
use contracts::{Storage, Tool};

/// Инструменты, специфичные для Telegram Bot API — регистрируются в общий
/// `ToolRegistry` бота вместе с channel-агностичными из `bot_core::tools::tools`.
pub fn tools<B>(bot: teloxide::Bot, bot_user_id: i64, buffer: BufferStore<B>) -> Vec<Arc<dyn Tool>>
where
    B: Storage + Clone + Send + Sync + 'static,
{
    vec![
        Arc::new(TelegramSendMessage::new(bot.clone(), bot_user_id, buffer)),
        Arc::new(TelegramSendReaction::new(bot)),
    ]
}
