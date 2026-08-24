use chrono::Utc;
use teloxide::{
    Bot,
    dispatching::dialogue::GetChatId,
    net::Download,
    requests::Requester,
    types::{
        ChatId, FileId, MaybeAnonymousUser, Message, MessageReactionUpdated, ReactionType,
        UpdateKind,
    },
};

use bot_core::bot::ChatBot;
use contracts::{ChannelId, Llm, Memory, Storage};

/// Файлы крупнее этого не скачиваются под vision-описание — чтобы не тратить
/// трафик/деньги на заведомо неприемлемый для модели по размеру файл (Bot
/// API и так не отдаёт боту файлы больше 20 МБ, это дополнительный, более
/// строгий предохранитель именно для дорогого vision-вызова).
const MAX_IMAGE_BYTES: u32 = 10 * 1024 * 1024;

use crate::errors::DispatchError;
use crate::history::{BufferStore, BufferedMessage};

const TELEGRAM_CHANNEL: &str = "telegram";

/// Единая точка входа для апдейтов поллинга — разбирает `UpdateKind` и
/// перенаправляет в конкретный обработчик; остальные виды апдейтов (не
/// запрошены `AllowedUpdate` при подписке) молча игнорируются.
pub async fn handle_update<L, M, S>(
    bot: &Bot,
    bot_user_id: i64,
    chat_bot: &ChatBot<L, M>,
    history: &BufferStore<S>,
    kind: UpdateKind,
) -> Result<(), DispatchError>
where
    L: Llm + Clone + Send + Sync + 'static,
    M: Memory + Clone + Send + Sync + 'static,
    S: Storage + Clone + Send + Sync + 'static,
{
    match kind {
        UpdateKind::Message(msg) => handle_message(bot, chat_bot, history, msg).await,
        UpdateKind::EditedMessage(msg) => handle_edited_message(chat_bot, history, msg).await,
        UpdateKind::MessageReaction(reaction) => {
            handle_reaction(bot_user_id, chat_bot, history, reaction).await
        }
        _ => Ok(()),
    }
}

#[tracing::instrument(skip_all, fields(chat_id = tracing::field::Empty, user_id = tracing::field::Empty))]
async fn handle_message<L, M, S>(
    bot: &Bot,
    chat_bot: &ChatBot<L, M>,
    history: &BufferStore<S>,
    msg: Message,
) -> Result<(), DispatchError>
where
    L: Llm + Clone + Send + Sync + 'static,
    M: Memory + Clone + Send + Sync + 'static,
    S: Storage + Clone + Send + Sync + 'static,
{
    let Some(chat_id) = msg.chat_id() else {
        tracing::error!("Can`t get chat id");
        return Ok(());
    };
    tracing::Span::current().record("chat_id", chat_id.0);

    let Some(text) = describe_media(bot, chat_bot, &msg)
        .await
        .or_else(|| describe_message(&msg))
    else {
        tracing::debug!("Skipping message without representable content");
        return Ok(());
    };

    let Some(from) = &msg.from else {
        tracing::error!("Can`t get message sender");
        return Ok(());
    };
    tracing::Span::current().record("user_id", from.id.0);

    let incoming = BufferedMessage {
        telegram_message_id: msg.id.0,
        sender_id: ChatId::from(from.id).0,
        sender_name: from.first_name.clone(),
        text: text.clone(),
        timestamp: Utc::now(),
        is_bot: false,
    };
    history.push(chat_id.0, incoming).await;

    run_and_reply(chat_bot, chat_id, from.id.0 as i64, &text).await
}

/// Правки в Telegram применяются только к тексту/подписи — dice, стикер,
/// опрос и т.п. отредактировать в другой тип контента нельзя, поэтому здесь
/// достаточно текста/подписи, в отличие от `describe_message`.
#[tracing::instrument(skip_all, fields(chat_id = tracing::field::Empty, user_id = tracing::field::Empty))]
async fn handle_edited_message<L, M, S>(
    chat_bot: &ChatBot<L, M>,
    history: &BufferStore<S>,
    msg: Message,
) -> Result<(), DispatchError>
where
    L: Llm + Clone + Send + Sync + 'static,
    M: Memory + Clone + Send + Sync + 'static,
    S: Storage + Clone + Send + Sync + 'static,
{
    let Some(chat_id) = msg.chat_id() else {
        tracing::error!("Can`t get chat id for edited message");
        return Ok(());
    };
    tracing::Span::current().record("chat_id", chat_id.0);

    let Some(new_text) = msg.text().or_else(|| msg.caption()) else {
        tracing::debug!("Skipping edited message without text/caption");
        return Ok(());
    };

    let Some(from) = &msg.from else {
        tracing::error!("Can`t get edited message sender");
        return Ok(());
    };
    tracing::Span::current().record("user_id", from.id.0);

    let text = format!(
        "{} отредактировал(а) сообщение #{}, теперь: {}",
        from.first_name, msg.id.0, new_text
    );

    let incoming = BufferedMessage {
        telegram_message_id: msg.id.0,
        sender_id: ChatId::from(from.id).0,
        sender_name: from.first_name.clone(),
        text: text.clone(),
        timestamp: Utc::now(),
        is_bot: false,
    };
    history.push(chat_id.0, incoming).await;

    run_and_reply(chat_bot, chat_id, from.id.0 as i64, &text).await
}

#[tracing::instrument(skip_all, fields(chat_id = tracing::field::Empty, user_id = tracing::field::Empty))]
async fn handle_reaction<L, M, S>(
    bot_user_id: i64,
    chat_bot: &ChatBot<L, M>,
    history: &BufferStore<S>,
    reaction: MessageReactionUpdated,
) -> Result<(), DispatchError>
where
    L: Llm + Clone + Send + Sync + 'static,
    M: Memory + Clone + Send + Sync + 'static,
    S: Storage + Clone + Send + Sync + 'static,
{
    let MaybeAnonymousUser::User(user) = &reaction.actor else {
        tracing::debug!("Skipping reaction from an anonymous/channel actor");
        return Ok(());
    };
    tracing::Span::current().record("user_id", user.id.0);

    if user.id.0 as i64 == bot_user_id {
        // Не реагируем на собственные же реакции — иначе потенциальный цикл.
        return Ok(());
    }

    if reaction.new_reaction.is_empty() {
        // Реакцию сняли, а не поставили — этот случай не описан заданием.
        return Ok(());
    }

    let chat_id = reaction.chat.id;
    tracing::Span::current().record("chat_id", chat_id.0);
    let emoji = describe_reactions(&reaction.new_reaction);
    let text = format!(
        "{} поставил(а) реакцию {} на сообщение #{}",
        user.first_name, emoji, reaction.message_id.0
    );

    let incoming = BufferedMessage {
        telegram_message_id: reaction.message_id.0,
        sender_id: user.id.0 as i64,
        sender_name: user.first_name.clone(),
        text: text.clone(),
        timestamp: reaction.date,
        is_bot: false,
    };
    history.push(chat_id.0, incoming).await;

    run_and_reply(chat_bot, chat_id, user.id.0 as i64, &text).await
}

/// Отдаёт ход модели через общий tool-calling цикл `ChatBot` — сама
/// отправка (если она вообще случилась) уже произошла как побочный эффект
/// вызова инструмента отправки (`send_message`) внутри этого цикла, здесь
/// её дожидаться и записывать в историю не нужно.
async fn run_and_reply<L, M>(
    chat_bot: &ChatBot<L, M>,
    chat_id: ChatId,
    user_id: i64,
    query: &str,
) -> Result<(), DispatchError>
where
    L: Llm + Clone + Send + Sync + 'static,
    M: Memory + Clone + Send + Sync + 'static,
{
    let chat = ChannelId {
        channel: TELEGRAM_CHANNEL,
        id: chat_id.0.to_string(),
    };
    let user = ChannelId {
        channel: TELEGRAM_CHANNEL,
        id: user_id.to_string(),
    };

    chat_bot.run_turn(chat, user, query).await?;

    Ok(())
}

/// Статичное изображение (фото, файл-картинка, нестатичный стикер сюда не
/// попадает) — через vision-модель в текстовое описание, которое дальше идёт
/// в буфер как обычное сообщение. `None` — во входящем нет статичного
/// изображения, тогда `handle_message` падает на обычный `describe_message`.
/// Скачивание/распознавание не должно ронять весь апдейт: любая ошибка здесь
/// превращается в текстовый фолбэк, а не пробрасывается наружу.
async fn describe_media<L, M>(bot: &Bot, chat_bot: &ChatBot<L, M>, msg: &Message) -> Option<String>
where
    L: Llm + Clone + Send + Sync + 'static,
    M: Memory + Clone + Send + Sync + 'static,
{
    let (file_id, file_size, mime_type, action) = if let Some(sizes) = msg.photo() {
        let photo = sizes.last()?;
        (
            photo.file.id.clone(),
            photo.file.size,
            "image/jpeg".to_owned(),
            "прислал(а) фото",
        )
    } else if let Some(document) = msg.document() {
        let mime_type = document.mime_type.as_ref()?.essence_str().to_owned();
        if !mime_type.starts_with("image/") || mime_type == "image/gif" {
            return None;
        }
        (
            document.file.id.clone(),
            document.file.size,
            mime_type,
            "прислал(а) файл-изображение",
        )
    } else {
        let sticker = msg.sticker()?;
        if sticker.flags.is_animated || sticker.flags.is_video {
            return None;
        }
        (
            sticker.file.id.clone(),
            sticker.file.size,
            "image/webp".to_owned(),
            "прислал(а) стикер",
        )
    };

    let caption = msg.caption();

    if file_size > MAX_IMAGE_BYTES {
        tracing::warn!(file_size, "image too large to download for vision");
        return Some(with_caption(
            action,
            caption,
            "но файл слишком большой, чтобы его посмотреть",
        ));
    }

    match download_and_describe(bot, chat_bot, file_id, &mime_type).await {
        Ok(description) => Some(match caption {
            Some(caption) => format!("{action} с подписью «{caption}»: {description}"),
            None => format!("{action}: {description}"),
        }),
        Err(err) => {
            tracing::warn!(%err, "failed to describe image");
            Some(with_caption(action, caption, "не удалось распознать"))
        }
    }
}

fn with_caption(action: &str, caption: Option<&str>, note: &str) -> String {
    match caption {
        Some(caption) => format!("{action} с подписью «{caption}», {note}"),
        None => format!("{action}, {note}"),
    }
}

/// Скачивает файл по `file_id` и просит vision-модель его описать. Ошибки
/// (Telegram-скачивание или сам вызов LLM) схлопываются в одну строку — это
/// внутренний служебный результат для лога, не то, что уходит пользователю.
async fn download_and_describe<L, M>(
    bot: &Bot,
    chat_bot: &ChatBot<L, M>,
    file_id: FileId,
    mime_type: &str,
) -> Result<String, String>
where
    L: Llm + Clone + Send + Sync + 'static,
    M: Memory + Clone + Send + Sync + 'static,
{
    let file = bot.get_file(file_id).await.map_err(|e| e.to_string())?;
    let mut bytes = Vec::new();
    bot.download_file(&file.path, &mut bytes)
        .await
        .map_err(|e| e.to_string())?;
    chat_bot
        .describe_image(&bytes, mime_type)
        .await
        .map_err(|e| e.to_string())
}

/// Текстовое описание содержимого сообщения — вместо голого `.text()`, чтобы
/// опросы/dice/геолокация/контакты/стикеры/подписи тоже попадали в буфер и
/// проходили через общий tool-calling цикл, а не отбрасывались молча.
/// Статичные изображения (фото/файлы-картинки/статичные стикеры) сюда не
/// доходят — их разбирает `describe_media` через vision-модель; здесь
/// остаются только анимированные/видео-стикеры (эмодзи-плейсхолдер) и всё
/// остальное неразбираемое медиа (голос и т.п. по-прежнему не описываются).
fn describe_message(msg: &Message) -> Option<String> {
    if let Some(text) = msg.text() {
        return Some(text.to_owned());
    }
    if let Some(caption) = msg.caption() {
        return Some(caption.to_owned());
    }
    if let Some(poll) = msg.poll() {
        let options = poll
            .options
            .iter()
            .map(|o| o.text.as_str())
            .collect::<Vec<_>>()
            .join(", ");
        let mut description = format!("запустил опрос: {} — варианты: {}", poll.question, options);
        if let Some(explanation) = &poll.explanation {
            description.push_str(&format!(" (пояснение: {explanation})"));
        }
        return Some(description);
    }
    if let Some(dice) = msg.dice() {
        let emoji = match dice.emoji {
            teloxide::types::DiceEmoji::Dice => "🎲",
            teloxide::types::DiceEmoji::Darts => "🎯",
            teloxide::types::DiceEmoji::Bowling => "🎳",
            teloxide::types::DiceEmoji::Basketball => "🏀",
            teloxide::types::DiceEmoji::Football => "⚽",
            teloxide::types::DiceEmoji::SlotMachine => "🎰",
        };
        return Some(format!("бросил {emoji}, результат: {}", dice.value));
    }
    if let Some(venue) = msg.venue() {
        return Some(format!(
            "поделился гео локацией: {} ({})",
            venue.title, venue.address
        ));
    }
    if msg.location().is_some() {
        return Some("поделился гео-локацией".to_owned());
    }
    if let Some(contact) = msg.contact() {
        let last_name = contact.last_name.clone().unwrap_or_default();
        return Some(
            format!(
                "поделился контактом: {} {}, {}",
                contact.first_name, last_name, contact.phone_number
            )
            .trim()
            .to_owned(),
        );
    }
    if let Some(sticker) = msg.sticker() {
        let emoji = sticker.emoji.clone().unwrap_or_default();
        return Some(format!("отправил стикер {emoji}"));
    }

    None
}

/// Эмодзи, реально поставленные (после изменения) — для текста в буфер.
fn describe_reactions(reactions: &[ReactionType]) -> String {
    reactions
        .iter()
        .map(|r| match r {
            ReactionType::Emoji { emoji } => emoji.clone(),
            ReactionType::CustomEmoji { .. } => "кастомный эмодзи".to_owned(),
            ReactionType::Paid => "⭐".to_owned(),
        })
        .collect::<Vec<_>>()
        .join(", ")
}
