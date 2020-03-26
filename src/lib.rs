pub mod commands;
pub mod metadata_store;

use commands::CommandInvocation;
use metadata_store::MetadataStore;
use serde_json::json;
use std::collections::HashMap;
use std::convert::TryInto;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

pub type TelegramUserId = i32;
pub type TelegramChatId = i64;
pub type TelegramMessageId = i32;

pub struct StatsBot {
    api_url_get_updates: String,
    api_url_send_message: String,
    api_url_edit_message_text: String,
    reqwest_client: reqwest::Client,
    metadata_store: MetadataStore,
    last_command_invocation_and_message_id_by_chat:
        HashMap<TelegramChatId, (CommandInvocation, TelegramMessageId)>,
    messages_after_last_post_by_chat: HashMap<TelegramChatId, usize>,
}

impl StatsBot {
    pub fn new(
        api_url: &str,
        reqwest_client: reqwest::Client,
        metadata_store: MetadataStore,
    ) -> Self {
        Self {
            api_url_get_updates: format!("{}/getUpdates", api_url),
            api_url_send_message: format!("{}/sendMessage", api_url),
            api_url_edit_message_text: format!("{}/editMessageText", api_url),
            reqwest_client,
            metadata_store,
            last_command_invocation_and_message_id_by_chat: HashMap::new(),
            messages_after_last_post_by_chat: HashMap::new(),
        }
    }

    async fn send_message(
        &self,
        chat_id: TelegramChatId,
        text: &str,
    ) -> reqwest::Result<TelegramMessageId> {
        let response = self
            .reqwest_client
            .post(&self.api_url_send_message)
            .json(&json!({
                "chat_id": chat_id,
                "text": text
            }))
            .send()
            .await?;

        let response: serde_json::Value = response.json().await?;

        Ok(response["result"]["message_id"]
            .as_i64()
            .unwrap()
            .try_into()
            .unwrap())
    }

    async fn update_message(
        &self,
        chat_id: TelegramChatId,
        message_id: TelegramMessageId,
        text: &str,
    ) -> reqwest::Result<()> {
        self.reqwest_client
            .post(&self.api_url_edit_message_text)
            .json(&json!({
                "chat_id": chat_id,
                "message_id": message_id,
                "text": text
            }))
            .send()
            .await?;

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

    async fn process_updates(
        &mut self,
        updates: &[serde_json::Value],
    ) -> Result<(), metadata_store::Error> {
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
                        match entity["type"].as_str().unwrap() {
                            "bot_command" => {
                                let command = message["text"].as_str().unwrap();
                                log::info!("Received command: '{}' from {}", command, user_id);

                                // Get the command part of a command message and pattern match it
                                // Won't panic, always contains at least a '/'
                                let word = command.split_whitespace().next().unwrap();
                                let word = word.split('@').next().unwrap_or(word);
                                let procedure: Option<commands::CommandProcedure> = match word {
                                    "/tilasto" => Some(commands::command_stats::render),
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

                                    // Send result (TODO handle error)
                                    let message_id =
                                        self.send_message(chat_id, &text).await.unwrap();

                                    // Store last command invocation and response ids
                                    self.last_command_invocation_and_message_id_by_chat
                                        .insert(chat_id, (invocation, message_id));
                                    self.messages_after_last_post_by_chat.insert(chat_id, 0);
                                }

                                continue 'update_loop; // Do not count bot commands
                            }
                            _ => {}
                        }
                    }
                }

                // Count message
                let count = self
                    .messages_after_last_post_by_chat
                    .get(&chat_id)
                    .map(|i| *i)
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
                        self.update_message(chat_id, *message_id, &text)
                            .await
                            .unwrap();
                        // TODO handle error
                    }
                }
            }
        }

        Ok(())
    }

    pub async fn poll(
        &mut self,
        running: Arc<AtomicBool>,
        timeout_secs: u64,
    ) -> reqwest::Result<()> {
        use reqwest::StatusCode;

        let mut params_get_updates = json!({ "timeout": timeout_secs });

        log::info!("Starting polling, timeout {}s", timeout_secs);

        let mut error_count = 0;
        while running.load(Ordering::SeqCst) {
            log::trace!("Sending a new update request");
            let response = match self
                .reqwest_client
                .get(&self.api_url_get_updates)
                .json(&params_get_updates)
                .send()
                .await
            {
                Ok(response) => response,
                Err(e) => {
                    log::info!("Update request error: {}", e);
                    error_count += 1;
                    if error_count > 32 {
                        log::error!("Too many consecutive errors, aborting");
                        return Err(e);
                    }
                    continue;
                }
            };

            error_count = 0;

            match response.status() {
                StatusCode::OK => {
                    let updates: serde_json::Value = response.json().await?;
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

                    self.process_updates(updates).await.unwrap();
                    // TODO Error handling goes here
                }
                other => log::error!(
                    "Server returned {}.\n{}",
                    other,
                    response.text().await.unwrap_or(String::new())
                ),
            }
        }

        Ok(())
    }
}
