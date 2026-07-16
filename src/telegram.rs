use crate::Config;
use crate::backend::TelegramBackend;
use crate::domain::{Completeness, FileSendMode, MessagePage, SearchMessagesRequest};
use crate::fs_security::{
    directory_is_owner_only, restrict_private_directory, restrict_private_file,
};
use async_trait::async_trait;
use ferogram::tl;
use ferogram::{Client, InputMessage, ShutdownToken, SignInError};
use fs2::FileExt;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::fs::{File, OpenOptions};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use thiserror::Error;
use tokio::io::AsyncWriteExt;
use tokio::sync::Mutex;
use tokio::time::timeout;

const CONNECT_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Debug, Error)]
pub enum TelegramError {
    #[error("Telegram API error: {0}")]
    Telegram(String),
    #[error("Telegram rejected the send request: {0}")]
    SendRejected(String),
    #[error("Telegram send was not dispatched and can be retried: {0}")]
    SendRetryable(String),
    #[error("Telegram send delivery is uncertain: {0}")]
    DeliveryUncertain(String),
    #[error("Telegram rate limit requires waiting {seconds} seconds")]
    FloodWait { seconds: u64 },
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("No usable Telegram session found. Checked: {0}")]
    NoSession(String),
    #[error("Multiple Telegram sessions found; select one explicitly: {0}")]
    AmbiguousSession(String),
    #[error("Invalid Telegram session target: {0}")]
    InvalidSessionTarget(String),
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
    #[error("Message not found: {0}")]
    MessageNotFound(String),
    #[error("Message has no downloadable attachment: {0}")]
    AttachmentNotFound(String),
    #[error("Attachment exceeds the configured download limit of {max_bytes} bytes")]
    AttachmentTooLarge { max_bytes: u64 },
    #[error("Telegram rejected the login code as invalid or expired")]
    InvalidLoginCode,
    #[error("This phone number must be registered with an official Telegram client first")]
    SignUpRequired,
    #[error("Telegram rejected the two-step verification password")]
    InvalidPassword,
    #[error("Failed to save the Telegram session; the new authorization was revoked: {0}")]
    SessionSaveFailed(String),
    #[error(
        "Failed to save the Telegram session and could not prove cleanup of the new authorization: {0}"
    )]
    SessionSaveUncertain(String),
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct UserInfo {
    pub user_id: i64,
    pub first_name: String,
    pub username: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct MessageSummary {
    pub message_id: i32,
    pub chat_ref: Option<String>,
    pub sender_ref: Option<String>,
    pub outgoing: bool,
    pub text: Option<String>,
    pub date: i32,
    pub edit_date: Option<i32>,
    pub reply_to_message_id: Option<i32>,
    pub views: Option<i32>,
    pub forwards: Option<i32>,
    pub reply_count: Option<i32>,
    pub media_type: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct DialogSummary {
    #[serde(skip)]
    #[schemars(skip)]
    pub chat_id: i64,
    pub canonical_chat_id: String,
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

/// One live, exclusively locked Telegram connection used for interactive login.
///
/// The login code token and an optional 2FA challenge are intentionally kept in
/// this process and cannot be serialized or resumed by a different invocation.
pub struct LoginSession {
    client: Client,
    _shutdown: ShutdownToken,
    path: PathBuf,
    _lock: File,
}

/// Opaque challenge returned after Telegram has sent a login code.
pub struct LoginCodeChallenge(ferogram::LoginToken);

/// Opaque challenge returned when the account requires two-step verification.
pub struct LoginPasswordChallenge(Box<ferogram::PasswordToken>);

pub enum LoginCodeOutcome {
    Authorized,
    PasswordRequired(LoginPasswordChallenge),
}

impl LoginSession {
    pub fn path(&self) -> &Path {
        &self.path
    }

    pub async fn is_authorized(&self) -> Result<bool, TelegramError> {
        self.client
            .is_authorized()
            .await
            .map_err(|error| TelegramError::Telegram(error.to_string()))
    }

    pub async fn get_me(&self) -> Result<UserInfo, TelegramError> {
        let me = self
            .client
            .get_me()
            .await
            .map_err(|error| TelegramError::Telegram(error.to_string()))?;
        Ok(user_info(me))
    }

    pub async fn request_login_code(
        &self,
        phone: &str,
    ) -> Result<LoginCodeChallenge, TelegramError> {
        self.client
            .request_login_code(phone)
            .await
            .map(LoginCodeChallenge)
            .map_err(|error| TelegramError::Telegram(error.to_string()))
    }

    pub async fn sign_in(
        &self,
        challenge: &LoginCodeChallenge,
        code: &str,
    ) -> Result<LoginCodeOutcome, TelegramError> {
        match self.client.sign_in(&challenge.0, code).await {
            Ok(_) => Ok(LoginCodeOutcome::Authorized),
            Err(SignInError::PasswordRequired(challenge)) => {
                Ok(LoginCodeOutcome::PasswordRequired(LoginPasswordChallenge(challenge)))
            },
            Err(SignInError::InvalidCode) => Err(TelegramError::InvalidLoginCode),
            Err(SignInError::SignUpRequired) => Err(TelegramError::SignUpRequired),
            Err(SignInError::Other(error)) => Err(TelegramError::Telegram(error.to_string())),
        }
    }

    pub async fn check_password(
        &self,
        challenge: LoginPasswordChallenge,
        password: &[u8],
    ) -> Result<(), TelegramError> {
        self.client.check_password(*challenge.0, password).await.map(|_| ()).map_err(|error| {
            if error.is("PASSWORD_HASH_INVALID") {
                TelegramError::InvalidPassword
            } else {
                TelegramError::Telegram(error.to_string())
            }
        })
    }

    /// Persist a freshly created authorization. If persistence fails, revoke
    /// the server-side authorization and persist the cleared local state.
    pub async fn save_session_or_revoke(&self) -> Result<(), TelegramError> {
        let save_error = match self.persist_session().await {
            Ok(()) => return Ok(()),
            Err(error) => error,
        };

        match self.client.sign_out().await {
            Ok(_) => match self.persist_session().await {
                Ok(()) => Err(TelegramError::SessionSaveFailed(save_error.to_string())),
                Err(cleanup_error) => Err(TelegramError::SessionSaveUncertain(format!(
                    "save error: {save_error}; authorization was revoked, but saving cleared state failed: {cleanup_error}"
                ))),
            },
            Err(revoke_error) => Err(TelegramError::SessionSaveUncertain(format!(
                "save error: {save_error}; revocation failed: {revoke_error}"
            ))),
        }
    }

    async fn persist_session(&self) -> Result<(), String> {
        self.client.save_session().await.map_err(|error| error.to_string())?;
        restrict_private_file(&self.path).map_err(|error| error.to_string())
    }
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
        Ok((user_info(me), connected.path.clone()))
    }

    pub async fn resolve_chat_id(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
    ) -> Result<String, TelegramError> {
        let connected = self.connected(session_path).await?;
        let peer_ref = Self::resolve_peer_ref(&connected.client, chat_id).await?;
        let peer = connected
            .client
            .resolve(peer_ref.as_str())
            .await
            .map_err(|error| TelegramError::Telegram(error.to_string()))?;
        Ok(canonical_peer_ref(&peer))
    }

    /// Open one exclusive connection for the complete interactive login flow.
    /// Existing session selection stays fail-closed: multiple candidates
    /// require an explicit `session_path`; zero candidates create
    /// `<session_cache_dir>/default.session`.
    pub async fn begin_login(&self) -> Result<LoginSession, TelegramError> {
        let candidates = self.config.session_candidates()?;
        let selected_path = select_login_session_path(
            self.config.session_path.as_deref(),
            &self.config.session_cache_dir(),
            &candidates,
        )?;
        let selected_path = Self::prepare_login_target(&selected_path)?;
        let lock = Self::lock_session_file(&selected_path)?;
        let existed_before_connect = selected_path.is_file();
        let (client, shutdown) = self.connect_client(&selected_path).await?;
        if selected_path.is_file()
            && let Err(error) = restrict_private_file(&selected_path)
        {
            if !existed_before_connect {
                let _ = std::fs::remove_file(&selected_path);
            }
            return Err(TelegramError::Io(error));
        }
        Ok(LoginSession { client, _shutdown: shutdown, path: selected_path, _lock: lock })
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
            let canonical_chat_id = public_chat_ref(chat_id, &dialog);
            let chat_ref = username
                .as_ref()
                .map(|name| format!("@{name}"))
                .unwrap_or_else(|| canonical_chat_id.clone());
            let last = dialog.message.as_ref();
            dialogs.push(DialogSummary {
                chat_id,
                canonical_chat_id,
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
        self.history(session_path, chat_id, limit, offset_id, 0).await
    }

    pub async fn history(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        limit: usize,
        offset_id: Option<i32>,
        add_offset: i32,
    ) -> Result<Vec<MessageSummary>, TelegramError> {
        let connected = self.connected(session_path).await?;
        let peer = Self::resolve_peer_ref(&connected.client, chat_id).await?;
        let telegram_limit = i32::try_from(limit)
            .map_err(|_| TelegramError::InvalidPeer("read_chat.limit is too large".to_string()))?;
        let raw_messages = connected
            .client
            .get_message_history(
                peer.as_str(),
                telegram_limit,
                offset_id.unwrap_or_default(),
                add_offset,
            )
            .await
            .map_err(|error| TelegramError::Telegram(error.to_string()))?;

        let mut messages = Vec::new();
        for message in raw_messages {
            messages.push(summarize_message(&message));
            if messages.len() >= limit {
                break;
            }
        }

        Ok(messages)
    }

    pub async fn search_messages(
        &self,
        request: SearchMessagesRequest,
    ) -> Result<MessagePage, TelegramError> {
        let connected = self.connected(request.session_path).await?;
        let telegram_limit = i32::try_from(request.limit).map_err(|_| {
            TelegramError::InvalidPeer("search_messages.limit is too large".to_string())
        })?;
        let raw_messages = if let Some(chat_id) = &request.chat_id {
            let peer = Self::resolve_peer_ref(&connected.client, chat_id).await?;
            let mut search =
                connected.client.search(peer.as_str(), &request.query).limit(telegram_limit);
            if let Some(offset_id) = request.offset_id {
                search = search.offset_id(offset_id);
            }
            search
                .fetch(&connected.client)
                .await
                .map_err(|error| TelegramError::Telegram(error.to_string()))?
        } else {
            let mut search = connected.client.search_global(&request.query).limit(telegram_limit);
            if let Some(offset_id) = request.offset_id {
                search = search.offset_id(offset_id);
            }
            search
                .fetch(&connected.client)
                .await
                .map_err(|error| TelegramError::Telegram(error.to_string()))?
        };
        let messages = raw_messages.iter().map(summarize_message).collect::<Vec<_>>();
        let next_offset_id = request
            .chat_id
            .is_some()
            .then(|| {
                (messages.len() == request.limit).then(|| messages.last().map(|m| m.message_id))
            })
            .flatten()
            .flatten();
        let completeness = if request.chat_id.is_none() {
            Completeness::FirstPageOnly
        } else if next_offset_id.is_some() {
            Completeness::Page
        } else {
            Completeness::Complete
        };
        Ok(MessagePage { messages, next_offset_id, completeness })
    }

    pub async fn get_message(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        message_id: i32,
    ) -> Result<Option<MessageSummary>, TelegramError> {
        let connected = self.connected(session_path).await?;
        let peer = Self::resolve_peer_ref(&connected.client, chat_id).await?;
        let messages = connected
            .client
            .get_messages(peer.as_str(), &[message_id])
            .await
            .map_err(|error| TelegramError::Telegram(error.to_string()))?;
        Ok(messages.iter().find(|message| is_concrete_message(message)).map(summarize_message))
    }

    pub async fn list_attachments(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        message_id: i32,
    ) -> Result<Vec<crate::domain::AttachmentDescriptor>, TelegramError> {
        let connected = self.connected(session_path).await?;
        let peer = Self::resolve_peer_ref(&connected.client, chat_id).await?;
        let messages = connected
            .client
            .get_messages(peer.as_str(), &[message_id])
            .await
            .map_err(|error| TelegramError::Telegram(error.to_string()))?;
        let message = messages
            .iter()
            .find(|message| is_concrete_message(message))
            .ok_or_else(|| TelegramError::MessageNotFound(format!("{chat_id}:{message_id}")))?;
        Ok(message
            .media()
            .and_then(|media| attachment_descriptor(chat_id, message_id, media))
            .into_iter()
            .collect())
    }

    pub async fn download_attachment_to(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        message_id: i32,
        destination: &Path,
        max_bytes: u64,
    ) -> Result<u64, TelegramError> {
        let connected = self.connected(session_path).await?;
        let peer = Self::resolve_peer_ref(&connected.client, chat_id).await?;
        let messages = connected
            .client
            .get_messages(peer.as_str(), &[message_id])
            .await
            .map_err(|error| TelegramError::Telegram(error.to_string()))?;
        let message = messages
            .iter()
            .find(|message| is_concrete_message(message))
            .ok_or_else(|| TelegramError::MessageNotFound(format!("{chat_id}:{message_id}")))?;
        let media = message
            .media()
            .ok_or_else(|| TelegramError::AttachmentNotFound(format!("{chat_id}:{message_id}")))?;
        if attachment_descriptor(chat_id, message_id, media).is_none() {
            return Err(TelegramError::AttachmentNotFound(format!("{chat_id}:{message_id}")));
        }
        let mut download = connected
            .client
            .iter_download(media)
            .ok_or_else(|| TelegramError::AttachmentNotFound(format!("{chat_id}:{message_id}")))?;
        let mut file = tokio::fs::File::create(destination).await?;
        restrict_private_file(destination)?;
        let mut bytes_written = 0_u64;
        while let Some(chunk) =
            download.next().await.map_err(|error| TelegramError::Telegram(error.to_string()))?
        {
            let chunk_bytes = u64::try_from(chunk.len()).unwrap_or(u64::MAX);
            bytes_written = bytes_written
                .checked_add(chunk_bytes)
                .ok_or(TelegramError::AttachmentTooLarge { max_bytes })?;
            if bytes_written > max_bytes {
                return Err(TelegramError::AttachmentTooLarge { max_bytes });
            }
            file.write_all(&chunk).await?;
        }
        file.flush().await?;
        file.sync_all().await?;
        Ok(bytes_written)
    }

    pub async fn send_file(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        file_path: &Path,
        caption: &str,
        reply_to_message_id: Option<i32>,
        mode: FileSendMode,
    ) -> Result<i32, TelegramError> {
        let connected = self.connected(session_path).await?;
        let peer = Self::resolve_peer_ref(&connected.client, chat_id).await?;
        let uploaded = connected
            .client
            .upload_file(file_path)
            .await
            .map_err(|error| TelegramError::Telegram(error.to_string()))?;
        let media = match mode {
            FileSendMode::Auto => uploaded.as_auto_media(),
            FileSendMode::Document => uploaded.as_document_media(),
            FileSendMode::Photo => uploaded.as_photo_media(),
        };
        let options = InputMessage::text(caption).reply_to(reply_to_message_id);
        let sent = connected
            .client
            .send_file(peer.as_str(), media, &options)
            .await
            .map_err(send_invocation_error)?;
        Ok(sent.id())
    }

    pub async fn send_text_with_random_id(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        text: &str,
        reply_to_message_id: Option<i32>,
        random_id: i64,
    ) -> Result<i32, TelegramError> {
        let connected = self.connected(session_path).await?;
        let peer_ref = Self::resolve_peer_ref(&connected.client, chat_id).await?;
        let peer = connected
            .client
            .resolve(peer_ref.as_str())
            .await
            .map_err(|error| TelegramError::Telegram(error.to_string()))?;
        let input_peer = connected
            .client
            .resolve_to_input_peer(&peer)
            .await
            .map_err(|error| TelegramError::Telegram(error.to_string()))?;
        let reply_to = reply_to_message_id.map(|message_id| {
            tl::enums::InputReplyTo::Message(tl::types::InputReplyToMessage {
                reply_to_msg_id: message_id,
                top_msg_id: None,
                reply_to_peer_id: None,
                quote_text: None,
                quote_entities: None,
                quote_offset: None,
                monoforum_peer_id: None,
                todo_item_id: None,
                poll_option: None,
            })
        });
        let request = tl::functions::messages::SendMessage {
            no_webpage: false,
            silent: false,
            background: false,
            clear_draft: false,
            noforwards: false,
            update_stickersets_order: false,
            invert_media: false,
            allow_paid_floodskip: false,
            peer: input_peer,
            reply_to,
            message: text.to_string(),
            random_id,
            reply_markup: None,
            entities: None,
            schedule_date: None,
            schedule_repeat_period: None,
            send_as: None,
            quick_reply_shortcut: None,
            effect: None,
            allow_paid_stars: None,
            suggested_post: None,
            rich_message: None,
        };
        let updates = connected.client.invoke(&request).await.map_err(send_invocation_error)?;
        sent_message_id(&updates, random_id).ok_or_else(|| {
            TelegramError::DeliveryUncertain(
                "Telegram accepted send request but response contained no message ID".to_string(),
            )
        })
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
            .send_message(peer.as_str(), InputMessage::text(text))
            .await
            .map_err(send_invocation_error)?;
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
        let (client, shutdown) = self.connect_client(&selected_path).await?;
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

    async fn connect_client(
        &self,
        session_path: &Path,
    ) -> Result<(Client, ShutdownToken), TelegramError> {
        let mut builder = Client::builder()
            .api_id(self.config.api_id)
            .api_hash(self.config.api_hash.clone())
            .session(session_path);
        if let Some(proxy) = &self.config.proxy {
            builder = apply_proxy(builder, proxy)?;
        }
        let telegram_config =
            builder.build().map_err(|error| TelegramError::Telegram(error.to_string()))?;
        timeout(CONNECT_TIMEOUT, Client::connect(telegram_config))
            .await
            .map_err(|_| TelegramError::Telegram("connect timed out".to_string()))?
            .map_err(|error| TelegramError::Telegram(error.to_string()))
    }

    async fn resolve_session_path(
        &self,
        explicit_path: Option<PathBuf>,
    ) -> Result<PathBuf, TelegramError> {
        if let Some(path) = explicit_path {
            return if path.is_file() {
                let canonical = std::fs::canonicalize(&path)?;
                ensure_session_parent_private(&canonical)?;
                restrict_private_file(&canonical)?;
                Ok(canonical)
            } else {
                Err(TelegramError::NoSession(path.display().to_string()))
            };
        }
        let candidates = self.config.session_candidates()?;
        match candidates.as_slice() {
            [path] => {
                ensure_session_parent_private(path)?;
                restrict_private_file(path)?;
                Ok(path.clone())
            },
            [] => {
                Err(TelegramError::NoSession(self.config.session_cache_dir().display().to_string()))
            },
            many => Err(TelegramError::AmbiguousSession(
                many.iter().map(|path| path.display().to_string()).collect::<Vec<_>>().join(", "),
            )),
        }
    }

    fn lock_session_file(session_path: &Path) -> Result<File, TelegramError> {
        let lock_path = session_path.with_extension("session.lock");
        let lock = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&lock_path)?;
        restrict_private_file(&lock_path)?;
        lock.try_lock_exclusive().map_err(|error| {
            TelegramError::SessionLocked(format!("{} ({error})", lock_path.display()))
        })?;
        Ok(lock)
    }

    fn prepare_login_target(session_path: &Path) -> Result<PathBuf, TelegramError> {
        if session_path.exists() && !session_path.is_file() {
            return Err(TelegramError::InvalidSessionTarget(format!(
                "{} exists but is not a file",
                session_path.display()
            )));
        }
        if let Some(parent) = session_path.parent()
            && !parent.as_os_str().is_empty()
        {
            if parent.exists() && !parent.is_dir() {
                return Err(TelegramError::InvalidSessionTarget(format!(
                    "parent {} is not a directory",
                    parent.display()
                )));
            }
            let create_private_parent = !parent.exists();
            std::fs::create_dir_all(parent)?;
            if create_private_parent {
                restrict_private_directory(parent)?;
            }
        }
        if session_path.is_file() {
            let canonical = std::fs::canonicalize(session_path)?;
            ensure_session_parent_private(&canonical)?;
            restrict_private_file(&canonical)?;
            return Ok(canonical);
        }
        if std::fs::symlink_metadata(session_path)
            .is_ok_and(|metadata| metadata.file_type().is_symlink())
        {
            return Err(TelegramError::InvalidSessionTarget(format!(
                "{} is a dangling symbolic link",
                session_path.display()
            )));
        }
        let file_name = session_path.file_name().ok_or_else(|| {
            TelegramError::InvalidSessionTarget(format!(
                "{} has no session filename",
                session_path.display()
            ))
        })?;
        let parent = session_path
            .parent()
            .filter(|path| !path.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        let canonical_parent = std::fs::canonicalize(parent)?;
        if !directory_is_owner_only(&canonical_parent)? {
            return Err(TelegramError::InvalidSessionTarget(format!(
                "session directory {} must be owner-only (0700 on Unix)",
                canonical_parent.display()
            )));
        }
        Ok(canonical_parent.join(file_name))
    }

    async fn resolve_peer_ref(_client: &Client, identifier: &str) -> Result<String, TelegramError> {
        normalize_peer_ref(identifier)
    }
}

fn normalize_peer_ref(identifier: &str) -> Result<String, TelegramError> {
    let trimmed = identifier.trim();
    if trimmed.is_empty() {
        return Err(TelegramError::InvalidPeer("empty peer reference".to_string()));
    }
    let looks_numeric = trimmed.bytes().all(|byte| byte.is_ascii_digit())
        || trimmed.strip_prefix('-').is_some_and(|digits| {
            !digits.is_empty() && digits.bytes().all(|byte| byte.is_ascii_digit())
        });
    if looks_numeric {
        let peer_id = trimmed.parse::<i64>().map_err(|_| {
            TelegramError::InvalidPeer("numeric peer reference is outside i64 range".to_string())
        })?;
        if peer_id == 0 {
            return Err(TelegramError::InvalidPeer(
                "numeric peer reference must not be zero".to_string(),
            ));
        }
        return Ok(trimmed.to_string());
    }
    let username = trimmed.strip_prefix('@').unwrap_or(trimmed);
    if !username.is_empty()
        && username.len() <= 64
        && username.bytes().all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
    {
        return Ok(trimmed.to_string());
    }
    Err(TelegramError::InvalidPeer(
        "peer reference must be a Telegram username or canonical signed numeric chat ID"
            .to_string(),
    ))
}

#[async_trait]
impl TelegramBackend for TelegramClient {
    async fn get_me(
        &self,
        session_path: Option<PathBuf>,
    ) -> Result<(UserInfo, PathBuf), TelegramError> {
        TelegramClient::get_me(self, session_path).await
    }

    async fn resolve_chat_id(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
    ) -> Result<String, TelegramError> {
        TelegramClient::resolve_chat_id(self, session_path, chat_id).await
    }

    async fn list_dialogs(
        &self,
        session_path: Option<PathBuf>,
        limit: usize,
    ) -> Result<Vec<DialogSummary>, TelegramError> {
        TelegramClient::list_dialogs(self, session_path, limit).await
    }

    async fn history(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        limit: usize,
        offset_id: Option<i32>,
        add_offset: i32,
    ) -> Result<Vec<MessageSummary>, TelegramError> {
        TelegramClient::history(self, session_path, chat_id, limit, offset_id, add_offset).await
    }

    async fn search_messages(
        &self,
        request: SearchMessagesRequest,
    ) -> Result<MessagePage, TelegramError> {
        TelegramClient::search_messages(self, request).await
    }

    async fn get_message(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        message_id: i32,
    ) -> Result<Option<MessageSummary>, TelegramError> {
        TelegramClient::get_message(self, session_path, chat_id, message_id).await
    }

    async fn list_attachments(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        message_id: i32,
    ) -> Result<Vec<crate::domain::AttachmentDescriptor>, TelegramError> {
        TelegramClient::list_attachments(self, session_path, chat_id, message_id).await
    }

    async fn download_attachment_to(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        message_id: i32,
        destination: &Path,
        max_bytes: u64,
    ) -> Result<u64, TelegramError> {
        TelegramClient::download_attachment_to(
            self,
            session_path,
            chat_id,
            message_id,
            destination,
            max_bytes,
        )
        .await
    }

    async fn send_file(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        file_path: &Path,
        caption: &str,
        reply_to_message_id: Option<i32>,
        mode: FileSendMode,
    ) -> Result<i32, TelegramError> {
        TelegramClient::send_file(
            self,
            session_path,
            chat_id,
            file_path,
            caption,
            reply_to_message_id,
            mode,
        )
        .await
    }

    async fn send_text_with_random_id(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        text: &str,
        reply_to_message_id: Option<i32>,
        random_id: i64,
    ) -> Result<i32, TelegramError> {
        TelegramClient::send_text_with_random_id(
            self,
            session_path,
            chat_id,
            text,
            reply_to_message_id,
            random_id,
        )
        .await
    }

    async fn send_message(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        text: &str,
    ) -> Result<i32, TelegramError> {
        TelegramClient::send_message(self, session_path, chat_id, text).await
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

fn send_invocation_error(error: ferogram::InvocationError) -> TelegramError {
    if let Some(seconds) = error.flood_wait_seconds() {
        return TelegramError::FloodWait { seconds };
    }
    if error.is("RANDOM_ID_DUPLICATE") {
        return TelegramError::DeliveryUncertain(error.to_string());
    }
    let message = error.to_string();
    match error {
        ferogram::InvocationError::Rpc(_) => TelegramError::SendRejected(message),
        ferogram::InvocationError::StaleHash
        | ferogram::InvocationError::PeerNotCached(_)
        | ferogram::InvocationError::Migrate(_) => TelegramError::SendRetryable(message),
        ferogram::InvocationError::Io(_)
        | ferogram::InvocationError::Deserialize(_)
        | ferogram::InvocationError::Dropped => TelegramError::DeliveryUncertain(message),
        _ => TelegramError::DeliveryUncertain(message),
    }
}

fn select_login_session_path(
    explicit_path: Option<&Path>,
    cache_dir: &Path,
    candidates: &[PathBuf],
) -> Result<PathBuf, TelegramError> {
    if let Some(path) = explicit_path {
        return Ok(path.to_path_buf());
    }
    match candidates {
        [path] => Ok(path.clone()),
        [] => Ok(cache_dir.join("default.session")),
        many => Err(TelegramError::AmbiguousSession(
            many.iter().map(|path| path.display().to_string()).collect::<Vec<_>>().join(", "),
        )),
    }
}

fn ensure_session_parent_private(session_path: &Path) -> Result<(), TelegramError> {
    let parent = session_path.parent().ok_or_else(|| {
        TelegramError::InvalidSessionTarget(format!(
            "{} has no parent directory",
            session_path.display()
        ))
    })?;
    if !directory_is_owner_only(parent)? {
        return Err(TelegramError::InvalidSessionTarget(format!(
            "session directory {} must be owner-only (0700 on Unix)",
            parent.display()
        )));
    }
    Ok(())
}

fn user_info(me: tl::types::User) -> UserInfo {
    UserInfo {
        user_id: me.id,
        first_name: me.first_name.unwrap_or_default(),
        username: me.username,
    }
}

fn dialog_peer_id(dialog: &ferogram::Dialog) -> Option<i64> {
    match dialog.peer()? {
        tl::enums::Peer::User(peer) => Some(peer.user_id),
        tl::enums::Peer::Chat(peer) => Some(peer.chat_id),
        tl::enums::Peer::Channel(peer) => Some(peer.channel_id),
    }
}

fn public_chat_ref(chat_id: i64, dialog: &ferogram::Dialog) -> String {
    match dialog.peer() {
        Some(tl::enums::Peer::Channel(_)) => format!("-100{chat_id}"),
        Some(tl::enums::Peer::Chat(_)) => format!("-{chat_id}"),
        _ => chat_id.to_string(),
    }
}

fn canonical_peer_ref(peer: &tl::enums::Peer) -> String {
    match peer {
        tl::enums::Peer::User(peer) => peer.user_id.to_string(),
        tl::enums::Peer::Chat(peer) => format!("-{}", peer.chat_id),
        tl::enums::Peer::Channel(peer) => format!("-100{}", peer.channel_id),
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

fn summarize_message(message: &ferogram::update::IncomingMessage) -> MessageSummary {
    MessageSummary {
        message_id: message.id(),
        chat_ref: message.peer_id().map(canonical_peer_ref),
        sender_ref: message.sender_id().map(canonical_peer_ref),
        outgoing: message.outgoing(),
        text: message.text().map(ToOwned::to_owned),
        date: message.date(),
        edit_date: message.edit_date(),
        reply_to_message_id: message.reply_to_message_id(),
        views: message.view_count(),
        forwards: message.forward_count(),
        reply_count: message.reply_count(),
        media_type: message.media().map(media_type_name),
    }
}

fn is_concrete_message(message: &ferogram::update::IncomingMessage) -> bool {
    is_concrete_raw_message(&message.raw)
}

fn is_concrete_raw_message(message: &tl::enums::Message) -> bool {
    !matches!(message, tl::enums::Message::Empty(_))
}

fn media_type_name(media: &tl::enums::MessageMedia) -> String {
    match media {
        tl::enums::MessageMedia::Photo(_) => "photo",
        tl::enums::MessageMedia::Document(_) => "document",
        tl::enums::MessageMedia::Geo(_) | tl::enums::MessageMedia::GeoLive(_) => "geo",
        tl::enums::MessageMedia::Contact(_) => "contact",
        tl::enums::MessageMedia::Poll(_) => "poll",
        tl::enums::MessageMedia::Dice(_) => "dice",
        tl::enums::MessageMedia::WebPage(_) => "web_page",
        _ => "other",
    }
    .to_string()
}

fn attachment_descriptor(
    chat_id: &str,
    message_id: i32,
    media: &tl::enums::MessageMedia,
) -> Option<crate::domain::AttachmentDescriptor> {
    if let Some(document) = ferogram::Document::from_media(media) {
        return Some(crate::domain::AttachmentDescriptor {
            attachment_id: format!("{chat_id}:{message_id}:0"),
            chat_id: chat_id.to_string(),
            message_id,
            kind: "document".to_string(),
            file_name: document.file_name().map(ToOwned::to_owned),
            mime_type: Some(document.mime_type().to_string()),
            size_bytes: u64::try_from(document.size()).ok(),
            downloadable: true,
        });
    }
    if ferogram::Photo::from_media(media).is_some() {
        return Some(crate::domain::AttachmentDescriptor {
            attachment_id: format!("{chat_id}:{message_id}:0"),
            chat_id: chat_id.to_string(),
            message_id,
            kind: "photo".to_string(),
            file_name: Some(format!("telegram-photo-{message_id}.jpg")),
            mime_type: Some("image/jpeg".to_string()),
            size_bytes: ferogram::media::size_from_media(media).map(|size| size as u64),
            downloadable: true,
        });
    }
    None
}

fn sent_message_id(updates: &tl::enums::Updates, random_id: i64) -> Option<i32> {
    match updates {
        tl::enums::Updates::UpdateShortSentMessage(value) => Some(value.id),
        tl::enums::Updates::UpdateShortMessage(value) => Some(value.id),
        tl::enums::Updates::UpdateShortChatMessage(value) => Some(value.id),
        tl::enums::Updates::UpdateShort(value) => {
            sent_message_id_from_update(&value.update, random_id)
        },
        tl::enums::Updates::Combined(value) => {
            value.updates.iter().find_map(|update| sent_message_id_from_update(update, random_id))
        },
        tl::enums::Updates::Updates(value) => {
            value.updates.iter().find_map(|update| sent_message_id_from_update(update, random_id))
        },
        tl::enums::Updates::TooLong => None,
    }
}

fn sent_message_id_from_update(update: &tl::enums::Update, random_id: i64) -> Option<i32> {
    match update {
        tl::enums::Update::MessageId(value) if value.random_id == random_id => Some(value.id),
        tl::enums::Update::NewMessage(value) => Some(message_id(&value.message)),
        tl::enums::Update::NewChannelMessage(value) => Some(message_id(&value.message)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn login_session_selection_uses_explicit_target_even_when_cache_is_ambiguous() {
        let explicit = Path::new("custom/new.session");
        let candidates = vec![PathBuf::from("a.session"), PathBuf::from("b.session")];

        assert_eq!(
            select_login_session_path(Some(explicit), Path::new("sessions"), &candidates)
                .expect("explicit target"),
            explicit
        );
    }

    #[test]
    fn empty_message_placeholder_is_not_a_real_message() {
        let empty = tl::enums::Message::Empty(tl::types::MessageEmpty { id: 42, peer_id: None });
        assert!(!is_concrete_raw_message(&empty));
    }

    #[test]
    fn peer_parser_rejects_links_phones_and_invites() {
        for invalid in ["https://t.me/alex", "+14155550132", "tg://join?invite=secret"] {
            assert!(matches!(normalize_peer_ref(invalid), Err(TelegramError::InvalidPeer(_))));
        }
        assert_eq!(normalize_peer_ref("@alex").expect("username"), "@alex");
        assert_eq!(normalize_peer_ref("-202").expect("group"), "-202");
        assert_eq!(normalize_peer_ref("-100123").expect("channel"), "-100123");
    }

    #[cfg(unix)]
    #[test]
    fn session_parent_must_be_owner_only() {
        let temp = tempfile::tempdir().expect("tempdir");
        let public_dir = temp.path().join("public");
        std::fs::create_dir(&public_dir).expect("create public dir");
        std::fs::set_permissions(&public_dir, std::fs::Permissions::from_mode(0o755))
            .expect("set public permissions");
        let session = public_dir.join("account.session");
        std::fs::write(&session, b"fixture").expect("write session");

        assert!(matches!(
            ensure_session_parent_private(&session),
            Err(TelegramError::InvalidSessionTarget(_))
        ));
    }

    #[cfg(unix)]
    #[test]
    fn new_login_session_directory_is_created_owner_only() {
        let temp = tempfile::tempdir().expect("tempdir");
        let target = temp.path().join("private/session/account.session");
        let normalized =
            TelegramClient::prepare_login_target(&target).expect("prepare login target");
        let parent = normalized.parent().expect("session parent");
        assert_eq!(
            std::fs::metadata(parent).expect("metadata").permissions().mode() & 0o777,
            0o700
        );
    }

    #[test]
    fn login_session_selection_creates_stable_default_for_empty_cache() {
        assert_eq!(
            select_login_session_path(None, Path::new("sessions"), &[]).expect("default target"),
            PathBuf::from("sessions/default.session")
        );
    }

    #[test]
    fn login_session_selection_reuses_only_candidate() {
        let candidate = PathBuf::from("sessions/account.session");
        assert_eq!(
            select_login_session_path(
                None,
                Path::new("sessions"),
                std::slice::from_ref(&candidate)
            )
            .expect("single target"),
            candidate
        );
    }

    #[test]
    fn login_session_selection_fails_closed_for_multiple_candidates() {
        let candidates = vec![PathBuf::from("a.session"), PathBuf::from("b.session")];
        let error = select_login_session_path(None, Path::new("sessions"), &candidates)
            .expect_err("ambiguous target must fail");

        assert!(matches!(error, TelegramError::AmbiguousSession(_)));
        assert!(error.to_string().contains("a.session"));
        assert!(error.to_string().contains("b.session"));
    }
}
