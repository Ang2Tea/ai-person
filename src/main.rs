use std::env;

use ai_chat_person::{
    adapters::{local_file_storage::LocalFileStorage, timeweb_client::TimewebClient},
    bot::ChatBot,
    buffer::BufferStore,
};
use teloxide::Bot;

const SYSTEM_PROMPT: &str = include_str!("../SYSTEM_PROMPT.md");

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let _ = dotenvy::from_path(".env");

    let bot_token = env::var("BOT_TOKEN")?;
    let bot: Bot = Bot::new(bot_token);

    let timeweb_token = env::var("TIMEWEB_KEY")?;
    let timeweb_client = TimewebClient::try_new(&timeweb_token)?;

    let buffer_storage = LocalFileStorage::new("data.json");
    let buffer = BufferStore::new(buffer_storage)?;

    let chat_bot = ChatBot::new(timeweb_client, buffer.clone(), SYSTEM_PROMPT);

    teloxide::repl(bot.clone(), move |bot, msg| {
        let chat_bot = chat_bot.clone();
        async move {
            if let Err(err) = chat_bot.handle_message(bot, msg).await {
                tracing::error!(%err, "Error handling message");
            }
            Ok(())
        }
    })
    .await;

    if let Err(err) = buffer.flush().await {
        tracing::error!(%err, "Can`t flush chat buffer on shutdown");
    }

    Ok(())
}
