use crate::{MetadataStore, TelegramChatId};

pub fn render(
    command: &str,
    chat_id: TelegramChatId,
    metadata_store: &mut MetadataStore,
) -> String {
    let mut response = Vec::new();
    if let Some((_, word)) = command.split_once(|c: char| c.is_whitespace()) {
        let user_scores = metadata_store.get_scores_by_user(word, chat_id);
        if user_scores.is_empty() {
            response.push(format!("Ei pisteitä sanalle {}.", word));
        } else {
            response.push(format!("Pisteet sanalle {}:\n\n", word));
            for (user, score) in user_scores {
                response.push(format!(
                    "{}: {}\n",
                    metadata_store
                        .get_user_name(user)
                        .unwrap_or(&user.to_string()),
                    score
                ));
            }
        }
    } else {
        response.push("Käyttöohje: /pisteet sana".into());
    }

    response.concat()
}
