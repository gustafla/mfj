use clap::{self, App, Arg};
use log;
use mfj::metadata_store::MetadataStore;
use simple_logger;
use std::error::Error;
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::{env, time::Duration};

fn find_dumps() -> Result<Vec<PathBuf>, Box<dyn Error>> {
    let mut entries: Vec<_> = fs::read_dir(".")?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| {
            p.file_name()
                .map(|os| {
                    os.to_str()
                        .map(|s| s.starts_with("messages"))
                        .unwrap_or(false)
                })
                .unwrap_or(false)
        })
        .collect();
    entries.sort_unstable_by(|p1, p2| p2.cmp(p1)); // newest first order (filenames alphabetically)
    Ok(entries)
}

#[tokio::main]
async fn main() {
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
        .arg(
            Arg::with_name("write_interval")
                .short("w")
                .long("write-interval")
                .help("Sets the interval for writing metadata storage (default 30min)")
                .takes_value(true),
        )
        .arg(
            Arg::with_name("metadata_store_path")
                .long("metadata-store-path")
                .help(
                    "Sets the file path where metadata storage is written\n\
                     (default ./messages.json.gz)",
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

    let write_interval = cmd_options
        .value_of("write_interval")
        .map(|s| humantime::parse_duration(s).expect(&format!("Failed to parse {}", s)))
        .unwrap_or(Duration::new(60 * 30, 0)); // 30m if -w / --write-interval is not provided

    let default_filename = format!("./messages-{}.json.gz", chrono::Local::now());
    let metadata_store_path = cmd_options
        .value_of("metadata_store_path")
        .unwrap_or(&default_filename);

    let reqwest_client = reqwest::Client::builder()
        .timeout(Duration::new(
            // 0s timeout means short polling, set 60s timeout for that
            if timeout_secs == 0 {
                60
            } else {
                timeout_secs * 2
            },
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

    let metadata_store = find_dumps()
        .unwrap()
        .iter()
        .filter_map(|read_path| {
            log::info!("Trying to load {}", read_path.display());
            MetadataStore::new(Some(read_path), metadata_store_path, write_interval).ok()
        })
        .next()
        .unwrap_or_else(|| {
            log::info!("Failed to load backups, starting fresh");
            MetadataStore::new(None::<&PathBuf>, metadata_store_path, write_interval).unwrap()
        });
    // TODO error handling goes here

    let running = Arc::new(AtomicBool::new(true));
    let twice = Arc::new(AtomicBool::new(false));
    let r = running.clone();
    ctrlc::set_handler(move || {
        if twice.load(Ordering::SeqCst) {
            std::process::exit(1);
        } else {
            log::info!("Interrupt signal received, waiting for requests to finish");
            log::warn!("Press twice to force quit and lose recent data");
            r.store(false, Ordering::SeqCst);
            twice.store(true, Ordering::SeqCst);
        }
    })
    .expect("Failed to set ctrl-c handler");

    mfj::StatsBot::new(&api_url, reqwest_client, metadata_store)
        .poll(running, timeout_secs)
        .await
        .unwrap();
    // TODO error handling goes here
}
