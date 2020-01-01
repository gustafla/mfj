use crate::{MetadataStore, TelegramChatId};

pub fn render(
    command: &str,
    chat_id: TelegramChatId,
    metadata_store: &mut MetadataStore,
) -> String {
    let (after_unix, from) = super::convert_time(command).unwrap_or((0, "kaikki"));

    let user_message_counts = metadata_store.get_message_counts_by_user(chat_id, after_unix);
    let total: usize = user_message_counts.iter().map(|e| e.1).sum();

    let mut response = vec![format!("Viestejä yhteensä {}: {}\n\n", from, total)];

    for (user, count) in user_message_counts {
        response.push(format!(
            "{}: {} ({:.1}%)\n",
            metadata_store
                .get_user_name(user)
                .unwrap_or(&user.to_string()),
            count,
            (count * 100) as f64 / total as f64
        ));
    }

    response.concat()
}
