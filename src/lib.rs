pub mod metadata_store;

//use chrono::prelude::*;
use metadata_store::{Message, MetadataStore};
use serde_json::json;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

fn process_updates(
    updates: &[serde_json::Value],
    metadata_store: &mut MetadataStore,
) -> Result<(), metadata_store::Error> {
    for update in updates {
        log::trace!("{}", update);

        if let Some(message) = update.get("message") {
            let chat_id = message["chat"]["id"].as_i64().unwrap();
            let user_id = message["from"]["id"].as_i64().unwrap();
            let timestamp = message["date"].as_u64().unwrap();

            metadata_store.add_message(chat_id, Message { user_id, timestamp })?;
        }
    }
    Ok(())
}

pub fn poll(
    running: Arc<AtomicBool>,
    api_url: &str,
    reqwest_client: reqwest::Client,
    timeout_secs: u64,
    mut metadata_store: &mut MetadataStore,
) -> reqwest::Result<()> {
    use reqwest::StatusCode;

    let api_url_get_updates = format!("{}/getUpdates", api_url);

    let mut params_get_updates = json!({ "timeout": timeout_secs });

    log::info!("Starting polling, timeout {}s", timeout_secs);

    while running.load(Ordering::SeqCst) {
        log::trace!("Sending a new update request");
        let mut response = match reqwest_client
            .get(&api_url_get_updates)
            .json(&params_get_updates)
            .send()
        {
            Ok(response) => response,
            Err(e) => {
                if e.is_timeout() { // Ignore timeouts
                    log::info!("Update request timed out");
                    continue;
                } else { // Don't ignore other errors
                    return Err(e);
                }
            }
        };

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

                process_updates(updates, &mut metadata_store).unwrap();
                // TODO Error handling goes here
            }
            _ => println!(
                "Server returned {}.\n{}",
                response.status(),
                response.text()?
            ),
        }
    }

    Ok(())
}
