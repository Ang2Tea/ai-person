/// Разрешён ли доступ инструмента, работающего с конкретным чатом (`send_message`,
/// `read_chat_history`), из чата `current` к чату `target`. Сейчас — только тот же
/// чат; единая точка, которую позже можно ослабить (white-list разрешённых пар
/// чатов, системный вызов не через модель для проактивной отправки), не меняя
/// каждый инструмент по отдельности.
pub fn is_chat_access_allowed(current: i64, target: i64) -> bool {
    current == target
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_chat_is_allowed() {
        assert!(is_chat_access_allowed(341832691, 341832691));
    }

    #[test]
    fn different_chat_is_denied() {
        assert!(!is_chat_access_allowed(341832691, 5113698655));
    }
}
