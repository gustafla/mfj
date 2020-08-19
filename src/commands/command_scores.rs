use crate::{MetadataStore, TelegramChatId};

pub fn render(
    command: &str,
    chat_id: TelegramChatId,
    metadata_store: &mut MetadataStore,
) -> String {
    let mut response = Vec::new();
    for word in command.split_whitespace() {
        let user_scores = metadata_store.get_scores_by_user(command, chat_id);
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

    if response.is_empty() {
        response.push("Anna vähintään yksi sana".into());
    }

    response.concat()
}
