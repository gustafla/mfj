use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    fs::{File, OpenOptions},
    io::{Read, Seek, SeekFrom, Write},
    path::Path,
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

#[derive(Serialize, Deserialize, Debug, Default)]
pub struct Message {
    pub user_id: i64,
    pub timestamp: u64,
}

#[derive(Serialize, Deserialize, Debug, Default)]
struct MessagesByChat(HashMap<i64, Vec<Message>>);

#[derive(Debug)]
pub struct MetadataStore {
    messages_by_chat: MessagesByChat,
    file: File,
}

impl MetadataStore {
    pub fn new<P: AsRef<Path>>(file_path: P) -> Result<Self, Error> {
        let mut file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .open(&file_path)?;

        Ok(MetadataStore {
            messages_by_chat: {
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
        })
    }

    pub fn add_message(&mut self, chat_id: i64, message: Message) {
        let messages = self.messages_by_chat.0.entry(chat_id).or_insert(Vec::new());
        messages.push(message);
    }

    pub fn sync_file(&mut self) -> Result<(), Error> {
        let json = serde_json::to_string(&self.messages_by_chat.0)?;
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
