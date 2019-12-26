pub mod command_stats;

use crate::{MetadataStore, TelegramChatId};

pub type CommandProcedure =
    fn(command: &str, chat_id: TelegramChatId, metadata_store: &mut MetadataStore) -> String;

pub struct CommandInvocation {
    pub procedure: CommandProcedure,
    pub command_string: String,
    pub chat_id: TelegramChatId,
}

impl CommandInvocation {
    pub fn run(&self, metadata_store: &mut MetadataStore) -> String {
        (self.procedure)(&self.command_string, self.chat_id, metadata_store)
    }
}
