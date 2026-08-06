use teloxide::types::ChatId;

use crate::buffer::BufferStore;

pub struct ToolContext<B> {
    pub chat_id: ChatId,
    pub user_id: i64,
    pub buffer: BufferStore<B>,
}
