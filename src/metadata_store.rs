use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    fs::{File, OpenOptions},
    io::{Read, Seek, SeekFrom, Write},
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

#[derive(Serialize, Deserialize, Debug, Default)]
struct TimestampsByChatUser(
    HashMap</* chat_id */ i64, HashMap</* user_id */ i64, Vec</* timestamp */ u64>>>,
);

#[derive(Debug)]
pub struct MetadataStore {
    timestamps_by_chat_user: TimestampsByChatUser,
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

        let mut file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .open(&file_path)?;

        Ok(MetadataStore {
            timestamps_by_chat_user: {
                let mut json = String::new();
                file.read_to_string(&mut json)?;
                serde_json::from_str(&json).unwrap_or({
                    log::info!(
                        "Failed to load {}, initializing new",
                        file_path.as_ref().to_string_lossy()
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
        let chat_users = self.timestamps_by_chat_user.0.entry(chat_id).or_insert(HashMap::new());
        let timestamps = chat_users.entry(message.user_id).or_insert(Vec::new());
        timestamps.push(message.timestamp);

        if self.last_written.elapsed() > self.write_interval {
            self.sync_file()?;
            self.last_written = Instant::now();
        }
        Ok(())
    }

    fn sync_file(&mut self) -> Result<(), Error> {
        log::info!("Writing to disk");
        let json = serde_json::to_string(&self.timestamps_by_chat_user.0)?;
        self.file.seek(SeekFrom::Start(0))?;
        self.file.set_len(0)?;
        self.file.write_all(json.as_bytes())?;
        Ok(())
    }
}

impl Drop for MetadataStore {
    fn drop(&mut self) {
        self.sync_file().unwrap();
    }
}
