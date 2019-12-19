mod metadata_store;

//use chrono::prelude::*;
use metadata_store::{Message, MetadataStore};
use serde_json::json;
use std::sync::Mutex;
use lazy_static::lazy_static;

lazy_static! {
    static ref METADATA_STORE: Mutex<MetadataStore> = Mutex::new(Default::default());
}

fn process_updates(updates: &[serde_json::Value]) {
    for update in updates {
        log::trace!("{}", update);

        if let Some(message) = update.get("message") {
            let chat_id = message["chat"]["id"].as_i64().unwrap();
            let user_id = message["from"]["id"].as_i64().unwrap();
            let timestamp = message["date"].as_u64().unwrap();

            METADATA_STORE
                .lock()
                .unwrap()
                .add_message(chat_id, Message { user_id, timestamp });
        }
    }
}

pub fn poll(api_url: &str, reqwest_client: reqwest::Client, timeout_secs: u64) -> reqwest::Result<()> {
    use reqwest::StatusCode;

    let api_url_get_updates = format!("{}/getUpdates", api_url);

    let mut params_get_updates = json!({ "timeout": timeout_secs });

    log::info!("Starting polling, timeout {}s", timeout_secs);

    loop {
        let mut response = reqwest_client
            .get(&api_url_get_updates)
            .json(&params_get_updates)
            .send()?;

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

                process_updates(updates);
            }
            _ => println!(
                "Server returned {}.\n{}",
                response.status(),
                response.text()?
            ),
        }
    }
}
