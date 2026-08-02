use std::{env, error::Error};

use ai_chat_person::{
    adapters::{local_file_storage::LocalFileStorage, timeweb_client::TimewebClient},
    buffer::{BufferStore, BufferedMessage, ChatBuffer},
    contracts::{BufferStorage, ChatMessage},
};
use chrono::Utc;
use teloxide::{
    Bot,
    dispatching::dialogue::GetChatId,
    requests::{Requester, ResponseResult},
    types::{ChatId, Message},
};

const SYSTEM_PROMPT: &str = include_str!("../SYSTEM_PROMPT.md");
const MODEL: &str = "deepseek/deepseek-v4-flash";

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let _ = dotenvy::from_path(".env");

    let bot_token = env::var("BOT_TOKEN")?;
    let bot: Bot = Bot::new(bot_token);

    let timeweb_token = env::var("TIMEWEB_KEY")?;
    let timeweb_client = TimewebClient::try_new(&timeweb_token)?;

    let buffer_storage = LocalFileStorage::new("data.json");
    let buffer = BufferStore::new(buffer_storage)?;

    let repl_buffer = buffer.clone();
    teloxide::repl(bot.clone(), move |bot, msg| {
        answer(timeweb_client.clone(), repl_buffer.clone(), bot, msg)
    })
    .await;

    if let Err(err) = buffer.flush().await {
        tracing::error!(%err, "Can`t flush chat buffer on shutdown");
    }

    Ok(())
}

async fn answer<B>(
    ai_client: TimewebClient,
    buffer: BufferStore<B>,
    bot: Bot,
    msg: Message,
) -> ResponseResult<()>
where
    B: BufferStorage + Clone + Send + Sync + 'static,
{
    let Some(chat_id) = msg.chat_id() else {
        tracing::error!("Can`t get chat id");
        return Ok(());
    };

    let Some(text) = msg.text() else {
        tracing::debug!("Skipping non-text message");
        return Ok(());
    };

    let Some(from) = &msg.from else {
        tracing::error!("Can`t get message sender");
        return Ok(());
    };

    let incoming = BufferedMessage {
        telegram_message_id: msg.id.0,
        sender_id: ChatId::from(from.id).0,
        sender_name: from.first_name.clone(),
        text: text.to_owned(),
        timestamp: Utc::now(),
        is_bot: false,
    };

    buffer.push(chat_id.0, incoming).await;

    let Some(chat_buffer) = buffer.get(chat_id.0).await else {
        tracing::error!("Can`t get chat buffer");
        return Ok(());
    };

    let messages = build_request_messages(SYSTEM_PROMPT, &chat_buffer);

    let reply = match ai_client.chat(MODEL, &messages).await {
        Ok(reply) => reply,
        Err(err) => {
            tracing::error!(%err, "Error from ai client");
            return Ok(());
        }
    };

    bot.send_message(msg.chat.id, &reply).await?;

    let outgoing = BufferedMessage {
        telegram_message_id: msg.id.0,
        sender_id: from.id.0 as i64,
        sender_name: "bot".to_owned(),
        text: reply,
        timestamp: Utc::now(),
        is_bot: true,
    };

    buffer.push(chat_id.0, outgoing).await;

    Ok(())
}

pub fn build_request_messages(system_prompt: &str, buffer: &ChatBuffer) -> Vec<ChatMessage> {
    vec![
        ChatMessage {
            role: "system".into(),
            content: system_prompt.into(),
        },
        ChatMessage {
            role: "user".into(),
            content: buffer.to_transcript(),
        },
    ]
}
