use crate::{TelegramChatId, TelegramUserId};
use flate2::{read::GzDecoder, write::GzEncoder, Compression};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    fs::{File, OpenOptions},
    io::{Seek, SeekFrom},
    path::Path,
    time::{Duration, Instant},
};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error("Failed to process json")]
    Json(#[from] serde_json::Error),
    #[error("An I/O error occured")]
    Io(#[from] std::io::Error),
}

type ChatUserMap<T> = HashMap<TelegramChatId, HashMap<TelegramUserId, T>>;

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct MetadataContent {
    timestamps_by_chat_user: ChatUserMap<Vec<i64>>,
    keyword_scores_by_keyword_chat_user: HashMap<String, ChatUserMap<u64>>,
    user_names: HashMap<TelegramUserId, String>,
}

#[derive(Debug)]
pub struct MetadataStore {
    content: MetadataContent,
    file: File,
    last_written: Instant,
    write_interval: Duration,
}

impl MetadataStore {
    pub fn new(
        read_path: Option<impl AsRef<Path>>,
        write_path: impl AsRef<Path>,
        write_interval: Duration,
    ) -> Result<Self, Error> {
        log::info!(
            "Initializing metadata storage, path: {}, write interval: {}",
            write_path.as_ref().to_string_lossy(),
            humantime::format_duration(write_interval)
        );

        let content = if let Some(read_path) = read_path {
            let read_file = File::open(&read_path)?;
            serde_json::from_reader(GzDecoder::new(&read_file))?
        } else {
            Default::default()
        };

        let write_file = OpenOptions::new()
            .write(true)
            .create(true)
            .open(&write_path)?;

        Ok(Self {
            content,
            file: write_file,
            last_written: Instant::now(),
            write_interval,
        })
    }

    pub fn add_message(
        &mut self,
        chat_id: TelegramChatId,
        user_id: TelegramUserId,
        timestamp: i64,
    ) -> Result<(), Error> {
        let users_timestamps = self
            .content
            .timestamps_by_chat_user
            .entry(chat_id)
            .or_insert_with(HashMap::new);
        users_timestamps
            .entry(user_id)
            .or_insert_with(Vec::new)
            .push(timestamp);

        if self.last_written.elapsed() > self.write_interval {
            self.sync_file()?;
            self.last_written = Instant::now();
        }
        Ok(())
    }

    pub fn add_keyword_point(
        &mut self,
        keyword: &str,
        chat_id: TelegramChatId,
        user_id: TelegramUserId,
    ) -> Result<(), Error> {
        let chat_users_scores = self
            .content
            .keyword_scores_by_keyword_chat_user
            .entry(keyword.to_string())
            .or_insert_with(HashMap::new);
        let users_scores = chat_users_scores
            .entry(chat_id)
            .or_insert_with(HashMap::new);
        *users_scores.entry(user_id).or_insert(0) += 1;

        if self.last_written.elapsed() > self.write_interval {
            self.sync_file()?;
            self.last_written = Instant::now();
        }
        Ok(())
    }

    pub fn add_user_name(&mut self, user_id: TelegramUserId, name: String) {
        self.content.user_names.insert(user_id, name);
    }

    pub fn get_user_name(&self, user_id: TelegramUserId) -> Option<&str> {
        self.content.user_names.get(&user_id).map(|s| s.as_str())
    }

    pub fn get_message_counts_by_user(
        &self,
        chat_id: TelegramChatId,
        after_unix: i64,
    ) -> Vec<(TelegramUserId, usize)> {
        let mut result = Vec::new();

        if let Some(users_timestamps) = self.content.timestamps_by_chat_user.get(&chat_id) {
            result = users_timestamps
                .iter()
                .map(|(u, t)| (*u, t.iter().filter(|t| **t > after_unix).count()))
                .filter(|(_, n)| *n > 0)
                .collect();
            result.sort_unstable_by(|a, b| b.1.cmp(&a.1));
        }

        result
    }

    pub fn get_scores_by_user(
        &self,
        keyword: &str,
        chat_id: TelegramChatId,
    ) -> Vec<(TelegramUserId, u64)> {
        let mut result: Vec<(TelegramUserId, u64)> = Vec::new();

        if let Some(chat_users_scores) = self
            .content
            .keyword_scores_by_keyword_chat_user
            .get(keyword)
        {
            if let Some(users_scores) = chat_users_scores.get(&chat_id) {
                result = users_scores
                    .iter()
                    .map(|(u, s)| (*u, *s))
                    .filter(|(_, s)| *s > 0)
                    .collect();
                result.sort_unstable_by(|a, b| b.1.cmp(&a.1));
            }
        }

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
