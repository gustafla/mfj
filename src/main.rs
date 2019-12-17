use clap::{self, App, Arg};
use lazy_static::lazy_static;
use log;
use simple_logger;
use std::{env, thread, time::Duration};

lazy_static! {
    static ref REQWEST: reqwest::Client = reqwest::Client::new();
}

fn poll(api_url: &str) -> reqwest::Result<()> {
    use reqwest::StatusCode;

    let api_get_updates = format!("{}/getUpdates", api_url);

    log::info!("Starting polling");
    loop {
        let mut response = REQWEST.get(&api_get_updates).send()?;
        match response.status() {
            StatusCode::OK => println!("{}", response.text()?),
            _ => println!(
                "Server returned {}.\n{}",
                response.status(),
                response.text()?
            ),
        }
        thread::sleep(Duration::new(2, 0));
    }
}

fn main() {
    // Try to load .env file
    if cfg!(debug_assertions) {
        if let Some(e) = dotenv::dotenv().err() {
            eprintln!("Cannot load .env file: {}", e);
        }
    }

    let var_token = env::var("API_TOKEN");

    let matches = App::new("Telegram bot")
        .version(clap::crate_version!())
        .about("Collects message metadata")
        .arg(
            Arg::with_name("token")
                .help("Telegram Bot API token to use (if API_TOKEN envvar is not set)")
                .required(var_token.is_err()),
        )
        .get_matches();

    let token = matches
        .value_of("token")
        .map(|m| m.to_string())
        .unwrap_or(var_token.unwrap());
    let api_url = format!("https://telegram.org/bot{}", token);

    simple_logger::init().expect("Logger failed to initialize");
    log::info!("Starting version {}", clap::crate_version!());

    poll(&api_url).unwrap();
}
