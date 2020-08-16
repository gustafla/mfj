use anyhow::{Context, Result};
use mfj::metadata_store::MetadataStore;
use pico_args::Arguments;
use std::{
    env, fs,
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Duration,
};

fn find_dumps() -> Result<Vec<PathBuf>> {
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

fn main() -> Result<()> {
    // Try to load .env file
    #[cfg(feature = "dotenv")]
    {
        if let Some(e) = dotenv::dotenv().err() {
            eprintln!("Cannot load .env file: {}", e);
        }
    }

    let var_token = match env::var("API_TOKEN") {
        Ok(var) => Some(var),
        Err(env::VarError::NotPresent) => None,
        Err(e) => return Err(e).context("Failed to read environment"),
    };

    let mut args = Arguments::from_env();

    let timeout_secs = args
        .opt_value_from_str(["-t", "--poll-timeout"])?
        .unwrap_or(60);
    let write_interval = args
        .opt_value_from_fn(["-w", "--write-interval"], |s| humantime::parse_duration(s))?
        .unwrap_or_else(|| Duration::from_secs(60 * 30));
    let log_level = if args.contains(["-v", "--verbose"]) {
        log::Level::Trace
    } else {
        log::Level::Info
    };

    if let Some(token) = args
        .free()
        .context("Unexpected argument")?
        .into_iter()
        .next()
        .or(var_token)
    {
        let api_url = format!("https://api.telegram.org/bot{}", token);
        let default_filename = format!("./messages-{}.json.gz", chrono::Local::now());

        simple_logger::init_with_level(log_level).context("Logger failed to initialize")?;
        log::info!("Starting version {}", env!("CARGO_PKG_VERSION"));

        // TODO cleanup this
        let metadata_store = find_dumps()?
            .iter()
            .filter_map(|read_path| {
                log::info!("Trying to load {}", read_path.display());
                MetadataStore::new(Some(read_path), &default_filename, write_interval).ok()
            })
            .next()
            .unwrap_or_else(|| {
                log::info!("Failed to load backups, starting fresh");
                MetadataStore::new(None::<&PathBuf>, &default_filename, write_interval).unwrap()
            });

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
        .context("Failed to set ctrl-c handler")?;

        mfj::StatsBot::new(&api_url, Duration::from_secs(timeout_secs), metadata_store)
            .poll(running)
            .with_context(|| {
                format!(
                    "{} encountered an unrecoverable error",
                    env!("CARGO_PKG_NAME")
                )
            })
    } else {
        Err(anyhow::anyhow!("Please supply a Telegram bot API_TOKEN"))
    }
}
