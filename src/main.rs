use std::{env, fs};

use ai_chat_person::{
    adapters::{local_file_storage::LocalFileStorage, timeweb_client::TimewebClient},
    bot::ChatBot,
    buffer::BufferStore,
    memory::MemoryStore,
    settings::Settings,
};
use teloxide::Bot;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let _ = dotenvy::from_path_override(".env");

    let env_filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"));
    tracing_subscriber::fmt().with_env_filter(env_filter).init();

    let settings = Settings::load()?;

    tracing::info!(
        diary_dir = %settings.personality.diary_dir_path().display(),
        search_similarity_threshold = settings.memory.search_similarity_threshold,
        dedup_similarity_threshold = settings.memory.dedup_similarity_threshold,
        token_threshold = settings.memory.token_threshold,
        "effective config loaded",
    );

    let bot_token = env::var("BOT_TOKEN")?;
    let bot: Bot = Bot::new(bot_token);

    let timeweb_token = env::var("TIMEWEB_KEY")?;
    let timeweb_client = TimewebClient::try_new(&timeweb_token)?;

    let buffer_storage = LocalFileStorage::new(settings.personality.working_memory_path());
    let buffer = BufferStore::new(buffer_storage)?;

    let system_prompt = fs::read_to_string(settings.personality.system_prompt_path())?;
    let memory = MemoryStore::new(settings.personality.diary_dir_path());
    let chat_bot = ChatBot::new(
        bot.clone(),
        timeweb_client,
        buffer.clone(),
        memory,
        settings.memory,
        system_prompt,
    );

    tracing::info!("Starting bot");

    teloxide::repl(bot.clone(), move |msg: teloxide::types::Message| {
        let chat_bot = chat_bot.clone();
        async move {
            if let Err(err) = chat_bot.handle_message(msg).await {
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
