pub mod command_scores;
pub mod command_stats;

use crate::{MetadataStore, TelegramChatId};
use std::time::{SystemTime, UNIX_EPOCH};

pub fn convert_time(command: &str) -> Option<(i64, &str)> {
    if let Some(param) = command.split_whitespace().next() {
        let time_str = &command[param.len()..].trim();
        if let Ok(duration) = humantime::parse_duration(time_str) {
            if let Some(after) = SystemTime::now().checked_sub(duration) {
                if let Ok(after_since_epoch) = after.duration_since(UNIX_EPOCH) {
                    return Some((after_since_epoch.as_secs().try_into().unwrap(), time_str));
                } else {
                    log::error!("System time conversion failed for command {}", command);
                }
            }
        }
    }
    None
}

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
