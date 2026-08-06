use std::future::Future;
use std::time::Duration;

/// Общий паттерн периодических фоновых задач бота (флаш буфера, простойное
/// извлечение фактов, проактивные сообщения) — раз в `interval` вызывать
/// `task` бесконечно. Ошибки и их логирование — забота самой `task`, этот
/// хелпер отвечает только за сам цикл `tokio::spawn`/`tokio::time::interval`.
///
/// `task` — не `async fn`, а фабрика future: каждый тик получает свежий
/// `Future`, поэтому всё нужное состояние клонируется внутри `task` заново
/// на каждый вызов (обычно дёшево — большинство типов здесь сами по себе
/// обёртки над `Arc`).
pub fn spawn_periodic<F, Fut>(interval: Duration, task: F)
where
    F: Fn() -> Fut + Send + 'static,
    Fut: Future<Output = ()> + Send,
{
    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(interval);
        loop {
            ticker.tick().await;
            task().await;
        }
    });
}
