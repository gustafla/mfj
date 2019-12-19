use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Serialize, Deserialize, Debug, Default)]
pub struct Message {
    pub user_id: i64,
    pub timestamp: u64,
}

#[derive(Serialize, Deserialize, Debug, Default)]
pub struct MetadataStore {
    messages_by_chat: HashMap<i64, Vec<Message>>,
}

impl MetadataStore {
    pub fn add_message(&mut self, chat_id: i64, message: Message) {
        let messages = self.messages_by_chat.entry(chat_id).or_insert(Vec::new());
        messages.push(message);
    }
}
