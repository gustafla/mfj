use clap::{self, App, Arg};
use log;
use simple_logger;
use std::env;

fn main() {
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

    let token = matches.value_of("token").unwrap_or(&var_token.unwrap());

    simple_logger::init().expect("Logger failed to initialize");
    log::info!("Starting version {}", clap::crate_version!());
}
