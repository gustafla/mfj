use crate::{MetadataStore, TelegramChatId};
use std::time::{SystemTime, UNIX_EPOCH};

pub fn render(
    command: &str,
    chat_id: TelegramChatId,
    metadata_store: &mut MetadataStore,
) -> String {
    let mut after_unix = 0;
    let mut from = "kaikki";

    if let Some(param) = command.split_whitespace().next() {
        let time_str = &command[param.len()..].trim();
        if let Some(duration) = humantime::parse_duration(time_str).ok() {
            if let Some(after) = SystemTime::now().checked_sub(duration) {
                if let Some(after_since_epoch) = after.duration_since(UNIX_EPOCH).ok() {
                    after_unix = after_since_epoch.as_secs();
                    from = time_str;
                } else {
                    log::error!("System time conversion failed for command {}", command);
                }
            }
        }
    }

    let user_message_counts =
        metadata_store.get_chat_message_counts_by_user(chat_id, after_unix as i64);
    let total: usize = user_message_counts.iter().map(|e| e.1).sum();

    let mut response = vec![format!("Viestejä yhteensä {}: {}\n\n", from, total)];

    if total > 0 {
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
    }

    response.concat()
}
