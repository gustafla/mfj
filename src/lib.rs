pub mod metadata_store;

//use chrono::prelude::*;
use metadata_store::MetadataStore;
use serde_json::json;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

pub struct StatsBot {
    api_url_get_updates: String,
    api_url_send_message: String,
    reqwest_client: reqwest::Client,
    metadata_store: MetadataStore,
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
            reqwest_client,
            metadata_store,
        }
    }

    fn command_stats(&self, _command: &str, chat_id: i64) -> reqwest::Result<()> {
        let user_message_counts = self.metadata_store.get_chat_message_counts_by_user(chat_id);
        let total: usize = user_message_counts.iter().map(|e| e.1).sum();

        let mut response = vec![format!("Total messages: {}\n\n", total)];
        for (user, count) in user_message_counts {
            response.push(format!(
                "{}: {}\n",
                self.metadata_store
                    .get_user_name(user)
                    .unwrap_or(&user.to_string()),
                count
            ));
        }

        self.reqwest_client
            .post(&self.api_url_send_message)
            .json(&json!({
                "chat_id": chat_id,
                "text": response.concat()
            }))
            .send()?;
        Ok(())
    }

    fn store_user_name(&mut self, user_id: i64, user: &serde_json::Value) {
        let mut user_name = user["first_name"].as_str().unwrap().to_string();

        if let Some(last_name) = user.get("last_name") {
            user_name.push_str(&format!(" {}", last_name.as_str().unwrap()));
        }

        if let Some(username) = user.get("username") {
            user_name.push_str(&format!(" ({})", username.as_str().unwrap()));
        }

        self.metadata_store.add_user_name(user_id, user_name);
    }

    fn process_updates(
        &mut self,
        updates: &[serde_json::Value],
    ) -> Result<(), metadata_store::Error> {
        'update_loop: for update in updates {
            log::trace!("{}", update);

            if let Some(message) = update.get("message") {
                let chat_id = message["chat"]["id"].as_i64().unwrap();
                let user = &message["from"];
                let user_id = user["id"].as_i64().unwrap();
                let timestamp = message["date"].as_u64().unwrap();

                self.store_user_name(user_id, user);

                if let Some(entities) = message.get("entities") {
                    for entity in entities.as_array().unwrap() {
                        match entity["type"].as_str().unwrap() {
                            "bot_command" => {
                                let command = message["text"].as_str().unwrap();
                                log::info!("Received command: '{}' from {}", command, user_id);

                                // Get the command part of a command message and pattern match it
                                match command.split('@').next().unwrap_or_else(|| {
                                    command.split_whitespace().next().unwrap_or(command)
                                }) {
                                    "/tilasto" => {
                                        // TODO handle error
                                        self.command_stats(command, chat_id).unwrap()
                                    }
                                    _ => {}
                                }
                                continue 'update_loop; // Do not count bot commands
                            }
                            _ => {}
                        }
                    }
                }

                self.metadata_store
                    .add_message(chat_id, user_id, timestamp)?;
            }
        }
        Ok(())
    }

    pub fn poll(&mut self, running: Arc<AtomicBool>, timeout_secs: u64) -> reqwest::Result<()> {
        use reqwest::StatusCode;

        let mut params_get_updates = json!({ "timeout": timeout_secs });

        log::info!("Starting polling, timeout {}s", timeout_secs);

        let mut error_count = 0;
        while running.load(Ordering::SeqCst) {
            log::trace!("Sending a new update request");
            let mut response = match self
                .reqwest_client
                .get(&self.api_url_get_updates)
                .json(&params_get_updates)
                .send()
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
                    let updates: serde_json::Value = response.json()?;
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

                    self.process_updates(updates).unwrap();
                    // TODO Error handling goes here
                }
                other => log::error!(
                    "Server returned {}.\n{}",
                    other,
                    response.text().unwrap_or(String::new())
                ),
            }
        }

        Ok(())
    }
}
