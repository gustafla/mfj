use flate2::{read::GzDecoder, write::GzEncoder, Compression};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    fs::{File, OpenOptions},
    io::{Seek, SeekFrom},
    path::Path,
    time::{Duration, Instant},
};

#[derive(Debug)]
pub enum Error {
    JSON(serde_json::Error),
    IO(std::io::Error),
}

impl From<serde_json::Error> for Error {
    fn from(error: serde_json::Error) -> Self {
        Error::JSON(error)
    }
}

impl From<std::io::Error> for Error {
    fn from(error: std::io::Error) -> Self {
        Error::IO(error)
    }
}

#[derive(Debug, Default)]
pub struct Message {
    pub user_id: i64,
    pub timestamp: u64,
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct MetadataContent {
    timestamps_by_chat_user:
        HashMap</* chat_id */ i64, HashMap</* user_id */ i64, Vec</* timestamp */ u64>>>,
    user_names: HashMap<i64, String>,
}

#[derive(Debug)]
pub struct MetadataStore {
    content: MetadataContent,
    file: File,
    last_written: Instant,
    write_interval: Duration,
}

impl MetadataStore {
    pub fn new<P: AsRef<Path>>(file_path: P, write_interval: Duration) -> Result<Self, Error> {
        log::info!(
            "Initializing metadata storage, path: {}, write interval: {}",
            file_path.as_ref().to_string_lossy(),
            humantime::format_duration(write_interval)
        );

        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .open(&file_path)?;

        Ok(MetadataStore {
            content: {
                serde_json::from_reader(GzDecoder::new(&file)).unwrap_or_else(|e| {
                    log::info!(
                        "Failed to load {}, initializing new ({})",
                        file_path.as_ref().to_string_lossy(),
                        e
                    );
                    Default::default()
                })
            },
            file,
            last_written: Instant::now(),
            write_interval,
        })
    }

    pub fn add_message(&mut self, chat_id: i64, message: Message) -> Result<(), Error> {
        let chat_users = self
            .content
            .timestamps_by_chat_user
            .entry(chat_id)
            .or_insert(HashMap::new());
        let timestamps = chat_users.entry(message.user_id).or_insert(Vec::new());
        timestamps.push(message.timestamp);

        if self.last_written.elapsed() > self.write_interval {
            self.sync_file()?;
            self.last_written = Instant::now();
        }
        Ok(())
    }

    pub fn add_user_name(&mut self, user_id: i64, name: String) {
        self.content.user_names.insert(user_id, name);
    }

    pub fn get_user_name(&self, user_id: i64) -> Option<&str> {
        self.content.user_names.get(&user_id).map(|s| s.as_str())
    }

    pub fn get_chat_message_counts_by_user(&self, chat_id: i64) -> Vec<(i64, usize)> {
        let mut result = Vec::new();
        if let Some(user_timestamps) = self.content.timestamps_by_chat_user.get(&chat_id) {
            for (user, timestamps) in user_timestamps {
                result.push((*user, timestamps.len()));
            }
        }
        result.sort_unstable_by(|a, b| b.1.cmp(&a.1));
        result
    }

    fn sync_file(&mut self) -> Result<(), Error> {
        log::info!("Writing to disk");
        self.file.seek(SeekFrom::Start(0))?;
        self.file.set_len(0)?;
        serde_json::to_writer(
            GzEncoder::new(&self.file, Compression::default()),
            &self.content,
        )?;
        Ok(())
    }
}

impl Drop for MetadataStore {
    fn drop(&mut self) {
        self.sync_file().unwrap();
    }
}
