/// Разбивает текст на абзацы по пустым строкам — модель пишет один связный
/// ответ, а получателю он должен прийти как несколько отдельных сообщений
/// подряд, как в живой переписке, а не одним блоком с пустыми строками внутри.
pub(crate) fn split_into_paragraphs(text: &str) -> Vec<String> {
    let mut paragraphs = Vec::new();
    let mut current = String::new();

    for line in text.lines() {
        if line.trim().is_empty() {
            if !current.is_empty() {
                paragraphs.push(std::mem::take(&mut current));
            }
        } else {
            if !current.is_empty() {
                current.push('\n');
            }
            current.push_str(line);
        }
    }
    if !current.is_empty() {
        paragraphs.push(current);
    }

    paragraphs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_on_blank_lines() {
        assert_eq!(
            split_into_paragraphs("мы приедем\n\nс викой\n\nк вечеру"),
            vec!["мы приедем", "с викой", "к вечеру"]
        );
    }

    #[test]
    fn keeps_single_newlines_within_a_paragraph() {
        assert_eq!(
            split_into_paragraphs("строка один\nстрока два"),
            vec!["строка один\nстрока два"]
        );
    }

    #[test]
    fn collapses_multiple_blank_lines() {
        assert_eq!(
            split_into_paragraphs("первое\n\n\n\nвторое"),
            vec!["первое", "второе"]
        );
    }

    #[test]
    fn no_blank_lines_is_a_single_message() {
        assert_eq!(split_into_paragraphs("просто текст"), vec!["просто текст"]);
    }
}
