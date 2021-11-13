pub mod commands;
pub mod metadata_store;

use aho_corasick::{AhoCorasick, AhoCorasickBuilder};
use commands::CommandInvocation;
use metadata_store::MetadataStore;
use serde_json::json;
use std::{
    collections::HashMap,
    convert::TryInto,
    io,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Duration,
};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error("Failed to parse request body")]
    JsonConversion(#[from] io::Error),
    #[error("Request failed with {0}")]
    Request(#[from] ureq::Error),
    #[error("Failed to store message metadata")]
    MetadataStore(#[from] metadata_store::Error),
}

pub type TelegramUserId = i32;
pub type TelegramChatId = i64;
pub type TelegramMessageId = i32;

pub struct StatsBot {
    timeout: Duration,
    api_url_get_updates: String,
    api_url_send_message: String,
    api_url_edit_message_text: String,
    metadata_store: MetadataStore,
    keyword_finder: AhoCorasick,
    last_command_invocation_and_message_id_by_chat:
        HashMap<TelegramChatId, (CommandInvocation, TelegramMessageId)>,
    messages_after_last_post_by_chat: HashMap<TelegramChatId, usize>,
}

impl StatsBot {
    const KEYWORDS: &'static [&'static str] = &["kesko"];

    pub fn new(api_url: &str, timeout: Duration, metadata_store: MetadataStore) -> Self {
        Self {
            timeout,
            api_url_get_updates: format!("{}/getUpdates", api_url),
            api_url_send_message: format!("{}/sendMessage", api_url),
            api_url_edit_message_text: format!("{}/editMessageText", api_url),
            metadata_store,
            keyword_finder: AhoCorasickBuilder::new()
                .ascii_case_insensitive(true)
                .build(Self::KEYWORDS),
            last_command_invocation_and_message_id_by_chat: HashMap::new(),
            messages_after_last_post_by_chat: HashMap::new(),
        }
    }

    fn send_message(
        &self,
        chat_id: TelegramChatId,
        text: &str,
    ) -> Result<TelegramMessageId, Error> {
        let response = ureq::post(&self.api_url_send_message).send_json(json!({
                "chat_id": chat_id,
                "text": text
        }));

        match response {
            Ok(response) => {
                let response: serde_json::Value = response.into_json()?;
                Ok(response["result"]["message_id"]
                    .as_i64()
                    .unwrap()
                    .try_into()
                    .unwrap())
            }
            Err(e @ ureq::Error::Status(400, _)) => {
                log::info!("Failed to send message {}", text);
                Err(e.into())
            }
            Err(e) => Err(e.into()),
        }
    }

    fn update_message(
        &self,
        chat_id: TelegramChatId,
        message_id: TelegramMessageId,
        text: &str,
    ) -> Result<(), Error> {
        ureq::post(&self.api_url_edit_message_text).send_json(json!({
                "chat_id": chat_id,
                "message_id": message_id,
                "text": text
        }))?;

        Ok(())
    }

    fn store_user_name(&mut self, user_id: TelegramUserId, user: &serde_json::Value) {
        let mut user_name = user["first_name"].as_str().unwrap().to_string();

        if let Some(last_name) = user.get("last_name") {
            user_name.push_str(&format!(" {}", last_name.as_str().unwrap()));
        }

        if let Some(username) = user.get("username") {
            user_name.push_str(&format!(" ({})", username.as_str().unwrap()));
        }

        // Remove cheeky Right to Left codes from names (TODO more sanitization)
        user_name = user_name
            .chars()
            .filter(|c| *c as u32 != 0x200f_u32)
            .collect();

        self.metadata_store.add_user_name(user_id, user_name);
    }

    fn process_updates(&mut self, updates: &[serde_json::Value]) -> Result<(), Error> {
        'update_loop: for update in updates {
            log::trace!("{}", update);

            if let Some(message) = update.get("message") {
                let chat_id: TelegramChatId = message["chat"]["id"].as_i64().unwrap();
                let user = &message["from"];
                let user_id: TelegramUserId = user["id"].as_i64().unwrap().try_into().unwrap();
                let timestamp = message["date"].as_i64().unwrap();

                self.store_user_name(user_id, user);

                if let Some(entities) = message.get("entities") {
                    for entity in entities.as_array().unwrap() {
                        if entity["type"] == json!("bot_command") {
                            let command = message["text"].as_str().unwrap();
                            log::info!("Received command: '{}' from {}", command, user_id);

                            // Get the command part of a command message and pattern match it
                            // Won't panic, always contains at least a '/'
                            let word = command.split_whitespace().next().unwrap();
                            let word = word.split('@').next().unwrap_or(word);
                            let procedure: Option<commands::CommandProcedure> = match word {
                                "/tilasto" => Some(commands::command_stats::render),
                                "/pisteet" => Some(commands::command_scores::render),
                                _ => None,
                            };

                            if let Some(procedure) = procedure {
                                let invocation = CommandInvocation {
                                    procedure,
                                    command_string: command.to_string(),
                                    chat_id,
                                };

                                // Run command
                                let text = invocation.run(&mut self.metadata_store);

                                // Send result
                                let message_id = self.send_message(chat_id, &text)?;

                                // Store last command invocation and response ids
                                self.last_command_invocation_and_message_id_by_chat
                                    .insert(chat_id, (invocation, message_id));
                                self.messages_after_last_post_by_chat.insert(chat_id, 0);

                                continue 'update_loop; // Do not count bot commands
                            }
                        }
                    }
                }

                // Check keywords
                if let Some(text) = message["text"].as_str() {
                    for mat in self.keyword_finder.find_iter(text) {
                        self.metadata_store.add_keyword_point(
                            Self::KEYWORDS[mat.pattern()],
                            chat_id,
                            user_id,
                        )?;
                        self.send_message(
                            chat_id,
                            &format!(
                                "Yksi (1) {} lisätty {0}-tilillesi",
                                Self::KEYWORDS[mat.pattern()]
                            ),
                        )?;
                    }
                }

                // Count message
                let count = self
                    .messages_after_last_post_by_chat
                    .get(&chat_id)
                    .copied()
                    .unwrap_or(0);
                self.messages_after_last_post_by_chat
                    .insert(chat_id, count + 1);
                self.metadata_store
                    .add_message(chat_id, user_id, timestamp)?;

                // Update previous response with new invocation
                log::debug!("messages_after_last_post_by_chat[{}] = {}", chat_id, count);
                if count <= 100 {
                    if let Some((invocation, message_id)) = self
                        .last_command_invocation_and_message_id_by_chat
                        .get(&chat_id)
                    {
                        log::info!(
                            "Updating last response to {} (message {}) in chat {}",
                            invocation.command_string,
                            message_id,
                            chat_id
                        );

                        let text = invocation.run(&mut self.metadata_store);
                        self.update_message(chat_id, *message_id, &text)?;
                    }
                }
            }
        }

        Ok(())
    }

    pub fn poll(&mut self, running: Arc<AtomicBool>) -> Result<(), Error> {
        let mut params_get_updates = json!({ "timeout": self.timeout.as_secs() });

        log::info!(
            "Starting polling, timeout {}",
            humantime::format_duration(self.timeout)
        );

        let mut error_count = 0;
        while running.load(Ordering::SeqCst) {
            log::trace!("Sending a new update request");
            let response = ureq::get(&self.api_url_get_updates)
                .timeout(self.timeout + Duration::from_secs(10))
                .send_json(params_get_updates.clone());

            match response {
                Ok(response) => {
                    error_count = 0;

                    let updates: serde_json::Value = response.into_json()?;
                    if !updates["ok"].as_bool().unwrap() {
                        panic!("Telegram getUpdates returned ok: false");
                    }

                    let updates: &Vec<serde_json::Value> = updates["result"].as_array().unwrap();

                    if let Some(next_id) = updates
                        .iter()
                        .map(|v| v["update_id"].as_u64().unwrap())
                        .max()
                    {
                        log::debug!("next_id = {}", next_id);
                        params_get_updates["offset"] = json!(next_id + 1);
                    }

                    self.process_updates(updates)?;
                }
                Err(error) => {
                    if let ureq::Error::Status(status, _) = error {
                        log::info!("Update request error status: {}", status);
                    }

                    error_count += 1;
                    if error_count > 32 {
                        log::error!("Too many consecutive errors, aborting");
                        return Err(error.into());
                    }
                    continue;
                }
            }
        }

        Ok(())
    }
}
