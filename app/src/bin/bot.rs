use std::{env, sync::Arc};

use app::{init_history, init_llm, init_memory, init_tracing, load_settings};
use bot_core::{bot::ChatBot, consolidation::ConsolidationJob, idle_extraction::IdleExtractionJob};
use channel_telegram_bot::{dispatch, jobs::proactive::ProactiveJob};
use contracts::BackgroundJob;
use futures::StreamExt;
use teloxide::{
    Bot,
    requests::Requester,
    types::AllowedUpdate,
    update_listeners::{AsUpdateStream, Polling},
};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let _ = dotenvy::from_path_override(".env");

    init_tracing();

    let settings = load_settings()?;

    tracing::info!(
        diary_dir = %settings.personality.files.diary_dir,
        auto_retrieval_similarity_threshold = settings.memory.auto_retrieval_similarity_threshold,
        dedup_similarity_threshold = settings.memory.dedup_similarity_threshold,
        token_threshold = settings.memory.token_threshold,
        "effective config loaded",
    );

    let bot_token = env::var("BOT_TOKEN")?;
    let bot: Bot = Bot::new(bot_token);
    let bot_user_id = bot.get_me().await?.id.0 as i64;

    let timeweb_client = init_llm()?;
    let history = init_history(&settings).await?;
    let memory = init_memory(&settings, timeweb_client.clone()).await?;
    let history_dyn = channel_telegram_bot::channel_history(history.clone());

    let model: Arc<str> = settings.llm.model.clone().into();

    let mut tools = bot_core::tools::tools(memory.clone());
    tools.extend(channel_telegram_bot::tools(
        bot.clone(),
        bot_user_id,
        history.clone(),
    ));

    let chat_bot = ChatBot::new(
        history_dyn.clone(),
        memory.clone(),
        timeweb_client,
        model,
        settings.memory.token_threshold,
        tools,
    );

    let jobs: Vec<Box<dyn BackgroundJob>> = vec![
        Box::new(ConsolidationJob::new(memory.clone())),
        Box::new(IdleExtractionJob::new(
            history_dyn,
            memory,
            settings.memory.idle_extraction_after_minutes,
        )),
        Box::new(ProactiveJob::new(
            bot.clone(),
            bot_user_id,
            chat_bot.clone(),
            history.clone(),
            settings.proactive,
        )),
    ];
    for job in jobs {
        job.spawn();
    }

    tracing::info!("Starting bot");

    let mut listener = Polling::builder(bot.clone())
        .allowed_updates(vec![
            AllowedUpdate::Message,
            AllowedUpdate::EditedMessage,
            AllowedUpdate::MessageReaction,
        ])
        .build();
    let stream = listener.as_stream();
    tokio::pin!(stream);

    let mut ctrl_c = std::pin::pin!(tokio::signal::ctrl_c());
    loop {
        tokio::select! {
            update = stream.next() => {
                let Some(update) = update else { break };
                let update = match update {
                    Ok(u) => u,
                    Err(err) => {
                        tracing::error!(%err, "polling error");
                        continue;
                    }
                };

                let bot = bot.clone();
                let chat_bot = chat_bot.clone();
                let history = history.clone();
                tokio::spawn(async move {
                    if let Err(err) =
                        dispatch::handle_update(&bot, bot_user_id, &chat_bot, &history, update.kind).await
                    {
                        tracing::error!(%err, "Error handling update");
                    }
                });
            }
            _ = &mut ctrl_c => {
                tracing::info!("Ctrl+C received, shutting down");
                break;
            }
        }
    }

    if let Err(err) = history.flush().await {
        tracing::error!(%err, "Can`t flush chat buffer on shutdown");
    }

    Ok(())
}
