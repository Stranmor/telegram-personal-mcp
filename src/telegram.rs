use crate::Config;
use ferogram::tl;
use ferogram::{Client, InputMessage, ShutdownToken};
use fs2::FileExt;
use serde::Serialize;
use std::fs::{File, OpenOptions};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use thiserror::Error;
use tokio::sync::Mutex;
use tokio::time::timeout;

const CONNECT_TIMEOUT: Duration = Duration::from_secs(30);
const RESOLVE_TIMEOUT: Duration = Duration::from_secs(12);

#[derive(Debug, Error)]
pub enum TelegramError {
    #[error("Telegram API error: {0}")]
    Telegram(String),
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("No usable Telegram session found. Checked: {0}")]
    NoSession(String),
    #[error("Telegram session is locked by another MCP process: {0}")]
    SessionLocked(String),
    #[error(
        "Telegram session exists but is not authorized; create or refresh it with a Telegram client first"
    )]
    Unauthorized,
    #[error("Invalid peer reference: {0}")]
    InvalidPeer(String),
    #[error("Peer not found: {0}")]
    PeerNotFound(String),
}

#[derive(Debug, Clone, Serialize)]
pub struct UserInfo {
    pub user_id: i64,
    pub first_name: String,
    pub username: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct MessageSummary {
    pub message_id: i32,
    pub text: Option<String>,
    pub date: i32,
    pub views: i32,
    pub forwards: i32,
    pub reply_count: i32,
    pub media_type: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DialogSummary {
    pub chat_id: i64,
    pub chat_ref: String,
    pub chat_kind: String,
    pub title: String,
    pub username: Option<String>,
    pub last_message_id: Option<i32>,
    pub last_message_text: Option<String>,
    pub last_message_outgoing: Option<bool>,
    pub last_message_date: Option<i32>,
}

struct ConnectedSession {
    client: Client,
    _shutdown: ShutdownToken,
    path: PathBuf,
    _lock: File,
}

pub struct TelegramClient {
    config: Config,
    connected: Mutex<Option<Arc<ConnectedSession>>>,
}

impl TelegramClient {
    pub fn new(config: Config) -> Self {
        Self { config, connected: Mutex::new(None) }
    }

    pub async fn get_me(
        &self,
        session_path: Option<PathBuf>,
    ) -> Result<(UserInfo, PathBuf), TelegramError> {
        let connected = self.connected(session_path).await?;
        let me = connected
            .client
            .get_me()
            .await
            .map_err(|error| TelegramError::Telegram(error.to_string()))?;
        Ok((
            UserInfo {
                user_id: me.id,
                first_name: me.first_name.unwrap_or_default(),
                username: me.username,
            },
            connected.path.clone(),
        ))
    }

    pub async fn list_dialogs(
        &self,
        session_path: Option<PathBuf>,
        limit: usize,
    ) -> Result<Vec<DialogSummary>, TelegramError> {
        let connected = self.connected(session_path).await?;
        let mut iter = connected.client.iter_dialogs();
        let mut dialogs = Vec::new();

        while let Some(dialog) = iter
            .next(&connected.client)
            .await
            .map_err(|error| TelegramError::Telegram(error.to_string()))?
        {
            let chat_id = dialog_peer_id(&dialog).unwrap_or_default();
            let username = dialog_username(&dialog);
            let chat_ref = username
                .as_ref()
                .map(|name| format!("@{name}"))
                .unwrap_or_else(|| public_chat_ref(chat_id, &dialog));
            let last = dialog.message.as_ref();
            dialogs.push(DialogSummary {
                chat_id,
                chat_ref,
                chat_kind: dialog_kind(&dialog),
                title: dialog.title(),
                username,
                last_message_id: last.map(message_id),
                last_message_text: last.and_then(message_text),
                last_message_outgoing: last.map(message_outgoing),
                last_message_date: last.map(message_date),
            });

            if dialogs.len() >= limit {
                break;
            }
        }

        Ok(dialogs)
    }

    pub async fn read_chat(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        limit: usize,
        offset_id: Option<i32>,
    ) -> Result<Vec<MessageSummary>, TelegramError> {
        let connected = self.connected(session_path).await?;
        let peer = Self::resolve_peer_ref(&connected.client, chat_id).await?;
        let telegram_limit = i32::try_from(limit)
            .map_err(|_| TelegramError::InvalidPeer("read_chat.limit is too large".to_string()))?;
        let raw_messages = connected
            .client
            .get_message_history(peer.as_str(), telegram_limit, offset_id.unwrap_or_default(), 0)
            .await
            .map_err(|error| TelegramError::Telegram(error.to_string()))?;

        let mut messages = Vec::new();
        for message in raw_messages {
            messages.push(MessageSummary {
                message_id: message.id(),
                text: message.text().map(ToOwned::to_owned),
                date: message.date(),
                views: 0,
                forwards: 0,
                reply_count: 0,
                media_type: None,
            });
            if messages.len() >= limit {
                break;
            }
        }

        Ok(messages)
    }

    pub async fn send_message(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        text: &str,
    ) -> Result<i32, TelegramError> {
        let connected = self.connected(session_path).await?;
        let peer = Self::resolve_peer_ref(&connected.client, chat_id).await?;
        let sent = connected
            .client
            .send_message(peer.as_str(), InputMessage::html(text))
            .await
            .map_err(|error| TelegramError::Telegram(error.to_string()))?;
        Ok(sent.id())
    }

    async fn connected(
        &self,
        session_path: Option<PathBuf>,
    ) -> Result<Arc<ConnectedSession>, TelegramError> {
        let selected_path = self.resolve_session_path(session_path).await?;
        let mut guard = self.connected.lock().await;
        if let Some(current) = guard.as_ref()
            && current.path == selected_path
        {
            return Ok(current.clone());
        }

        let lock = Self::lock_session_file(&selected_path)?;
        let mut builder = Client::builder()
            .api_id(self.config.api_id)
            .api_hash(self.config.api_hash.clone())
            .session(&selected_path);
        if let Some(proxy) = &self.config.proxy {
            builder = apply_proxy(builder, proxy)?;
        }
        let telegram_config =
            builder.build().map_err(|error| TelegramError::Telegram(error.to_string()))?;
        let (client, shutdown) = timeout(CONNECT_TIMEOUT, Client::connect(telegram_config))
            .await
            .map_err(|_| TelegramError::Telegram("connect timed out".to_string()))?
            .map_err(|error| TelegramError::Telegram(error.to_string()))?;
        if !client
            .is_authorized()
            .await
            .map_err(|error| TelegramError::Telegram(error.to_string()))?
        {
            return Err(TelegramError::Unauthorized);
        }

        let connected = Arc::new(ConnectedSession {
            client,
            _shutdown: shutdown,
            path: selected_path,
            _lock: lock,
        });
        *guard = Some(connected.clone());
        Ok(connected)
    }

    async fn resolve_session_path(
        &self,
        explicit_path: Option<PathBuf>,
    ) -> Result<PathBuf, TelegramError> {
        let mut checked = Vec::new();
        if let Some(path) = explicit_path.or_else(|| self.config.session_path.clone()) {
            checked.push(path.display().to_string());
            if path.is_file() {
                return Ok(path);
            }
        }

        let cache_dir = self.config.session_cache_dir();
        if cache_dir.is_dir() {
            let mut candidates = tokio::fs::read_dir(&cache_dir).await?;
            while let Some(entry) = candidates.next_entry().await? {
                let path = entry.path();
                checked.push(path.display().to_string());
                if path.extension().and_then(|value| value.to_str()) == Some("session") {
                    return Ok(path);
                }
            }
        }

        Err(TelegramError::NoSession(checked.join(", ")))
    }

    fn lock_session_file(session_path: &Path) -> Result<File, TelegramError> {
        let lock_path = session_path.with_extension("session.lock");
        let lock = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&lock_path)?;
        lock.try_lock_exclusive().map_err(|error| {
            TelegramError::SessionLocked(format!("{} ({error})", lock_path.display()))
        })?;
        Ok(lock)
    }

    async fn resolve_peer_ref(client: &Client, identifier: &str) -> Result<String, TelegramError> {
        let trimmed = identifier.trim();
        if trimmed.is_empty() {
            return Err(TelegramError::InvalidPeer("empty peer reference".to_string()));
        }
        if let Ok(peer_id) = trimmed.parse::<i64>() {
            return Self::resolve_numeric_peer(client, peer_id).await;
        }
        Ok(trimmed.to_string())
    }

    async fn resolve_numeric_peer(client: &Client, peer_id: i64) -> Result<String, TelegramError> {
        let mut dialogs = client.iter_dialogs();
        let future = async move {
            while let Some(dialog) = dialogs.next(client).await? {
                if numeric_peer_matches(peer_id, &dialog) {
                    return Ok(Some(public_chat_ref(
                        dialog_peer_id(&dialog).unwrap_or(peer_id),
                        &dialog,
                    )));
                }
            }
            Ok::<_, ferogram::InvocationError>(None)
        };
        match timeout(RESOLVE_TIMEOUT, future).await {
            Ok(Ok(Some(peer))) => Ok(peer),
            Ok(Ok(None)) => Err(TelegramError::PeerNotFound(format!(
                "numeric peer {peer_id} is not present in dialogs; access_hash is unavailable"
            ))),
            Ok(Err(error)) => Err(TelegramError::Telegram(error.to_string())),
            Err(_) => Err(TelegramError::PeerNotFound(format!(
                "numeric peer {peer_id}: dialog scan timed out"
            ))),
        }
    }
}

fn apply_proxy(
    builder: ferogram::builder::ClientBuilder,
    proxy: &str,
) -> Result<ferogram::builder::ClientBuilder, TelegramError> {
    let trimmed = proxy.trim();
    if let Some(addr) = trimmed.strip_prefix("socks5://") {
        return Ok(builder.socks5(addr));
    }
    if let Some(addr) = trimmed.strip_prefix("socks5h://") {
        return Ok(builder.socks5(addr));
    }
    if trimmed.starts_with("127.") || trimmed.starts_with("localhost:") || trimmed.contains(':') {
        return Ok(builder.socks5(trimmed));
    }
    Err(TelegramError::InvalidPeer(
        "only SOCKS5 proxy values are supported, for example socks5://127.0.0.1:1080".to_string(),
    ))
}

fn numeric_peer_matches(peer_id: i64, dialog: &ferogram::Dialog) -> bool {
    let Some(id) = dialog_peer_id(dialog) else {
        return false;
    };
    if let Some(channel_id) = public_channel_id_to_internal(peer_id) {
        return matches!(dialog.peer(), Some(tl::enums::Peer::Channel(_))) && id == channel_id;
    }
    id == peer_id || (peer_id < 0 && peer_id.checked_abs().is_some_and(|wanted| id == wanted))
}

fn dialog_peer_id(dialog: &ferogram::Dialog) -> Option<i64> {
    match dialog.peer()? {
        tl::enums::Peer::User(peer) => Some(peer.user_id),
        tl::enums::Peer::Chat(peer) => Some(peer.chat_id),
        tl::enums::Peer::Channel(peer) => Some(peer.channel_id),
    }
}

fn public_chat_ref(chat_id: i64, dialog: &ferogram::Dialog) -> String {
    if matches!(dialog.peer(), Some(tl::enums::Peer::Channel(_))) {
        format!("-100{chat_id}")
    } else {
        chat_id.to_string()
    }
}

fn dialog_kind(dialog: &ferogram::Dialog) -> String {
    match dialog.peer() {
        Some(tl::enums::Peer::User(_)) => "user",
        Some(tl::enums::Peer::Chat(_)) => "group",
        Some(tl::enums::Peer::Channel(_)) => "channel",
        None => "unknown",
    }
    .to_string()
}

fn dialog_username(dialog: &ferogram::Dialog) -> Option<String> {
    if let Some(tl::enums::User::User(user)) = &dialog.entity {
        return user.username.clone();
    }
    match &dialog.chat {
        Some(tl::enums::Chat::Channel(channel)) => channel.username.clone(),
        _ => None,
    }
}

fn message_id(message: &tl::enums::Message) -> i32 {
    match message {
        tl::enums::Message::Message(message) => message.id,
        tl::enums::Message::Service(message) => message.id,
        tl::enums::Message::Empty(message) => message.id,
    }
}

fn message_text(message: &tl::enums::Message) -> Option<String> {
    match message {
        tl::enums::Message::Message(message) if !message.message.is_empty() => {
            Some(message.message.clone())
        },
        _ => None,
    }
}

fn message_outgoing(message: &tl::enums::Message) -> bool {
    match message {
        tl::enums::Message::Message(message) => message.out,
        tl::enums::Message::Service(message) => message.out,
        tl::enums::Message::Empty(_) => false,
    }
}

fn message_date(message: &tl::enums::Message) -> i32 {
    match message {
        tl::enums::Message::Message(message) => message.date,
        tl::enums::Message::Service(message) => message.date,
        tl::enums::Message::Empty(_) => 0,
    }
}

fn public_channel_id_to_internal(peer_id: i64) -> Option<i64> {
    if peer_id <= -1_000_000_000_000 {
        return peer_id.checked_abs()?.checked_sub(1_000_000_000_000);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numeric_peer_supports_public_channel_ids() {
        assert_eq!(public_channel_id_to_internal(-1_001_234_567_890), Some(1_234_567_890));
        assert_eq!(public_channel_id_to_internal(1_234_567_890), None);
    }
}
