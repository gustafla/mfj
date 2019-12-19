use clap::{self, App, Arg};
use log;
use serde_json::json;
use simple_logger;
use std::{env, time::Duration};

fn process_updates(updates: &[serde_json::Value]) {
    for update in updates {
        log::trace!("{}", update);
    }
}

fn poll(api_url: &str, reqwest_client: reqwest::Client, timeout_secs: u64) -> reqwest::Result<()> {
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

fn main() {
    // Try to load .env file
    #[cfg(feature = "dotenv")]
    {
        if let Some(e) = dotenv::dotenv().err() {
            eprintln!("Cannot load .env file: {}", e);
        }
    }

    let var_token = env::var("API_TOKEN");

    let cmd_options = App::new("Telegram bot")
        .version(clap::crate_version!())
        .about("Collects message metadata")
        .arg(
            Arg::with_name("verbose")
                .short("v")
                .long("verbose")
                .multiple(true)
                .help("Prints debugging information (pass twice for trace)"),
        )
        .arg(
            Arg::with_name("token")
                .help("Telegram Bot API token to use (if API_TOKEN envvar is not set)")
                .required(var_token.is_err()),
        )
        .arg(
            Arg::with_name("poll_timeout")
                .short("t")
                .long("poll-timeout")
                .help(
                    "Sets the timeout for long polling requests\n\
                     Default 60s, 0s for short polling",
                )
                .takes_value(true),
        )
        .get_matches();

    let token = cmd_options
        .value_of("token")
        .map(|s| s.to_string())
        .unwrap_or(var_token.unwrap());
    let api_url = format!("https://api.telegram.org/bot{}", token);

    let timeout_secs = cmd_options
        .value_of("poll_timeout")
        .map(|s| {
            humantime::parse_duration(s)
                .expect(&format!("Failed to parse {}", s))
                .as_secs()
        })
        .unwrap_or(60); // 60s if -t / --poll-timeout is not provided

    let reqwest_client = reqwest::Client::builder()
        .timeout(Duration::new(
            // 0s timeout means short polling, set 60s timeout for that
            if timeout_secs == 0 { 60 } else { timeout_secs },
            0,
        ))
        .build()
        .unwrap();

    simple_logger::init_with_level(match cmd_options.occurrences_of("verbose") {
        0 => log::Level::Info,
        1 => log::Level::Debug,
        _ => log::Level::Trace,
    })
    .expect("Logger failed to initialize");
    log::info!("Starting version {}", clap::crate_version!());

    poll(&api_url, reqwest_client, timeout_secs).unwrap();
    // TODO error handling goes here
}
