use anyhow::{Context, Result};
use argh::FromArgs;
use mfj::metadata_store::MetadataStore;
use std::{
    env, fs,
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Duration,
};

struct MyDuration(Duration);

impl std::str::FromStr for MyDuration {
    type Err = humantime::DurationError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        humantime::parse_duration(s).map(Self)
    }
}

#[derive(FromArgs)]
#[argh(description = "A telegram bot")]
struct MfjOptions {
    #[argh(option, description = "polling timeout in seconds", default = "60")]
    poll_timeout: u64,
    #[argh(
        option,
        description = "database file write interval (example: '30 min')",
        default = "MyDuration(Duration::from_secs(60 * 30))"
    )]
    write_interval: MyDuration,
    #[argh(switch, short = 'v', description = "log more information")]
    verbose: bool,
    #[argh(positional)]
    bot_api_token: Option<String>,
}

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

    let var_token = match env::var("MFJ_API_TOKEN") {
        Ok(var) => Some(var),
        Err(env::VarError::NotPresent) => None,
        Err(e) => return Err(e).context("Failed to read environment"),
    };

    let keywords = match env::var("MFJ_KEYWORDS") {
        Ok(var) => var.split(',').map(String::from).collect(),
        Err(env::VarError::NotPresent) => vec![String::from("kesko")],
        Err(e) => return Err(e).context("Failed to read environment"),
    };

    let args: MfjOptions = argh::from_env();

    if let Some(token) = args.bot_api_token.as_ref().or_else(|| var_token.as_ref()) {
        let api_url = format!("https://api.telegram.org/bot{}", token);
        let default_filename = format!("./messages-{}.json.gz", chrono::Local::now());

        simple_logger::init_with_level(if args.verbose {
            log::Level::Trace
        } else {
            log::Level::Info
        })
        .context("Logger failed to initialize")?;
        log::info!("Starting version {}", env!("CARGO_PKG_VERSION"));

        // TODO cleanup this
        let metadata_store = find_dumps()?
            .iter()
            .find_map(|read_path| {
                log::info!("Trying to load {}", read_path.display());
                MetadataStore::new(Some(read_path), &default_filename, args.write_interval.0).ok()
            })
            .unwrap_or_else(|| {
                log::info!("Failed to load backups, starting fresh");
                MetadataStore::new(None::<&PathBuf>, &default_filename, args.write_interval.0)
                    .unwrap()
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

        mfj::StatsBot::new(
            &api_url,
            Duration::from_secs(args.poll_timeout),
            metadata_store,
            keywords,
        )
        .poll(running)
        .with_context(|| {
            format!(
                "{} encountered an unrecoverable error",
                env!("CARGO_PKG_NAME")
            )
        })
    } else {
        Err(anyhow::anyhow!("Please supply a Telegram bot API token"))
    }
}
