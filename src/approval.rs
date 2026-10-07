#![cfg_attr(
    test,
    allow(
        clippy::arithmetic_side_effects,
        clippy::cast_possible_truncation,
        clippy::cast_possible_wrap,
        clippy::expect_used,
        clippy::indexing_slicing,
        clippy::panic,
        clippy::unwrap_used,
    )
)]

use crate::domain::{
    AccountEmojiStatusSpec, BusinessAwayMessageSummary, BusinessAwayScheduleSummary,
    BusinessChatLinkContent, BusinessChatLinkSummary, BusinessGreetingMessageSummary,
    BusinessIntroSpec, BusinessIntroSummary, BusinessLocationSummary, BusinessRecipientsSummary,
    BusinessWorkHoursConfig, BusinessWorkHoursSummary, DefaultReactionSpec, EmojiStatusKind,
    EmojiStatusSummary, InviteBatchProgress, InviteTargetState, MutationAction, MutationApproval,
    MutationApprovalState, OutboundSourceRelation, PreparedMessageSource,
    PreparedNonTelegramSource, PreparedOutboundSource, PreparedSendKind, PreparedSendTarget,
    ReactionIdentity, ReactionSpec, RichMediaSpec, SavedStarGiftRef, SendApproval,
    SendApprovalState, StarGiftSummary, TypingAction,
};
use crate::fs_security::{restrict_private_directory, restrict_private_file};
use async_trait::async_trait;
use chrono::{DateTime, Datelike, FixedOffset, Timelike, Utc};
use fs2::FileExt;
use reqwest::StatusCode;
use rusqlite::{Connection, OpenFlags, params};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use sha2::{Digest, Sha256};
use std::collections::VecDeque;
use std::fs::{File, OpenOptions};
use std::io;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};
use thiserror::Error;
use tokio::sync::{Mutex, Notify};
use uuid::Uuid;
use zeroize::Zeroizing;

const MAX_APPROVAL_CARD_UTF16: usize = 3_800;
const MAX_MEDIA_CAPTION_UTF16: usize = 1_024;
const MAX_CALLBACK_QUERY_ID_BYTES: usize = 256;
const MAX_CALLBACK_DATA_BYTES: usize = 64;
const APPROVE_PREFIX: &str = "tpa:a:";
const CANCEL_PREFIX: &str = "tpa:c:";
const MUTATION_APPROVE_PREFIX: &str = "tpm:a:";
const MUTATION_CANCEL_PREFIX: &str = "tpm:c:";

#[derive(Debug, Clone)]
pub struct ApprovalPolicy {
    pub approval_chat_id: i64,
    pub approval_user_id: i64,
    pub ttl_seconds: u64,
    pub poll_timeout_seconds: u64,
    /// Owner grant file: an explicit, time-boxed permission (peers + message
    /// budget + expiry) that lets an interactive session dispatch without a
    /// confirmation card. `None` keeps the card path exactly as it was.
    pub owner_grant_file: Option<std::path::PathBuf>,
}

impl ApprovalPolicy {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.approval_chat_id == 0 {
            return Err("approval_chat_id must not be zero");
        }
        if self.approval_user_id <= 0 {
            return Err("approval_user_id must be positive");
        }
        if !(60..=7 * 24 * 60 * 60).contains(&self.ttl_seconds) {
            return Err("ttl_seconds must be between 60 seconds and 7 days");
        }
        if !(1..=50).contains(&self.poll_timeout_seconds) {
            return Err("poll_timeout_seconds must be between 1 and 50 seconds");
        }
        if let Some(path) = &self.owner_grant_file {
            if !path.is_absolute() {
                return Err("owner_grant_file must be absolute");
            }
        }
        Ok(())
    }
}

impl Default for ApprovalPolicy {
    fn default() -> Self {
        Self {
            approval_chat_id: 0,
            approval_user_id: 0,
            ttl_seconds: 24 * 60 * 60,
            poll_timeout_seconds: 5,
            owner_grant_file: None,
        }
    }
}

/// An owner authorization recorded on this host, never inferred.
///
/// The owner may reply in chat "да, отправляй" while we are working together;
/// that permission becomes this file instead of a vibe: it names who granted
/// it, which peers it covers, how many messages remain, and when it dies.
/// Every use is journalled, the budget is decremented atomically, and an
/// expired or exhausted grant is simply ignored — so an autonomous contour
/// gains nothing unless the owner wrote a grant for exactly its target.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct OwnerGrant {
    pub granted_by: i64,
    pub granted_at_unix: i64,
    pub expires_at_unix: i64,
    pub remaining_messages: u32,
    #[serde(default)]
    pub allow_peers: Vec<String>,
    #[serde(default)]
    pub reason: String,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct OwnerGrantUse {
    pub granted_by: i64,
    pub target: String,
    pub message: String,
    pub remaining_messages: u32,
    pub expires_at_unix: i64,
    pub reason: String,
}

/// @BotFather is never grantable in any form (owner 2026-10-07): an autonomous
/// agent created and deleted two bots from the owner account on 2026-10-06 and
/// Telegram anti-abuse then froze those bots and reset the owner's sessions.
/// Every message to BotFather goes through the owner's approval card, including
/// a grant file written directly by hand.
const BOTFATHER_USERNAME: &str = "botfather";
const BOTFATHER_USER_ID: &str = "93372553";

/// Normalise a peer reference to a comparable identity: strip any scheme,
/// `www.`, `t.me/`/`telegram.me/` prefix, leading `@` and surrounding slashes,
/// repeating until stable, then lowercase. `@BotFather`,
/// `https://telegram.me/BotFather/` and `botfather` all collapse to the same
/// value, so this is the single authority both the grant and the MCP rely on.
fn normalize_peer(value: &str) -> String {
    let mut current = value.trim().to_string();
    loop {
        let mut next = current.trim().to_string();
        for prefix in ["https://", "http://", "www."] {
            if let Some(stripped) = strip_prefix_ignore_case(&next, prefix) {
                next = stripped.to_string();
                break;
            }
        }
        for prefix in ["telegram.me/", "t.me/"] {
            if let Some(stripped) = strip_prefix_ignore_case(&next, prefix) {
                next = stripped.to_string();
                break;
            }
        }
        next = next.trim_matches('/').trim_start_matches('@').trim().to_string();
        if next == current {
            return current.to_ascii_lowercase();
        }
        current = next;
    }
}

fn strip_prefix_ignore_case<'a>(value: &'a str, prefix: &str) -> Option<&'a str> {
    value
        .get(..prefix.len())
        .filter(|head| head.eq_ignore_ascii_case(prefix))
        .map(|_| &value[prefix.len()..])
}

/// `true` when a peer reference resolves to a denied identity: its normalised
/// form is the BotFather username or user id, a punctuation-folded spelling of
/// the username (`bot-father`, `bot_father`), or carries the BotFather user id
/// among its digits. Denied peers can be neither a grant target nor a grant
/// entry, so a grant can never cover BotFather.
fn is_denied_peer(value: &str) -> bool {
    let normalized = normalize_peer(value);
    if normalized.is_empty() {
        return false;
    }
    let folded: String = normalized.chars().filter(|c| c.is_ascii_alphanumeric()).collect();
    let digits: String = normalized.chars().filter(|c| c.is_ascii_digit()).collect();
    normalized == BOTFATHER_USERNAME
        || normalized == BOTFATHER_USER_ID
        || folded == BOTFATHER_USERNAME
        || digits.contains(BOTFATHER_USER_ID)
}

impl OwnerGrant {
    pub fn is_live(&self, now_unix: i64) -> bool {
        self.granted_by > 0 && self.remaining_messages > 0 && now_unix < self.expires_at_unix
    }

    pub fn allows(&self, peers: &[&str]) -> bool {
        peers.iter().filter(|peer| !peer.trim().is_empty()).any(|peer| {
            let wanted = normalize_peer(peer);
            !is_denied_peer(&wanted)
                && self.allow_peers.iter().any(|allowed| {
                    let candidate = normalize_peer(allowed);
                    !is_denied_peer(&candidate) && candidate == wanted
                })
        })
    }
}

/// Consume one message from a live owner grant, or `None` when there is no
/// grant, it expired, its budget ran out, or the peers are not covered.
pub fn consume_owner_grant(
    path: &std::path::Path,
    peers: &[&str],
    now_unix: i64,
) -> Option<OwnerGrantUse> {
    let raw = std::fs::read_to_string(path).ok()?;
    let mut grant: OwnerGrant = serde_json::from_str(&raw).ok()?;
    // Belt and braces: a denied peer (BotFather) never consumes a grant, even
    // if a hand-written grant lists it in `allow_peers`. This keeps the
    // invariant local to the consumer instead of relying on the writer.
    if peers.iter().any(|peer| is_denied_peer(peer)) {
        return None;
    }
    if !grant.is_live(now_unix) || !grant.allows(peers) {
        return None;
    }
    grant.remaining_messages = grant.remaining_messages.saturating_sub(1);
    let target = peers
        .iter()
        .find(|peer| !peer.trim().is_empty())
        .map(|peer| (*peer).to_string())
        .unwrap_or_default();
    let use_record = OwnerGrantUse {
        granted_by: grant.granted_by,
        target,
        message: String::new(),
        remaining_messages: grant.remaining_messages,
        expires_at_unix: grant.expires_at_unix,
        reason: grant.reason.clone(),
    };
    write_grant_atomically(path, &grant)?;
    journal_grant_use(path, &use_record);
    Some(use_record)
}

fn write_grant_atomically(path: &std::path::Path, grant: &OwnerGrant) -> Option<()> {
    let payload = serde_json::to_vec_pretty(grant).ok()?;
    let temp = path.with_extension("json.tmp");
    std::fs::write(&temp, payload).ok()?;
    std::fs::set_permissions(
        &temp,
        std::os::unix::fs::PermissionsExt::from_mode(0o600),
    )
    .ok()?;
    std::fs::rename(&temp, path).ok()?;
    Some(())
}

fn journal_grant_use(path: &std::path::Path, use_record: &OwnerGrantUse) {
    let log_path = path.with_extension("json.log");
    let line = match serde_json::to_string(use_record) {
        Ok(line) => line,
        Err(_) => return,
    };
    if std::fs::metadata(&log_path).is_err() {
        let _ = std::fs::write(&log_path, b"");
        let _ = std::fs::set_permissions(
            &log_path,
            std::os::unix::fs::PermissionsExt::from_mode(0o600),
        );
    }
    if let Ok(mut file) = std::fs::OpenOptions::new().append(true).open(&log_path) {
        use std::io::Write;
        let _ = writeln!(file, "{line}");
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApprovalAction {
    Approve,
    Cancel,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApprovalKind {
    Send,
    Mutation,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApprovalDecision {
    pub update_id: i64,
    pub callback_query_id: String,
    pub approval_id: String,
    pub kind: ApprovalKind,
    pub action: ApprovalAction,
    pub from_user_id: i64,
    pub chat_id: i64,
    pub message_id: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApprovalUpdate {
    pub update_id: i64,
    pub decision: Option<ApprovalDecision>,
}

#[derive(Debug, Clone)]
pub struct ApprovalCard {
    pub approval: SendApproval,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CardMediaKind {
    Photo,
    Video,
    Animation,
    Voice,
    Document,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CardMedia {
    pub path: PathBuf,
    pub mime: String,
    pub kind: CardMediaKind,
}

impl CardMedia {
    pub fn from_path(path: PathBuf, file_name: &str) -> Self {
        let extension = Path::new(file_name)
            .extension()
            .and_then(|value| value.to_str())
            .or_else(|| path.extension().and_then(|value| value.to_str()))
            .unwrap_or_default()
            .to_ascii_lowercase();
        let (mime, kind) = match extension.as_str() {
            "jpg" | "jpeg" => ("image/jpeg", CardMediaKind::Photo),
            "png" => ("image/png", CardMediaKind::Photo),
            "webp" => ("image/webp", CardMediaKind::Photo),
            "mp4" => ("video/mp4", CardMediaKind::Video),
            "webm" => ("video/webm", CardMediaKind::Video),
            "mov" => ("video/quicktime", CardMediaKind::Video),
            "m4v" => ("video/x-m4v", CardMediaKind::Video),
            "gif" => ("image/gif", CardMediaKind::Animation),
            "ogg" | "oga" => ("audio/ogg", CardMediaKind::Voice),
            "opus" => ("audio/opus", CardMediaKind::Voice),
            _ => ("application/octet-stream", CardMediaKind::Document),
        };
        Self { path, mime: mime.to_string(), kind }
    }

    fn api_method_and_field(&self) -> (&'static str, &'static str) {
        match self.kind {
            CardMediaKind::Photo => ("sendPhoto", "photo"),
            CardMediaKind::Video => ("sendVideo", "video"),
            CardMediaKind::Animation => ("sendAnimation", "animation"),
            CardMediaKind::Voice => ("sendVoice", "voice"),
            CardMediaKind::Document => ("sendDocument", "document"),
        }
    }
}

#[derive(Debug, Clone)]
pub struct MutationApprovalCard {
    pub approval: MutationApproval,
    pub invite_progress: Option<InviteBatchProgress>,
    pub media: Option<CardMedia>,
}

#[derive(Debug, Clone)]
pub struct ApprovalCardReceipt {
    pub message_id: i64,
}

pub type SentCard = ApprovalCardReceipt;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OperatorRichMessageReceipt {
    pub message_id: i64,
    pub chat_id: i64,
    pub bot_user_id: i64,
    pub bot_username: Option<String>,
    pub payload_sha256: String,
    pub provider_rich_message_sha256: String,
}

#[derive(Debug, Clone, Error)]
#[error("approval bot operation failed ({code})")]
pub struct ApprovalBotError {
    pub code: &'static str,
    pub delivery_uncertain: bool,
    pub message: Option<String>,
}
impl ApprovalBotError {
    fn known(code: &'static str) -> Self {
        Self { code, delivery_uncertain: false, message: None }
    }

    fn uncertain(code: &'static str) -> Self {
        Self { code, delivery_uncertain: true, message: None }
    }
}

#[async_trait]
pub trait ApprovalBot: Send + Sync + 'static {
    async fn send_operator_rich_message(
        &self,
        chat_id: i64,
        html: &str,
    ) -> Result<OperatorRichMessageReceipt, ApprovalBotError>;

    async fn send_operator_media_card(
        &self,
        chat_id: i64,
        media: &CardMedia,
        caption_html: &str,
        keyboard: Option<&InlineKeyboardMarkup>,
    ) -> Result<SentCard, ApprovalBotError>;

    async fn send_card(&self, card: &ApprovalCard)
    -> Result<ApprovalCardReceipt, ApprovalBotError>;
    async fn edit_card(&self, card: &ApprovalCard, message_id: i64)
    -> Result<(), ApprovalBotError>;

    async fn send_mutation_card(
        &self,
        card: &MutationApprovalCard,
    ) -> Result<ApprovalCardReceipt, ApprovalBotError>;

    async fn edit_mutation_card(
        &self,
        card: &MutationApprovalCard,
        message_id: i64,
    ) -> Result<(), ApprovalBotError>;

    async fn poll_updates(
        &self,
        offset: i64,
        timeout_seconds: u64,
    ) -> Result<Vec<ApprovalUpdate>, ApprovalBotError>;

    async fn answer_callback(
        &self,
        callback_query_id: &str,
        text: &str,
    ) -> Result<(), ApprovalBotError>;
}

#[derive(Clone)]
pub struct BotApiApprovalBot {
    client: reqwest::Client,
    api_base: String,
    token: Arc<Zeroizing<String>>,
    _update_lease: Option<Arc<BotApiUpdateLease>>,
}

impl BotApiApprovalBot {
    pub fn new(
        token: String,
        api_base: String,
        proxy: Option<String>,
    ) -> Result<Self, ApprovalBotError> {
        if token.trim().is_empty() {
            return Err(ApprovalBotError::known("BOT_TOKEN_EMPTY"));
        }
        let update_lease = acquire_bot_api_update_lease(&token)?;
        Self::new_inner(token, api_base, proxy, Some(update_lease))
    }

    pub fn new_outbound(
        token: String,
        api_base: String,
        proxy: Option<String>,
    ) -> Result<Self, ApprovalBotError> {
        Self::new_inner(token, api_base, proxy, None)
    }

    fn new_inner(
        token: String,
        api_base: String,
        proxy: Option<String>,
        update_lease: Option<Arc<BotApiUpdateLease>>,
    ) -> Result<Self, ApprovalBotError> {
        if token.trim().is_empty() {
            return Err(ApprovalBotError::known("BOT_TOKEN_EMPTY"));
        }
        let api_base = api_base.trim().trim_end_matches('/').to_string();
        if !(api_base.starts_with("https://") || api_base.starts_with("http://127.0.0.1")) {
            return Err(ApprovalBotError::known("BOT_API_BASE_INVALID"));
        }
        let mut client = reqwest::Client::builder().user_agent("telegram-personal-mcp/0.1");
        if let Some(proxy) = proxy.as_deref() {
            let proxy = approval_proxy_url(proxy)?;
            client = client.proxy(
                reqwest::Proxy::all(proxy)
                    .map_err(|_| ApprovalBotError::known("BOT_PROXY_INVALID"))?,
            );
        }
        let client =
            client.build().map_err(|_| ApprovalBotError::known("BOT_HTTP_CLIENT_INIT_FAILED"))?;
        Ok(Self {
            client,
            api_base,
            token: Arc::new(Zeroizing::new(token)),
            _update_lease: update_lease,
        })
    }

    fn endpoint(&self, method: &str) -> String {
        format!("{}/bot{}/{}", self.api_base, self.token.as_str(), method)
    }

    async fn post<T, R>(&self, method: &str, body: &T) -> Result<R, ApprovalBotError>
    where
        T: Serialize + Sync,
        R: DeserializeOwned,
    {
        let response = self
            .client
            .post(self.endpoint(method))
            .json(body)
            .send()
            .await
            .map_err(|_| ApprovalBotError::uncertain("BOT_API_TRANSPORT_UNCERTAIN"))?;
        let status = response.status();
        let envelope = response
            .json::<BotApiEnvelope<R>>()
            .await
            .map_err(|_| ApprovalBotError::uncertain("BOT_API_RESPONSE_INVALID"))?;
        if !status.is_success() || !envelope.ok {
            let mut error = ApprovalBotError::known(status_error_code(status));
            error.message = envelope.description;
            return Err(error);
        }
        envelope.result.ok_or_else(|| ApprovalBotError::uncertain("BOT_API_RESULT_MISSING"))
    }

    async fn post_media(
        &self,
        chat_id: i64,
        media: &CardMedia,
        caption_html: &str,
        keyboard: Option<&InlineKeyboardMarkup>,
    ) -> Result<BotMessageResult, ApprovalBotError> {
        let bytes = tokio::fs::read(&media.path)
            .await
            .map_err(|_| ApprovalBotError::known("BOT_MEDIA_READ_FAILED"))?;
        let file_name = media
            .path
            .file_name()
            .and_then(|value| value.to_str())
            .filter(|value| !value.is_empty())
            .unwrap_or("attachment");
        let part = reqwest::multipart::Part::bytes(bytes)
            .file_name(file_name.to_string())
            .mime_str(&media.mime)
            .map_err(|_| ApprovalBotError::known("BOT_MEDIA_MIME_INVALID"))?;
        let mut form = reqwest::multipart::Form::new()
            .text("chat_id", chat_id.to_string())
            .text("caption", caption_html.to_string())
            .text("parse_mode", "HTML")
            .part(media.api_method_and_field().1, part);
        if let Some(keyboard) = keyboard {
            let markup = serde_json::to_string(keyboard)
                .map_err(|_| ApprovalBotError::known("BOT_REPLY_MARKUP_INVALID"))?;
            form = form.text("reply_markup", markup);
        }
        let response = self
            .client
            .post(self.endpoint(media.api_method_and_field().0))
            .multipart(form)
            .send()
            .await
            .map_err(|_| ApprovalBotError::uncertain("BOT_API_TRANSPORT_UNCERTAIN"))?;
        let status = response.status();
        let envelope = response
            .json::<BotApiEnvelope<BotMessageResult>>()
            .await
            .map_err(|_| ApprovalBotError::uncertain("BOT_API_RESPONSE_INVALID"))?;
        if !status.is_success() || !envelope.ok {
            let mut error = ApprovalBotError::known(status_error_code(status));
            error.message = envelope.description;
            return Err(error);
        }
        envelope.result.ok_or_else(|| ApprovalBotError::uncertain("BOT_API_RESULT_MISSING"))
    }
}

#[derive(Clone)]
pub struct FanoutApprovalBot {
    outbound: BotApiApprovalBot,
    database_path: Arc<PathBuf>,
}

impl FanoutApprovalBot {
    pub fn new(
        token: String,
        api_base: String,
        proxy: Option<String>,
        database_path: PathBuf,
    ) -> Result<Self, ApprovalBotError> {
        let database_path = validate_fanout_database(&database_path)?;
        Ok(Self {
            outbound: BotApiApprovalBot::new_outbound(token, api_base, proxy)?,
            database_path: Arc::new(database_path),
        })
    }
}

fn approval_proxy_url(proxy: &str) -> Result<String, ApprovalBotError> {
    let proxy = proxy.trim();
    if proxy.is_empty() {
        return Err(ApprovalBotError::known("BOT_PROXY_INVALID"));
    }
    if proxy.starts_with("socks5h://") {
        return Ok(proxy.to_string());
    }
    if let Some(authority) = proxy.strip_prefix("socks5://") {
        if authority.is_empty() {
            return Err(ApprovalBotError::known("BOT_PROXY_INVALID"));
        }
        return Ok(format!("socks5h://{authority}"));
    }
    if !proxy.contains("://") && proxy.contains(':') {
        return Ok(format!("socks5h://{proxy}"));
    }
    Err(ApprovalBotError::known("BOT_PROXY_INVALID"))
}

#[derive(Debug)]
struct BotApiUpdateLease {
    file: File,
}

impl Drop for BotApiUpdateLease {
    fn drop(&mut self) {
        let _ = FileExt::unlock(&self.file);
    }
}

fn acquire_bot_api_update_lease(token: &str) -> Result<Arc<BotApiUpdateLease>, ApprovalBotError> {
    let runtime_dir = std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .ok_or_else(|| ApprovalBotError::known("BOT_API_UPDATE_LEASE_UNAVAILABLE"))?;
    acquire_bot_api_update_lease_in(token, &runtime_dir)
}

fn acquire_bot_api_update_lease_in(
    token: &str,
    runtime_dir: &Path,
) -> Result<Arc<BotApiUpdateLease>, ApprovalBotError> {
    if token.trim().is_empty() || !runtime_dir.is_absolute() || !runtime_dir.is_dir() {
        return Err(ApprovalBotError::known("BOT_API_UPDATE_LEASE_UNAVAILABLE"));
    }
    let lease_dir = runtime_dir.join("hermes-telegram-get-updates");
    std::fs::create_dir_all(&lease_dir)
        .map_err(|_| ApprovalBotError::known("BOT_API_UPDATE_LEASE_UNAVAILABLE"))?;
    restrict_private_directory(&lease_dir)
        .map_err(|_| ApprovalBotError::known("BOT_API_UPDATE_LEASE_UNAVAILABLE"))?;
    let digest = Sha256::digest(token.trim().as_bytes());
    let fingerprint = digest.iter().take(16).map(|byte| format!("{byte:02x}")).collect::<String>();
    let lease_path = lease_dir.join(format!("{fingerprint}.lock"));
    let file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(&lease_path)
        .map_err(|_| ApprovalBotError::known("BOT_API_UPDATE_LEASE_UNAVAILABLE"))?;
    restrict_private_file(&lease_path)
        .map_err(|_| ApprovalBotError::known("BOT_API_UPDATE_LEASE_UNAVAILABLE"))?;
    match file.try_lock_exclusive() {
        Ok(()) => Ok(Arc::new(BotApiUpdateLease { file })),
        Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
            Err(ApprovalBotError::known("BOT_API_UPDATE_LEASE_BUSY"))
        },
        Err(_) => Err(ApprovalBotError::known("BOT_API_UPDATE_LEASE_UNAVAILABLE")),
    }
}

fn validate_fanout_database(path: &Path) -> Result<PathBuf, ApprovalBotError> {
    if !path.is_absolute() {
        return Err(ApprovalBotError::known("FANOUT_PATH_INVALID"));
    }
    let path =
        path.canonicalize().map_err(|_| ApprovalBotError::known("FANOUT_PATH_UNAVAILABLE"))?;
    let metadata =
        std::fs::metadata(&path).map_err(|_| ApprovalBotError::known("FANOUT_PATH_UNAVAILABLE"))?;
    if !metadata.is_file() || metadata.permissions().mode() & 0o077 != 0 {
        return Err(ApprovalBotError::known("FANOUT_PATH_INSECURE"));
    }
    let connection = open_fanout_database(&path)?;
    connection
        .prepare(
            "SELECT update_id,from_user_id,chat_id,message_id,callback_query_id,callback_data \
             FROM telegram_ingress_updates WHERE kind='callback' AND update_id>=?1 \
             ORDER BY update_id LIMIT 20",
        )
        .map_err(|_| ApprovalBotError::known("FANOUT_SCHEMA_INVALID"))?;
    Ok(path)
}

fn open_fanout_database(path: &Path) -> Result<Connection, ApprovalBotError> {
    let connection = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)
        .map_err(|_| ApprovalBotError::uncertain("FANOUT_READ_UNCERTAIN"))?;
    connection
        .busy_timeout(Duration::from_secs(5))
        .map_err(|_| ApprovalBotError::uncertain("FANOUT_READ_UNCERTAIN"))?;
    let schema_version = connection
        .query_row(
            "SELECT schema_version FROM telegram_ingress_runtime WHERE singleton=1",
            [],
            |row| row.get::<_, i64>(0),
        )
        .map_err(|_| ApprovalBotError::known("FANOUT_SCHEMA_INVALID"))?;
    if schema_version != 1 {
        return Err(ApprovalBotError::known("FANOUT_SCHEMA_INVALID"));
    }
    Ok(connection)
}

fn poll_fanout_updates(
    database_path: &Path,
    offset: i64,
    timeout_seconds: u64,
) -> Result<Vec<ApprovalUpdate>, ApprovalBotError> {
    let connection = open_fanout_database(database_path)?;
    let timeout = Duration::from_secs(timeout_seconds);
    let started = Instant::now();
    loop {
        let updates = read_fanout_updates(&connection, offset)?;
        if !updates.is_empty() || started.elapsed() >= timeout {
            return Ok(updates);
        }
        let remaining = timeout.saturating_sub(started.elapsed());
        std::thread::sleep(remaining.min(Duration::from_millis(250)));
    }
}

fn read_fanout_updates(
    connection: &Connection,
    offset: i64,
) -> Result<Vec<ApprovalUpdate>, ApprovalBotError> {
    let mut statement = connection
        .prepare(
            "SELECT update_id,from_user_id,chat_id,message_id,callback_query_id,callback_data \
             FROM telegram_ingress_updates WHERE kind='callback' AND update_id>=?1 \
             ORDER BY update_id LIMIT 20",
        )
        .map_err(|_| ApprovalBotError::uncertain("FANOUT_READ_UNCERTAIN"))?;
    let rows = statement
        .query_map(params![offset.max(0)], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, i64>(3)?,
                row.get::<_, Option<String>>(4)?,
                row.get::<_, Option<String>>(5)?,
            ))
        })
        .map_err(|_| ApprovalBotError::uncertain("FANOUT_READ_UNCERTAIN"))?;
    let mut updates = Vec::new();
    for row in rows {
        let (update_id, from_user_id, chat_id, message_id, callback_query_id, callback_data) =
            row.map_err(|_| ApprovalBotError::uncertain("FANOUT_READ_UNCERTAIN"))?;
        let decision = callback_query_id.zip(callback_data).and_then(|(callback_id, data)| {
            if update_id < 0
                || callback_id.is_empty()
                || callback_id.len() > MAX_CALLBACK_QUERY_ID_BYTES
                || data.is_empty()
                || data.len() > MAX_CALLBACK_DATA_BYTES
                || from_user_id <= 0
                || chat_id == 0
                || message_id <= 0
            {
                return None;
            }
            let (kind, action, approval_id) = parse_callback_data(&data)?;
            Some(ApprovalDecision {
                update_id,
                callback_query_id: callback_id,
                approval_id,
                kind,
                action,
                from_user_id,
                chat_id,
                message_id,
            })
        });
        updates.push(ApprovalUpdate { update_id, decision });
    }
    Ok(updates)
}

#[async_trait]
impl ApprovalBot for FanoutApprovalBot {
    async fn send_operator_rich_message(
        &self,
        chat_id: i64,
        html: &str,
    ) -> Result<OperatorRichMessageReceipt, ApprovalBotError> {
        self.outbound.send_operator_rich_message(chat_id, html).await
    }

    async fn send_operator_media_card(
        &self,
        chat_id: i64,
        media: &CardMedia,
        caption_html: &str,
        keyboard: Option<&InlineKeyboardMarkup>,
    ) -> Result<SentCard, ApprovalBotError> {
        self.outbound.send_operator_media_card(chat_id, media, caption_html, keyboard).await
    }

    async fn send_card(
        &self,
        card: &ApprovalCard,
    ) -> Result<ApprovalCardReceipt, ApprovalBotError> {
        self.outbound.send_card(card).await
    }

    async fn edit_card(
        &self,
        card: &ApprovalCard,
        message_id: i64,
    ) -> Result<(), ApprovalBotError> {
        self.outbound.edit_card(card, message_id).await
    }

    async fn send_mutation_card(
        &self,
        card: &MutationApprovalCard,
    ) -> Result<ApprovalCardReceipt, ApprovalBotError> {
        self.outbound.send_mutation_card(card).await
    }

    async fn edit_mutation_card(
        &self,
        card: &MutationApprovalCard,
        message_id: i64,
    ) -> Result<(), ApprovalBotError> {
        self.outbound.edit_mutation_card(card, message_id).await
    }

    async fn poll_updates(
        &self,
        offset: i64,
        timeout_seconds: u64,
    ) -> Result<Vec<ApprovalUpdate>, ApprovalBotError> {
        let database_path = Arc::clone(&self.database_path);
        tokio::task::spawn_blocking(move || {
            poll_fanout_updates(database_path.as_path(), offset, timeout_seconds)
        })
        .await
        .map_err(|_| ApprovalBotError::uncertain("FANOUT_READ_UNCERTAIN"))?
    }

    async fn answer_callback(
        &self,
        callback_query_id: &str,
        text: &str,
    ) -> Result<(), ApprovalBotError> {
        self.outbound.answer_callback(callback_query_id, text).await
    }
}

#[async_trait]
impl ApprovalBot for BotApiApprovalBot {
    async fn send_operator_rich_message(
        &self,
        chat_id: i64,
        html: &str,
    ) -> Result<OperatorRichMessageReceipt, ApprovalBotError> {
        let identity: BotIdentityResult = self.post("getMe", &serde_json::json!({})).await?;
        if !identity.is_bot || identity.id <= 0 {
            return Err(ApprovalBotError::known("BOT_IDENTITY_INVALID"));
        }
        let response: BotRichMessageResult = self
            .post(
                "sendRichMessage",
                &SendRichMessageRequest {
                    chat_id,
                    rich_message: RichMessageRequest { html, skip_entity_detection: false },
                },
            )
            .await?;
        if response.message_id <= 0 {
            return Err(ApprovalBotError::uncertain("BOT_MESSAGE_ID_INVALID"));
        }
        if response.chat.id != chat_id {
            return Err(ApprovalBotError::uncertain("BOT_MESSAGE_CHAT_MISMATCH"));
        }
        let rich_message = response
            .rich_message
            .filter(|value| !value.is_null())
            .ok_or_else(|| ApprovalBotError::uncertain("BOT_RICH_MESSAGE_READBACK_MISSING"))?;
        let provider_bytes = serde_json::to_vec(&rich_message)
            .map_err(|_| ApprovalBotError::uncertain("BOT_RICH_MESSAGE_READBACK_INVALID"))?;
        Ok(OperatorRichMessageReceipt {
            message_id: response.message_id,
            chat_id,
            bot_user_id: identity.id,
            bot_username: identity.username,
            payload_sha256: format!("{:x}", Sha256::digest(html.as_bytes())),
            provider_rich_message_sha256: format!("{:x}", Sha256::digest(provider_bytes)),
        })
    }

    async fn send_operator_media_card(
        &self,
        chat_id: i64,
        media: &CardMedia,
        caption_html: &str,
        keyboard: Option<&InlineKeyboardMarkup>,
    ) -> Result<SentCard, ApprovalBotError> {
        if chat_id == 0 || caption_html.encode_utf16().count() > 1_024 {
            return Err(ApprovalBotError::known("BOT_MEDIA_CARD_INVALID"));
        }
        let response = self.post_media(chat_id, media, caption_html, keyboard).await?;
        if response.message_id <= 0 {
            return Err(ApprovalBotError::uncertain("BOT_MESSAGE_ID_INVALID"));
        }
        Ok(SentCard { message_id: response.message_id })
    }

    async fn send_card(
        &self,
        card: &ApprovalCard,
    ) -> Result<ApprovalCardReceipt, ApprovalBotError> {
        let text = render_card(card)?;
        let response: BotMessageResult = self
            .post(
                "sendMessage",
                &SendMessageRequest {
                    chat_id: card.approval.approval_chat_id,
                    text,
                    parse_mode: Some("HTML"),
                    link_preview_options: LinkPreviewOptions { is_disabled: true },
                    reply_markup: keyboard_for(&card.approval),
                    reply_parameters: None,
                },
            )
            .await?;
        if response.message_id <= 0 {
            return Err(ApprovalBotError::uncertain("BOT_MESSAGE_ID_INVALID"));
        }
        Ok(ApprovalCardReceipt { message_id: response.message_id })
    }

    async fn edit_card(
        &self,
        card: &ApprovalCard,
        message_id: i64,
    ) -> Result<(), ApprovalBotError> {
        if message_id <= 0 {
            return Err(ApprovalBotError::known("BOT_MESSAGE_ID_INVALID"));
        }
        let text = render_card(card)?;
        let _: serde_json::Value = self
            .post(
                "editMessageText",
                &EditMessageRequest {
                    chat_id: card.approval.approval_chat_id,
                    message_id,
                    text,
                    parse_mode: Some("HTML"),
                    reply_markup: keyboard_for(&card.approval),
                },
            )
            .await?;
        Ok(())
    }

    async fn send_mutation_card(
        &self,
        card: &MutationApprovalCard,
    ) -> Result<ApprovalCardReceipt, ApprovalBotError> {
        let text = render_mutation_card(card)?;
        if let Some(media) = card.media.as_ref() {
            let keyboard = keyboard_for_mutation(&card.approval);
            if mutation_card_is_media_preview(&text) {
                let response = self
                    .post_media(card.approval.approval_chat_id, media, &text, Some(&keyboard))
                    .await?;
                if response.message_id <= 0 {
                    return Err(ApprovalBotError::uncertain("BOT_MESSAGE_ID_INVALID"));
                }
                return Ok(ApprovalCardReceipt { message_id: response.message_id });
            }
            let preview_caption =
                format!("Предпросмотр: {}", escape_html(&card_preview_file_name(card)));
            let preview = self
                .post_media(card.approval.approval_chat_id, media, &preview_caption, None)
                .await?;
            if preview.message_id <= 0 {
                return Err(ApprovalBotError::uncertain("BOT_MESSAGE_ID_INVALID"));
            }
            let response: BotMessageResult = self
                .post(
                    "sendMessage",
                    &SendMessageRequest {
                        chat_id: card.approval.approval_chat_id,
                        text,
                        parse_mode: mutation_card_parse_mode(&card.approval.action),
                        link_preview_options: LinkPreviewOptions { is_disabled: true },
                        reply_markup: keyboard,
                        reply_parameters: Some(ReplyParameters {
                            message_id: preview.message_id,
                        }),
                    },
                )
                .await?;
            if response.message_id <= 0 {
                return Err(ApprovalBotError::uncertain("BOT_MESSAGE_ID_INVALID"));
            }
            return Ok(ApprovalCardReceipt { message_id: response.message_id });
        }
        let response: BotMessageResult = self
            .post(
                "sendMessage",
                &SendMessageRequest {
                    chat_id: card.approval.approval_chat_id,
                    text,
                    parse_mode: mutation_card_parse_mode(&card.approval.action),
                    link_preview_options: LinkPreviewOptions { is_disabled: true },
                    reply_markup: keyboard_for_mutation(&card.approval),
                    reply_parameters: None,
                },
            )
            .await?;
        if response.message_id <= 0 {
            return Err(ApprovalBotError::uncertain("BOT_MESSAGE_ID_INVALID"));
        }
        Ok(ApprovalCardReceipt { message_id: response.message_id })
    }

    async fn edit_mutation_card(
        &self,
        card: &MutationApprovalCard,
        message_id: i64,
    ) -> Result<(), ApprovalBotError> {
        if message_id <= 0 {
            return Err(ApprovalBotError::known("BOT_MESSAGE_ID_INVALID"));
        }
        let text = render_mutation_card(card)?;
        let edit = EditMessageRequest {
            chat_id: card.approval.approval_chat_id,
            message_id,
            text: text.clone(),
            parse_mode: mutation_card_parse_mode(&card.approval.action),
            reply_markup: keyboard_for_mutation(&card.approval),
        };
        match self.post::<EditMessageRequest, serde_json::Value>("editMessageText", &edit).await {
            Ok(_) => Ok(()),
            Err(error) if is_no_text_edit_error(&error) => {
                let _: serde_json::Value = self
                    .post(
                        "editMessageCaption",
                        &EditMessageCaptionRequest {
                            chat_id: card.approval.approval_chat_id,
                            message_id,
                            caption: text,
                            parse_mode: mutation_card_parse_mode(&card.approval.action),
                            reply_markup: keyboard_for_mutation(&card.approval),
                        },
                    )
                    .await?;
                Ok(())
            },
            Err(error) => Err(error),
        }
    }

    async fn poll_updates(
        &self,
        offset: i64,
        timeout_seconds: u64,
    ) -> Result<Vec<ApprovalUpdate>, ApprovalBotError> {
        let response: Vec<RawUpdate> = self
            .post(
                "getUpdates",
                &GetUpdatesRequest {
                    offset,
                    timeout: timeout_seconds,
                    limit: 20,
                    allowed_updates: ["callback_query"],
                },
            )
            .await?;
        let mut updates = response
            .into_iter()
            .filter(|update| update.update_id >= 0)
            .map(sanitize_update)
            .collect::<Vec<_>>();
        updates.sort_by_key(|update| update.update_id);
        Ok(updates)
    }

    async fn answer_callback(
        &self,
        callback_query_id: &str,
        text: &str,
    ) -> Result<(), ApprovalBotError> {
        if callback_query_id.is_empty()
            || callback_query_id.len() > MAX_CALLBACK_QUERY_ID_BYTES
            || text.encode_utf16().count() > 180
        {
            return Err(ApprovalBotError::known("CALLBACK_ANSWER_INVALID"));
        }
        let _: bool = self
            .post("answerCallbackQuery", &AnswerCallbackRequest { callback_query_id, text })
            .await?;
        Ok(())
    }
}

fn status_error_code(status: StatusCode) -> &'static str {
    match status.as_u16() {
        400 => "BOT_API_BAD_REQUEST",
        401 => "BOT_API_UNAUTHORIZED",
        403 => "BOT_API_FORBIDDEN",
        409 => "BOT_API_CONFLICT",
        429 => "BOT_API_RATE_LIMITED",
        500..=599 => "BOT_API_SERVER_ERROR",
        _ => "BOT_API_REJECTED",
    }
}

fn render_card(card: &ApprovalCard) -> Result<String, ApprovalBotError> {
    let approval = &card.approval;
    let (title, note) = match approval.state {
        SendApprovalState::DispatchingCard | SendApprovalState::Pending => {
            ("✉️ <b>Отправить сообщение?</b>", "После нажатия «Отправить» сообщение уйдёт сразу.")
        },
        SendApprovalState::CardDeliveryUncertain => (
            "⚠️ <b>Карточка могла не доставиться</b>",
            "Не создавайте повторную отправку: система сначала сверит состояние.",
        ),
        SendApprovalState::Sending => {
            ("⏳ <b>Отправляю сообщение…</b>", "Решение принято. Ожидаю подтверждение Telegram.")
        },
        SendApprovalState::DeliveryUncertain => (
            "⚠️ <b>Результат отправки уточняется</b>",
            "Telegram не подтвердил результат. Автоматический повтор возможен только после безопасной сверки.",
        ),
        SendApprovalState::Sent => {
            ("✅ <b>Сообщение отправлено</b>", "Telegram подтвердил отправку.")
        },
        SendApprovalState::Cancelled => {
            ("Отправка отменена", "Сообщение адресату не отправлялось.")
        },
        SendApprovalState::Expired => {
            ("Подтверждение истекло", "Сообщение адресату не отправлялось.")
        },
        SendApprovalState::Failed => {
            ("⚠️ <b>Не удалось завершить запрос</b>", "Сообщение не считается отправленным.")
        },
    };
    let display_name = escape_html(&approval.target.display_name);
    let username = approval
        .target
        .username
        .as_deref()
        .filter(|username| !username.trim().is_empty())
        .map(|username| format!(" · @{}", escape_html(username.trim_start_matches('@'))))
        .unwrap_or_default();
    let destination_detail = if approval.target.kind == "saved_messages" {
        String::new()
    } else if username.is_empty() {
        format!("\n<code>{}</code>", escape_html(&approval.chat_id))
    } else {
        String::new()
    };
    let reply_detail = approval
        .reply_to_message_id
        .map(|message_id| format!("\n\n↩️ Ответ на сообщение {message_id}"))
        .unwrap_or_default();
    let format_detail = matches!(approval.kind, PreparedSendKind::RichMarkdown)
        .then_some("\n\nФормат: Rich Text")
        .unwrap_or_default();
    let source_detail = outbound_source_detail(approval.source.as_ref());
    let receipt_detail = if approval.state == SendApprovalState::Sent {
        approval
            .target_message_id
            .map(|message_id| format!(" · message {message_id}"))
            .unwrap_or_default()
    } else {
        String::new()
    };
    let text = format!(
        "{title}\n\n<b>{display_name}</b>{username}{destination_detail}{source_detail}\n\n<blockquote>{}</blockquote>{reply_detail}{format_detail}\n\n{note}{receipt_detail}",
        escape_html(&approval.text),
    );
    if text.encode_utf16().count() > MAX_APPROVAL_CARD_UTF16 {
        return Err(ApprovalBotError::known("APPROVAL_CARD_TOO_LONG"));
    }
    Ok(text)
}

fn escape_html(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

fn reaction_label(reaction: Option<&ReactionSpec>) -> String {
    match reaction {
        None | Some(ReactionSpec::Remove) => "нет".to_string(),
        Some(ReactionSpec::Emoji { emoji, big, add_to_recent }) => {
            format!("emoji {emoji} (big: {big}, add_to_recent: {add_to_recent})")
        },
        Some(ReactionSpec::CustomEmoji { document_id, big, add_to_recent }) => {
            format!("custom_emoji {document_id} (big: {big}, add_to_recent: {add_to_recent})")
        },
    }
}

fn reaction_identity_label(reaction: Option<&ReactionIdentity>) -> String {
    match reaction {
        None => "не задана".to_string(),
        Some(ReactionIdentity::Empty) => "empty".to_string(),
        Some(ReactionIdentity::Emoji { emoji }) => format!("emoji {emoji}"),
        Some(ReactionIdentity::CustomEmoji { document_id }) => {
            format!("custom_emoji {document_id}")
        },
        Some(ReactionIdentity::Paid) => "paid".to_string(),
    }
}

fn default_reaction_label(reaction: &DefaultReactionSpec) -> String {
    match reaction {
        DefaultReactionSpec::Emoji { emoji } => format!("emoji {emoji}"),
        DefaultReactionSpec::CustomEmoji { document_id } => {
            format!("custom_emoji {document_id}")
        },
    }
}

fn story_report_result_label(report: &crate::domain::StoryReportResult) -> String {
    match report {
        crate::domain::StoryReportResult::ChooseOption { title, options } => {
            let options = options
                .iter()
                .take(20)
                .map(|option| format!("{} (option_hex: {})", option.text, option.option_hex))
                .collect::<Vec<_>>()
                .join("\n");
            format!("Telegram запрашивает выбор: {title}\n{options}")
        },
        crate::domain::StoryReportResult::AddComment { optional, option_hex } => format!(
            "Telegram запрашивает комментарий (optional={optional}, option_hex={option_hex})"
        ),
        crate::domain::StoryReportResult::Reported => {
            "Telegram подтвердил отправку жалобы".to_string()
        },
    }
}

fn poll_options_label(options: &[crate::domain::PollOptionBinding]) -> String {
    options
        .iter()
        .map(|option| {
            format!(
                "{}. {} (option_hex: {})",
                option.index.saturating_add(1),
                option.text,
                option.option_hex
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn poll_selection_label(
    options: &[crate::domain::PollOptionBinding],
    selected_indices: &[usize],
) -> String {
    if selected_indices.is_empty() {
        return "нет".to_string();
    }
    selected_indices
        .iter()
        .map(|index| {
            options.iter().find(|option| option.index == *index).map_or_else(
                || format!("{}: [вариант отсутствует]", index.saturating_add(1)),
                |option| {
                    format!(
                        "{}. {} (option_hex: {})",
                        option.index.saturating_add(1),
                        option.text,
                        option.option_hex
                    )
                },
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn mutation_dialog_result_text(result: &Option<crate::domain::MutationResult>) -> String {
    match result {
        Some(crate::domain::MutationResult::DialogStateUpdated { chat_id, state }) => format!(
            "{chat_id}: pinned={}, unread_mark={}, archived={}, folder_id={:?}, unread_count={}",
            state.pinned, state.unread_mark, state.archived, state.folder_id, state.unread_count
        ),
        _ => "—".to_string(),
    }
}

fn privacy_rules_label(rules: &[crate::domain::PrivacyRuleSummary]) -> String {
    rules
        .iter()
        .enumerate()
        .map(|(index, rule)| {
            let mut suffix = Vec::new();
            if !rule.user_ids.is_empty() {
                suffix.push(format!(
                    "users=[{}]",
                    rule.user_ids.iter().map(i64::to_string).collect::<Vec<_>>().join(", ")
                ));
            }
            if !rule.chat_ids.is_empty() {
                suffix.push(format!(
                    "chats=[{}]",
                    rule.chat_ids.iter().map(i64::to_string).collect::<Vec<_>>().join(", ")
                ));
            }
            if suffix.is_empty() {
                format!("{}. {}", index.saturating_add(1), rule.kind)
            } else {
                format!("{}. {} ({})", index.saturating_add(1), rule.kind, suffix.join(", "))
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn notification_sound_label(sound: Option<&crate::domain::NotificationSoundSummary>) -> String {
    match sound {
        None => "не задан".to_string(),
        Some(sound) if sound.kind == "local" => format!(
            "local(title={}, data={})",
            sound.title.as_deref().unwrap_or("не задан"),
            sound.data.as_deref().unwrap_or("не задан")
        ),
        Some(sound) if sound.kind == "ringtone" => {
            format!("ringtone({})", sound.ringtone_id.unwrap_or_default())
        },
        Some(sound) => sound.kind.clone(),
    }
}

fn notification_settings_label(settings: &crate::domain::NotificationSettingsSummary) -> String {
    format!(
        "show_previews={:?}, silent={:?}, mute_until={:?}\nsound ios={}, android={}, other={}\nstories_muted={:?}, stories_hide_sender={:?}\nstories_sound ios={}, android={}, other={}",
        settings.show_previews,
        settings.silent,
        settings.mute_until_unix,
        notification_sound_label(settings.ios_sound.as_ref()),
        notification_sound_label(settings.android_sound.as_ref()),
        notification_sound_label(settings.other_sound.as_ref()),
        settings.stories_muted,
        settings.stories_hide_sender,
        notification_sound_label(settings.stories_ios_sound.as_ref()),
        notification_sound_label(settings.stories_android_sound.as_ref()),
        notification_sound_label(settings.stories_other_sound.as_ref())
    )
}

fn notification_patch_label(patch: &crate::domain::NotificationSettingsPatch) -> String {
    format!(
        "show_previews={:?}, silent={:?}, mute_until={:?}\nsound={}\nstories_muted={:?}, stories_hide_sender={:?}\nstories_sound={}",
        patch.show_previews,
        patch.silent,
        patch.mute_until_unix,
        notification_sound_label(patch.sound.as_ref()),
        patch.stories_muted,
        patch.stories_hide_sender,
        notification_sound_label(patch.stories_sound.as_ref())
    )
}

fn profile_optional_text(value: Option<&str>) -> &str {
    value.filter(|value| !value.is_empty()).unwrap_or("[не задано]")
}

fn one_line_preview(value: &str, max_chars: usize) -> String {
    let normalized = value.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut chars = normalized.chars();
    let preview = chars.by_ref().take(max_chars).collect::<String>();
    if chars.next().is_some() { format!("{preview}…") } else { preview }
}

fn render_invite_batch_status(progress: &InviteBatchProgress) -> String {
    let mut lines = vec![format!(
        "Сводка: ожидают {}, выполняется {}, retry {}, подтверждены {}, уже состояли {}, отклонены {}, неизвестны {}",
        progress.pending_count,
        progress.issuing_count,
        progress.retry_wait_count,
        progress.confirmed_count,
        progress.already_present_count,
        progress.rejected_count,
        progress.outcome_unknown_count,
    )];
    for target in &progress.targets {
        let identity = target
            .target
            .username_at_prepare
            .as_deref()
            .map(|username| format!("@{}", one_line_preview(username, 20)))
            .unwrap_or_else(|| one_line_preview(&target.target.display_name_at_prepare, 20));
        let state = match target.state {
            InviteTargetState::Pending => "pending",
            InviteTargetState::Issuing => "issuing",
            InviteTargetState::RetryWait => "retry",
            InviteTargetState::Confirmed => "confirmed",
            InviteTargetState::AlreadyPresent => "already",
            InviteTargetState::Rejected => "rejected",
            InviteTargetState::OutcomeUnknown => "unknown",
        };
        let error = target
            .last_error_code
            .as_deref()
            .map(|code| format!(" ({})", one_line_preview(code, 22)))
            .unwrap_or_default();
        lines.push(format!(
            "{:02}. {} {} — {}{}",
            target.target.index.saturating_add(1),
            target.target.user_id,
            identity,
            state,
            error
        ));
    }
    lines.join("\n")
}

fn birthday_label(birthday: Option<&crate::domain::BirthdaySummary>) -> String {
    birthday.map_or_else(
        || "[не задан]".to_string(),
        |birthday| {
            birthday.year.map_or_else(
                || format!("{:02}.{:02}", birthday.day, birthday.month),
                |year| format!("{:02}.{:02}.{year:04}", birthday.day, birthday.month),
            )
        },
    )
}

fn account_profile_result_label(profile: &crate::domain::AccountProfileSummary) -> String {
    format!(
        "user_id {}\nИмя: {}\nФамилия: {}\nUsername: {}\nBio: {}\nДата рождения: {}\nЦвет имени: {}\nЦвет профиля: {}\nЛичный канал: {}",
        profile.user_id,
        profile.first_name,
        profile_optional_text(profile.last_name.as_deref()),
        profile_optional_text(profile.username.as_deref()),
        profile_optional_text(profile.about.as_deref()),
        birthday_label(profile.birthday.as_ref()),
        peer_color_label(profile.name_color.as_ref()),
        peer_color_label(profile.profile_color.as_ref()),
        personal_channel_label(profile.personal_channel.as_ref())
    )
}

fn peer_color_label(color: Option<&crate::domain::PeerColorSummary>) -> String {
    match color {
        None => "[по умолчанию]".to_string(),
        Some(crate::domain::PeerColorSummary::Simple { color, background_emoji_id }) => {
            format!(
                "обычный color={} bg_emoji={}",
                color.map_or_else(|| "default".to_string(), |value| value.to_string()),
                background_emoji_id.as_deref().unwrap_or("none")
            )
        },
        Some(crate::domain::PeerColorSummary::Collectible {
            collectible_id,
            gift_emoji_id,
            background_emoji_id,
            accent_color,
            ..
        }) => format!(
            "collectible {} gift={} bg={} accent={}",
            collectible_id, gift_emoji_id, background_emoji_id, accent_color
        ),
        Some(crate::domain::PeerColorSummary::InputCollectible { collectible_id }) => {
            format!("input collectible {collectible_id}")
        },
    }
}

fn personal_channel_label(channel: Option<&crate::domain::PersonalChannelSummary>) -> String {
    channel.map_or_else(
        || "[не задан]".to_string(),
        |channel| {
            format!(
                "{} ({}, {}, message_id={})",
                channel.chat.title, channel.chat.chat_id, channel.chat.kind, channel.message_id
            )
        },
    )
}

fn personal_channel_target_label(channel: Option<&crate::domain::ChatSummary>) -> String {
    channel.map_or_else(
        || "[очистить]".to_string(),
        |channel| format!("{} ({}, {})", channel.title, channel.chat_id, channel.kind),
    )
}

fn bot_command_payload_label(commands: &[crate::domain::BotCommandSummary]) -> String {
    if commands.is_empty() {
        return "[пусто — reset default scope]".to_string();
    }
    commands
        .iter()
        .map(|command| {
            format!(
                "/{} — {}{}",
                command.command,
                command.description,
                if command.ephemeral { " [ephemeral]" } else { "" }
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn emoji_status_summary_label(status: &EmojiStatusSummary) -> String {
    match status.kind {
        EmojiStatusKind::Empty => "[очищен]".to_string(),
        EmojiStatusKind::Default => format!(
            "default document_id={} until={}",
            status.document_id.as_deref().unwrap_or("—"),
            status.until_unix.map_or_else(|| "без срока".to_string(), |value| value.to_string())
        ),
        EmojiStatusKind::Collectible => format!(
            "collectible_id={} document_id={} until={} title={} slug={} pattern_document_id={} colors={}/{}/{}/{}",
            status.collectible_id.as_deref().unwrap_or("—"),
            status.document_id.as_deref().unwrap_or("—"),
            status.until_unix.map_or_else(|| "без срока".to_string(), |value| value.to_string()),
            status.title.as_deref().unwrap_or("—"),
            status.slug.as_deref().unwrap_or("—"),
            status.pattern_document_id.as_deref().unwrap_or("—"),
            status.center_color.map_or_else(|| "—".to_string(), |value| value.to_string()),
            status.edge_color.map_or_else(|| "—".to_string(), |value| value.to_string()),
            status.pattern_color.map_or_else(|| "—".to_string(), |value| value.to_string()),
            status.text_color.map_or_else(|| "—".to_string(), |value| value.to_string()),
        ),
    }
}

fn emoji_status_spec_label(status: &AccountEmojiStatusSpec) -> String {
    match status {
        AccountEmojiStatusSpec::Empty => "[очистить]".to_string(),
        AccountEmojiStatusSpec::Default { document_id, until_unix } => format!(
            "default document_id={document_id} until={}",
            until_unix.map_or_else(|| "без срока".to_string(), |value| value.to_string())
        ),
        AccountEmojiStatusSpec::Collectible { collectible_id, until_unix } => format!(
            "collectible_id={collectible_id} until={}",
            until_unix.map_or_else(|| "без срока".to_string(), |value| value.to_string())
        ),
    }
}

fn emoji_status_id_list(statuses: &[EmojiStatusSummary]) -> String {
    statuses
        .iter()
        .map(|status| match status.kind {
            EmojiStatusKind::Empty => "empty".to_string(),
            EmojiStatusKind::Default => {
                format!("d:{}", status.document_id.as_deref().unwrap_or("—"))
            },
            EmojiStatusKind::Collectible => format!(
                "c:{}:d:{}",
                status.collectible_id.as_deref().unwrap_or("—"),
                status.document_id.as_deref().unwrap_or("—")
            ),
        })
        .collect::<Vec<_>>()
        .join(", ")
}

fn chat_details_label(details: &crate::domain::ChatDetailsSummary) -> String {
    format!(
        "{} ({})\nНазвание: {}\nUsername: {}\nAbout: {}\nУчастников: {}\nCreator: {}\nФото: {}",
        details.chat.chat_id,
        details.chat.kind,
        details.chat.title,
        details.chat.username.as_deref().unwrap_or("нет"),
        details.about.as_deref().unwrap_or("нет"),
        details
            .chat
            .participants_count
            .map_or_else(|| "неизвестно".to_string(), |count| count.to_string()),
        details.chat.creator,
        details.photo_handle.as_deref().unwrap_or("нет")
    )
}

fn channel_settings_label(settings: &crate::domain::ChannelSettingsSummary) -> String {
    format!(
        "{} ({})\nНазвание: {}\nUsername: {}\nSlow mode: {} сек\nSignatures: {} (profiles={})\nPre-history hidden: {}\nJoin requests: {}\nJoin-to-send: {}\nNo forwards: {}\nForum: {}\nTTL истории: {}\nLinked chat: {}",
        settings.details.chat.chat_id,
        settings.details.chat.kind,
        settings.details.chat.title,
        settings.details.chat.username.as_deref().unwrap_or("нет"),
        settings.slow_mode_seconds,
        settings.signatures_enabled,
        settings.signature_profiles_enabled,
        settings.pre_history_hidden,
        settings.join_request_required,
        settings.join_to_send_required,
        settings.no_forwards,
        settings.forum_enabled,
        settings.history_ttl_seconds.map_or_else(|| "нет".to_string(), |value| value.to_string()),
        settings.linked_chat_id.as_deref().unwrap_or("нет"),
    )
}

fn chat_banned_rights_label(rights: Option<&crate::domain::ChatBannedRightsSummary>) -> String {
    let Some(rights) = rights else {
        return "не возвращены Telegram".to_string();
    };
    let mut banned = Vec::new();
    macro_rules! push_banned {
        ($field:ident) => {
            if rights.$field {
                banned.push(stringify!($field));
            }
        };
    }
    push_banned!(view_messages);
    push_banned!(send_messages);
    push_banned!(send_media);
    push_banned!(send_stickers);
    push_banned!(send_gifs);
    push_banned!(send_games);
    push_banned!(send_inline);
    push_banned!(embed_links);
    push_banned!(send_polls);
    push_banned!(change_info);
    push_banned!(invite_users);
    push_banned!(pin_messages);
    push_banned!(manage_topics);
    push_banned!(send_photos);
    push_banned!(send_videos);
    push_banned!(send_roundvideos);
    push_banned!(send_audios);
    push_banned!(send_voices);
    push_banned!(send_docs);
    push_banned!(send_plain);
    push_banned!(send_reactions);
    push_banned!(manage_linked_peers);
    let banned = if banned.is_empty() { "нет".to_string() } else { banned.join(", ") };
    format!("Запрещено: {banned}; until={}", rights.until_date_unix)
}

fn chat_reactions_policy_label(policy: Option<&crate::domain::ChatReactionsPolicy>) -> String {
    match policy {
        None => "не возвращена Telegram".to_string(),
        Some(crate::domain::ChatReactionsPolicy::None) => "реакции отключены".to_string(),
        Some(crate::domain::ChatReactionsPolicy::All { allow_custom }) => {
            format!("все реакции; custom emoji: {allow_custom}")
        },
        Some(crate::domain::ChatReactionsPolicy::Some { reactions }) => format!(
            "только: {}",
            reactions
                .iter()
                .map(|reaction| reaction_identity_label(Some(reaction)))
                .collect::<Vec<_>>()
                .join(", ")
        ),
    }
}

fn chat_moderation_settings_label(
    settings: &crate::domain::ChatModerationSettingsSummary,
) -> String {
    format!(
        "{}\nDefault permissions: {}\nReactions: {}\nReaction limit: {}\nNative anti-spam: {}\nForum: {}\nForum tabs: {}",
        chat_details_label(&settings.details),
        chat_banned_rights_label(settings.default_banned_rights.as_ref()),
        chat_reactions_policy_label(settings.available_reactions.as_ref()),
        settings.reactions_limit.map_or_else(|| "нет".to_string(), |value| value.to_string()),
        settings
            .anti_spam_enabled
            .map_or_else(|| "не применимо".to_string(), |value| value.to_string()),
        settings
            .forum_enabled
            .map_or_else(|| "не применимо".to_string(), |value| value.to_string()),
        settings
            .forum_tabs_enabled
            .map_or_else(|| "не применимо".to_string(), |value| value.to_string()),
    )
}

fn dialog_folder_label(folder: &crate::domain::DialogFolderSummary) -> String {
    format!(
        "folder_id {} ({:?})\nНазвание: {}\nIcon: {}\nColor: {}\nКатегории: contacts={}, non_contacts={}, groups={}, broadcasts={}, bots={}\nИсключения: muted={}, read={}, archived={}\nPinned peers: {}\nIncluded peers: {}\nExcluded peers: {}",
        folder.folder_id,
        folder.kind,
        folder.title,
        folder.emoticon.as_deref().unwrap_or("нет"),
        folder.color.map_or_else(|| "нет".to_string(), |color| color.to_string()),
        folder.contacts,
        folder.non_contacts,
        folder.groups,
        folder.broadcasts,
        folder.bots,
        folder.exclude_muted,
        folder.exclude_read,
        folder.exclude_archived,
        if folder.pinned_chat_ids.is_empty() {
            "нет".to_string()
        } else {
            folder.pinned_chat_ids.join(", ")
        },
        if folder.included_chat_ids.is_empty() {
            "нет".to_string()
        } else {
            folder.included_chat_ids.join(", ")
        },
        if folder.excluded_chat_ids.is_empty() {
            "нет".to_string()
        } else {
            folder.excluded_chat_ids.join(", ")
        }
    )
}

fn global_privacy_settings_label(settings: &crate::domain::GlobalPrivacySettingsSummary) -> String {
    let gifts = settings.disallowed_gifts.as_ref().map_or_else(
        || "нет".to_string(),
        |gifts| {
            format!(
                "unlimited={}, limited={}, unique={}, premium={}, from_channels={}",
                gifts.disallow_unlimited_stargifts,
                gifts.disallow_limited_stargifts,
                gifts.disallow_unique_stargifts,
                gifts.disallow_premium_gifts,
                gifts.disallow_stargifts_from_channels
            )
        },
    );
    format!(
        "Auto-archive/mute non-contacts: {}\nKeep archived unmuted: {}\nKeep archived folders: {}\nHide read marks: {}\nRequire Premium from non-contacts: {}\nDisplay gifts button: {}\nPaid messages Stars: {}\nDisallowed gifts: {}",
        settings.archive_and_mute_new_noncontact_peers,
        settings.keep_archived_unmuted,
        settings.keep_archived_folders,
        settings.hide_read_marks,
        settings.new_noncontact_peers_require_premium,
        settings.display_gifts_button,
        settings.noncontact_peers_paid_stars.as_deref().unwrap_or("нет"),
        gifts,
    )
}

fn saved_star_gift_reference_label(reference: &SavedStarGiftRef) -> String {
    match reference {
        SavedStarGiftRef::UserMessage { message_id } => {
            format!("личный подарок, message_id {message_id}")
        },
        SavedStarGiftRef::ChatSaved { chat_id, saved_id } => {
            format!("подарок чата {chat_id}, saved_id {saved_id}")
        },
    }
}

fn star_gift_identity_label(gift: &StarGiftSummary) -> String {
    match gift {
        StarGiftSummary::Regular { gift_id, title, stars, .. } => format!(
            "обычный gift_id {gift_id}, название: {}, цена: {stars} Stars",
            title.as_deref().unwrap_or("нет")
        ),
        StarGiftSummary::Unique { unique_gift_id, gift_id, title, number, .. } => format!(
            "уникальный unique_gift_id {unique_gift_id}, исходный gift_id {gift_id}, {title} #{number}"
        ),
    }
}

fn business_chat_link_target_label(link: &BusinessChatLinkSummary) -> String {
    format!(
        "Slug: {}\nНедоверенная ссылка (не открывается автоматически): {}\nНазвание: {}\nПросмотры: {}\nEntities: {}, SHA-256: {}",
        link.slug,
        link.link,
        link.title.as_deref().unwrap_or("нет"),
        link.views,
        link.entity_count,
        link.entities_sha256,
    )
}

fn business_chat_link_content_label(content: &BusinessChatLinkContent) -> String {
    format!(
        "Название: {}\nParse mode: {:?}\nEntities: {}, SHA-256: {}\nИсходное сообщение:\n────────────\n{}\n────────────\nРезультат разбора:\n────────────\n{}\n────────────",
        content.title.as_deref().unwrap_or("нет"),
        content.parse_mode,
        content.entity_count,
        content.entities_sha256,
        one_line_preview(&content.source_message, 900),
        one_line_preview(&content.rendered_message, 900),
    )
}

fn business_week_minute_label(minute: i32) -> String {
    const DAY_NAMES: [&str; 7] = ["Пн", "Вт", "Ср", "Чт", "Пт", "Сб", "Вс"];
    let week_offset = minute.div_euclid(10_080);
    let in_week = minute.rem_euclid(10_080);
    let day = usize::try_from(in_week / 1_440).unwrap_or_default();
    let minute_of_day = in_week.rem_euclid(1_440);
    let suffix = match week_offset {
        0 => String::new(),
        1 => " (+1 нед.)".to_string(),
        offset => format!(" ({offset:+} нед.)"),
    };
    format!(
        "{} {:02}:{:02}{suffix}",
        DAY_NAMES.get(day).copied().unwrap_or("?"),
        minute_of_day / 60,
        minute_of_day % 60,
    )
}

fn business_work_hours_config_label(config: Option<&BusinessWorkHoursConfig>) -> String {
    let Some(config) = config else {
        return "не настроены".to_string();
    };
    let intervals = config
        .weekly_open
        .iter()
        .map(|interval| {
            format!(
                "{}–{} [{}..{})",
                business_week_minute_label(interval.start_minute),
                business_week_minute_label(interval.end_minute),
                interval.start_minute,
                interval.end_minute,
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    format!("Часовой пояс: {}\n{intervals}", config.timezone_id)
}

fn business_work_hours_result_label(summary: Option<&BusinessWorkHoursSummary>) -> String {
    summary.map_or_else(
        || "Telegram подтвердил очистку рабочих часов".to_string(),
        |summary| {
            format!(
                "{}\nСейчас открыто (серверный readback): {}",
                business_work_hours_config_label(Some(&summary.config)),
                summary.open_now,
            )
        },
    )
}

fn business_location_label(location: Option<&BusinessLocationSummary>) -> String {
    location.map_or_else(
        || "не настроена".to_string(),
        |location| {
            let point = location.point.as_ref().map_or_else(
                || "нет".to_string(),
                |point| {
                    format!(
                        "latitude_e7={}, longitude_e7={}",
                        point.latitude_e7, point.longitude_e7
                    )
                },
            );
            format!("Адрес: {}\nКоординаты: {point}", one_line_preview(&location.address, 256))
        },
    )
}

fn business_recipients_label(recipients: &BusinessRecipientsSummary) -> String {
    let user_ids = if recipients.user_ids.is_empty() {
        "нет".to_string()
    } else {
        one_line_preview(
            &recipients.user_ids.iter().map(ToString::to_string).collect::<Vec<_>>().join(", "),
            640,
        )
    };
    format!(
        "existing_chats={}, new_chats={}, contacts={}, non_contacts={}, exclude_selected={}\nТочные user_id ({}): {}",
        recipients.existing_chats,
        recipients.new_chats,
        recipients.contacts,
        recipients.non_contacts,
        recipients.exclude_selected,
        recipients.user_ids.len(),
        user_ids,
    )
}

fn business_greeting_label(greeting: Option<&BusinessGreetingMessageSummary>) -> String {
    greeting.map_or_else(
        || "не настроено".to_string(),
        |greeting| {
            format!(
                "Quick-reply shortcut_id: {}\nБез активности: {} дней\nПолучатели:\n{}",
                greeting.shortcut_id,
                greeting.no_activity_days,
                business_recipients_label(&greeting.recipients),
            )
        },
    )
}

fn business_away_schedule_label(schedule: &BusinessAwayScheduleSummary) -> String {
    match schedule {
        BusinessAwayScheduleSummary::Always => "всегда".to_string(),
        BusinessAwayScheduleSummary::OutsideHours => "вне рабочих часов".to_string(),
        BusinessAwayScheduleSummary::Custom { start_date_unix, end_date_unix } => {
            format!("Unix {start_date_unix}..{end_date_unix}")
        },
    }
}

fn business_away_label(away: Option<&BusinessAwayMessageSummary>) -> String {
    away.map_or_else(
        || "не настроено".to_string(),
        |away| {
            format!(
                "Quick-reply shortcut_id: {}\nРасписание: {}\nТолько offline: {}\nПолучатели:\n{}",
                away.shortcut_id,
                business_away_schedule_label(&away.schedule),
                away.offline_only,
                business_recipients_label(&away.recipients),
            )
        },
    )
}

fn business_intro_summary_label(intro: Option<&BusinessIntroSummary>) -> String {
    intro.map_or_else(
        || "не настроено".to_string(),
        |intro| {
            format!(
                "Заголовок: {}\nОписание: {}\nOpaque sticker handle: {}",
                one_line_preview(&intro.title, 96),
                one_line_preview(&intro.description, 240),
                intro.sticker_handle.as_deref().unwrap_or("нет"),
            )
        },
    )
}

fn business_intro_spec_label(intro: Option<&BusinessIntroSpec>) -> String {
    intro.map_or_else(
        || "не настроено".to_string(),
        |intro| {
            format!(
                "Заголовок: {}\nОписание: {}\nНабор стикеров: {}\nOpaque sticker handle: {}",
                one_line_preview(&intro.title, 96),
                one_line_preview(&intro.description, 240),
                intro.sticker_set_short_name.as_deref().unwrap_or("нет"),
                intro.sticker_handle.as_deref().unwrap_or("нет"),
            )
        },
    )
}

fn mutation_card_parse_mode(action: &MutationAction) -> Option<&'static str> {
    matches!(
        action,
        MutationAction::ScheduleMessage { .. }
            | MutationAction::ScheduleFile { .. }
            | MutationAction::SendFile { .. }
            | MutationAction::SendStory { .. }
    )
    .then_some("HTML")
}

fn human_file_size(size_bytes: u64) -> String {
    const KIB: f64 = 1_024.0;
    const MIB: f64 = 1_024.0 * 1_024.0;
    const GIB: f64 = 1_024.0 * 1_024.0 * 1_024.0;
    let size = size_bytes as f64;
    if size < KIB {
        format!("{size_bytes} Б")
    } else if size < MIB {
        format!("{:.1} КБ", size / KIB)
    } else if size < GIB {
        format!("{:.1} МБ", size / MIB)
    } else {
        format!("{:.1} ГБ", size / GIB)
    }
}

fn short_sha256(sha256: &str) -> String {
    sha256.chars().take(12).collect()
}

fn send_file_kind_label(mode: crate::domain::FileSendMode) -> &'static str {
    match mode {
        crate::domain::FileSendMode::Photo => "фото",
        crate::domain::FileSendMode::Document => "документ",
        crate::domain::FileSendMode::Gif => "гифка",
        crate::domain::FileSendMode::Auto => "файл",
    }
}

fn story_visibility_label(privacy: &crate::domain::StoryPrivacySpec) -> String {
    match privacy.kind.as_str() {
        "everyone" => "всем".to_string(),
        "contacts" => "контактам".to_string(),
        "close_friends" => "близким друзьям".to_string(),
        "selected_contacts" => {
            if privacy.selected_user_ids.is_empty() {
                "выбранным контактам".to_string()
            } else {
                format!("выбранным контактам ({})", privacy.selected_user_ids.len())
            }
        },
        other => {
            if other.trim().is_empty() {
                "всем".to_string()
            } else {
                other.to_string()
            }
        },
    }
}

fn story_lifetime_label(period_seconds: Option<i32>) -> String {
    match period_seconds {
        None | Some(86_400) => "24 часа".to_string(),
        Some(seconds) if seconds % 3_600 == 0 => {
            let hours = seconds / 3_600;
            format!("{hours} ч")
        },
        Some(seconds) => format!("{seconds} сек"),
    }
}

fn card_preview_file_name(card: &MutationApprovalCard) -> String {
    match &card.approval.action {
        MutationAction::SendFile { artifact, .. } | MutationAction::SendStory { artifact, .. } => {
            artifact.file_name.clone()
        },
        _ => card
            .media
            .as_ref()
            .and_then(|media| {
                media.path.file_name().and_then(|value| value.to_str()).map(str::to_string)
            })
            .unwrap_or_else(|| "вложение".to_string()),
    }
}

fn mutation_card_is_media_preview(text: &str) -> bool {
    text.encode_utf16().count() <= MAX_MEDIA_CAPTION_UTF16
}

fn is_no_text_edit_error(error: &ApprovalBotError) -> bool {
    error
        .message
        .as_deref()
        .unwrap_or_default()
        .to_ascii_lowercase()
        .contains("no text")
}

fn render_send_file_card(
    approval: &MutationApproval,
    chat_id: &str,
    artifact: &crate::domain::MutationFileArtifact,
    caption: &str,
    mode: crate::domain::FileSendMode,
    reply_to_message_id: Option<i32>,
) -> Result<String, ApprovalBotError> {
    let (title, note) = if approval.external_owner_ready {
        (
            "✅ <b>Подтверждено для Hermes</b>",
            "Оператор подтвердил действие. Hermes получил одноразовое право выполнить внешний provider-вызов.",
        )
    } else {
        match approval.state {
            MutationApprovalState::DispatchingCard | MutationApprovalState::Pending => (
                "📤 <b>Отправка в Telegram</b>",
                "После нажатия «Опубликовать» файл уйдёт сразу.",
            ),
            MutationApprovalState::CardDeliveryUncertain => (
                "⚠️ <b>Карточка могла не доставиться</b>",
                "Не создавайте повтор: система сначала сверит состояние.",
            ),
            MutationApprovalState::Applying => (
                "⏳ <b>Отправляю файл…</b>",
                "Решение принято. Ожидаю подтверждение Telegram.",
            ),
            MutationApprovalState::OutcomeUnknown => (
                "⚠️ <b>Результат отправки уточняется</b>",
                "Telegram не подтвердил результат. Автоматический повтор возможен только после безопасной сверки.",
            ),
            MutationApprovalState::Applied => (
                "✅ <b>Файл отправлен</b>",
                "Telegram подтвердил отправку.",
            ),
            MutationApprovalState::Cancelled => {
                ("Отправка отменена", "Файл адресату не отправлялся.")
            },
            MutationApprovalState::Expired => {
                ("Подтверждение истекло", "Файл адресату не отправлялся.")
            },
            MutationApprovalState::Failed => (
                "⚠️ <b>Не удалось отправить файл</b>",
                "Файл не считается отправленным.",
            ),
        }
    };
    let caption_len = caption.chars().count();
    let caption_detail = if caption.is_empty() { "[без подписи]" } else { caption };
    let reply_detail = reply_to_message_id
        .map(|message_id| format!("\n↩️ Ответ на сообщение {message_id}"))
        .unwrap_or_default();
    let receipt_detail = match &approval.result {
        Some(crate::domain::MutationResult::FileSent { message_id, .. }) => {
            format!(" · сообщение {message_id}")
        },
        _ => String::new(),
    };
    let rendered = format!(
        "{title}\n\n<b>Куда:</b> <code>{}</code>\n<b>Что:</b> {} · <code>{}</code> · {}{reply_detail}\n<b>Подпись ({caption_len} знаков):</b>\n<blockquote>{}</blockquote>\n🔐 <code>{}…</code>\n\n{note}{receipt_detail}",
        escape_html(chat_id),
        send_file_kind_label(mode),
        escape_html(&artifact.file_name),
        human_file_size(artifact.size_bytes),
        escape_html(caption_detail),
        short_sha256(&artifact.sha256),
    );
    if rendered.encode_utf16().count() > MAX_APPROVAL_CARD_UTF16 {
        return Err(ApprovalBotError::known("APPROVAL_CARD_TOO_LONG"));
    }
    Ok(rendered)
}

fn render_send_story_card(
    approval: &MutationApproval,
    peer_ref: &str,
    artifact: &crate::domain::MutationFileArtifact,
    caption: &str,
    privacy: &crate::domain::StoryPrivacySpec,
    period_seconds: Option<i32>,
) -> Result<String, ApprovalBotError> {
    let (title, note) = if approval.external_owner_ready {
        (
            "✅ <b>Подтверждено для Hermes</b>",
            "Оператор подтвердил действие. Hermes получил одноразовое право выполнить внешний provider-вызов.",
        )
    } else {
        match approval.state {
            MutationApprovalState::DispatchingCard | MutationApprovalState::Pending => (
                "📸 <b>Публикация сторис</b>",
                "После нажатия «Опубликовать» сторис уйдёт сразу.",
            ),
            MutationApprovalState::CardDeliveryUncertain => (
                "⚠️ <b>Карточка могла не доставиться</b>",
                "Не создавайте повтор: система сначала сверит состояние.",
            ),
            MutationApprovalState::Applying => (
                "⏳ <b>Публикую сторис…</b>",
                "Решение принято. Ожидаю подтверждение Telegram.",
            ),
            MutationApprovalState::OutcomeUnknown => (
                "⚠️ <b>Результат публикации уточняется</b>",
                "Telegram не подтвердил результат. Автоматический повтор возможен только после безопасной сверки.",
            ),
            MutationApprovalState::Applied => (
                "✅ <b>Сторис опубликована</b>",
                "Telegram подтвердил публикацию.",
            ),
            MutationApprovalState::Cancelled => {
                ("Публикация отменена", "Сторис не публиковалась.")
            },
            MutationApprovalState::Expired => {
                ("Подтверждение истекло", "Сторис не публиковалась.")
            },
            MutationApprovalState::Failed => (
                "⚠️ <b>Не удалось опубликовать сторис</b>",
                "Сторис не считается опубликованной.",
            ),
        }
    };
    let caption_len = caption.chars().count();
    let caption_detail = if caption.is_empty() { "[без подписи]" } else { caption };
    let receipt_detail = match &approval.result {
        Some(crate::domain::MutationResult::StorySent { story, .. }) => {
            format!(" · сторис {}", story.story_id)
        },
        _ => String::new(),
    };
    let rendered = format!(
        "{title}\n\n<b>Куда:</b> <code>{}</code>\n<b>Кадр:</b> <code>{}</code> · {}\n<b>Видно:</b> {}\n<b>Время жизни:</b> {}\n<b>Подпись ({caption_len} знаков):</b>\n<blockquote>{}</blockquote>\n🔐 <code>{}…</code>\n\n{note}{receipt_detail}",
        escape_html(peer_ref),
        escape_html(&artifact.file_name),
        human_file_size(artifact.size_bytes),
        escape_html(&story_visibility_label(privacy)),
        escape_html(&story_lifetime_label(period_seconds)),
        escape_html(caption_detail),
        short_sha256(&artifact.sha256),
    );
    if rendered.encode_utf16().count() > MAX_APPROVAL_CARD_UTF16 {
        return Err(ApprovalBotError::known("APPROVAL_CARD_TOO_LONG"));
    }
    Ok(rendered)
}

fn scheduled_delivery_label(schedule_date_unix: i32) -> String {
    if schedule_date_unix == 0x7fff_fffe {
        return "когда получатель появится онлайн".to_string();
    }
    moscow_datetime_label(i64::from(schedule_date_unix))
}

fn moscow_datetime_label(timestamp_unix: i64) -> String {
    let Some(utc) = DateTime::<Utc>::from_timestamp(timestamp_unix, 0) else {
        return "время не распознано".to_string();
    };
    let moscow = FixedOffset::east_opt(3 * 60 * 60).expect("Moscow UTC offset must be valid");
    let local = utc.with_timezone(&moscow);
    let month = [
        "января",
        "февраля",
        "марта",
        "апреля",
        "мая",
        "июня",
        "июля",
        "августа",
        "сентября",
        "октября",
        "ноября",
        "декабря",
    ]
    .get(local.month0() as usize)
    .copied()
    .unwrap_or("?");
    format!(
        "{} {month} {}, {:02}:{:02} МСК",
        local.day(),
        local.year(),
        local.hour(),
        local.minute(),
    )
}

fn scheduled_target_html(chat_id: &str, target: Option<&PreparedSendTarget>) -> String {
    let Some(target) = target else {
        return format!("<code>{}</code>", escape_html(chat_id));
    };
    let username = target
        .username
        .as_deref()
        .filter(|username| !username.trim().is_empty())
        .map(|username| format!(" · @{}", escape_html(username.trim_start_matches('@'))))
        .unwrap_or_default();
    let fallback_id = if username.is_empty() && target.kind != "saved_messages" {
        format!(" · <code>{}</code>", escape_html(chat_id))
    } else {
        String::new()
    };
    format!("<b>{}</b>{username}{fallback_id}", escape_html(&target.display_name))
}

fn telegram_source_html(source: &PreparedMessageSource) -> String {
    let label = format!("{} · сообщение {}", source.target.display_name, source.message_id);
    let username = source
        .target
        .username
        .as_deref()
        .map(str::trim)
        .filter(|username| !username.is_empty())
        .map(|username| username.trim_start_matches('@'));
    if source.chat_id.starts_with('-')
        && let Some(username) = username
    {
        return format!(
            "<a href=\"https://t.me/{}/{}\">{}</a>",
            escape_html(username),
            source.message_id,
            escape_html(&label),
        );
    }
    format!("{} · <code>{}</code>", escape_html(&label), escape_html(&source.chat_id))
}

fn source_relation_label(relation: Option<OutboundSourceRelation>) -> &'static str {
    match relation {
        Some(OutboundSourceRelation::TriggeredOutreach) => "повод для обращения",
        Some(OutboundSourceRelation::MateriallyInformed) => "контекст сообщения",
        Some(OutboundSourceRelation::ReplyContext) => "контекст ответа",
        None => "связь не была зафиксирована в устаревшей записи",
    }
}

fn outbound_source_detail(source: Option<&PreparedOutboundSource>) -> String {
    match source {
        Some(PreparedOutboundSource::TelegramMessage(source)) => format!(
            "\n\n🔎 <b>Источник:</b> {}\n📅 <b>Опубликован:</b> {}\n↔️ <b>Связь:</b> {}",
            telegram_source_html(source),
            moscow_datetime_label(i64::from(source.message_date_unix)),
            source_relation_label(source.relation),
        ),
        Some(PreparedOutboundSource::NonTelegram(PreparedNonTelegramSource::PublicWeb {
            relation,
            publisher,
            title,
            url,
            published_at_unix,
            observed_at_unix,
        })) => {
            let (date_label, date) = published_at_unix.map_or_else(
                || ("Обнаружен", *observed_at_unix),
                |published| ("Опубликован", published),
            );
            format!(
                "\n\n🔎 <b>Источник:</b> <a href=\"{}\">{}</a> · {}\n📅 <b>{date_label}:</b> {}\n↔️ <b>Связь:</b> {}",
                escape_html(url),
                escape_html(title),
                escape_html(publisher),
                moscow_datetime_label(date),
                source_relation_label(Some(*relation)),
            )
        },
        Some(PreparedOutboundSource::NonTelegram(PreparedNonTelegramSource::SelfOriginated)) => {
            "\n\n🔎 <b>Источник:</b> собственная инициатива".to_string()
        },
        None => "\n\n🔎 <b>Источник:</b> ⚠️ не указан в устаревшей записи".to_string(),
    }
}

#[allow(clippy::too_many_arguments)]
fn render_schedule_message_card(
    approval: &MutationApproval,
    chat_id: &str,
    target: Option<&PreparedSendTarget>,
    source: Option<&PreparedOutboundSource>,
    text: &str,
    reply_to: Option<&crate::domain::MutationMessageSnapshot>,
    schedule_date_unix: i32,
    silent: bool,
    noforwards: bool,
) -> Result<String, ApprovalBotError> {
    let (title, note) = if approval.external_owner_ready {
        (
            "✅ <b>Подтверждено для Hermes</b>",
            "Оператор подтвердил действие. Hermes получил одноразовое право выполнить внешний provider-вызов.",
        )
    } else {
        match approval.state {
            MutationApprovalState::DispatchingCard | MutationApprovalState::Pending => (
                "<b>🗓 Запланировать сообщение?</b>",
                "После подтверждения Telegram добавит сообщение в расписание.",
            ),
            MutationApprovalState::CardDeliveryUncertain => (
                "⚠️ <b>Карточка могла не доставиться</b>",
                "Не создавайте повтор: система сначала сверит состояние.",
            ),
            MutationApprovalState::Applying => (
                "⏳ <b>Добавляю сообщение в расписание…</b>",
                "Решение принято. Ожидаю подтверждение Telegram.",
            ),
            MutationApprovalState::OutcomeUnknown => (
                "⚠️ <b>Проверяю расписание</b>",
                "Telegram не подтвердил результат. Повтор без безопасной сверки отключён.",
            ),
            MutationApprovalState::Applied => (
                "✅ <b>Сообщение запланировано</b>",
                "Telegram подтвердил, что сообщение находится в расписании.",
            ),
            MutationApprovalState::Cancelled => {
                ("Планирование отменено", "Сообщение не добавлялось в расписание.")
            },
            MutationApprovalState::Expired
                if approval.last_error_code.as_deref() == Some("SCHEDULE_TIME_ELAPSED") =>
            {
                (
                    "⌛ <b>Время отправки прошло</b>",
                    "Ничего не отправлено. Для другого времени требуется новое точное подтверждение.",
                )
            },
            MutationApprovalState::Expired => {
                ("Подтверждение истекло", "Сообщение не добавлялось в расписание.")
            },
            MutationApprovalState::Failed => (
                "⚠️ <b>Не удалось запланировать сообщение</b>",
                "Сообщение не считается добавленным в расписание.",
            ),
        }
    };
    let reply_detail = reply_to
        .map(|reply| format!("\n↩️ Ответ на сообщение {}", reply.message_id))
        .unwrap_or_default();
    let mut delivery_details = Vec::new();
    if silent {
        delivery_details.push("🔕 Без уведомления");
    }
    if noforwards {
        delivery_details.push("🔒 Пересылка запрещена");
    }
    let delivery_detail = if delivery_details.is_empty() {
        String::new()
    } else {
        format!("\n{}", delivery_details.join(" · "))
    };
    let source_detail = outbound_source_detail(source);
    let receipt_detail = match &approval.result {
        Some(crate::domain::MutationResult::MessageScheduled { message_id, .. }) => {
            format!(" · сообщение {message_id}")
        },
        _ => String::new(),
    };
    let rendered = format!(
        "{title}\n\n👤 <b>Кому:</b> {}{source_detail}\n🕒 <b>Отправка:</b> {}{reply_detail}{delivery_detail}\n\n<b>✉️ Точный текст</b>\n<blockquote>{}</blockquote>\n\n{note}{receipt_detail}",
        scheduled_target_html(chat_id, target),
        scheduled_delivery_label(schedule_date_unix),
        escape_html(text),
    );
    if rendered.encode_utf16().count() > MAX_APPROVAL_CARD_UTF16 {
        return Err(ApprovalBotError::known("APPROVAL_CARD_TOO_LONG"));
    }
    Ok(rendered)
}

#[allow(clippy::too_many_arguments)]
fn render_schedule_file_card(
    approval: &MutationApproval,
    chat_id: &str,
    target: Option<&PreparedSendTarget>,
    source: Option<&PreparedOutboundSource>,
    artifact: &crate::domain::MutationFileArtifact,
    caption: &str,
    caption_parse_mode: crate::domain::CaptionParseMode,
    reply_to: Option<&crate::domain::MutationMessageSnapshot>,
    schedule_date_unix: i32,
    silent: bool,
    noforwards: bool,
    mode: crate::domain::FileSendMode,
) -> Result<String, ApprovalBotError> {
    let (title, note) = if approval.external_owner_ready {
        (
            "✅ <b>Подтверждено для Hermes</b>",
            "Оператор подтвердил действие. Hermes получил одноразовое право выполнить внешний provider-вызов.",
        )
    } else {
        match approval.state {
            MutationApprovalState::DispatchingCard | MutationApprovalState::Pending => (
                "<b>🗓 Запланировать медиа?</b>",
                "После подтверждения Telegram добавит файл в расписание.",
            ),
            MutationApprovalState::CardDeliveryUncertain => (
                "⚠️ <b>Карточка могла не доставиться</b>",
                "Не создавайте повтор: система сначала сверит состояние.",
            ),
            MutationApprovalState::Applying => (
                "⏳ <b>Добавляю медиа в расписание…</b>",
                "Решение принято. Ожидаю подтверждение Telegram.",
            ),
            MutationApprovalState::OutcomeUnknown => (
                "⚠️ <b>Проверяю расписание</b>",
                "Telegram не подтвердил результат. Повтор без безопасной сверки отключён.",
            ),
            MutationApprovalState::Applied => (
                "✅ <b>Медиа запланировано</b>",
                "Telegram подтвердил, что файл находится в расписании.",
            ),
            MutationApprovalState::Cancelled => {
                ("Планирование отменено", "Файл не добавлялся в расписание.")
            },
            MutationApprovalState::Expired
                if approval.last_error_code.as_deref() == Some("SCHEDULE_TIME_ELAPSED") =>
            {
                (
                    "⌛ <b>Время отправки прошло</b>",
                    "Ничего не отправлено. Для другого времени требуется новое точное подтверждение.",
                )
            },
            MutationApprovalState::Expired => {
                ("Подтверждение истекло", "Файл не добавлялся в расписание.")
            },
            MutationApprovalState::Failed => (
                "⚠️ <b>Не удалось запланировать медиа</b>",
                "Файл не считается добавленным в расписание.",
            ),
        }
    };
    let reply_detail = reply_to
        .map(|reply| format!("\n↩️ Ответ на сообщение {}", reply.message_id))
        .unwrap_or_default();
    let mut delivery_details = Vec::new();
    if silent {
        delivery_details.push("🔕 Без уведомления");
    }
    if noforwards {
        delivery_details.push("🔒 Пересылка запрещена");
    }
    let delivery_detail = if delivery_details.is_empty() {
        String::new()
    } else {
        format!("\n{}", delivery_details.join(" · "))
    };
    let source_detail = outbound_source_detail(source);
    let receipt_detail = match &approval.result {
        Some(crate::domain::MutationResult::FileScheduled { message_id, .. }) => {
            format!(" · сообщение {message_id}")
        },
        _ => String::new(),
    };
    let caption_detail = if caption.is_empty() { "[без подписи]" } else { caption };
    let rendered = format!(
        "{title}\n\n👤 <b>Кому:</b> {}{source_detail}\n🕒 <b>Отправка:</b> {}{reply_detail}{delivery_detail}\n\n<b>📎 Точный файл</b>\nИмя: <code>{}</code> · Размер: {} байт · Режим: {:?}\nSHA-256: <code>{}</code>\n\n<b>Подпись</b> · {:?}\n<blockquote>{}</blockquote>\n\n{note}{receipt_detail}",
        scheduled_target_html(chat_id, target),
        scheduled_delivery_label(schedule_date_unix),
        escape_html(&artifact.file_name),
        artifact.size_bytes,
        mode,
        artifact.sha256,
        caption_parse_mode,
        escape_html(caption_detail),
    );
    if rendered.encode_utf16().count() > MAX_APPROVAL_CARD_UTF16 {
        return Err(ApprovalBotError::known("APPROVAL_CARD_TOO_LONG"));
    }
    Ok(rendered)
}

fn render_mutation_card(card: &MutationApprovalCard) -> Result<String, ApprovalBotError> {
    let approval = &card.approval;
    if let MutationAction::ScheduleMessage {
        chat_id,
        target,
        source,
        text,
        reply_to,
        schedule_date_unix,
        silent,
        noforwards,
        ..
    } = &approval.action
    {
        return render_schedule_message_card(
            approval,
            chat_id,
            target.as_ref(),
            source.as_ref(),
            text,
            reply_to.as_ref(),
            *schedule_date_unix,
            *silent,
            *noforwards,
        );
    }
    if let MutationAction::ScheduleFile {
        chat_id,
        target,
        source,
        artifact,
        caption,
        caption_parse_mode,
        reply_to,
        schedule_date_unix,
        silent,
        noforwards,
        mode,
        random_id: _,
    } = &approval.action
    {
        return render_schedule_file_card(
            approval,
            chat_id,
            target.as_ref(),
            source.as_ref(),
            artifact,
            caption,
            *caption_parse_mode,
            reply_to.as_ref(),
            *schedule_date_unix,
            *silent,
            *noforwards,
            *mode,
        );
    }
    if let MutationAction::SendFile {
        chat_id,
        artifact,
        caption,
        reply_to_message_id,
        mode,
        random_id: _,
    } = &approval.action
    {
        return render_send_file_card(
            approval,
            chat_id,
            artifact,
            caption,
            *mode,
            *reply_to_message_id,
        );
    }
    if let MutationAction::SendStory {
        peer_ref,
        artifact,
        caption,
        privacy,
        period_seconds,
        ..
    } = &approval.action
    {
        return render_send_story_card(
            approval,
            peer_ref,
            artifact,
            caption,
            privacy,
            *period_seconds,
        );
    }
    let status = if approval.external_owner_ready {
        "✅ Подтверждено. Передано владельцу операции"
    } else {
        match approval.state {
            MutationApprovalState::DispatchingCard | MutationApprovalState::Pending => {
                "⏳ Ожидает вашего решения"
            },
            MutationApprovalState::CardDeliveryUncertain => {
                "⚠️ Доставка этой карточки не была подтверждена"
            },
            MutationApprovalState::Applying => "⏳ Подтверждено. Применяю…",
            MutationApprovalState::OutcomeUnknown if approval.next_retry_at_unix.is_none() => {
                "⚠️ Результат неизвестен; автоматический повтор отключён"
            },
            MutationApprovalState::OutcomeUnknown => {
                "⚠️ Telegram пока не подтвердил результат; выполняется проверка состояния"
            },
            MutationApprovalState::Applied => "✅ Применено",
            MutationApprovalState::Cancelled => "❌ Отменено",
            MutationApprovalState::Expired => "⌛ Подтверждение истекло",
            MutationApprovalState::Failed => "⚠️ Изменение завершилось ошибкой",
        }
    };
    let (title, target, payload, result) = match &approval.action {
        MutationAction::SendTypingAction { chat_id, action } => {
            let action_label = match action {
                TypingAction::Typing => "печатает",
                TypingAction::UploadPhoto => "загружает фото",
                TypingAction::UploadDocument => "загружает документ",
                TypingAction::Cancel => "отменить индикатор",
            };
            (
                "ИНДИКАТОР АКТИВНОСТИ TELEGRAM",
                format!("Чат: {chat_id}"),
                format!(
                    "Действие: {action_label}\n\nЭто видимый получателю кратковременный сигнал от личного аккаунта. Telegram не предоставляет канонический readback; неопределённый сетевой результат не будет повторён автоматически."
                ),
                match &approval.result {
                    Some(crate::domain::MutationResult::TypingActionSent { action, .. }) => {
                        let sent_label = match action {
                            TypingAction::Typing => "печатает",
                            TypingAction::UploadPhoto => "загружает фото",
                            TypingAction::UploadDocument => "загружает документ",
                            TypingAction::Cancel => "индикатор отменён",
                        };
                        format!("Telegram принял действие: {sent_label}")
                    },
                    _ => "—".to_string(),
                },
            )
        },
        MutationAction::PressCallbackButton { button } => (
            "НАЖАТИЕ CALLBACK-КНОПКИ TELEGRAM",
            format!(
                "Чат: {}\nСообщение: #{}\nПозиция: строка {}, колонка {} (общий индекс {})",
                button.chat_id, button.message_id, button.row, button.column, button.index
            ),
            format!(
                "Метка кнопки: {}\nCallback SHA-256: {}\nCallback bytes: {}\nSnapshot сообщения: {}\n\nТекст кнопки получен из Telegram и недоверен. Нажатие может изменить состояние бота или внешней системы. URL, WebView, game, платежи, запросы данных и 2FA этим действием не открываются. При неопределённом сетевом результате автоматический повтор отключён.",
                one_line_preview(&button.label, 256),
                button.callback_data_sha256,
                button.callback_data_length,
                button.message_date_unix,
            ),
            match &approval.result {
                Some(crate::domain::MutationResult::CallbackButtonPressed { answer, .. }) => {
                    format!(
                        "Telegram вернул callback answer: alert={}, has_url={}, native_ui={}, cache={} сек.\nСообщение: {}\nURL: {}",
                        answer.alert,
                        answer.has_url,
                        answer.native_ui,
                        answer.cache_time_seconds,
                        answer.message.as_deref().unwrap_or("—"),
                        answer.url.as_deref().unwrap_or("—"),
                    )
                },
                _ => "—".to_string(),
            },
        ),
        MutationAction::SendInlineBotResult {
            query,
            result: selected,
            target_chat_id,
            reply_to,
            silent,
            hide_via,
            clear_draft,
            clear_draft_before,
            random_id: _,
        } => (
            "ОТПРАВКА INLINE-РЕЗУЛЬТАТА TELEGRAM",
            format!(
                "Целевой чат: {target_chat_id}\nИсходный query-context: {}\nБот: {} (user_id {}, @{})\nReply to: {}",
                query.context_chat_id,
                one_line_preview(&query.bot.display_name, 128),
                query.bot.user_id,
                query.bot.username.as_deref().unwrap_or("—"),
                reply_to
                    .as_ref()
                    .map_or_else(|| "нет".to_string(), |reply| format!(
                        "#{}; snapshot Unix {}; {}",
                        reply.message_id,
                        reply.date_unix,
                        one_line_preview(reply.text.as_deref().unwrap_or("[нет текста]"), 160)
                    )),
            ),
            format!(
                "Query ID: {}\nQuery: {}\nOffset: {}\nQuery snapshot: Unix {}\nQuery expires: Unix {}\n\nResult ID: {}\nType: {} / {:?}\nTitle: {}\nDescription: {}\nURL: {}\nMedia: {}\nSend message kind: {}\nSend preview: {}\nSend URL: {}\nSend geo: {}\nHas result reply markup: {}\n\nБез уведомления: {silent}\nСкрыть via-бота: {hide_via}\nОчистить draft: {clear_draft}\nDraft snapshot: {}\n\nВсе тексты, URL и preview результата получены от inline-бота и недоверены; URL/WebView не открываются. Повтор после неопределённого transport receipt использует тот же Telegram random_id и никогда не продолжается после expiry query-id.",
                query.query_id,
                one_line_preview(&query.query, 256),
                one_line_preview(&query.offset, 128),
                query.observed_at_unix,
                query.expires_at_unix,
                one_line_preview(&selected.id, 256),
                one_line_preview(&selected.result_type, 64),
                selected.source_kind,
                selected.title.as_deref().map_or("—".to_string(), |value| one_line_preview(value, 256)),
                selected.description.as_deref().map_or("—".to_string(), |value| one_line_preview(value, 320)),
                selected.url.as_deref().map_or("—".to_string(), |value| one_line_preview(value, 256)),
                selected.media_kind.as_deref().unwrap_or("нет"),
                selected.send_message_kind,
                selected.send_message_text.as_deref().map_or("—".to_string(), |value| one_line_preview(value, 320)),
                selected.send_message_url.as_deref().map_or("—".to_string(), |value| one_line_preview(value, 256)),
                selected.send_message_geo.as_ref().map_or_else(
                    || "—".to_string(),
                    |geo| format!(
                        "point={:?}; heading={:?}; live_period={:?}; proximity_radius={:?}",
                        geo.point,
                        geo.heading_degrees,
                        geo.live_period_seconds,
                        geo.proximity_notification_radius_m,
                    )
                ),
                selected.send_message_has_reply_markup,
                clear_draft_before.as_ref().map_or_else(
                    || "нет draft на момент подготовки".to_string(),
                    |draft| format!(
                        "Unix {}; {}",
                        draft.date_unix,
                        one_line_preview(&draft.message, 320)
                    )
                ),
            ),
            match &approval.result {
                Some(crate::domain::MutationResult::InlineBotResultSent {
                    chat_id,
                    message_id,
                    query_id,
                    result,
                }) => format!(
                    "Отправлен result_id {} из query_id {} в {} как message_id {}",
                    one_line_preview(&result.id, 256),
                    query_id,
                    chat_id,
                    message_id,
                ),
                _ => "—".to_string(),
            },
        ),
        MutationAction::DeleteSavedHistory { before } => (
            "УДАЛЕНИЕ ИСТОРИИ SAVED MESSAGES",
            format!("Origin peer: {}", before.peer_ref),
            format!(
                "Диапазон message_id: до {}\nДата от (Unix): {}\nДата до (Unix): {}\nНаблюдалось сообщений в первой странице: {}\nВсего по Telegram: {}\nFingerprint: {}\n\nДействие необратимо и удалит все сообщения этого Saved sub-dialog, попадающие в диапазон.",
                if before.max_id == 0 { "без ограничения".to_string() } else { before.max_id.to_string() },
                before.min_date.map_or_else(|| "без ограничения".to_string(), |value| value.to_string()),
                before.max_date.map_or_else(|| "без ограничения".to_string(), |value| value.to_string()),
                before.observed_message_ids.len(),
                before.observed_total_count.map_or_else(|| "неизвестно".to_string(), |value| value.to_string()),
                before.observed_fingerprint,
            ),
            match &approval.result {
                Some(crate::domain::MutationResult::SavedHistoryDeleted {
                    remaining_count,
                    ..
                }) => format!("Осталось сообщений в утверждённом диапазоне: {remaining_count}"),
                _ => "—".to_string(),
            },
        ),
        MutationAction::SetSavedDialogPinned { before, pinned } => (
            "ИЗМЕНЕНИЕ PIN SAVED MESSAGES",
            format!(
                "Origin peer: {}\nНазвание: {}",
                before.peer_ref,
                before.title.as_deref().unwrap_or("—")
            ),
            format!(
                "Было: {}\nСтанет: {}\nTop message: #{}",
                if before.pinned { "закреплено" } else { "не закреплено" },
                if *pinned { "закреплено" } else { "не закреплено" },
                before.top_message_id,
            ),
            match &approval.result {
                Some(crate::domain::MutationResult::SavedDialogPinUpdated { dialog }) => {
                    format!(
                        "Telegram подтвердил: {}",
                        if dialog.pinned { "закреплено" } else { "не закреплено" }
                    )
                },
                _ => "—".to_string(),
            },
        ),
        MutationAction::UpdateSavedReactionTag { before, title } => (
            "ИЗМЕНЕНИЕ ТЕГА SAVED MESSAGES",
            format!("Реакция: {}", reaction_identity_label(Some(&before.reaction))),
            format!(
                "Текущее название: {}\nНовое название: {}\nСообщений с тегом: {}",
                before.title.as_deref().unwrap_or("—"),
                title.as_deref().unwrap_or("[очистить]"),
                before.count,
            ),
            match &approval.result {
                Some(crate::domain::MutationResult::SavedReactionTagUpdated { tag }) => {
                    format!(
                        "Telegram подтвердил название: {}",
                        tag.title.as_deref().unwrap_or("—")
                    )
                },
                _ => "—".to_string(),
            },
        ),
        MutationAction::TranslateMessages { chat_id, messages, to_language } => {
            let message_ids = messages
                .iter()
                .map(|message| message.message_id.to_string())
                .collect::<Vec<_>>()
                .join(", ");
            let previews = messages
                .iter()
                .take(5)
                .map(|message| {
                    format!(
                        "#{}: {}",
                        message.message_id,
                        one_line_preview(message.text.as_deref().unwrap_or("[без текста]"), 120)
                    )
                })
                .collect::<Vec<_>>()
                .join("\n");
            let omitted = messages.len().saturating_sub(5);
            let result = match &approval.result {
                Some(crate::domain::MutationResult::MessagesTranslated { translation }) => {
                    format!(
                        "Получено переводов: {}; язык: {}",
                        translation.translations.len(), translation.to_language
                    )
                },
                _ => "—".to_string(),
            };
            (
                "ПЕРЕВОД СООБЩЕНИЙ TELEGRAM — РАСХОД КВОТЫ",
                format!("Чат: {chat_id}\nЯзык назначения: {to_language}"),
                format!(
                    "Точные message_id ({}): {message_ids}\nSnapshot-превью:\n{previews}{}\n\nЗапрос может расходовать Premium/аккаунтную квоту перевода. При неопределённом сетевом результате автоматический повтор отключён.",
                    messages.len(),
                    if omitted > 0 { format!("\n…и ещё {omitted}") } else { String::new() }
                ),
                result,
            )
        },
        MutationAction::TranscribeAudio { chat_id, before } => (
            "РАСШИФРОВКА АУДИО TELEGRAM — ВОЗМОЖЕН РАСХОД КВОТЫ",
            format!("Чат: {chat_id}\nСообщение: #{}", before.message_id),
            format!(
                "Тип медиа: {}\nДата сообщения (Unix): {}\n\nПервичный запрос может использовать Premium или оставшийся бесплатный лимит. Последующие опросы разрешаются только для этой уже подтверждённой расшифровки.",
                before.media_type.as_deref().unwrap_or("неизвестен"),
                before.date_unix,
            ),
            match &approval.result {
                Some(crate::domain::MutationResult::AudioTranscribed { transcription }) => {
                    format!(
                        "ID: {}; pending: {}; текст: {}",
                        transcription.transcription_id,
                        transcription.pending,
                        one_line_preview(&transcription.text, 240)
                    )
                },
                _ => "—".to_string(),
            },
        ),
        MutationAction::RateTranscription { before, good } => (
            "ОЦЕНКА РАСШИФРОВКИ TELEGRAM",
            format!(
                "Чат: {}\nСообщение: #{}\nTranscription ID: {}",
                before.chat_id, before.message_id, before.transcription_id
            ),
            format!(
                "Оценка: {}\nТекст: {}",
                if *good { "хорошая" } else { "плохая" },
                one_line_preview(&before.text, 300)
            ),
            match &approval.result {
                Some(crate::domain::MutationResult::TranscriptionRated { good, .. }) => {
                    format!("Оценка сохранена: {}", if *good { "хорошая" } else { "плохая" })
                },
                _ => "—".to_string(),
            },
        ),
        MutationAction::EditFactCheck { chat_id, before, text } => (
            "ИЗМЕНЕНИЕ FACT-CHECK TELEGRAM",
            format!("Чат: {chat_id}\nСообщение: #{}", before.message_id),
            format!(
                "Было: {}\nСтанет: {}\nПредыдущий hash: {}",
                one_line_preview(before.text.as_deref().unwrap_or("—"), 500),
                one_line_preview(text, 700),
                before.hash,
            ),
            match &approval.result {
                Some(crate::domain::MutationResult::FactCheckUpdated { fact_check, .. }) => {
                    format!("Подтверждён текст; новый hash: {}", fact_check.hash)
                },
                _ => "—".to_string(),
            },
        ),
        MutationAction::DeleteFactCheck { chat_id, before } => (
            "УДАЛЕНИЕ FACT-CHECK TELEGRAM",
            format!("Чат: {chat_id}\nСообщение: #{}", before.message_id),
            format!(
                "Удаляемый текст: {}\nПредыдущий hash: {}",
                one_line_preview(before.text.as_deref().unwrap_or("—"), 900),
                before.hash,
            ),
            match &approval.result {
                Some(crate::domain::MutationResult::FactCheckDeleted { fact_check, .. }) => {
                    format!("Удаление подтверждено; новый hash: {}", fact_check.hash)
                },
                _ => "—".to_string(),
            },
        ),
        MutationAction::SetBotCommands { before, commands } => (
            "ИЗМЕНЕНИЕ КОМАНД TELEGRAM-БОТА",
            format!(
                "Бот: {}\nuser_id: {}\nUsername: {}\nScope: default\nLanguage: {}",
                before.bot.display_name,
                before.bot.user_id,
                before.bot.username.as_deref().unwrap_or("нет"),
                if before.language_code.is_empty() {
                    "[default]"
                } else {
                    &before.language_code
                },
            ),
            format!(
                "Текущих команд: {}\nНовый точный список ({}):\n{}\n\nПустой список сбрасывает default scope. При неопределённом receipt повтор выполняется только если readback всё ещё равен исходному снимку.",
                before.commands.len(),
                commands.len(),
                bot_command_payload_label(commands),
            ),
            match &approval.result {
                Some(crate::domain::MutationResult::BotCommandsSet { state }) => format!(
                    "Telegram подтвердил {} команд для language {}",
                    state.commands.len(),
                    if state.language_code.is_empty() {
                        "[default]"
                    } else {
                        &state.language_code
                    },
                ),
                _ => "—".to_string(),
            },
        ),
        MutationAction::SetAccountEmojiStatus { before, status } => (
            "ИЗМЕНЕНИЕ EMOJI-СТАТУСА TELEGRAM",
            format!("Аккаунт: user_id {}", approval.account_user_id),
            format!(
                "Было: {}\nСтанет: {}\n\nДействие меняет публичный Premium emoji-status аккаунта. Если задан expiry, срок approval не выходит за него. При неопределённом receipt повтор разрешён только после readback исходного снимка.",
                emoji_status_summary_label(before),
                emoji_status_spec_label(status),
            ),
            match &approval.result {
                Some(crate::domain::MutationResult::AccountEmojiStatusSet { status }) => {
                    format!("Telegram подтвердил: {}", emoji_status_summary_label(status))
                },
                _ => "—".to_string(),
            },
        ),
        MutationAction::ClearRecentEmojiStatuses { before } => (
            "ОЧИСТКА НЕДАВНИХ EMOJI-СТАТУСОВ TELEGRAM",
            format!("Аккаунт: user_id {}", approval.account_user_id),
            format!(
                "Точный текущий список ({}):\n{}\nResponse hash: {}\n\nДействие очищает весь связанный список. При неопределённом receipt повтор разрешён только если readback всё ещё совпадает с этим снимком.",
                before.statuses.len(),
                emoji_status_id_list(&before.statuses),
                before.response_hash.as_deref().unwrap_or("—"),
            ),
            match &approval.result {
                Some(crate::domain::MutationResult::RecentEmojiStatusesCleared { remaining }) => {
                    format!(
                        "Telegram подтвердил очистку; осталось статусов: {}",
                        remaining.statuses.len()
                    )
                },
                _ => "—".to_string(),
            },
        ),
        MutationAction::UpdateAccountProfile {
            before,
            first_name,
            last_name,
            about,
        } => (
            "ИЗМЕНЕНИЕ ПРОФИЛЯ TELEGRAM",
            format!(
                "Аккаунт: user_id {}\nUsername: {}",
                before.user_id,
                profile_optional_text(before.username.as_deref())
            ),
            format!(
                "Имя: {} → {}\nФамилия: {} → {}\nBio: {} → {}",
                before.first_name,
                first_name.as_deref().unwrap_or("[без изменения]"),
                profile_optional_text(before.last_name.as_deref()),
                last_name
                    .as_deref()
                    .map_or("[без изменения]", |value| profile_optional_text(Some(value))),
                profile_optional_text(before.about.as_deref()),
                about
                    .as_deref()
                    .map_or("[без изменения]", |value| profile_optional_text(Some(value)))
            ),
            match &approval.result {
                Some(crate::domain::MutationResult::AccountProfileUpdated { profile }) => {
                    account_profile_result_label(profile)
                },
                _ => "—".to_string(),
            },
        ),
        MutationAction::UpdateAccountUsername { before, username } => (
            "ИЗМЕНЕНИЕ USERNAME TELEGRAM",
            format!("Аккаунт: user_id {}", before.user_id),
            format!(
                "Текущий username: {}\nНовый username: {}",
                profile_optional_text(before.username.as_deref()),
                profile_optional_text(username.as_deref())
            ),
            match &approval.result {
                Some(crate::domain::MutationResult::AccountUsernameUpdated { profile }) => {
                    account_profile_result_label(profile)
                },
                _ => "—".to_string(),
            },
        ),
        MutationAction::UpdateAccountBirthday { before, birthday } => (
            "ИЗМЕНЕНИЕ ДАТЫ РОЖДЕНИЯ TELEGRAM",
            format!("Аккаунт: user_id {}", before.user_id),
            format!(
                "Текущая дата рождения: {}\nНовая дата рождения: {}",
                birthday_label(before.birthday.as_ref()),
                birthday_label(birthday.as_ref())
            ),
            match &approval.result {
                Some(crate::domain::MutationResult::AccountBirthdayUpdated { profile }) => {
                    account_profile_result_label(profile)
                },
                _ => "—".to_string(),
            },
        ),
        MutationAction::SetAccountPeerColor {
            before,
            for_profile,
            color,
            background_emoji_id,
        } => {
            let target = if color.is_none() && background_emoji_id.is_none() {
                None
            } else {
                Some(crate::domain::PeerColorSummary::Simple {
                    color: *color,
                    background_emoji_id: background_emoji_id.clone(),
                })
            };
            (
                if *for_profile {
                    "ИЗМЕНЕНИЕ ЦВЕТА ПРОФИЛЯ TELEGRAM"
                } else {
                    "ИЗМЕНЕНИЕ ЦВЕТА ИМЕНИ TELEGRAM"
                },
                format!("Аккаунт: user_id {}", approval.account_user_id),
                format!(
                    "Было: {}\nСтанет: {}\n\nApproval связан с полным TL-вариантом текущего цвета; collectible-состояние не будет перезаписано после stale/uncertain readback.",
                    peer_color_label(before.as_ref()),
                    peer_color_label(target.as_ref())
                ),
                match &approval.result {
                    Some(crate::domain::MutationResult::AccountPeerColorUpdated { profile }) => {
                        account_profile_result_label(profile)
                    },
                    _ => "—".to_string(),
                },
            )
        },
        MutationAction::SetAccountPersonalChannel { before, channel } => (
            "ИЗМЕНЕНИЕ ЛИЧНОГО КАНАЛА TELEGRAM",
            format!("Аккаунт: user_id {}", approval.account_user_id),
            format!(
                "Было: {}\nСтанет: {}\n\nИзменяется только exact channel identity; итоговый featured message Telegram выбирает и возвращает через полный профиль.",
                personal_channel_label(before.as_ref()),
                personal_channel_target_label(channel.as_ref())
            ),
            match &approval.result {
                Some(crate::domain::MutationResult::AccountPersonalChannelUpdated { profile }) => {
                    account_profile_result_label(profile)
                },
                _ => "—".to_string(),
            },
        ),
        MutationAction::SetAccountPresence { online } => (
            "ИЗМЕНЕНИЕ ПРИСУТСТВИЯ TELEGRAM",
            format!("Аккаунт: user_id {}", approval.account_user_id),
            format!(
                "Новое состояние: {}\nTelegram не предоставляет канонический readback присутствия; неопределённый сетевой результат не будет повторён автоматически.",
                if *online { "онлайн" } else { "офлайн" }
            ),
            match &approval.result {
                Some(crate::domain::MutationResult::AccountPresenceUpdated { online }) => {
                    format!("Присутствие обновлено: {}", if *online { "онлайн" } else { "офлайн" })
                },
                _ => "—".to_string(),
            },
        ),
        MutationAction::UploadAccountProfilePhoto {
            before_handles,
            artifact,
            fallback,
        } => (
            "ЗАГРУЗКА ФОТО ПРОФИЛЯ TELEGRAM",
            format!("Аккаунт: user_id {}", approval.account_user_id),
            format!(
                "Файл: {}\nРазмер: {} байт\nSHA-256: {}\nFallback photo: {}\nФотографий до загрузки: {}\n\nБудут отправлены только точные immutable-байты, связанные с этой карточкой.",
                artifact.file_name,
                artifact.size_bytes,
                artifact.sha256,
                fallback,
                before_handles.len()
            ),
            match &approval.result {
                Some(crate::domain::MutationResult::AccountProfilePhotoUploaded { photo }) => {
                    format!(
                        "Загружено: {}\nДата Unix: {}\nВидео: {}\nСтикеры: {}",
                        photo.photo_handle,
                        photo.date_unix,
                        photo.has_video,
                        photo.has_stickers
                    )
                },
                _ => "—".to_string(),
            },
        ),
        MutationAction::DeleteAccountProfilePhotos { before } => (
            "УДАЛЕНИЕ ФОТО ПРОФИЛЯ TELEGRAM",
            format!("Аккаунт: user_id {}", approval.account_user_id),
            before
                .iter()
                .map(|photo| {
                    format!(
                        "{} — Unix {}, video={}, stickers={}",
                        photo.photo_handle,
                        photo.date_unix,
                        photo.has_video,
                        photo.has_stickers
                    )
                })
                .collect::<Vec<_>>()
                .join("\n"),
            match &approval.result {
                Some(crate::domain::MutationResult::AccountProfilePhotosDeleted {
                    photo_handles,
                }) => format!("Удалены: {}", photo_handles.join(", ")),
                _ => "—".to_string(),
            },
        ),
        MutationAction::CreateDialogFolder { before_folder_ids, target } => (
            "СОЗДАНИЕ ПАПКИ TELEGRAM",
            format!("Аккаунт: user_id {}", approval.account_user_id),
            format!(
                "Папки до создания: {}\n\nНовая точная папка:\n{}",
                if before_folder_ids.is_empty() {
                    "нет".to_string()
                } else {
                    before_folder_ids.iter().map(i32::to_string).collect::<Vec<_>>().join(", ")
                },
                dialog_folder_label(target)
            ),
            match &approval.result {
                Some(crate::domain::MutationResult::DialogFolderCreated { folder }) => {
                    dialog_folder_label(folder)
                },
                _ => "—".to_string(),
            },
        ),
        MutationAction::EditDialogFolder {
            before,
            target,
            preserve_title_entities,
        } => (
            "ИЗМЕНЕНИЕ ПАПКИ TELEGRAM",
            format!("Аккаунт: user_id {}\nПапка: {}", approval.account_user_id, before.folder_id),
            format!(
                "Текущее состояние:\n{}\n\nНовое точное состояние:\n{}\n\nСохранить существующие title entities: {}",
                dialog_folder_label(before),
                dialog_folder_label(target),
                preserve_title_entities
            ),
            match &approval.result {
                Some(crate::domain::MutationResult::DialogFolderUpdated { folder }) => {
                    dialog_folder_label(folder)
                },
                _ => "—".to_string(),
            },
        ),
        MutationAction::DeleteDialogFolder { before } => (
            "УДАЛЕНИЕ ПАПКИ TELEGRAM",
            format!("Аккаунт: user_id {}", approval.account_user_id),
            dialog_folder_label(before),
            match &approval.result {
                Some(crate::domain::MutationResult::DialogFolderDeleted { folder_id }) => {
                    format!("Удалена папка folder_id {folder_id}")
                },
                _ => "—".to_string(),
            },
        ),
        MutationAction::ReorderDialogFolders { before_order, target_order } => (
            "ИЗМЕНЕНИЕ ПОРЯДКА ПАПОК TELEGRAM",
            format!("Аккаунт: user_id {}", approval.account_user_id),
            format!(
                "Текущий порядок: {}\nНовый порядок: {}",
                before_order.iter().map(i32::to_string).collect::<Vec<_>>().join(", "),
                target_order.iter().map(i32::to_string).collect::<Vec<_>>().join(", ")
            ),
            match &approval.result {
                Some(crate::domain::MutationResult::DialogFoldersReordered { folder_ids }) => {
                    format!(
                        "Новый порядок: {}",
                        folder_ids.iter().map(i32::to_string).collect::<Vec<_>>().join(", ")
                    )
                },
                _ => "—".to_string(),
            },
        ),
        MutationAction::SetDialogFolderMembership { before, chat_id, included } => (
            if *included {
                "ДОБАВЛЕНИЕ ЧАТА В ПАПКУ TELEGRAM"
            } else {
                "УДАЛЕНИЕ ЧАТА ИЗ ПАПКИ TELEGRAM"
            },
            format!(
                "Аккаунт: user_id {}\nПапка: {}\nЧат: {chat_id}",
                approval.account_user_id, before.folder_id
            ),
            format!(
                "Текущее состояние папки:\n{}\n\nЦелевое explicit membership: {}",
                dialog_folder_label(before),
                included
            ),
            match &approval.result {
                Some(crate::domain::MutationResult::DialogFolderMembershipUpdated {
                    folder,
                    chat_id,
                    included,
                }) => format!(
                    "Чат {chat_id}, included={included}\n{}",
                    dialog_folder_label(folder)
                ),
                _ => "—".to_string(),
            },
        ),
        MutationAction::SetDialogFolderTags { before, enabled } => (
            "ИЗМЕНЕНИЕ ТЕГОВ ПАПОК TELEGRAM",
            format!("Аккаунт: user_id {}", approval.account_user_id),
            format!("Текущее состояние: {before}\nНовое состояние: {enabled}"),
            match &approval.result {
                Some(crate::domain::MutationResult::DialogFolderTagsUpdated {
                    tags_enabled,
                }) => format!("Folder tags enabled: {tags_enabled}"),
                _ => "—".to_string(),
            },
        ),
        MutationAction::SetGlobalPrivacySettings { before, target } => (
            "ИЗМЕНЕНИЕ ГЛОБАЛЬНОЙ ПРИВАТНОСТИ TELEGRAM",
            format!("Аккаунт: user_id {}", approval.account_user_id),
            format!(
                "Текущее точное состояние:\n{}\n\nНовое точное состояние:\n{}",
                global_privacy_settings_label(before),
                global_privacy_settings_label(target)
            ),
            match &approval.result {
                Some(crate::domain::MutationResult::GlobalPrivacySettingsUpdated { settings }) => {
                    global_privacy_settings_label(settings)
                },
                _ => "—".to_string(),
            },
        ),
        MutationAction::EditMessage { chat_id, before, text } => (
            "ИЗМЕНЕНИЕ СООБЩЕНИЯ",
            format!("Чат: {chat_id}\nСообщение: {}", before.message_id),
            format!(
                "Текущий точный текст:\n────────────\n{}\n────────────\n\nНовый точный текст:\n────────────\n{text}\n────────────\n\nИсходный edit_date: {}\nМедиа: {}",
                before.text.as_deref().unwrap_or("[нет текста]"),
                before.edit_date_unix.map_or_else(|| "нет".to_string(), |value| value.to_string()),
                before.media_type.as_deref().unwrap_or("нет")
            ),
            match &approval.result {
                Some(crate::domain::MutationResult::MessageEdited { message_id }) => {
                    format!("Изменено message_id: {message_id}")
                },
                _ => "—".to_string(),
            },
        ),
        MutationAction::DeleteMessages { chat_id, messages, revoke } => (
            "УДАЛЕНИЕ СООБЩЕНИЙ",
            format!("Чат: {chat_id}"),
            format!(
                "Точные сообщения:\n{}\n\nУдалить для всех: {}\nСнимок восстановления: {}",
                messages
                    .iter()
                    .map(|message| format!(
                        "message_id {} (sender: {}, media: {}):\n────────────\n{}\n────────────",
                        message.message_id,
                        message.sender_ref.as_deref().unwrap_or("неизвестен"),
                        message.media_type.as_deref().unwrap_or("нет"),
                        message.text.as_deref().unwrap_or("[нет текста]")
                    ))
                    .collect::<Vec<_>>()
                    .join("\n"),
                if *revoke { "да" } else { "нет" },
                if messages.iter().all(|message| message.restore.is_some()) {
                    "готов для каждого сообщения"
                } else {
                    "неполный; необратимые элементы явно разрешены"
                }
            ),
            match &approval.result {
                Some(crate::domain::MutationResult::MessagesDeleted { message_ids }) => format!(
                    "Удалены message_id: {}",
                    message_ids.iter().map(i32::to_string).collect::<Vec<_>>().join(", ")
                ),
                _ => "—".to_string(),
            },
        ),
        MutationAction::RestoreDeletedMessages {
            source_deletion_approval_id,
            chat_id,
            messages,
            allow_sender_change,
            random_ids: _,
        } => (
            "ВОССТАНОВЛЕНИЕ УДАЛЁННЫХ СООБЩЕНИЙ",
            format!(
                "Чат: {chat_id}\nИсходное удаление: {source_deletion_approval_id}"
            ),
            format!(
                "Будут созданы новые Telegram message_id; старые ID, дата, реакции, просмотры и ответы других участников не возвращаются.\nИзменение отправителя разрешено: {}\n\nВосстанавливаемое содержимое:\n{}",
                if *allow_sender_change { "да" } else { "нет" },
                messages
                    .iter()
                    .map(|message| format!(
                        "original message_id {} (sender: {}, media: {}, grouped_id: {}):\n────────────\n{}\n────────────",
                        message.message_id,
                        message.sender_ref.as_deref().unwrap_or("неизвестен"),
                        message.media_type.as_deref().unwrap_or("нет"),
                        message
                            .restore
                            .as_ref()
                            .and_then(|snapshot| snapshot.grouped_id)
                            .map_or_else(|| "нет".to_string(), |value| value.to_string()),
                        message.text.as_deref().unwrap_or("[нет текста]")
                    ))
                    .collect::<Vec<_>>()
                    .join("\n")
            ),
            match &approval.result {
                Some(crate::domain::MutationResult::MessagesRestored { mappings, .. }) => mappings
                    .iter()
                    .map(|mapping| format!(
                        "{} → {}",
                        mapping.original_message_id, mapping.restored_message_id
                    ))
                    .collect::<Vec<_>>()
                    .join(", "),
                _ => "—".to_string(),
            },
        ),
        MutationAction::SetMessagePinned { chat_id, before, pinned, silent, one_sided } => (
            if *pinned {
                "ЗАКРЕПЛЕНИЕ СООБЩЕНИЯ"
            } else {
                "ОТКРЕПЛЕНИЕ СООБЩЕНИЯ"
            },
            format!("Чат: {chat_id}\nСообщение: {}", before.message_id),
            format!(
                "Текущий pinned: {}\nНовый pinned: {}\nБез уведомления: {}\nТолько для текущего аккаунта: {}\n\nТочный текст:\n────────────\n{}\n────────────",
                before.pinned,
                pinned,
                silent,
                one_sided,
                before.text.as_deref().unwrap_or("[нет текста]")
            ),
            match &approval.result {
                Some(crate::domain::MutationResult::MessagePinUpdated { message_id, pinned }) => {
                    format!("message_id {message_id}, pinned: {pinned}")
                },
                _ => "—".to_string(),
            },
        ),
        MutationAction::SetMessageReaction { chat_id, before, before_reaction, reaction } => (
            "РЕАКЦИЯ НА СООБЩЕНИЕ",
            format!("Чат: {chat_id}\nСообщение: {}", before.message_id),
            format!(
                "Текущая реакция аккаунта: {}\nНовая реакция: {}\n\nТочный текст:\n────────────\n{}\n────────────",
                reaction_label(before_reaction.as_ref()),
                reaction_label(Some(reaction)),
                before.text.as_deref().unwrap_or("[нет текста]")
            ),
            match &approval.result {
                Some(crate::domain::MutationResult::MessageReactionUpdated {
                    message_id,
                    reaction,
                }) => format!(
                    "message_id {message_id}, реакция: {}",
                    reaction_label(reaction.as_ref())
                ),
                _ => "—".to_string(),
            },
        ),
        MutationAction::SetDefaultReaction { before, reaction } => (
            "ИЗМЕНЕНИЕ РЕАКЦИИ ПО УМОЛЧАНИЮ TELEGRAM",
            format!("Аккаунт: user_id {}", approval.account_user_id),
            format!(
                "Текущая реакция: {}\nНовая реакция: {}\n\nПосле применения Telegram config будет прочитан повторно; неопределённый сетевой результат не приведёт к слепому повтору.",
                reaction_identity_label(before.as_ref()),
                default_reaction_label(reaction)
            ),
            match &approval.result {
                Some(crate::domain::MutationResult::DefaultReactionUpdated { reaction }) => {
                    format!("Реакция по умолчанию: {}", reaction_identity_label(Some(reaction)))
                },
                _ => "—".to_string(),
            },
        ),
        MutationAction::SetDraft { chat_id, before, text } => (
            if text.is_some() {
                "СОХРАНЕНИЕ ЧЕРНОВИКА"
            } else {
                "ОЧИСТКА ЧЕРНОВИКА"
            },
            format!("Чат: {chat_id}"),
            format!(
                "Текущий черновик:\n────────────\n{}\n────────────\n\nНовый черновик:\n────────────\n{}\n────────────",
                before.as_ref().map(|draft| draft.message.as_str()).unwrap_or("[нет черновика]"),
                text.as_deref().unwrap_or("[очистить]")
            ),
            match &approval.result {
                Some(crate::domain::MutationResult::DraftUpdated { chat_id, draft }) => format!(
                    "{chat_id}: {}",
                    draft
                        .as_ref()
                        .map(|draft| draft.message.as_str())
                        .unwrap_or("[черновик очищен]")
                ),
                _ => "—".to_string(),
            },
        ),
        MutationAction::ClearAllDrafts { before } => {
            let shown = before
                .iter()
                .take(12)
                .map(|item| {
                    let mut message = item.draft.message.chars().take(80).collect::<String>();
                    if item.draft.message.chars().count() > 80 {
                        message.push('…');
                    }
                    format!(
                        "{} — {}\n  {}\n  state_sha256={}",
                        item.chat_id,
                        item.title,
                        if message.is_empty() { "[нет текста]" } else { &message },
                        item.draft.state_sha256
                    )
                })
                .collect::<Vec<_>>()
                .join("\n");
            let omitted = before.len().saturating_sub(12);
            (
                "ОЧИСТКА ВСЕХ ЧЕРНОВИКОВ TELEGRAM",
                format!(
                    "Аккаунт: user_id {}\nЧерновиков в точном снимке: {}",
                    approval.account_user_id,
                    before.len()
                ),
                format!(
                    "Будут удалены все черновики из неизменяемого снимка:\n{shown}{}\n\nНовый или изменившийся черновик после подготовки блокирует применение. При неопределённом receipt операция повторяется только если полный снимок остался прежним.\nPayload SHA-256: {}",
                    if omitted > 0 {
                        format!("\n… и ещё {omitted}")
                    } else {
                        String::new()
                    },
                    approval.payload_sha256
                ),
                match &approval.result {
                    Some(crate::domain::MutationResult::AllDraftsCleared {
                        cleared_chat_ids,
                        remaining,
                    }) => format!(
                        "Очищено: {}\nОсталось после readback: {}",
                        cleared_chat_ids.len(),
                        remaining.len()
                    ),
                    _ => "—".to_string(),
                },
            )
        },
        MutationAction::SetDialogUnread { chat_id, before, unread } => (
            if *unread {
                "ОТМЕТКА ДИАЛОГА КАК НЕПРОЧИТАННОГО"
            } else {
                "ОТМЕТКА ДИАЛОГА КАК ПРОЧИТАННОГО"
            },
            format!("Чат: {chat_id}"),
            format!(
                "Текущий unread_mark: {}\nНовый unread_mark: {}\nНепрочитанных сообщений: {}",
                before.unread_mark, unread, before.unread_count
            ),
            mutation_dialog_result_text(&approval.result),
        ),
        MutationAction::MarkMessagesRead { chat_id, before, max_id } => (
            "ОТМЕТКА СООБЩЕНИЙ КАК ПРОЧИТАННЫХ",
            format!("Чат: {chat_id}\nДо message_id: {max_id} включительно"),
            format!(
                "Снимок при подготовке:\ntop_message_id: {}\nread_inbox_max_id: {}\nunread_mark: {}\nunread_count: {}\n\nОперация навсегда ограничена message_id {max_id}; новые сообщения после подготовки не будут затронуты.",
                before.top_message_id,
                before.read_inbox_max_id,
                before.unread_mark,
                before.unread_count
            ),
            match &approval.result {
                Some(crate::domain::MutationResult::MessagesMarkedRead {
                    chat_id,
                    max_id,
                    state,
                }) => format!(
                    "{chat_id}: прочитано до {max_id}; read_inbox_max_id={}, unread_mark={}, unread_count={}",
                    state.read_inbox_max_id, state.unread_mark, state.unread_count
                ),
                _ => "—".to_string(),
            },
        ),
        MutationAction::MarkReactionsRead {
            chat_id,
            before_unread_reactions_count,
        } => (
            "ОТМЕТКА РЕАКЦИЙ КАК ПРОЧИТАННЫХ",
            format!("Чат: {chat_id}"),
            format!(
                "Непрочитанных реакций в точном снимке: {before_unread_reactions_count}\n\nПри неопределённом сетевом результате широкая операция не будет повторена без доказанного readback."
            ),
            match &approval.result {
                Some(crate::domain::MutationResult::MessageReactionsMarkedRead {
                    unread_reactions_count,
                    server_receipt,
                    ..
                }) => server_receipt.as_ref().map_or_else(
                    || format!("unread_reactions_count={unread_reactions_count}; применено по readback"),
                    |receipt| format!(
                        "unread_reactions_count={unread_reactions_count}; pts={}, pts_count={}, offset={}",
                        receipt.pts, receipt.pts_count, receipt.offset
                    ),
                ),
                _ => "—".to_string(),
            },
        ),
        MutationAction::SetDialogArchived { chat_id, before, archived } => (
            if *archived {
                "АРХИВАЦИЯ ДИАЛОГА"
            } else {
                "ВОЗВРАТ ДИАЛОГА ИЗ АРХИВА"
            },
            format!("Чат: {chat_id}"),
            format!(
                "Текущий archived: {}\nНовый archived: {}\nТекущая folder_id: {:?}",
                before.archived, archived, before.folder_id
            ),
            mutation_dialog_result_text(&approval.result),
        ),
        MutationAction::SetDialogPinned { chat_id, before, pinned } => (
            if *pinned {
                "ЗАКРЕПЛЕНИЕ ДИАЛОГА"
            } else {
                "ОТКРЕПЛЕНИЕ ДИАЛОГА"
            },
            format!("Чат: {chat_id}"),
            format!("Текущий pinned: {}\nНовый pinned: {}", before.pinned, pinned),
            mutation_dialog_result_text(&approval.result),
        ),
        MutationAction::SetUserBlocked { user, before_blocked, blocked } => (
            if *blocked {
                "БЛОКИРОВКА ПОЛЬЗОВАТЕЛЯ"
            } else {
                "РАЗБЛОКИРОВКА ПОЛЬЗОВАТЕЛЯ"
            },
            format!(
                "Пользователь: {}\nuser_id: {}\nusername: {}",
                user.display_name,
                user.user_id,
                user.username.as_deref().unwrap_or("нет")
            ),
            format!("Текущий blocked: {before_blocked}\nНовый blocked: {blocked}"),
            match &approval.result {
                Some(crate::domain::MutationResult::UserBlocked { user_id, blocked }) => {
                    format!("user_id {user_id}, blocked: {blocked}")
                },
                _ => "—".to_string(),
            },
        ),
        MutationAction::SetContact {
            before,
            contact,
            first_name,
            last_name,
            phone,
            add_phone_privacy_exception,
        } => (
            if *contact {
                "ДОБАВЛЕНИЕ КОНТАКТА"
            } else {
                "УДАЛЕНИЕ КОНТАКТА"
            },
            format!(
                "Пользователь: {}\nuser_id: {}\nusername: {}",
                before.display_name,
                before.user_id,
                before.username.as_deref().unwrap_or("нет")
            ),
            format!(
                "Текущий is_contact: {}\nНовый is_contact: {}\nИмя контакта: {} {}\nТелефон задан: {}\nИсключение приватности телефона: {}",
                before.is_contact,
                contact,
                first_name,
                last_name,
                !phone.is_empty(),
                add_phone_privacy_exception
            ),
            match &approval.result {
                Some(crate::domain::MutationResult::ContactStateUpdated {
                    user_id,
                    is_contact,
                }) => format!("user_id {user_id}, is_contact: {is_contact}"),
                _ => "—".to_string(),
            },
        ),
        MutationAction::SetPrivacyRules { key, before, rules, user_bindings } => (
            "ЗАМЕНА ПРАВИЛ ПРИВАТНОСТИ",
            format!("Настройка аккаунта: {key:?}"),
            format!(
                "Текущий полный набор:\n{}\n\nНовый полный набор:\n{}\n\nПривязанные пользователи:\n{}",
                privacy_rules_label(before),
                privacy_rules_label(rules),
                if user_bindings.is_empty() {
                    "нет".to_string()
                } else {
                    user_bindings
                        .iter()
                        .map(|binding| {
                            format!(
                                "{} (user_id {}, source_ref {})",
                                binding.user.display_name, binding.user.user_id, binding.source_ref
                            )
                        })
                        .collect::<Vec<_>>()
                        .join("\n")
                }
            ),
            match &approval.result {
                Some(crate::domain::MutationResult::PrivacyRulesUpdated { key, rules }) => {
                    format!("{key:?}:\n{}", privacy_rules_label(rules))
                },
                _ => "—".to_string(),
            },
        ),
        MutationAction::SetNotificationSettings { chat_id, before, patch, sound_target } => (
            "ИЗМЕНЕНИЕ УВЕДОМЛЕНИЙ",
            format!(
                "Чат: {chat_id}\nЗвуковая привязка: {}",
                sound_target.as_ref().map_or_else(
                    || "не требуется".to_string(),
                    |target| format!(
                        "authorization_id {}, platform {:?}",
                        target.authorization_id, target.platform
                    )
                )
            ),
            format!(
                "Текущие настройки:\n{}\n\nТочное изменение:\n{}",
                notification_settings_label(before),
                notification_patch_label(patch)
            ),
            match &approval.result {
                Some(crate::domain::MutationResult::NotificationSettingsUpdated {
                    chat_id,
                    settings,
                }) => format!("{chat_id}:\n{}", notification_settings_label(settings)),
                _ => "—".to_string(),
            },
        ),
        MutationAction::ScheduleMessage {
            chat_id,
            target: _,
            source: _,
            text,
            reply_to,
            schedule_date_unix,
            silent,
            noforwards,
            ..
        } => (
            "ПЛАНИРОВАНИЕ СООБЩЕНИЯ",
            format!(
                "Чат: {chat_id}\nОтвет на message_id: {}",
                reply_to.as_ref().map_or_else(
                    || "нет".to_string(),
                    |reply| reply.message_id.to_string()
                )
            ),
            format!(
                "Доставка: {}\nБез уведомления: {silent}\nЗапретить пересылку: {noforwards}\n\nТочный текст:\n────────────\n{text}\n────────────",
                if *schedule_date_unix == 0x7fff_fffe {
                    "когда пользователь появится онлайн".to_string()
                } else {
                    format!("Unix {schedule_date_unix}")
                }
            ),
            match &approval.result {
                Some(crate::domain::MutationResult::MessageScheduled {
                    message_id,
                    schedule_date_unix,
                }) => format!("scheduled message_id {message_id}, Unix {schedule_date_unix}"),
                _ => "—".to_string(),
            },
        ),
        MutationAction::ScheduleFile {
            chat_id,
            artifact,
            caption,
            caption_parse_mode,
            reply_to,
            schedule_date_unix,
            silent,
            noforwards,
            mode,
            ..
        } => (
            "ПЛАНИРОВАНИЕ МЕДИА",
            format!(
                "Чат: {chat_id}\nОтвет на message_id: {}",
                reply_to.as_ref().map_or_else(
                    || "нет".to_string(),
                    |reply| reply.message_id.to_string()
                )
            ),
            format!(
                "Доставка: {}\nБез уведомления: {silent}\nЗапретить пересылку: {noforwards}\nИмя: {}\nРазмер: {} байт\nРежим: {mode:?}\nSHA-256 файла: {}\nРежим подписи: {caption_parse_mode:?}\n\nТочная подпись:\n────────────\n{}\n────────────",
                if *schedule_date_unix == 0x7fff_fffe {
                    "когда пользователь появится онлайн".to_string()
                } else {
                    format!("Unix {schedule_date_unix}")
                },
                artifact.file_name,
                artifact.size_bytes,
                artifact.sha256,
                if caption.is_empty() { "[без подписи]" } else { caption }
            ),
            match &approval.result {
                Some(crate::domain::MutationResult::FileScheduled {
                    message_id,
                    schedule_date_unix,
                    sha256,
                }) => format!(
                    "scheduled media message_id {message_id}, Unix {schedule_date_unix}, SHA-256 {sha256}"
                ),
                _ => "—".to_string(),
            },
        ),
        MutationAction::EditScheduledMessage {
            chat_id,
            before,
            text,
            parse_mode,
            schedule_date_unix,
        } => (
            "ИЗМЕНЕНИЕ ЗАПЛАНИРОВАННОГО СООБЩЕНИЯ",
            format!("Чат: {chat_id}\nScheduled message_id: {}", before.message_id),
            format!(
                "Текущая доставка: Unix {}\nНовая доставка: {}\nФормат: {parse_mode:?}\n\nТекущий текст:\n────────────\n{}\n────────────\n\nНовый текст:\n────────────\n{text}\n────────────",
                before.date_unix,
                if *schedule_date_unix == 0x7fff_fffe {
                    "когда пользователь появится онлайн".to_string()
                } else {
                    format!("Unix {schedule_date_unix}")
                },
                before.text.as_deref().unwrap_or("[нет текста]")
            ),
            match &approval.result {
                Some(crate::domain::MutationResult::ScheduledMessageEdited {
                    message_id,
                    schedule_date_unix,
                }) => format!("message_id {message_id}, Unix {schedule_date_unix}"),
                _ => "—".to_string(),
            },
        ),
        MutationAction::DeleteScheduledMessages { chat_id, messages } => (
            "УДАЛЕНИЕ ЗАПЛАНИРОВАННЫХ СООБЩЕНИЙ",
            format!("Чат: {chat_id}"),
            messages
                .iter()
                .map(|message| format!(
                    "message_id {}, Unix {}:\n────────────\n{}\n────────────",
                    message.message_id,
                    message.date_unix,
                    message.text.as_deref().unwrap_or("[нет текста]")
                ))
                .collect::<Vec<_>>()
                .join("\n"),
            match &approval.result {
                Some(crate::domain::MutationResult::ScheduledMessagesDeleted { message_ids }) => {
                    format!(
                        "Удалены scheduled message_id: {}",
                        message_ids.iter().map(i32::to_string).collect::<Vec<_>>().join(", ")
                    )
                },
                _ => "—".to_string(),
            },
        ),
        MutationAction::SendScheduledMessagesNow { chat_id, messages } => (
            "НЕМЕДЛЕННАЯ ОТПРАВКА ЗАПЛАНИРОВАННЫХ СООБЩЕНИЙ",
            format!("Чат: {chat_id}"),
            messages
                .iter()
                .map(|message| format!(
                    "scheduled message_id {}, было запланировано на Unix {}:\n────────────\n{}\n────────────",
                    message.message_id,
                    message.date_unix,
                    message.text.as_deref().unwrap_or("[нет текста]")
                ))
                .collect::<Vec<_>>()
                .join("\n"),
            match &approval.result {
                Some(crate::domain::MutationResult::ScheduledMessagesSent {
                    scheduled_message_ids,
                    sent_message_ids,
                }) => format!(
                    "Scheduled ID: {}\nНовые message_id: {}",
                    scheduled_message_ids
                        .iter()
                        .map(i32::to_string)
                        .collect::<Vec<_>>()
                        .join(", "),
                    sent_message_ids
                        .iter()
                        .map(i32::to_string)
                        .collect::<Vec<_>>()
                        .join(", ")
                ),
                _ => "—".to_string(),
            },
        ),
        MutationAction::CreatePoll {
            chat_id,
            poll,
            reply_to,
            silent,
            noforwards,
            random_id: _,
        } => (
            if poll.quiz { "СОЗДАНИЕ ВИКТОРИНЫ" } else { "СОЗДАНИЕ ОПРОСА" },
            format!(
                "Чат: {chat_id}\nОтвет на message_id: {}",
                reply_to.as_ref().map_or_else(
                    || "нет".to_string(),
                    |reply| reply.message_id.to_string()
                )
            ),
            format!(
                "Вопрос:\n────────────\n{}\n────────────\n\nВарианты:\n{}\n\nПубличные голоса: {}\nНесколько вариантов: {}\nВикторина: {}\nПравильный вариант: {}\nОткрытые ответы: {}\nПереголосование запрещено: {}\nПеремешать варианты: {}\nСкрыть результаты до закрытия: {}\nТолько подписчики: {}\nСтраны: {}\nПериод закрытия: {}\nДата закрытия (Unix): {}\nБез уведомления: {}\nЗапретить пересылку: {}\n\nРешение:\n────────────\n{}\n────────────",
                poll.question,
                poll.answers
                    .iter()
                    .enumerate()
                    .map(|(index, answer)| format!("{}. {answer}", index.saturating_add(1)))
                    .collect::<Vec<_>>()
                    .join("\n"),
                poll.public_voters,
                poll.multiple_choice,
                poll.quiz,
                poll.correct_option_index
                    .map_or_else(
                        || "нет".to_string(),
                        |index| index.saturating_add(1).to_string(),
                    ),
                poll.open_answers,
                poll.revoting_disabled,
                poll.shuffle_answers,
                poll.hide_results_until_close,
                poll.subscribers_only,
                if poll.countries_iso2.is_empty() {
                    "все".to_string()
                } else {
                    poll.countries_iso2.join(", ")
                },
                poll.close_period_seconds
                    .map_or_else(|| "нет".to_string(), |seconds| format!("{seconds} секунд")),
                poll.close_date_unix
                    .map_or_else(|| "нет".to_string(), |date| date.to_string()),
                silent,
                noforwards,
                poll.solution.as_deref().unwrap_or("[нет]")
            ),
            match &approval.result {
                Some(crate::domain::MutationResult::PollCreated { message_id, poll_id }) => {
                    format!("Создан message_id {message_id}, poll_id {poll_id}")
                },
                _ => "—".to_string(),
            },
        ),
        MutationAction::SetPollVote { chat_id, before, options } => (
            if options.is_empty() { "ОТЗЫВ ГОЛОСА В ОПРОСЕ" } else { "ГОЛОСОВАНИЕ В ОПРОСЕ" },
            format!(
                "Чат: {chat_id}\nСообщение: {}\npoll_id: {}",
                before.message_id, before.poll_id
            ),
            format!(
                "Вопрос:\n────────────\n{}\n────────────\n\nВсе варианты:\n{}\n\nТекущий выбор аккаунта:\n{}\n\nНовый точный выбор:\n{}\n\nНесколько вариантов: {}\nВикторина: {}\nПереголосование запрещено: {}",
                before.question,
                poll_options_label(&before.options),
                poll_selection_label(&before.options, &before.selected_option_indices),
                if options.is_empty() {
                    "нет — отозвать голос".to_string()
                } else {
                    poll_options_label(options)
                },
                before.multiple_choice,
                before.quiz,
                before.revoting_disabled
            ),
            match &approval.result {
                Some(crate::domain::MutationResult::PollVoteUpdated {
                    message_id,
                    selected_option_indices,
                }) => format!(
                    "message_id {message_id}, выбранные варианты: {}",
                    if selected_option_indices.is_empty() {
                        "нет".to_string()
                    } else {
                        selected_option_indices
                            .iter()
                            .map(|index| index.saturating_add(1).to_string())
                            .collect::<Vec<_>>()
                            .join(", ")
                    }
                ),
                _ => "—".to_string(),
            },
        ),
        MutationAction::StopPoll { chat_id, before } => (
            "ОСТАНОВКА ОПРОСА",
            format!(
                "Чат: {chat_id}\nСообщение: {}\npoll_id: {}",
                before.message_id, before.poll_id
            ),
            format!(
                "Вопрос:\n────────────\n{}\n────────────\n\nВарианты:\n{}\n\nТекущий closed: {}\nПосле применения: closed=true",
                before.question,
                poll_options_label(&before.options),
                before.closed
            ),
            match &approval.result {
                Some(crate::domain::MutationResult::PollStopped { message_id, poll_id }) => {
                    format!("Остановлен message_id {message_id}, poll_id {poll_id}")
                },
                _ => "—".to_string(),
            },
        ),
        MutationAction::CreateForumTopic {
            chat_id,
            title,
            icon_color,
            icon_emoji_id,
            random_id: _,
        } => (
            "СОЗДАНИЕ ТЕМЫ ФОРУМА",
            format!("Чат: {chat_id}"),
            format!(
                "Точный заголовок:\n────────────\n{title}\n────────────\n\nЦвет иконки RGB: {}\nCustom emoji ID: {}",
                icon_color.map_or_else(|| "по умолчанию".to_string(), |color| {
                    format!("#{color:06x}")
                }),
                icon_emoji_id.map_or_else(|| "нет".to_string(), |id| id.to_string())
            ),
            match &approval.result {
                Some(crate::domain::MutationResult::ForumTopicCreated { topic_id }) => {
                    format!("Создана тема topic_id {topic_id}")
                },
                _ => "—".to_string(),
            },
        ),
        MutationAction::EditForumTopic {
            chat_id,
            before,
            title,
            icon_emoji_id,
            closed,
            hidden,
        } => (
            "ИЗМЕНЕНИЕ ТЕМЫ ФОРУМА",
            format!(
                "Чат: {chat_id}\nТема: {}\nТекущий заголовок: {}",
                before.topic_id,
                before.title.as_deref().unwrap_or("[нет заголовка]")
            ),
            format!(
                "Новый заголовок: {}\nCustom emoji: {}\nClosed: {}\nHidden: {}",
                title.as_deref().unwrap_or("без изменения"),
                icon_emoji_id.map_or_else(
                    || "без изменения".to_string(),
                    |id| if id == 0 { "очистить".to_string() } else { id.to_string() }
                ),
                closed.map_or_else(|| "без изменения".to_string(), |value| value.to_string()),
                hidden.map_or_else(|| "без изменения".to_string(), |value| value.to_string())
            ),
            match &approval.result {
                Some(crate::domain::MutationResult::ForumTopicUpdated { topic }) => format!(
                    "topic_id {}, title={}, closed={}, hidden={}, icon_emoji_id={}",
                    topic.topic_id,
                    topic.title.as_deref().unwrap_or("[нет]"),
                    topic.closed,
                    topic.hidden,
                    topic.icon_emoji_id.map_or_else(|| "нет".to_string(), |id| id.to_string())
                ),
                _ => "—".to_string(),
            },
        ),
        MutationAction::SetForumTopicPinned { chat_id, before, pinned } => (
            if *pinned {
                "ЗАКРЕПЛЕНИЕ ТЕМЫ ФОРУМА"
            } else {
                "ОТКРЕПЛЕНИЕ ТЕМЫ ФОРУМА"
            },
            format!(
                "Чат: {chat_id}\nТема: {}\nЗаголовок: {}",
                before.topic_id,
                before.title.as_deref().unwrap_or("[нет заголовка]")
            ),
            format!("Текущий pinned: {}\nНовый pinned: {pinned}", before.pinned),
            match &approval.result {
                Some(crate::domain::MutationResult::ForumTopicPinned { topic_id, pinned }) => {
                    format!("topic_id {topic_id}, pinned: {pinned}")
                },
                _ => "—".to_string(),
            },
        ),
        MutationAction::DeleteForumTopic { chat_id, before } => (
            "УДАЛЕНИЕ ТЕМЫ ФОРУМА И ВСЕЙ ЕЁ ИСТОРИИ",
            format!(
                "Чат: {chat_id}\nТема: {}\nЗаголовок: {}",
                before.topic_id,
                before.title.as_deref().unwrap_or("[нет заголовка]")
            ),
            format!(
                "Будут удалены все сообщения темы.\nClosed: {}\nPinned: {}\nHidden: {}",
                before.closed, before.pinned, before.hidden
            ),
            match &approval.result {
                Some(crate::domain::MutationResult::ForumTopicDeleted { topic_id }) => {
                    format!("Удалена тема topic_id {topic_id}")
                },
                _ => "—".to_string(),
            },
        ),
        MutationAction::AddChatMember { chat_id, user, before } => (
            "ДОБАВЛЕНИЕ УЧАСТНИКА В TELEGRAM-ЧАТ",
            format!(
                "Чат: {chat_id}\nПользователь: {} (@{})",
                user.user_id,
                user.username.as_deref().unwrap_or("нет username")
            ),
            format!(
                "Текущее состояние: {}\nБудет отправлено приглашение в чат.\nИсходный членский снимок: {}",
                before
                    .as_ref()
                    .map(|member| format!("{:?}", member.role))
                    .unwrap_or_else(|| "не найден".to_string()),
                before.is_some()
            ),
            match &approval.result {
                Some(crate::domain::MutationResult::ChatMemberAdded { member }) => {
                    format!("Участник добавлен, роль: {:?}", member.role)
                },
                _ => "—".to_string(),
            },
        ),
        MutationAction::InviteChatMembers { chat, targets } => {
            let progress = card.invite_progress.as_ref();
            let status = progress.map(render_invite_batch_status).unwrap_or_else(|| {
                "Статус по отдельным пользователям временно недоступен".to_string()
            });
            let result = match &approval.result {
                Some(crate::domain::MutationResult::ChatMembersInvited { outcomes, .. }) => {
                    let confirmed = outcomes
                        .iter()
                        .filter(|target| target.state == InviteTargetState::Confirmed)
                        .count();
                    let already = outcomes
                        .iter()
                        .filter(|target| target.state == InviteTargetState::AlreadyPresent)
                        .count();
                    let rejected = outcomes
                        .iter()
                        .filter(|target| target.state == InviteTargetState::Rejected)
                        .count();
                    format!(
                        "Завершено: приглашены {confirmed}, уже состояли {already}, отклонены Telegram {rejected}"
                    )
                },
                _ => "—".to_string(),
            };
            (
                "ПАКЕТНОЕ ПРИГЛАШЕНИЕ УЧАСТНИКОВ В TELEGRAM-ЧАТ",
                format!(
                    "Чат: {}\nНазвание: {}\nТип: {:?}\nПользователей: {}",
                    chat.chat_id,
                    one_line_preview(&chat.title_at_prepare, 128),
                    chat.kind,
                    targets.len()
                ),
                format!(
                    "Каждый пользователь вызывается отдельным Telegram-запросом и имеет отдельное durable-состояние. Неопределённый provider receipt никогда не повторяет инвайт вслепую.\n\n{status}"
                ),
                result,
            )
        },
        MutationAction::RemoveChatMember { chat_id, before } => (
            "УДАЛЕНИЕ УЧАСТНИКА ИЗ TELEGRAM-ЧАТА",
            format!(
                "Чат: {chat_id}\nПользователь: {} (@{})\nТекущая роль: {:?}",
                before.user.user_id,
                before.user.username.as_deref().unwrap_or("нет username"),
                before.role
            ),
            format!(
                "Будет выполнено удаление пользователя из чата.\nИсходный rank: {}\nАдминистраторские права: {}",
                before.rank.as_deref().unwrap_or("нет"),
                before.admin_rights.as_ref().is_some_and(|rights| rights.any())
            ),
            match &approval.result {
                Some(crate::domain::MutationResult::ChatMemberRemoved { user_id }) => {
                    format!("Участник удалён, user_id: {user_id}")
                },
                _ => "—".to_string(),
            },
        ),
        MutationAction::SetChatMemberAdmin { chat_id, before, rights, rank } => (
            "ИЗМЕНЕНИЕ ПРАВ АДМИНИСТРАТОРА TELEGRAM",
            format!(
                "Чат: {chat_id}\nПользователь: {} (@{})\nТекущая роль: {:?}",
                before.user.user_id,
                before.user.username.as_deref().unwrap_or("нет username"),
                before.role
            ),
            format!(
                "Новая роль: {}\nНовый rank: {}\nПрава: {:?}",
                if rights.any() { "admin" } else { "member" },
                rank.as_deref().unwrap_or("нет"),
                rights
            ),
            match &approval.result {
                Some(crate::domain::MutationResult::ChatMemberAdminUpdated { member }) => {
                    format!("Роль после readback: {:?}, rank: {}", member.role, member.rank.as_deref().unwrap_or("нет"))
                },
                _ => "—".to_string(),
            },
        ),
        MutationAction::SetChatMemberRestrictions { chat_id, user, before, rights } => (
            "ИЗМЕНЕНИЕ ОГРАНИЧЕНИЙ УЧАСТНИКА TELEGRAM",
            format!(
                "Чат: {chat_id}\nПользователь: {} (@{})\nТекущая роль: {}",
                user.user_id,
                user.username.as_deref().unwrap_or("нет username"),
                before
                    .as_ref()
                    .map(|member| format!("{:?}", member.role))
                    .unwrap_or_else(|| "не найден".to_string())
            ),
            format!(
                "Новый набор ограничений: {:?}\nДо (Unix, 0=без срока): {}\nTrue означает запрет соответствующей возможности.",
                rights,
                rights.until_date_unix
            ),
            match &approval.result {
                Some(crate::domain::MutationResult::ChatMemberRestrictionsUpdated { member }) => {
                    format!("Роль после readback: {}", member.as_ref().map(|member| format!("{:?}", member.role)).unwrap_or_else(|| "отсутствует".to_string()))
                },
                _ => "—".to_string(),
            },
        ),
        MutationAction::CreateGroup { title, users } => (
            "СОЗДАНИЕ TELEGRAM-ГРУППЫ",
            format!("Новая basic group\nАккаунт: user_id {}", approval.account_user_id),
            format!(
                "Название: {title}\nУчастники:\n{}\n\nTelegram не предоставляет idempotency key для создания: неопределённый результат не будет повторён автоматически.",
                users
                    .iter()
                    .map(|user| format!(
                        "- {} ({})",
                        user.display_name,
                        user.username.as_deref().unwrap_or("без username")
                    ))
                    .collect::<Vec<_>>()
                    .join("\n")
            ),
            match &approval.result {
                Some(crate::domain::MutationResult::ChatCreated { chat }) => {
                    chat_details_label(chat)
                },
                _ => "—".to_string(),
            },
        ),
        MutationAction::CreateChannel { title, about, broadcast } => (
            if *broadcast {
                "СОЗДАНИЕ TELEGRAM-КАНАЛА"
            } else {
                "СОЗДАНИЕ TELEGRAM-СУПЕРГРУППЫ"
            },
            format!("Аккаунт: user_id {}", approval.account_user_id),
            format!(
                "Название: {title}\nAbout: {}\nТип: {}\n\nTelegram не предоставляет idempotency key для создания: неопределённый результат не будет повторён автоматически.",
                if about.is_empty() { "нет" } else { about },
                if *broadcast { "broadcast channel" } else { "supergroup" }
            ),
            match &approval.result {
                Some(crate::domain::MutationResult::ChatCreated { chat }) => {
                    chat_details_label(chat)
                },
                _ => "—".to_string(),
            },
        ),
        MutationAction::JoinChat { before } => (
            "ВСТУПЛЕНИЕ В TELEGRAM-ЧАТ",
            chat_details_label(&before.details),
            format!("Состояние в dialogs при подготовке: {}", before.in_dialogs),
            match &approval.result {
                Some(crate::domain::MutationResult::ChatJoined { state }) => format!(
                    "in_dialogs={}\n{}",
                    state.in_dialogs,
                    chat_details_label(&state.details)
                ),
                _ => "—".to_string(),
            },
        ),
        MutationAction::LeaveChat { before } => (
            "ВЫХОД ИЗ TELEGRAM-ЧАТА",
            chat_details_label(&before.details),
            "Чат будет удалён из текущих dialogs; его идентичность останется в immutable истории approval.".to_string(),
            match &approval.result {
                Some(crate::domain::MutationResult::ChatLeft { state }) => {
                    format!("in_dialogs={}", state.in_dialogs)
                },
                _ => "—".to_string(),
            },
        ),
        MutationAction::DeleteChat { before } => (
            "БЕЗВОЗВРАТНОЕ УДАЛЕНИЕ TELEGRAM-ЧАТА",
            chat_details_label(&before.details),
            "Будет удалён creator-owned group/channel. Неопределённый результат не будет повторён автоматически.".to_string(),
            match &approval.result {
                Some(crate::domain::MutationResult::ChatDeleted { state }) => format!(
                    "Удалён: {}\nin_dialogs={}",
                    state.details.chat.chat_id, state.in_dialogs
                ),
                _ => "—".to_string(),
            },
        ),
        MutationAction::UpdateChatTitle { before, title } => (
            "ИЗМЕНЕНИЕ НАЗВАНИЯ TELEGRAM-ЧАТА",
            chat_details_label(before),
            format!("Новое название: {title}"),
            match &approval.result {
                Some(crate::domain::MutationResult::ChatTitleUpdated { chat }) => {
                    chat_details_label(chat)
                },
                _ => "—".to_string(),
            },
        ),
        MutationAction::UpdateChatAbout { before, about } => (
            "ИЗМЕНЕНИЕ ABOUT TELEGRAM-ЧАТА",
            chat_details_label(before),
            format!("Новый about: {}", if about.is_empty() { "[очистить]" } else { about }),
            match &approval.result {
                Some(crate::domain::MutationResult::ChatAboutUpdated { chat }) => {
                    chat_details_label(chat)
                },
                _ => "—".to_string(),
            },
        ),
        MutationAction::UpdateChatPhoto { before, artifact } => (
            "ИЗМЕНЕНИЕ ФОТО TELEGRAM-ЧАТА",
            chat_details_label(before),
            format!(
                "Файл: {}\nРазмер: {} байт\nSHA-256: {}\n\nПосле неопределённого результата загрузка автоматически не повторяется.",
                artifact.file_name, artifact.size_bytes, artifact.sha256
            ),
            match &approval.result {
                Some(crate::domain::MutationResult::ChatPhotoUpdated { chat }) => {
                    chat_details_label(chat)
                },
                _ => "—".to_string(),
            },
        ),
        MutationAction::DeleteChatPhoto { before } => (
            "УДАЛЕНИЕ ФОТО TELEGRAM-ЧАТА",
            chat_details_label(before),
            format!(
                "Будет удалено текущее фото с opaque handle: {}",
                before.photo_handle.as_deref().unwrap_or("нет")
            ),
            match &approval.result {
                Some(crate::domain::MutationResult::ChatPhotoUpdated { chat }) => {
                    chat_details_label(chat)
                },
                _ => "—".to_string(),
            },
        ),
        MutationAction::UpdateChatUsername { before, username } => (
            "ИЗМЕНЕНИЕ USERNAME TELEGRAM-КАНАЛА",
            channel_settings_label(before),
            format!(
                "Новый username: {}",
                username.as_deref().map_or("очистить", |value| value)
            ),
            match &approval.result {
                Some(crate::domain::MutationResult::ChannelSettingsUpdated { settings }) => {
                    channel_settings_label(settings)
                },
                _ => "—".to_string(),
            },
        ),
        MutationAction::SetChatSlowMode { before, seconds } => (
            "ИЗМЕНЕНИЕ SLOW MODE TELEGRAM-КАНАЛА",
            channel_settings_label(before),
            format!("Новый интервал: {seconds} секунд"),
            match &approval.result {
                Some(crate::domain::MutationResult::ChannelSettingsUpdated { settings }) => {
                    channel_settings_label(settings)
                },
                _ => "—".to_string(),
            },
        ),
        MutationAction::ToggleChatSignatures {
            before,
            signatures_enabled,
            profiles_enabled,
        } => (
            "ИЗМЕНЕНИЕ ПОДПИСЕЙ TELEGRAM-КАНАЛА",
            channel_settings_label(before),
            format!(
                "Signatures: {signatures_enabled}; author profiles: {profiles_enabled}"
            ),
            match &approval.result {
                Some(crate::domain::MutationResult::ChannelSettingsUpdated { settings }) => {
                    channel_settings_label(settings)
                },
                _ => "—".to_string(),
            },
        ),
        MutationAction::ToggleChatPreHistoryHidden { before, enabled } => (
            "ИЗМЕНЕНИЕ PRE-HISTORY TELEGRAM-СУПЕРГРУППЫ",
            channel_settings_label(before),
            format!("Скрыть pre-history: {enabled}"),
            match &approval.result {
                Some(crate::domain::MutationResult::ChannelSettingsUpdated { settings }) => {
                    channel_settings_label(settings)
                },
                _ => "—".to_string(),
            },
        ),
        MutationAction::ToggleChatJoinRequest { before, enabled } => (
            "ИЗМЕНЕНИЕ JOIN REQUESTS TELEGRAM-ЧАТА",
            channel_settings_label(before),
            format!("Требовать заявку на вступление: {enabled}"),
            match &approval.result {
                Some(crate::domain::MutationResult::ChannelSettingsUpdated { settings }) => {
                    channel_settings_label(settings)
                },
                _ => "—".to_string(),
            },
        ),
        MutationAction::SetChatPermissions { before, target } => (
            "ИЗМЕНЕНИЕ DEFAULT PERMISSIONS TELEGRAM-ЧАТА",
            chat_moderation_settings_label(before),
            format!(
                "Новые default restrictions для не-админов:\n{}\n\nВ списке перечислены запрещённые возможности; остальные поля сохраняются из точного снимка.",
                chat_banned_rights_label(Some(target))
            ),
            match &approval.result {
                Some(crate::domain::MutationResult::ChatModerationSettingsUpdated {
                    settings,
                }) => chat_moderation_settings_label(settings),
                _ => "—".to_string(),
            },
        ),
        MutationAction::SetChatReactions { before, target } => (
            "ИЗМЕНЕНИЕ ДОСТУПНЫХ РЕАКЦИЙ TELEGRAM-ЧАТА",
            chat_moderation_settings_label(before),
            format!("Новая политика: {}", chat_reactions_policy_label(Some(target))),
            match &approval.result {
                Some(crate::domain::MutationResult::ChatModerationSettingsUpdated {
                    settings,
                }) => chat_moderation_settings_label(settings),
                _ => "—".to_string(),
            },
        ),
        MutationAction::ToggleChatAntiSpam { before, enabled } => (
            "ИЗМЕНЕНИЕ NATIVE ANTI-SPAM TELEGRAM-СУПЕРГРУППЫ",
            chat_moderation_settings_label(before),
            format!("Aggressive anti-spam станет: {enabled}"),
            match &approval.result {
                Some(crate::domain::MutationResult::ChatModerationSettingsUpdated {
                    settings,
                }) => chat_moderation_settings_label(settings),
                _ => "—".to_string(),
            },
        ),
        MutationAction::ToggleChatForum {
            before,
            enabled,
            tabs_enabled,
        } => (
            if *enabled {
                "ВКЛЮЧЕНИЕ FORUM MODE TELEGRAM-СУПЕРГРУППЫ"
            } else {
                "ОТКЛЮЧЕНИЕ FORUM MODE TELEGRAM-СУПЕРГРУППЫ"
            },
            chat_moderation_settings_label(before),
            format!(
                "Forum станет: {enabled}; tabs: {tabs_enabled}{}",
                if *enabled {
                    ""
                } else {
                    "\nОтключение удаляет все существующие темы."
                }
            ),
            match &approval.result {
                Some(crate::domain::MutationResult::ChatModerationSettingsUpdated {
                    settings,
                }) => chat_moderation_settings_label(settings),
                _ => "—".to_string(),
            },
        ),
        MutationAction::ViewStories { peer_ref, before, story_ids } => (
            "РЕГИСТРАЦИЯ ПРОСМОТРА TELEGRAM STORIES",
            format!(
                "Peer: {peer_ref}\nStories: {}\nСнимков: {}",
                story_ids.iter().map(i32::to_string).collect::<Vec<_>>().join(", "),
                before.len()
            ),
            "Telegram получит подтверждение просмотра только перечисленных story IDs. Повторное нажатие той же карточки не создаёт новую отправку.".to_string(),
            match &approval.result {
                Some(crate::domain::MutationResult::StoriesViewed {
                    peer_ref,
                    story_ids,
                }) => format!(
                    "Просмотр подтверждён для {peer_ref}: {}",
                    story_ids.iter().map(i32::to_string).collect::<Vec<_>>().join(", ")
                ),
                _ => "—".to_string(),
            },
        ),
        MutationAction::ReadStories { peer_ref, before_max_read_id, max_id, .. } => (
            "ОТМЕТКА TELEGRAM STORIES ПРОЧИТАННЫМИ",
            format!(
                "Peer: {peer_ref}\nТекущий max_read_id: {}",
                before_max_read_id.map_or_else(|| "нет".to_string(), |id| id.to_string())
            ),
            format!("Продвинуть точный маркер чтения до story {max_id}"),
            match &approval.result {
                Some(crate::domain::MutationResult::StoriesRead { peer_ref, max_id }) => {
                    format!("Stories {peer_ref} прочитаны до {max_id}")
                },
                _ => "—".to_string(),
            },
        ),
        MutationAction::SetStoryReaction { peer_ref, before, reaction } => (
            "РЕАКЦИЯ НА TELEGRAM STORY",
            format!(
                "Peer: {peer_ref}\nStory: {}\nТекущая реакция: {:?}",
                before.story.story_id, before.sent_reaction
            ),
            format!("Новая реакция: {reaction:?}"),
            match &approval.result {
                Some(crate::domain::MutationResult::StoryReactionUpdated {
                    peer_ref,
                    story_id,
                    reaction,
                }) => format!("Story {story_id} в {peer_ref}: {reaction:?}"),
                _ => "—".to_string(),
            },
        ),
        MutationAction::SetStoriesPinned { peer_ref, before, story_ids, pinned } => (
            "ИЗМЕНЕНИЕ PINNED-СОСТОЯНИЯ TELEGRAM STORIES",
            format!("Peer: {peer_ref}\nТочных снимков: {}", before.len()),
            format!(
                "Stories: {}\nPinned: {pinned}",
                story_ids.iter().map(i32::to_string).collect::<Vec<_>>().join(", ")
            ),
            match &approval.result {
                Some(crate::domain::MutationResult::StoriesPinnedUpdated {
                    peer_ref,
                    story_ids,
                    pinned,
                }) => format!(
                    "{peer_ref}: pinned={pinned} для {}",
                    story_ids.iter().map(i32::to_string).collect::<Vec<_>>().join(", ")
                ),
                _ => "—".to_string(),
            },
        ),
        MutationAction::SetStoriesPinnedToTop {
            peer_ref,
            before,
            story_ids,
            pinned_to_top,
        } => (
            "ИЗМЕНЕНИЕ TOP-PIN TELEGRAM STORIES",
            format!("Peer: {peer_ref}\nТочных снимков: {}", before.len()),
            format!(
                "Stories: {}\nPinned to top: {pinned_to_top}\nTelegram использует toggle; при неопределённом ответе повтор запрещён.",
                story_ids.iter().map(i32::to_string).collect::<Vec<_>>().join(", ")
            ),
            match &approval.result {
                Some(crate::domain::MutationResult::StoriesPinnedToTopUpdated {
                    peer_ref,
                    story_ids,
                    pinned_to_top,
                }) => format!(
                    "{peer_ref}: pinned_to_top={pinned_to_top} для {}",
                    story_ids.iter().map(i32::to_string).collect::<Vec<_>>().join(", ")
                ),
                _ => "—".to_string(),
            },
        ),
        MutationAction::ActivateStoryStealthMode { before, past, future } => (
            "АКТИВАЦИЯ STEALTH MODE ДЛЯ TELEGRAM STORIES",
            format!(
                "Active until: {}\nCooldown until: {}",
                before.active_until_unix.map_or_else(|| "нет".to_string(), |v| v.to_string()),
                before
                    .cooldown_until_unix
                    .map_or_else(|| "нет".to_string(), |v| v.to_string())
            ),
            format!(
                "Скрыть прошлые просмотры: {past}\nСкрывать будущие просмотры: {future}\nАктивация может расходовать cooldown и не повторяется вслепую."
            ),
            match &approval.result {
                Some(crate::domain::MutationResult::StoryStealthModeActivated {
                    mode,
                    past,
                    future,
                }) => format!(
                    "Stealth mode активирован (past={past}, future={future})\nActive until: {}\nCooldown until: {}",
                    mode.active_until_unix.map_or_else(|| "нет".to_string(), |v| v.to_string()),
                    mode.cooldown_until_unix.map_or_else(|| "нет".to_string(), |v| v.to_string())
                ),
                _ => "—".to_string(),
            },
        ),
        MutationAction::SendStory {
            peer_ref,
            before_story_ids,
            artifact,
            caption,
            privacy,
            pinned,
            noforwards,
            period_seconds,
            ..
        } => (
            "ПУБЛИКАЦИЯ TELEGRAM STORY",
            format!(
                "Peer: {peer_ref}\nТекущих stories в снимке: {}",
                before_story_ids.len()
            ),
            format!(
                "Файл: {}\nРазмер: {} байт\nSHA-256: {}\nCaption: {}\nPrivacy: {} (selected users={}, excluded users={})\nPinned: {pinned}\nNo forwards: {noforwards}\nПериод: {} секунд",
                artifact.file_name,
                artifact.size_bytes,
                artifact.sha256,
                if caption.is_empty() { "[нет]" } else { caption },
                privacy.kind,
                privacy.selected_user_ids.len(),
                privacy.disallowed_user_ids.len(),
                period_seconds.unwrap_or(86_400),
            ),
            match &approval.result {
                Some(crate::domain::MutationResult::StorySent { peer_ref, story }) => format!(
                    "Story {} опубликована в {}\nMedia: {}\nExpires: {}",
                    story.story_id,
                    peer_ref,
                    story.media_kind.as_deref().unwrap_or("unknown"),
                    story.expire_date_unix,
                ),
                _ => "—".to_string(),
            },
        ),
        MutationAction::EditStory {
            peer_ref,
            before,
            artifact,
            target_media_kind,
            caption,
            privacy,
        } => (
            "РЕДАКТИРОВАНИЕ TELEGRAM STORY",
            format!(
                "Peer: {peer_ref}\nStory: {}\nMedia: {}\nCaption: {}",
                before.story.story_id,
                before.story.media_kind.as_deref().unwrap_or("unknown"),
                before.caption.as_deref().unwrap_or("[нет]")
            ),
            format!(
                "Новый файл: {}\nНовый media kind: {}\nSHA-256: {}\nCaption: {}\nPrivacy: {}",
                artifact
                    .as_ref()
                    .map_or_else(|| "[без изменения]".to_string(), |artifact| artifact.file_name.clone()),
                target_media_kind.as_deref().unwrap_or("[без изменения]"),
                artifact.as_ref().map_or("[нет]", |artifact| artifact.sha256.as_str()),
                caption.as_deref().unwrap_or("[без изменения]"),
                privacy.as_ref().map_or("[без изменения]", |privacy| privacy.kind.as_str()),
            ),
            match &approval.result {
                Some(crate::domain::MutationResult::StoryEdited {
                    peer_ref,
                    story,
                    source_sha256,
                }) => format!(
                    "Story {} изменена в {}\nMedia: {}\nSHA-256 источника: {}",
                    story.story_id,
                    peer_ref,
                    story.media_kind.as_deref().unwrap_or("unknown"),
                    source_sha256.as_deref().unwrap_or("[не менялся]")
                ),
                _ => "—".to_string(),
            },
        ),
        MutationAction::ReportStories {
            peer_ref,
            before,
            story_ids,
            option_hex,
            message,
        } => (
            "ЖАЛОБА НА TELEGRAM STORIES",
            format!(
                "Peer: {peer_ref}\nStories: {}\nТочных снимков: {}",
                story_ids.iter().map(i32::to_string).collect::<Vec<_>>().join(", "),
                before.len()
            ),
            format!(
                "Opaque option_hex: {}\nКомментарий: {}\n\nКаждый шаг flow требует отдельного подтверждения. После неопределённого ответа жалоба автоматически не повторяется.",
                if option_hex.is_empty() { "[начать flow]" } else { option_hex },
                if message.is_empty() { "[нет]" } else { message },
            ),
            match &approval.result {
                Some(crate::domain::MutationResult::StoryReportStep {
                    peer_ref,
                    story_ids,
                    report,
                }) => format!(
                    "Peer: {peer_ref}\nStories: {}\n{}",
                    story_ids.iter().map(i32::to_string).collect::<Vec<_>>().join(", "),
                    story_report_result_label(report)
                ),
                _ => "—".to_string(),
            },
        ),
        MutationAction::DeleteStories { peer_ref, before, story_ids } => (
            "УДАЛЕНИЕ TELEGRAM STORIES",
            format!(
                "Peer: {peer_ref}\nStories: {}",
                story_ids.iter().map(i32::to_string).collect::<Vec<_>>().join(", ")
            ),
            format!(
                "Будут безвозвратно удалены {} stories, связанные с точными снимками identity/date/media.",
                before.len()
            ),
            match &approval.result {
                Some(crate::domain::MutationResult::StoriesDeleted {
                    peer_ref,
                    story_ids,
                }) => format!(
                    "Удалено из {peer_ref}: {}",
                    story_ids.iter().map(i32::to_string).collect::<Vec<_>>().join(", ")
                ),
                _ => "—".to_string(),
            },
        ),
        MutationAction::ExportInviteLink {
            chat_id,
            before_handles,
            expire_date_unix,
            usage_limit,
            request_needed,
            title,
        } => (
            "СОЗДАНИЕ НОВОЙ СЕКРЕТНОЙ ССЫЛКИ-ПРИГЛАШЕНИЯ TELEGRAM",
            format!("Чат: {chat_id}"),
            format!(
                "Срок действия (Unix): {}\nЛимит использований: {}\nВступление по заявке: {request_needed}\nНазвание: {}\nПолный снимок прежних opaque handles: {}\n\nСама Telegram-ссылка не передаётся в MCP и не показывается в этой карточке.",
                expire_date_unix.map_or_else(|| "без срока".to_string(), |value| value.to_string()),
                usage_limit.map_or_else(|| "без лимита".to_string(), |value| value.to_string()),
                title.as_deref().unwrap_or("нет"),
                before_handles.len(),
            ),
            match &approval.result {
                Some(crate::domain::MutationResult::InviteLinkExported { invite }) => format!(
                    "Создан opaque handle: {}\nRevoked: {}\nRequest needed: {}",
                    invite.invite_handle, invite.revoked, invite.request_needed
                ),
                _ => "—".to_string(),
            },
        ),
        MutationAction::RevokeInviteLink { chat_id, before } => (
            "ОТЗЫВ ССЫЛКИ-ПРИГЛАШЕНИЯ TELEGRAM",
            format!("Чат: {chat_id}\nOpaque handle: {}", before.invite_handle),
            format!(
                "Название: {}\nСрок действия (Unix): {}\nЛимит использований: {}\nИспользовано: {}\nПосле отзыва ссылка перестанет принимать новые вступления.",
                before.title.as_deref().unwrap_or("нет"),
                before.expire_date_unix.map_or_else(|| "без срока".to_string(), |value| value.to_string()),
                before.usage_limit.map_or_else(|| "без лимита".to_string(), |value| value.to_string()),
                before.usage_count.map_or_else(|| "неизвестно".to_string(), |value| value.to_string()),
            ),
            match &approval.result {
                Some(crate::domain::MutationResult::InviteLinkRevoked { invite }) => format!(
                    "Отозван opaque handle: {}\nRevoked: {}",
                    invite.invite_handle, invite.revoked
                ),
                _ => "—".to_string(),
            },
        ),
        MutationAction::DeleteRevokedInviteLink { chat_id, before } => (
            "БЕЗВОЗВРАТНОЕ УДАЛЕНИЕ ОТОЗВАННОЙ ССЫЛКИ-ПРИГЛАШЕНИЯ",
            format!("Чат: {chat_id}\nOpaque handle: {}", before.invite_handle),
            format!(
                "Название: {}\nДата создания (Unix): {}\nRevoked: {}\nБудет удалена только уже отозванная запись; сама Telegram-ссылка в MCP не раскрывается.",
                before.title.as_deref().unwrap_or("нет"),
                before.date_unix.map_or_else(|| "неизвестно".to_string(), |value| value.to_string()),
                before.revoked,
            ),
            match &approval.result {
                Some(crate::domain::MutationResult::RevokedInviteLinkDeleted { invite_handle }) => {
                    format!("Удалён opaque handle: {invite_handle}")
                },
                _ => "—".to_string(),
            },
        ),
        MutationAction::ProcessJoinRequest { chat_id, before, approve } => (
            if *approve {
                "ОДОБРЕНИЕ ЗАЯВКИ НА ВСТУПЛЕНИЕ В TELEGRAM-ЧАТ"
            } else {
                "ОТКЛОНЕНИЕ ЗАЯВКИ НА ВСТУПЛЕНИЕ В TELEGRAM-ЧАТ"
            },
            format!("Чат: {chat_id}\nПользователь: {}", before.user_id),
            format!(
                "Дата заявки (Unix): {}\nЧерез папку чатов: {}\nOpaque invite handle: {}\nРешение: {}\n\nТекст about заявителя не входит в исполняемую мутацию.",
                before.date_unix,
                before.via_chatlist,
                before.source_invite_handle.as_deref().unwrap_or("не привязан"),
                if *approve { "одобрить" } else { "отклонить/скрыть" },
            ),
            match &approval.result {
                Some(crate::domain::MutationResult::JoinRequestProcessed {
                    user_id,
                    approved,
                }) => format!("user_id {user_id}, approved: {approved}"),
                _ => "—".to_string(),
            },
        ),
        MutationAction::ProcessAllJoinRequests {
            chat_id,
            invite_handle,
            before,
            approve,
        } => {
            let preview = before
                .iter()
                .take(50)
                .map(|request| format!("{}@{}", request.user_id, request.date_unix))
                .collect::<Vec<_>>()
                .join(", ");
            let omitted = before.len().saturating_sub(50);
            (
                if *approve {
                    "ОДОБРЕНИЕ ЗАФИКСИРОВАННОГО НАБОРА ЗАЯВОК TELEGRAM"
                } else {
                    "ОТКЛОНЕНИЕ ЗАФИКСИРОВАННОГО НАБОРА ЗАЯВОК TELEGRAM"
                },
                format!(
                    "Чат: {chat_id}\nOpaque invite filter: {}",
                    invite_handle.as_deref().unwrap_or("все ссылки")
                ),
                format!(
                    "Заявок в неизменяемом снимке: {}\nРешение: {}\nПервые identity user_id@date: {}{}\nPayload SHA-256: {}\n\nНовые заявки после подготовки не входят в это одобрение.",
                    before.len(),
                    if *approve { "одобрить" } else { "отклонить/скрыть" },
                    preview,
                    if omitted > 0 {
                        format!(" … и ещё {omitted}")
                    } else {
                        String::new()
                    },
                    approval.payload_sha256,
                ),
                match &approval.result {
                    Some(crate::domain::MutationResult::JoinRequestsProcessed {
                        user_ids,
                        approved,
                    }) => format!(
                        "Обработано identity: {}\nApproved: {approved}",
                        user_ids.len()
                    ),
                    _ => "—".to_string(),
                },
            )
        },
        MutationAction::TerminateSession { before } => (
            "ЗАВЕРШЕНИЕ TELEGRAM-СЕССИИ",
            format!(
                "authorization_id: {}\nУстройство: {}\nПриложение: {} {}",
                before.authorization_id, before.device_model, before.app_name, before.app_version
            ),
            format!(
                "Платформа: {} {}\nIP: {}\nРегион: {}, {}\nПоследняя активность (Unix): {}\nТекущая сессия: {}",
                before.platform,
                before.system_version,
                before.ip,
                before.country,
                before.region,
                before.date_active_unix,
                before.current
            ),
            match &approval.result {
                Some(crate::domain::MutationResult::SessionTerminated { authorization_id }) => {
                    format!("Завершена authorization_id: {authorization_id}")
                },
                _ => "—".to_string(),
            },
        ),
        MutationAction::TerminateAllOtherSessions { before } => {
            let sessions = before
                .iter()
                .map(|session| {
                    format!(
                        "{} — {} / {} {} / {} {} / IP {} / active {}",
                        session.authorization_id,
                        session.device_model,
                        session.app_name,
                        session.app_version,
                        session.platform,
                        session.system_version,
                        session.ip,
                        session.date_active_unix,
                    )
                })
                .collect::<Vec<_>>()
                .join("\n");
            (
                "ЗАВЕРШЕНИЕ ВСЕХ ОСТАЛЬНЫХ TELEGRAM-СЕССИЙ",
                format!(
                    "Аккаунт: user_id {}\nЗавершаемых авторизаций: {}",
                    approval.account_user_id,
                    before.len()
                ),
                format!(
                    "Точный снимок:\n{sessions}\n\nТекущая MCP-сессия не входит в действие. Новая или изменившаяся авторизация после подготовки блокирует применение. При неопределённом receipt повтор разрешён только после полного readback этого снимка."
                ),
                match &approval.result {
                    Some(crate::domain::MutationResult::OtherSessionsTerminated {
                        authorization_ids,
                    }) => format!(
                        "Завершены authorization_id: {}",
                        authorization_ids
                            .iter()
                            .map(i64::to_string)
                            .collect::<Vec<_>>()
                            .join(", ")
                    ),
                    _ => "—".to_string(),
                },
            )
        },
        MutationAction::SetChatAutoDelete { before, period_seconds } => (
            "ИЗМЕНЕНИЕ AUTO-DELETE TELEGRAM-ЧАТА",
            format!(
                "Чат: {}\nТекущий период: {} секунд",
                before.chat_id, before.period_seconds
            ),
            if *period_seconds == 0 {
                "Новый период: отключено (0 секунд). Новые сообщения больше не будут автоматически удаляться по таймеру чата.".to_string()
            } else {
                format!(
                    "Новый период: {period_seconds} секунд. Будущие сообщения в этом чате будут автоматически удаляться Telegram после истечения периода."
                )
            },
            match &approval.result {
                Some(crate::domain::MutationResult::ChatAutoDeleteUpdated { state }) => format!(
                    "Чат: {}\nПериод после readback: {} секунд",
                    state.chat_id, state.period_seconds
                ),
                _ => "—".to_string(),
            },
        ),
        MutationAction::ForwardMessages {
            source_chat_id,
            messages,
            destination_chat_id,
            silent,
            drop_author,
            drop_media_captions,
            noforwards,
            random_ids: _,
        } => (
            if *drop_author {
                "КОПИРОВАНИЕ СООБЩЕНИЙ"
            } else {
                "ПЕРЕСЫЛКА СООБЩЕНИЙ"
            },
            format!("Из чата: {source_chat_id}\nВ чат: {destination_chat_id}"),
            format!(
                "Без уведомления: {silent}\nУдалить авторство: {drop_author}\nУдалить подписи медиа: {drop_media_captions}\nЗапретить дальнейшую пересылку: {noforwards}\n\nТочные исходные сообщения:\n{}",
                messages
                    .iter()
                    .map(|message| format!(
                        "message_id {} (media: {}):\n────────────\n{}\n────────────",
                        message.message_id,
                        message.media_type.as_deref().unwrap_or("нет"),
                        message.text.as_deref().unwrap_or("[нет текста]")
                    ))
                    .collect::<Vec<_>>()
                    .join("\n")
            ),
            match &approval.result {
                Some(crate::domain::MutationResult::MessagesForwarded {
                    source_message_ids,
                    destination_message_ids,
                }) => format!(
                    "Исходные message_id: {}\nНовые message_id: {}",
                    source_message_ids.iter().map(i32::to_string).collect::<Vec<_>>().join(", "),
                    destination_message_ids
                        .iter()
                        .map(i32::to_string)
                        .collect::<Vec<_>>()
                        .join(", ")
                ),
                _ => "—".to_string(),
            },
        ),
        MutationAction::SendRichMedia {
            chat_id,
            media,
            caption,
            reply_to_message_id,
            topic_id,
            silent,
            noforwards,
            random_id: _,
        } => {
            let media_details = match media {
                RichMediaSpec::Location {
                    point,
                    live_period_seconds,
                    heading_degrees,
                    proximity_notification_radius_m,
                } => format!(
                    "Тип: {}\nLatitude e7: {} ({:.7})\nLongitude e7: {} ({:.7})\nРадиус точности: {} м\nLive period: {} сек\nHeading: {}°\nProximity radius: {} м",
                    if live_period_seconds.is_some() {
                        "live_location"
                    } else {
                        "location"
                    },
                    point.latitude_e7,
                    f64::from(point.latitude_e7) / 10_000_000.0,
                    point.longitude_e7,
                    f64::from(point.longitude_e7) / 10_000_000.0,
                    point
                        .accuracy_radius_m
                        .map_or_else(|| "не задан".to_string(), |value| value.to_string()),
                    live_period_seconds
                        .map_or_else(|| "не задан".to_string(), |value| value.to_string()),
                    heading_degrees
                        .map_or_else(|| "не задан".to_string(), |value| value.to_string()),
                    proximity_notification_radius_m
                        .map_or_else(|| "не задан".to_string(), |value| value.to_string()),
                ),
                RichMediaSpec::Venue {
                    point,
                    title,
                    address,
                    provider,
                    venue_id,
                    venue_type,
                } => format!(
                    "Тип: venue\nНазвание: {title}\nАдрес: {address}\nLatitude e7: {} ({:.7})\nLongitude e7: {} ({:.7})\nРадиус точности: {} м\nProvider: {provider}\nVenue ID: {venue_id}\nVenue type: {venue_type}",
                    point.latitude_e7,
                    f64::from(point.latitude_e7) / 10_000_000.0,
                    point.longitude_e7,
                    f64::from(point.longitude_e7) / 10_000_000.0,
                    point
                        .accuracy_radius_m
                        .map_or_else(|| "не задан".to_string(), |value| value.to_string()),
                ),
                RichMediaSpec::Contact {
                    phone_number,
                    first_name,
                    last_name,
                    vcard,
                } => format!(
                    "Тип: contact\nТелефон: {phone_number}\nИмя: {first_name}\nФамилия: {}\n\nТочный vCard:\n────────────\n{}\n────────────",
                    if last_name.is_empty() { "[нет]" } else { last_name },
                    if vcard.is_empty() { "[нет]" } else { vcard },
                ),
                RichMediaSpec::Dice { emoji } => {
                    format!("Тип: dice\nEmoji из live capability catalog: {emoji}")
                },
            };
            (
                "ОТПРАВКА RICH MEDIA",
                format!(
                    "Чат: {chat_id}\nForum topic: {}\nОтвет на сообщение: {}",
                    topic_id.map_or_else(|| "нет".to_string(), |topic_id| topic_id.to_string()),
                    reply_to_message_id.map_or_else(
                        || "нет".to_string(),
                        |message_id| message_id.to_string()
                    )
                ),
                format!(
                    "{media_details}\nТихо: {silent}\nЗапрет пересылки: {noforwards}\n\nТочная подпись:\n────────────\n{}\n────────────",
                    if caption.is_empty() { "[без подписи]" } else { caption }
                ),
                match &approval.result {
                    Some(crate::domain::MutationResult::RichMediaSent { message_id, media }) => {
                        format!(
                            "Отправлен message_id: {message_id}, dice value: {}",
                            media
                                .dice_value
                                .map_or_else(|| "не применимо".to_string(), |value| value.to_string())
                        )
                    },
                    _ => "—".to_string(),
                },
            )
        },
        MutationAction::SendFile {
            chat_id,
            artifact,
            caption,
            reply_to_message_id,
            mode,
            random_id: _,
        } => (
            "ОТПРАВКА ФАЙЛА",
            format!(
                "Чат: {chat_id}\nОтвет на сообщение: {}",
                reply_to_message_id
                    .map_or_else(|| "нет".to_string(), |message_id| message_id.to_string())
            ),
            format!(
                "Имя: {}\nРазмер: {} байт\nРежим: {:?}\nSHA-256 файла: {}\n\nТочная подпись:\n────────────\n{}\n────────────",
                artifact.file_name,
                artifact.size_bytes,
                mode,
                artifact.sha256,
                if caption.is_empty() { "[без подписи]" } else { caption }
            ),
            match &approval.result {
                Some(crate::domain::MutationResult::FileSent { message_id, sha256 }) => {
                    format!("Отправлен message_id: {message_id}, SHA-256: {sha256}")
                },
                _ => "—".to_string(),
            },
        ),
        MutationAction::SendVoice {
            chat_id,
            artifact,
            caption,
            caption_parse_mode,
            reply_to_message_id,
            topic_id,
            silent,
            noforwards,
            random_id: _,
        } => (
            "ОТПРАВКА VOICE NOTE",
            format!(
                "Чат: {chat_id}\nForum topic: {}\nОтвет на сообщение: {}",
                topic_id.map_or_else(|| "нет".to_string(), |topic_id| topic_id.to_string()),
                reply_to_message_id
                    .map_or_else(|| "нет".to_string(), |message_id| message_id.to_string())
            ),
            format!(
                "Имя: {}\nРазмер: {} байт\nMIME-роль: voice\nParse mode: {:?}\nТихо: {silent}\nЗапрет пересылки: {noforwards}\nSHA-256 файла: {}\n\nТочная исходная подпись:\n────────────\n{}\n────────────",
                artifact.file_name,
                artifact.size_bytes,
                caption_parse_mode,
                artifact.sha256,
                if caption.is_empty() { "[без подписи]" } else { caption }
            ),
            match &approval.result {
                Some(crate::domain::MutationResult::VoiceSent {
                    message_id,
                    sha256,
                    media,
                }) => format!(
                    "Отправлен message_id: {message_id}, SHA-256: {sha256}, MIME: {}, duration: {} сек",
                    media.mime_type,
                    media
                        .duration_seconds
                        .map_or_else(|| "не определена".to_string(), |value| value.to_string())
                ),
                _ => "—".to_string(),
            },
        ),
        MutationAction::SendVideoNote {
            chat_id,
            artifact,
            duration_seconds,
            length_pixels,
            reply_to_message_id,
            topic_id,
            silent,
            noforwards,
            random_id: _,
        } => (
            "ОТПРАВКА VIDEO NOTE",
            format!(
                "Чат: {chat_id}\nForum topic: {}\nОтвет на сообщение: {}",
                topic_id.map_or_else(|| "нет".to_string(), |topic_id| topic_id.to_string()),
                reply_to_message_id
                    .map_or_else(|| "нет".to_string(), |message_id| message_id.to_string())
            ),
            format!(
                "Имя: {}\nРазмер: {} байт\nДлительность: {duration_seconds} сек\nКруглый кадр: {length_pixels}×{length_pixels} px\nТихо: {silent}\nЗапрет пересылки: {noforwards}\nSHA-256 файла: {}",
                artifact.file_name, artifact.size_bytes, artifact.sha256
            ),
            match &approval.result {
                Some(crate::domain::MutationResult::VideoNoteSent {
                    message_id,
                    sha256,
                    media,
                }) => format!(
                    "Отправлен message_id: {message_id}, SHA-256: {sha256}, MIME: {}, duration: {} сек, {}×{} px",
                    media.mime_type,
                    media
                        .duration_seconds
                        .map_or_else(|| "не определена".to_string(), |value| value.to_string()),
                    media.width_px.map_or_else(|| "?".to_string(), |value| value.to_string()),
                    media.height_px.map_or_else(|| "?".to_string(), |value| value.to_string())
                ),
                _ => "—".to_string(),
            },
        ),
        MutationAction::SendAlbum {
            chat_id,
            items,
            caption_parse_mode,
            reply_to_message_id,
            topic_id,
            silent,
            noforwards,
            random_ids: _,
        } => (
            "ОТПРАВКА АЛЬБОМА",
            format!(
                "Чат: {chat_id}\nForum topic: {}\nОтвет на сообщение: {}\nParse mode: {:?}\nТихо: {silent}\nЗапрет пересылки: {noforwards}",
                topic_id.map_or_else(|| "нет".to_string(), |topic_id| topic_id.to_string()),
                reply_to_message_id
                    .map_or_else(|| "нет".to_string(), |message_id| message_id.to_string()),
                caption_parse_mode
            ),
            items
                .iter()
                .enumerate()
                .map(|(index, item)| {
                    format!(
                        "{}. {:?} {} ({} байт)\nSHA-256: {}\nПодпись:\n────────────\n{}\n────────────",
                        index.saturating_add(1),
                        item.media_kind,
                        item.artifact.file_name,
                        item.artifact.size_bytes,
                        item.artifact.sha256,
                        if item.caption.is_empty() { "[без подписи]" } else { item.caption.as_str() }
                    )
                })
                .collect::<Vec<_>>()
                .join("\n"),
            match &approval.result {
                Some(crate::domain::MutationResult::AlbumSent {
                    message_ids,
                    grouped_id,
                    sha256s,
                }) => format!(
                    "Отправлены message_id: {}\n grouped_id: {grouped_id}\nSHA-256: {}",
                    message_ids.iter().map(i32::to_string).collect::<Vec<_>>().join(", "),
                    sha256s.join(", ")
                ),
                _ => "—".to_string(),
            },
        ),
        MutationAction::SendSticker {
            chat_id,
            sticker,
            reply_to_message_id,
            silent,
            noforwards,
            random_id: _,
        } => (
            "ОТПРАВКА СТИКЕРА",
            format!(
                "Чат: {chat_id}\nОтвет на сообщение: {}",
                reply_to_message_id
                    .map_or_else(|| "нет".to_string(), |message_id| message_id.to_string())
            ),
            format!(
                "Набор: {}\nOpaque handle: {}\nEmoji: {}\nMIME: {}\nРазмер: {} байт\nТихо: {silent}\nЗапрет пересылки: {noforwards}",
                sticker.set_short_name.as_deref().unwrap_or("не определён"),
                sticker.sticker_handle,
                sticker.emoji.as_deref().unwrap_or("нет"),
                sticker.mime_type,
                sticker.size_bytes,
            ),
            match &approval.result {
                Some(crate::domain::MutationResult::StickerSent {
                    message_id,
                    sticker_handle,
                }) => format!(
                    "Отправлен message_id: {message_id}, opaque handle: {sticker_handle}"
                ),
                _ => "—".to_string(),
            },
        ),
        MutationAction::SetStickerFavorite {
            sticker,
            before_favorite,
            favorite,
        } => (
            "ИЗМЕНЕНИЕ ИЗБРАННОГО СТИКЕРА",
            format!(
                "Набор: {}\nOpaque handle: {}",
                sticker.set_short_name.as_deref().unwrap_or("не определён"),
                sticker.sticker_handle,
            ),
            format!(
                "Было в избранном: {before_favorite}\nСтанет в избранном: {favorite}\nEmoji: {}\nMIME: {}\nРазмер: {} байт",
                sticker.emoji.as_deref().unwrap_or("нет"),
                sticker.mime_type,
                sticker.size_bytes,
            ),
            match &approval.result {
                Some(crate::domain::MutationResult::StickerFavoriteUpdated {
                    sticker_handle,
                    favorite,
                }) => format!("Opaque handle: {sticker_handle}, в избранном: {favorite}"),
                _ => "—".to_string(),
            },
        ),
        MutationAction::SetRecentSticker {
            sticker,
            attached,
            before_saved,
            saved,
        } => (
            "ИЗМЕНЕНИЕ НЕДАВНЕГО СТИКЕРА",
            format!(
                "Набор: {}\nOpaque handle: {}",
                sticker.set_short_name.as_deref().unwrap_or("не определён"),
                sticker.sticker_handle,
            ),
            format!(
                "Список прикреплённых: {attached}\nБыл сохранён: {before_saved}\nСтанет сохранён: {saved}\nEmoji: {}\nMIME: {}\nРазмер: {} байт",
                sticker.emoji.as_deref().unwrap_or("нет"),
                sticker.mime_type,
                sticker.size_bytes,
            ),
            match &approval.result {
                Some(crate::domain::MutationResult::RecentStickerUpdated {
                    sticker_handle,
                    attached,
                    saved,
                }) => format!(
                    "Opaque handle: {sticker_handle}, список прикреплённых: {attached}, сохранён: {saved}"
                ),
                _ => "—".to_string(),
            },
        ),
        MutationAction::CreateBusinessChatLink { content } => (
            "СОЗДАНИЕ TELEGRAM BUSINESS CHAT LINK",
            "Аккаунт создаст новую Business-ссылку; её slug и URL назначит Telegram."
                .to_string(),
            format!(
                "{}\n\nURL из результата считается недоверенным и не открывается автоматически. При потере ответа после создания автоматический повтор отключён, чтобы не создать дубликат.",
                business_chat_link_content_label(content)
            ),
            match &approval.result {
                Some(crate::domain::MutationResult::BusinessChatLinkCreated { link }) => {
                    business_chat_link_target_label(link)
                },
                _ => "—".to_string(),
            },
        ),
        MutationAction::EditBusinessChatLink { before, content } => (
            "ИЗМЕНЕНИЕ TELEGRAM BUSINESS CHAT LINK",
            business_chat_link_target_label(before),
            format!(
                "Было сообщение:\n────────────\n{}\n────────────\n\nСтанет:\n{}",
                one_line_preview(&before.message, 700),
                business_chat_link_content_label(content)
            ),
            match &approval.result {
                Some(crate::domain::MutationResult::BusinessChatLinkUpdated { link }) => {
                    business_chat_link_target_label(link)
                },
                _ => "—".to_string(),
            },
        ),
        MutationAction::DeleteBusinessChatLink { before } => (
            "УДАЛЕНИЕ TELEGRAM BUSINESS CHAT LINK",
            business_chat_link_target_label(before),
            format!(
                "Текущее сообщение:\n────────────\n{}\n────────────\n\nПосле удаления эта точная ссылка перестанет быть доступна. Автоматическое восстановление проверяет отсутствие slug перед повтором; URL не открывается.",
                one_line_preview(&before.message, 1_100),
            ),
            match &approval.result {
                Some(crate::domain::MutationResult::BusinessChatLinkDeleted { slug }) => {
                    format!("Удалён slug: {slug}")
                },
                _ => "—".to_string(),
            },
        ),
        MutationAction::SetBusinessWorkHours { before, target } => (
            if target.is_some() {
                "ИЗМЕНЕНИЕ РАБОЧИХ ЧАСОВ TELEGRAM BUSINESS"
            } else {
                "ОЧИСТКА РАБОЧИХ ЧАСОВ TELEGRAM BUSINESS"
            },
            format!("Было:\n{}", business_work_hours_config_label(before.as_ref())),
            format!(
                "Станет:\n{}\n\nИнтервалы нормализованы в точные минуты Telegram-недели. Флаг open_now не принимается: его вычисляет Telegram. При неопределённом receipt повтор разрешён только после readback исходного состояния.",
                business_work_hours_config_label(target.as_ref()),
            ),
            match &approval.result {
                Some(crate::domain::MutationResult::BusinessWorkHoursUpdated { work_hours }) => {
                    business_work_hours_result_label(work_hours.as_ref())
                },
                _ => "—".to_string(),
            },
        ),
        MutationAction::SetBusinessLocation { before, target } => (
            if target.is_some() {
                "ИЗМЕНЕНИЕ АДРЕСА TELEGRAM BUSINESS"
            } else {
                "ОЧИСТКА АДРЕСА TELEGRAM BUSINESS"
            },
            format!("Было:\n{}", business_location_label(before.as_ref())),
            format!(
                "Станет:\n{}\n\nКоординаты привязаны как точные E7-значения. При неопределённом receipt повтор разрешён только после readback исходного состояния.",
                business_location_label(target.as_ref()),
            ),
            match &approval.result {
                Some(crate::domain::MutationResult::BusinessLocationUpdated { location }) => {
                    business_location_label(location.as_ref())
                },
                _ => "—".to_string(),
            },
        ),
        MutationAction::SetBusinessGreeting { before, target } => (
            if target.is_some() {
                "ИЗМЕНЕНИЕ ПРИВЕТСТВИЯ TELEGRAM BUSINESS"
            } else {
                "ОТКЛЮЧЕНИЕ ПРИВЕТСТВИЯ TELEGRAM BUSINESS"
            },
            format!("Было:\n{}", business_greeting_label(before.as_ref())),
            format!(
                "Станет:\n{}\n\nСообщение берётся из существующего quick-reply shortcut. Полный immutable payload, включая весь список user_id, связан SHA-256 ниже; при неопределённом receipt повтор разрешён только после readback исходного состояния.",
                business_greeting_label(target.as_ref()),
            ),
            match &approval.result {
                Some(crate::domain::MutationResult::BusinessGreetingUpdated { greeting }) => {
                    business_greeting_label(greeting.as_ref())
                },
                _ => "—".to_string(),
            },
        ),
        MutationAction::SetBusinessAway { before, target } => (
            if target.is_some() {
                "ИЗМЕНЕНИЕ AWAY-СООБЩЕНИЯ TELEGRAM BUSINESS"
            } else {
                "ОТКЛЮЧЕНИЕ AWAY-СООБЩЕНИЯ TELEGRAM BUSINESS"
            },
            format!("Было:\n{}", business_away_label(before.as_ref())),
            format!(
                "Станет:\n{}\n\nСообщение берётся из существующего quick-reply shortcut. Полный immutable payload, включая весь список user_id, связан SHA-256 ниже; при неопределённом receipt повтор разрешён только после readback исходного состояния.",
                business_away_label(target.as_ref()),
            ),
            match &approval.result {
                Some(crate::domain::MutationResult::BusinessAwayUpdated { away }) => {
                    business_away_label(away.as_ref())
                },
                _ => "—".to_string(),
            },
        ),
        MutationAction::SetBusinessIntro { before, target } => (
            if target.is_some() {
                "ИЗМЕНЕНИЕ INTRO TELEGRAM BUSINESS"
            } else {
                "ОЧИСТКА INTRO TELEGRAM BUSINESS"
            },
            format!("Было:\n{}", business_intro_summary_label(before.as_ref())),
            format!(
                "Станет:\n{}\n\nСтикер выбирается только по проверенному набору и opaque handle. Raw document_id, access_hash и file_reference не принимаются. При неопределённом receipt повтор разрешён только после readback исходного состояния.",
                business_intro_spec_label(target.as_ref()),
            ),
            match &approval.result {
                Some(crate::domain::MutationResult::BusinessIntroUpdated { intro }) => {
                    business_intro_summary_label(intro.as_ref())
                },
                _ => "—".to_string(),
            },
        ),
        MutationAction::SetStarGiftSaved { before, unsaved } => (
            if *unsaved {
                "СКРЫТИЕ TELEGRAM STAR GIFT"
            } else {
                "ПОКАЗ TELEGRAM STAR GIFT"
            },
            format!(
                "Stars-кошелёк: {}\nТочная ссылка: {}",
                before.wallet_peer_ref,
                saved_star_gift_reference_label(&before.reference)
            ),
            format!(
                "Подарок: {}\nБыл скрыт: {}\nСтанет скрыт: {}",
                star_gift_identity_label(&before.gift.gift),
                before.gift.unsaved,
                unsaved
            ),
            match &approval.result {
                Some(crate::domain::MutationResult::StarGiftSavedStateUpdated { gift }) => {
                    format!(
                        "{}; скрыт: {}",
                        saved_star_gift_reference_label(&gift.reference),
                        gift.gift.unsaved
                    )
                },
                _ => "—".to_string(),
            },
        ),
        MutationAction::ConvertStarGift {
            before,
            before_balance,
            convert_stars,
        } => (
            "НЕОБРАТИМАЯ КОНВЕРТАЦИЯ TELEGRAM STAR GIFT",
            format!(
                "Stars-кошелёк: {}\nТочная ссылка: {}",
                before.wallet_peer_ref,
                saved_star_gift_reference_label(&before.reference)
            ),
            format!(
                "Подарок: {}\nБудет безвозвратно удалён и конвертирован в {convert_stars} Stars.\nБаланс до операции: {} Stars, nanos: {}\nПри потере ответа Telegram автоматический повтор отключён.",
                star_gift_identity_label(&before.gift.gift),
                before_balance.amount,
                before_balance.nanos.map_or_else(|| "нет".to_string(), |value| value.to_string())
            ),
            match &approval.result {
                Some(crate::domain::MutationResult::StarGiftConverted {
                    reference,
                    convert_stars,
                    wallet_balance,
                }) => format!(
                    "{} конвертирован в {convert_stars} Stars; текущий баланс: {} Stars, nanos: {}",
                    saved_star_gift_reference_label(reference),
                    wallet_balance.amount,
                    wallet_balance.nanos.map_or_else(|| "нет".to_string(), |value| value.to_string())
                ),
                _ => "—".to_string(),
            },
        ),
        MutationAction::SetStarsSubscriptionCanceled {
            wallet_peer_ref,
            before,
            canceled,
        } => (
            if *canceled {
                "ОТМЕНА TELEGRAM STARS-ПОДПИСКИ"
            } else {
                "ВОССТАНОВЛЕНИЕ TELEGRAM STARS-ПОДПИСКИ"
            },
            format!(
                "Stars-кошелёк: {wallet_peer_ref}\nПодписка: {}\nПолучатель: {}",
                before.subscription_id, before.peer_ref
            ),
            format!(
                "Название: {}\nСтоимость: {} Stars каждые {} секунд\nДействует до Unix: {}\nБыла отменена: {}\nСтанет отменена: {}",
                before.title.as_deref().unwrap_or("нет"),
                before.pricing.amount_stars,
                before.pricing.period_seconds,
                before.until_date_unix,
                before.canceled,
                canceled
            ),
            match &approval.result {
                Some(crate::domain::MutationResult::StarsSubscriptionUpdated {
                    wallet_peer_ref,
                    subscription,
                }) => format!(
                    "Stars-кошелёк: {wallet_peer_ref}; подписка: {}; отменена: {}",
                    subscription.subscription_id, subscription.canceled
                ),
                _ => "—".to_string(),
            },
        ),
        MutationAction::SetStickerSetInstalled {
            sticker_set,
            before_installed,
            installed,
            archived,
        } => (
            if *installed {
                "УСТАНОВКА НАБОРА СТИКЕРОВ"
            } else {
                "УДАЛЕНИЕ НАБОРА СТИКЕРОВ"
            },
            format!(
                "Набор: {}\nShort name: {}\nID: {}",
                sticker_set.title, sticker_set.short_name, sticker_set.sticker_set_id,
            ),
            format!(
                "Был установлен: {before_installed}\nСтанет установлен: {installed}\nАрхивный режим: {archived}\nСтикеров: {}\nContent hash: {}",
                sticker_set.sticker_count, sticker_set.content_hash,
            ),
            match &approval.result {
                Some(crate::domain::MutationResult::StickerSetInstallationUpdated {
                    short_name,
                    installed,
                    archived,
                }) => format!(
                    "Short name: {short_name}, установлен: {installed}, архивный режим: {archived}"
                ),
                _ => "—".to_string(),
            },
        ),
        MutationAction::ApplyBoost { chat_id, slots, before } => {
            let exact_slots = slots.iter().map(i32::to_string).collect::<Vec<_>>().join(", ");
            let before_assignments = slots
                .iter()
                .map(|requested| {
                    before
                        .slots
                        .iter()
                        .find(|slot| slot.slot == *requested)
                        .map_or_else(
                            || format!("{requested}: отсутствует в snapshot"),
                            |slot| {
                                format!(
                                    "{}: chat={}, cooldown={}",
                                    slot.slot,
                                    slot.chat_id.as_deref().unwrap_or("свободен"),
                                    slot.cooldown_until_unix.map_or_else(
                                        || "нет".to_string(),
                                        |value| value.to_string()
                                    )
                                )
                            },
                        )
                })
                .collect::<Vec<_>>()
                .join("\n");
            let result = match &approval.result {
                Some(crate::domain::MutationResult::BoostApplied {
                    chat_id,
                    slots,
                    boosts,
                }) => {
                    let confirmed = slots
                        .iter()
                        .filter_map(|requested| {
                            boosts
                                .slots
                                .iter()
                                .find(|slot| slot.slot == *requested)
                                .map(|slot| {
                                    format!(
                                        "{}→{}",
                                        slot.slot,
                                        slot.chat_id.as_deref().unwrap_or("свободен")
                                    )
                                })
                        })
                        .collect::<Vec<_>>()
                        .join(", ");
                    format!("Чат: {chat_id}; подтверждено: {confirmed}")
                },
                _ => "—".to_string(),
            };
            (
                "НАЗНАЧЕНИЕ PREMIUM BOOST",
                format!("Канал/супергруппа: {chat_id}\nТочные слоты: {exact_slots}"),
                format!("Назначения до применения:\n{before_assignments}"),
                result,
            )
        },
    };
    let text = format!(
        "{title}\n\nСтатус: {status}\n{target}\n\n{payload}\n\nSHA-256: {}\nMutation: {}\nApproval: {}\nДействует до (Unix): {}\nРезультат: {result}",
        approval.payload_sha256,
        approval.mutation_id,
        approval.approval_id,
        approval.expires_at_unix,
    );
    if text.encode_utf16().count() > MAX_APPROVAL_CARD_UTF16 {
        return Err(ApprovalBotError::known("APPROVAL_CARD_TOO_LONG"));
    }
    Ok(text)
}

fn keyboard_for(approval: &SendApproval) -> InlineKeyboardMarkup {
    let buttons = if matches!(
        approval.state,
        SendApprovalState::DispatchingCard
            | SendApprovalState::Pending
            | SendApprovalState::CardDeliveryUncertain
    ) {
        vec![
            vec![InlineKeyboardButton {
                text: "Отправить".to_string(),
                callback_data: format!("{APPROVE_PREFIX}{}", approval.approval_id),
            }],
            vec![InlineKeyboardButton {
                text: "Отмена".to_string(),
                callback_data: format!("{CANCEL_PREFIX}{}", approval.approval_id),
            }],
        ]
    } else {
        Vec::new()
    };
    InlineKeyboardMarkup { inline_keyboard: buttons }
}

fn keyboard_for_mutation(approval: &MutationApproval) -> InlineKeyboardMarkup {
    let buttons = if approval.external_owner_ready {
        Vec::new()
    } else if matches!(
        approval.state,
        MutationApprovalState::DispatchingCard
            | MutationApprovalState::Pending
            | MutationApprovalState::CardDeliveryUncertain
    ) {
        let approve_label = match &approval.action {
            MutationAction::ScheduleMessage { .. } | MutationAction::ScheduleFile { .. } => {
                "🗓 Запланировать"
            },
            MutationAction::EditScheduledMessage { .. } => "✅ Сохранить",
            MutationAction::DeleteScheduledMessages { .. } => "🗑 Удалить",
            MutationAction::SendScheduledMessagesNow { .. } => "✉️ Отправить сейчас",
            MutationAction::SendFile { .. } | MutationAction::SendStory { .. } => {
                "✅ Опубликовать"
            },
            _ => "✅ Применить",
        };
        vec![vec![
            InlineKeyboardButton {
                text: approve_label.to_string(),
                callback_data: format!("{MUTATION_APPROVE_PREFIX}{}", approval.approval_id),
            },
            InlineKeyboardButton {
                text: "❌ Отменить".to_string(),
                callback_data: format!("{MUTATION_CANCEL_PREFIX}{}", approval.approval_id),
            },
        ]]
    } else {
        Vec::new()
    };
    InlineKeyboardMarkup { inline_keyboard: buttons }
}

fn sanitize_update(raw: RawUpdate) -> ApprovalUpdate {
    let decision = raw.callback_query.and_then(|callback| {
        if callback.id.is_empty()
            || callback.id.len() > MAX_CALLBACK_QUERY_ID_BYTES
            || callback.from.id <= 0
        {
            return None;
        }
        let message = callback.message?;
        if message.message_id <= 0 || message.chat.id == 0 {
            return None;
        }
        let data = callback.data?;
        if data.is_empty() || data.len() > MAX_CALLBACK_DATA_BYTES {
            return None;
        }
        let (kind, action, approval_id) = parse_callback_data(&data)?;
        Some(ApprovalDecision {
            update_id: raw.update_id,
            callback_query_id: callback.id,
            approval_id,
            kind,
            action,
            from_user_id: callback.from.id,
            chat_id: message.chat.id,
            message_id: message.message_id,
        })
    });
    ApprovalUpdate { update_id: raw.update_id, decision }
}

fn parse_callback_data(data: &str) -> Option<(ApprovalKind, ApprovalAction, String)> {
    let (kind, action, raw_id) = if let Some(raw_id) = data.strip_prefix(APPROVE_PREFIX) {
        (ApprovalKind::Send, ApprovalAction::Approve, raw_id)
    } else if let Some(raw_id) = data.strip_prefix(CANCEL_PREFIX) {
        (ApprovalKind::Send, ApprovalAction::Cancel, raw_id)
    } else if let Some(raw_id) = data.strip_prefix(MUTATION_APPROVE_PREFIX) {
        (ApprovalKind::Mutation, ApprovalAction::Approve, raw_id)
    } else {
        let raw_id = data.strip_prefix(MUTATION_CANCEL_PREFIX)?;
        (ApprovalKind::Mutation, ApprovalAction::Cancel, raw_id)
    };
    let approval_id = Uuid::parse_str(raw_id).ok()?.to_string();
    Some((kind, action, approval_id))
}

#[derive(Debug, Serialize)]
struct SendMessageRequest {
    chat_id: i64,
    text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    parse_mode: Option<&'static str>,
    link_preview_options: LinkPreviewOptions,
    reply_markup: InlineKeyboardMarkup,
    #[serde(skip_serializing_if = "Option::is_none")]
    reply_parameters: Option<ReplyParameters>,
}
#[derive(Debug, Serialize)]
struct ReplyParameters {
    message_id: i64,
}

#[derive(Debug, Deserialize)]
struct BotApiEnvelope<T> {
    ok: bool,
    result: Option<T>,
    description: Option<String>,
}

#[derive(Debug, Serialize)]
struct SendRichMessageRequest<'a> {
    chat_id: i64,
    rich_message: RichMessageRequest<'a>,
}

#[derive(Debug, Serialize)]
struct RichMessageRequest<'a> {
    html: &'a str,
    skip_entity_detection: bool,
}

#[derive(Debug, Serialize)]
struct LinkPreviewOptions {
    is_disabled: bool,
}

#[derive(Debug, Serialize)]
struct EditMessageRequest {
    chat_id: i64,
    message_id: i64,
    text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    parse_mode: Option<&'static str>,
    reply_markup: InlineKeyboardMarkup,
}

#[derive(Debug, Serialize)]
struct EditMessageCaptionRequest {
    chat_id: i64,
    message_id: i64,
    caption: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    parse_mode: Option<&'static str>,
    reply_markup: InlineKeyboardMarkup,
}

#[derive(Debug, Serialize)]
struct GetUpdatesRequest<'a> {
    offset: i64,
    timeout: u64,
    limit: u8,
    allowed_updates: [&'a str; 1],
}

#[derive(Debug, Serialize)]
struct AnswerCallbackRequest<'a> {
    callback_query_id: &'a str,
    text: &'a str,
}

#[derive(Debug, Clone, Serialize)]
pub struct InlineKeyboardMarkup {
    inline_keyboard: Vec<Vec<InlineKeyboardButton>>,
}

#[derive(Debug, Clone, Serialize)]
struct InlineKeyboardButton {
    text: String,
    callback_data: String,
}

#[derive(Debug, Deserialize)]
struct BotMessageResult {
    message_id: i64,
}

#[derive(Debug, Deserialize)]
struct BotRichMessageResult {
    message_id: i64,
    chat: RawChat,
    rich_message: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
struct BotIdentityResult {
    id: i64,
    is_bot: bool,
    username: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RawUpdate {
    update_id: i64,
    callback_query: Option<RawCallbackQuery>,
}

#[derive(Debug, Deserialize)]
struct RawCallbackQuery {
    id: String,
    from: RawUser,
    message: Option<RawMessage>,
    data: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RawUser {
    id: i64,
}

#[derive(Debug, Deserialize)]
struct RawMessage {
    message_id: i64,
    chat: RawChat,
}

#[derive(Debug, Deserialize)]
struct RawChat {
    id: i64,
}

pub struct ApprovalWorkerLease {
    file: File,
}

impl ApprovalWorkerLease {
    pub fn try_acquire(state_db_path: &Path) -> io::Result<Option<Self>> {
        let lock_path = approval_lock_path(state_db_path);
        if let Some(parent) = lock_path.parent() {
            let create_private_parent = !parent.exists();
            std::fs::create_dir_all(parent)?;
            if create_private_parent {
                restrict_private_directory(parent)?;
            }
        }
        let file = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(&lock_path)?;
        restrict_private_file(&lock_path)?;
        match file.try_lock_exclusive() {
            Ok(()) => Ok(Some(Self { file })),
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => Ok(None),
            Err(error) => Err(error),
        }
    }
}

impl Drop for ApprovalWorkerLease {
    fn drop(&mut self) {
        let _ = FileExt::unlock(&self.file);
    }
}

fn approval_lock_path(state_db_path: &Path) -> PathBuf {
    let mut path = state_db_path.as_os_str().to_os_string();
    path.push(".approval-worker.lock");
    PathBuf::from(path)
}

#[derive(Debug, Clone)]
pub struct FakeApprovalCard {
    pub message_id: i64,
    pub card: ApprovalCard,
}

#[derive(Debug, Clone)]
pub struct FakeApprovalEdit {
    pub message_id: i64,
    pub card: ApprovalCard,
}

#[derive(Debug, Clone)]
pub struct FakeMutationApprovalCard {
    pub message_id: i64,
    pub card: MutationApprovalCard,
    pub rendered_text: String,
}

#[derive(Debug, Clone)]
pub struct FakeOperatorMediaCard {
    pub message_id: i64,
    pub chat_id: i64,
    pub media: CardMedia,
    pub caption_html: String,
    pub has_keyboard: bool,
    pub reply_to_message_id: Option<i64>,
}

#[derive(Debug, Clone)]
pub struct FakeMutationApprovalEdit {
    pub message_id: i64,
    pub card: MutationApprovalCard,
    pub rendered_text: String,
}

#[derive(Debug, Default)]
struct FakeApprovalState {
    next_message_id: i64,
    next_update_id: i64,
    updates: VecDeque<ApprovalUpdate>,
    sent_cards: Vec<FakeApprovalCard>,
    edits: Vec<FakeApprovalEdit>,
    sent_mutation_cards: Vec<FakeMutationApprovalCard>,
    sent_media_cards: Vec<FakeOperatorMediaCard>,
    mutation_edits: Vec<FakeMutationApprovalEdit>,
    answers: Vec<(String, String)>,
    sent_operator_rich_messages: Vec<(i64, String, OperatorRichMessageReceipt)>,
    lose_next_send_receipt: bool,
}

#[derive(Debug, Default)]
pub struct FakeApprovalBot {
    state: Mutex<FakeApprovalState>,
    notify: Notify,
}

impl FakeApprovalBot {
    pub async fn sent_cards(&self) -> Vec<FakeApprovalCard> {
        self.state.lock().await.sent_cards.clone()
    }

    pub async fn edits(&self) -> Vec<FakeApprovalEdit> {
        self.state.lock().await.edits.clone()
    }

    pub async fn sent_mutation_cards(&self) -> Vec<FakeMutationApprovalCard> {
        self.state.lock().await.sent_mutation_cards.clone()
    }

    pub async fn sent_media_cards(&self) -> Vec<FakeOperatorMediaCard> {
        self.state.lock().await.sent_media_cards.clone()
    }

    pub async fn mutation_edits(&self) -> Vec<FakeMutationApprovalEdit> {
        self.state.lock().await.mutation_edits.clone()
    }

    pub async fn answers(&self) -> Vec<(String, String)> {
        self.state.lock().await.answers.clone()
    }

    pub async fn sent_operator_rich_messages(
        &self,
    ) -> Vec<(i64, String, OperatorRichMessageReceipt)> {
        self.state.lock().await.sent_operator_rich_messages.clone()
    }

    pub async fn lose_next_send_receipt(&self) {
        self.state.lock().await.lose_next_send_receipt = true;
    }

    pub async fn push_decision(
        &self,
        approval_id: &str,
        action: ApprovalAction,
        from_user_id: i64,
        chat_id: i64,
        message_id: i64,
    ) {
        let mut state = self.state.lock().await;
        state.next_update_id = state.next_update_id.saturating_add(1).max(1);
        let update_id = state.next_update_id;
        state.updates.push_back(ApprovalUpdate {
            update_id,
            decision: Some(ApprovalDecision {
                update_id,
                callback_query_id: format!("fake-callback-{update_id}"),
                approval_id: approval_id.to_string(),
                kind: ApprovalKind::Send,
                action,
                from_user_id,
                chat_id,
                message_id,
            }),
        });
        drop(state);
        self.notify.notify_waiters();
    }

    pub async fn push_mutation_decision(
        &self,
        approval_id: &str,
        action: ApprovalAction,
        from_user_id: i64,
        chat_id: i64,
        message_id: i64,
    ) {
        let mut state = self.state.lock().await;
        state.next_update_id = state.next_update_id.saturating_add(1).max(1);
        let update_id = state.next_update_id;
        state.updates.push_back(ApprovalUpdate {
            update_id,
            decision: Some(ApprovalDecision {
                update_id,
                callback_query_id: format!("fake-callback-{update_id}"),
                approval_id: approval_id.to_string(),
                kind: ApprovalKind::Mutation,
                action,
                from_user_id,
                chat_id,
                message_id,
            }),
        });
        drop(state);
        self.notify.notify_waiters();
    }
}

#[async_trait]
impl ApprovalBot for FakeApprovalBot {
    async fn send_operator_rich_message(
        &self,
        chat_id: i64,
        html: &str,
    ) -> Result<OperatorRichMessageReceipt, ApprovalBotError> {
        let mut state = self.state.lock().await;
        state.next_message_id = state.next_message_id.saturating_add(1).max(1);
        let receipt = OperatorRichMessageReceipt {
            message_id: state.next_message_id,
            chat_id,
            bot_user_id: 42,
            bot_username: Some("fixture_agent_bot".to_string()),
            payload_sha256: format!("{:x}", Sha256::digest(html.as_bytes())),
            provider_rich_message_sha256: format!("{:x}", Sha256::digest(html.as_bytes())),
        };
        state.sent_operator_rich_messages.push((chat_id, html.to_string(), receipt.clone()));
        if state.lose_next_send_receipt {
            state.lose_next_send_receipt = false;
            return Err(ApprovalBotError::uncertain("BOT_API_TRANSPORT_UNCERTAIN"));
        }
        Ok(receipt)
    }

    async fn send_card(
        &self,
        card: &ApprovalCard,
    ) -> Result<ApprovalCardReceipt, ApprovalBotError> {
        render_card(card)?;
        let mut state = self.state.lock().await;
        state.next_message_id = state.next_message_id.saturating_add(1).max(1);
        let message_id = state.next_message_id;
        state.sent_cards.push(FakeApprovalCard { message_id, card: card.clone() });
        if state.lose_next_send_receipt {
            state.lose_next_send_receipt = false;
            return Err(ApprovalBotError::uncertain("BOT_API_TRANSPORT_UNCERTAIN"));
        }
        Ok(ApprovalCardReceipt { message_id })
    }

    async fn edit_card(
        &self,
        card: &ApprovalCard,
        message_id: i64,
    ) -> Result<(), ApprovalBotError> {
        render_card(card)?;
        self.state.lock().await.edits.push(FakeApprovalEdit { message_id, card: card.clone() });
        Ok(())
    }

    async fn send_operator_media_card(
        &self,
        chat_id: i64,
        media: &CardMedia,
        caption_html: &str,
        keyboard: Option<&InlineKeyboardMarkup>,
    ) -> Result<SentCard, ApprovalBotError> {
        if chat_id == 0 || caption_html.encode_utf16().count() > MAX_MEDIA_CAPTION_UTF16 {
            return Err(ApprovalBotError::known("BOT_MEDIA_CARD_INVALID"));
        }
        let mut state = self.state.lock().await;
        state.next_message_id = state.next_message_id.saturating_add(1).max(1);
        let message_id = state.next_message_id;
        state.sent_media_cards.push(FakeOperatorMediaCard {
            message_id,
            chat_id,
            media: media.clone(),
            caption_html: caption_html.to_string(),
            has_keyboard: keyboard.is_some_and(|keyboard| !keyboard.inline_keyboard.is_empty()),
            reply_to_message_id: None,
        });
        if state.lose_next_send_receipt {
            state.lose_next_send_receipt = false;
            return Err(ApprovalBotError::uncertain("BOT_API_TRANSPORT_UNCERTAIN"));
        }
        Ok(SentCard { message_id })
    }

    async fn send_mutation_card(
        &self,
        card: &MutationApprovalCard,
    ) -> Result<ApprovalCardReceipt, ApprovalBotError> {
        let rendered_text = render_mutation_card(card)?;
        if let Some(media) = card.media.as_ref() {
            let keyboard = keyboard_for_mutation(&card.approval);
            let has_keyboard = !keyboard.inline_keyboard.is_empty();
            let mut state = self.state.lock().await;
            if mutation_card_is_media_preview(&rendered_text) {
                state.next_message_id = state.next_message_id.saturating_add(1).max(1);
                let message_id = state.next_message_id;
                state.sent_media_cards.push(FakeOperatorMediaCard {
                    message_id,
                    chat_id: card.approval.approval_chat_id,
                    media: media.clone(),
                    caption_html: rendered_text.clone(),
                    has_keyboard,
                    reply_to_message_id: None,
                });
                if state.lose_next_send_receipt {
                    state.lose_next_send_receipt = false;
                    return Err(ApprovalBotError::uncertain("BOT_API_TRANSPORT_UNCERTAIN"));
                }
                return Ok(ApprovalCardReceipt { message_id });
            }
            state.next_message_id = state.next_message_id.saturating_add(1).max(1);
            let preview_message_id = state.next_message_id;
            state.sent_media_cards.push(FakeOperatorMediaCard {
                message_id: preview_message_id,
                chat_id: card.approval.approval_chat_id,
                media: media.clone(),
                caption_html: format!("Предпросмотр: {}", escape_html(&card_preview_file_name(card))),
                has_keyboard: false,
                reply_to_message_id: None,
            });
            state.next_message_id = state.next_message_id.saturating_add(1).max(1);
            let message_id = state.next_message_id;
            state.sent_mutation_cards.push(FakeMutationApprovalCard {
                message_id,
                card: card.clone(),
                rendered_text,
            });
            if state.lose_next_send_receipt {
                state.lose_next_send_receipt = false;
                return Err(ApprovalBotError::uncertain("BOT_API_TRANSPORT_UNCERTAIN"));
            }
            return Ok(ApprovalCardReceipt { message_id });
        }
        let mut state = self.state.lock().await;
        state.next_message_id = state.next_message_id.saturating_add(1).max(1);
        let message_id = state.next_message_id;
        state.sent_mutation_cards.push(FakeMutationApprovalCard {
            message_id,
            card: card.clone(),
            rendered_text,
        });
        if state.lose_next_send_receipt {
            state.lose_next_send_receipt = false;
            return Err(ApprovalBotError::uncertain("BOT_API_TRANSPORT_UNCERTAIN"));
        }
        Ok(ApprovalCardReceipt { message_id })
    }

    async fn edit_mutation_card(
        &self,
        card: &MutationApprovalCard,
        message_id: i64,
    ) -> Result<(), ApprovalBotError> {
        let rendered_text = render_mutation_card(card)?;
        self.state.lock().await.mutation_edits.push(FakeMutationApprovalEdit {
            message_id,
            card: card.clone(),
            rendered_text,
        });
        Ok(())
    }

    async fn poll_updates(
        &self,
        offset: i64,
        timeout_seconds: u64,
    ) -> Result<Vec<ApprovalUpdate>, ApprovalBotError> {
        loop {
            let notified = self.notify.notified();
            {
                let mut state = self.state.lock().await;
                while state.updates.front().is_some_and(|update| update.update_id < offset) {
                    state.updates.pop_front();
                }
                if !state.updates.is_empty() {
                    return Ok(state.updates.drain(..).collect());
                }
            }
            if tokio::time::timeout(Duration::from_secs(timeout_seconds), notified).await.is_err() {
                return Ok(Vec::new());
            }
        }
    }

    async fn answer_callback(
        &self,
        callback_query_id: &str,
        text: &str,
    ) -> Result<(), ApprovalBotError> {
        self.state.lock().await.answers.push((callback_query_id.to_string(), text.to_string()));
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::MutationExecutionOwner;

    fn live_grant(allow_peers: &[&str]) -> OwnerGrant {
        OwnerGrant {
            granted_by: 432_567_587,
            granted_at_unix: 1_000,
            expires_at_unix: 10_000,
            remaining_messages: 5,
            allow_peers: allow_peers.iter().map(|peer| (*peer).to_string()).collect(),
            reason: "owner-grant unit test".to_string(),
        }
    }

    fn write_grant(path: &std::path::Path, grant: &OwnerGrant) {
        std::fs::write(path, serde_json::to_vec_pretty(grant).expect("grant json"))
            .expect("write grant");
    }

    #[test]
    fn botfather_is_never_grantable_in_any_normalised_form() {
        // Even a hand-written grant that lists BotFather covers none of its
        // spellings, so the send and mutation paths keep the approval card.
        let grant = live_grant(&["@BotFather", "93372553", "botfather"]);
        for peer in [
            "BotFather",
            "@BotFather",
            "@botfather",
            "https://t.me/BotFather/",
            "https://telegram.me/botfather",
            "www.telegram.me/BotFather",
            "bot-father",
            "93372553",
        ] {
            assert!(!grant.allows(&[peer]), "{peer} must never be grantable");
        }
    }

    #[test]
    fn consume_owner_grant_refuses_botfather_written_into_the_file() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("owner-grant.json");
        write_grant(&path, &live_grant(&["@BotFather"]));
        // The send path passes chat_id, username and display_name of the same
        // target; here every one of them is a BotFather spelling.
        assert!(
            consume_owner_grant(&path, &["93372553", "BotFather", "BotFather"], 5_000).is_none()
        );
        let grant: OwnerGrant =
            serde_json::from_str(&std::fs::read_to_string(&path).expect("read")).expect("grant");
        assert_eq!(
            grant.remaining_messages, 5,
            "a refused grant must not be consumed"
        );
    }

    #[test]
    fn consume_owner_grant_matches_a_peer_by_identity_not_by_containment() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("owner-grant.json");
        write_grant(&path, &live_grant(&["@alex", "101"]));
        assert!(consume_owner_grant(&path, &["101", "alex", "Alex"], 5_000).is_some());
        // A peer that merely contains an allowed name is a different identity
        // and must not be covered.
        write_grant(&path, &live_grant(&["alex"]));
        assert!(consume_owner_grant(&path, &["@alexandra"], 5_000).is_none());
    }

    fn sample_send_approval(state: SendApprovalState) -> SendApproval {
        SendApproval {
            approval_id: "85a66192-03c4-462c-9c2a-cb58521fa17e".to_string(),
            draft_id: "db372e4d-7452-4218-9c40-9b87a726cb19".to_string(),
            account_user_id: 900_000_001,
            chat_id: "900000002".to_string(),
            target: crate::domain::PreparedSendTarget {
                display_name: "Example <Recipient>".to_string(),
                username: Some("example_recipient".to_string()),
                kind: "user".to_string(),
            },
            source: Some(PreparedOutboundSource::NonTelegram(
                PreparedNonTelegramSource::PublicWeb {
                    relation: OutboundSourceRelation::TriggeredOutreach,
                    publisher: "Example Marketplace".to_string(),
                    title: "Synthetic automation brief".to_string(),
                    url: "https://example.test/projects/fixture-123".to_string(),
                    published_at_unix: Some(1_785_518_000),
                    observed_at_unix: 1_785_518_656,
                },
            )),
            kind: PreparedSendKind::PlainText,
            text: "Первый абзац & точный текст.\n\nВторой абзац.".to_string(),
            reply_to_message_id: None,
            payload_sha256: "d29864568f7267b1c7699fd4e968995a65c5dd4c8210c8e0188902fdcf3be0fc"
                .to_string(),
            state,
            approval_chat_id: 900_000_003,
            approval_user_id: 900_000_004,
            approval_message_id: Some(331),
            created_at_unix: 1_785_518_656,
            expires_at_unix: 1_785_605_056,
            target_message_id: None,
            automated_retry_count: 0,
            next_retry_at_unix: None,
            last_error_code: None,
        }
    }

    fn sample_schedule_approval(state: MutationApprovalState) -> MutationApproval {
        MutationApproval {
            approval_id: "61da1fd2-feb2-4977-8ad3-bc59b9cf494b".to_string(),
            mutation_id: "dce1e663-e167-4695-ad2a-bab96d301858".to_string(),
            schema_version: 1,
            account_user_id: 900_000_001,
            action: MutationAction::ScheduleMessage {
                chat_id: "900000005".to_string(),
                target: Some(PreparedSendTarget {
                    display_name: "Example Recipient".to_string(),
                    username: Some("example_recipient".to_string()),
                    kind: "user".to_string(),
                }),
                source: Some(PreparedOutboundSource::TelegramMessage(PreparedMessageSource {
                    chat_id: "-1009000000006".to_string(),
                    target: PreparedSendTarget {
                        display_name: "Example Jobs".to_string(),
                        username: Some("example_jobs".to_string()),
                        kind: "channel".to_string(),
                    },
                    message_id: 1462,
                    message_date_unix: 1_783_703_400,
                    text: Some("Synthetic automation opportunity".to_string()),
                    relation: Some(OutboundSourceRelation::TriggeredOutreach),
                })),
                text: "Первый абзац сообщения.\n\nВторой абзац.".to_string(),
                parse_mode: crate::domain::CaptionParseMode::Plain,
                reply_to: None,
                schedule_date_unix: 1_785_565_800,
                silent: false,
                noforwards: false,
                random_id: 7_513_728_239_473_026_996,
            },
            payload_sha256: "8969cef6ec15b619f2cff6df93551464480507d4261782c91e096d0843531e45"
                .to_string(),
            state,
            execution_owner: MutationExecutionOwner::ManagedMcp,
            external_owner_ready: false,
            external_owner_receipt: None,
            approval_chat_id: 900_000_003,
            approval_user_id: 900_000_004,
            approval_message_id: Some(339),
            created_at_unix: 1_785_538_258,
            expires_at_unix: 1_785_624_658,
            result: None,
            automated_retry_count: 0,
            next_retry_at_unix: None,
            last_error_code: None,
            last_error_message: None,
        }
    }
    fn sample_send_file_approval(
        state: MutationApprovalState,
        caption: &str,
    ) -> MutationApproval {
        MutationApproval {
            approval_id: "72b8f9c1-4d2a-4e6b-9f1a-2c3d4e5f6071".to_string(),
            mutation_id: "a1b2c3d4-e5f6-4a7b-8c9d-e0f1a2b3c4d5".to_string(),
            schema_version: 1,
            account_user_id: 900_000_001,
            action: MutationAction::SendFile {
                chat_id: "900000007".to_string(),
                artifact: crate::domain::MutationFileArtifact {
                    artifact_id: "11111111-1111-1111-1111-111111111111".to_string(),
                    file_name: "праздник-на-байкале.jpg".to_string(),
                    size_bytes: 1_572_864,
                    sha256: "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
                        .to_string(),
                },
                caption: caption.to_string(),
                reply_to_message_id: None,
                mode: crate::domain::FileSendMode::Photo,
                random_id: 123_456_789,
            },
            payload_sha256: "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
                .to_string(),
            state,
            execution_owner: MutationExecutionOwner::ManagedMcp,
            external_owner_ready: false,
            external_owner_receipt: None,
            approval_chat_id: 900_000_003,
            approval_user_id: 900_000_004,
            approval_message_id: None,
            created_at_unix: 1_785_538_258,
            expires_at_unix: 1_785_624_658,
            result: None,
            automated_retry_count: 0,
            next_retry_at_unix: None,
            last_error_code: None,
            last_error_message: None,
        }
    }

    fn sample_send_story_approval(
        state: MutationApprovalState,
        caption: &str,
    ) -> MutationApproval {
        MutationApproval {
            approval_id: "83c9a0d2-5e3b-4f7c-a02b-3d4e5f607182".to_string(),
            mutation_id: "b2c3d4e5-f6a7-4b8c-9d0e-f1a2b3c4d5e6".to_string(),
            schema_version: 1,
            account_user_id: 900_000_001,
            action: MutationAction::SendStory {
                peer_ref: "900000009".to_string(),
                before_story_ids: Vec::new(),
                artifact: crate::domain::MutationFileArtifact {
                    artifact_id: "22222222-2222-2222-2222-222222222222".to_string(),
                    file_name: "кадр-иркутск.jpg".to_string(),
                    size_bytes: 2_097_152,
                    sha256: "fedcba9876543210fedcba9876543210fedcba9876543210fedcba9876543210"
                        .to_string(),
                },
                caption: caption.to_string(),
                privacy: crate::domain::StoryPrivacySpec {
                    kind: "everyone".to_string(),
                    selected_user_ids: Vec::new(),
                    disallowed_user_ids: Vec::new(),
                },
                pinned: false,
                noforwards: false,
                period_seconds: None,
                random_id: 987_654_321,
            },
            payload_sha256: "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
                .to_string(),
            state,
            execution_owner: MutationExecutionOwner::ManagedMcp,
            external_owner_ready: false,
            external_owner_receipt: None,
            approval_chat_id: 900_000_003,
            approval_user_id: 900_000_004,
            approval_message_id: None,
            created_at_unix: 1_785_538_258,
            expires_at_unix: 1_785_624_658,
            result: None,
            automated_retry_count: 0,
            next_retry_at_unix: None,
            last_error_code: None,
            last_error_message: None,
        }
    }

    #[test]
    fn send_approval_card_is_a_human_decision_surface() {
        let approval = sample_send_approval(SendApprovalState::Pending);
        let rendered = render_card(&ApprovalCard { approval: approval.clone() }).expect("card");
        assert!(rendered.starts_with("✉️ <b>Отправить сообщение?</b>"));
        assert!(rendered.contains("<b>Example &lt;Recipient&gt;</b> · @example_recipient"));
        assert!(rendered.contains(
            "🔎 <b>Источник:</b> <a href=\"https://example.test/projects/fixture-123\">Synthetic automation brief</a> · Example Marketplace"
        ));
        assert!(rendered.contains("↔️ <b>Связь:</b> повод для обращения"));
        assert!(rendered.contains(
            "<blockquote>Первый абзац &amp; точный текст.\n\nВторой абзац.</blockquote>"
        ));
        assert!(rendered.contains("сообщение уйдёт сразу"));
        assert!(!rendered.contains(&approval.payload_sha256));
        assert!(!rendered.contains(&approval.draft_id));
        assert!(!rendered.contains(&approval.approval_id));
        assert!(!rendered.contains("Unix"));
        assert!(!rendered.contains("SHA-256"));

        let keyboard = keyboard_for(&approval);
        assert_eq!(keyboard.inline_keyboard.len(), 2);
        assert_eq!(keyboard.inline_keyboard[0][0].text, "Отправить");
        assert_eq!(keyboard.inline_keyboard[1][0].text, "Отмена");
    }

    #[test]
    fn terminal_send_approval_card_has_no_live_buttons() {
        let mut approval = sample_send_approval(SendApprovalState::Sent);
        approval.target_message_id = Some(42);
        let rendered = render_card(&ApprovalCard { approval: approval.clone() }).expect("card");
        assert!(rendered.starts_with("✅ <b>Сообщение отправлено</b>"));
        assert!(rendered.contains("Telegram подтвердил отправку. · message 42"));
        assert!(keyboard_for(&approval).inline_keyboard.is_empty());
    }

    #[test]
    fn scheduled_message_approval_card_is_a_human_decision_surface() {
        let approval = sample_schedule_approval(MutationApprovalState::Pending);
        let rendered = render_mutation_card(&MutationApprovalCard {
            approval: approval.clone(),
            invite_progress: None,
            media: None,
        })
        .expect("card");

        assert!(rendered.starts_with("<b>🗓 Запланировать сообщение?</b>"));
        assert!(rendered.contains("👤 <b>Кому:</b> <b>Example Recipient</b> · @example_recipient"));
        assert!(rendered.contains(
            "🔎 <b>Источник:</b> <a href=\"https://t.me/example_jobs/1462\">Example Jobs · сообщение 1462</a>"
        ));
        assert!(rendered.contains("📅 <b>Опубликован:</b> 10 июля 2026, 20:10 МСК"));
        assert!(rendered.contains("↔️ <b>Связь:</b> повод для обращения"));
        assert!(rendered.contains("🕒 <b>Отправка:</b> 1 августа 2026, 09:30 МСК"));
        assert!(
            rendered.contains("<blockquote>Первый абзац сообщения.\n\nВторой абзац.</blockquote>")
        );
        assert!(rendered.contains("<b>✉️ Точный текст</b>"));
        assert!(rendered.contains("Telegram добавит сообщение в расписание"));
        assert!(!rendered.contains(&approval.payload_sha256));
        assert!(!rendered.contains(&approval.mutation_id));
        assert!(!rendered.contains(&approval.approval_id));
        assert!(!rendered.contains("Unix"));
        assert!(!rendered.contains("false"));
        assert_eq!(mutation_card_parse_mode(&approval.action), Some("HTML"));

        let keyboard = keyboard_for_mutation(&approval);
        assert_eq!(keyboard.inline_keyboard.len(), 1);
        assert_eq!(keyboard.inline_keyboard[0][0].text, "🗓 Запланировать");
        assert_eq!(keyboard.inline_keyboard[0][1].text, "❌ Отменить");
    }

    #[test]
    fn scheduled_message_approval_keeps_old_ledger_rows_readable() {
        let action: MutationAction = serde_json::from_value(serde_json::json!({
            "kind": "schedule_message",
            "chat_id": "900000005",
            "source": {
                "chat_id": "-1009000000006",
                "target": {
                    "display_name": "Example Jobs",
                    "username": "example_jobs",
                    "kind": "channel"
                },
                "message_id": 1462,
                "message_date_unix": 1783703400,
                "text": "Legacy source"
            },
            "text": "Legacy scheduled message",
            "reply_to": null,
            "schedule_date_unix": 1785565800,
            "silent": false,
            "noforwards": false,
            "random_id": 42
        }))
        .expect("legacy action");
        let MutationAction::ScheduleMessage { target, source, .. } = action else {
            panic!("expected scheduled message")
        };
        assert!(target.is_none());
        let Some(PreparedOutboundSource::TelegramMessage(source)) = source else {
            panic!("expected legacy Telegram source")
        };
        assert_eq!(source.message_id, 1462);
        assert_eq!(source.relation, None);
    }

    #[test]
    fn scheduled_public_web_source_is_clickable_and_date_bound() {
        let mut approval = sample_schedule_approval(MutationApprovalState::Pending);
        let MutationAction::ScheduleMessage { source, .. } = &mut approval.action else {
            panic!("expected scheduled message")
        };
        *source = Some(PreparedOutboundSource::NonTelegram(PreparedNonTelegramSource::PublicWeb {
            relation: OutboundSourceRelation::TriggeredOutreach,
            publisher: "Example Marketplace · fixture_workspace".to_string(),
            title: "Synthetic content automation brief".to_string(),
            url: "https://example.test/projects/fixture-3228097".to_string(),
            published_at_unix: Some(1_785_509_667),
            observed_at_unix: 1_785_540_576,
        }));

        let rendered = render_mutation_card(&MutationApprovalCard {
            approval,
            invite_progress: None,
            media: None,
        })
        .expect("card");
        assert!(rendered.contains(
            "<a href=\"https://example.test/projects/fixture-3228097\">Synthetic content automation brief</a> · Example Marketplace · fixture_workspace"
        ));
        assert!(rendered.contains("📅 <b>Опубликован:</b>"));
        assert!(rendered.contains("↔️ <b>Связь:</b> повод для обращения"));
    }

    #[test]
    fn terminal_scheduled_message_card_has_no_live_buttons() {
        let mut approval = sample_schedule_approval(MutationApprovalState::Applied);
        approval.result = Some(crate::domain::MutationResult::MessageScheduled {
            message_id: 42,
            schedule_date_unix: 1_785_565_800,
        });
        let rendered = render_mutation_card(&MutationApprovalCard {
            approval: approval.clone(),
            invite_progress: None,
            media: None,
        })
        .expect("card");
        assert!(rendered.starts_with("✅ <b>Сообщение запланировано</b>"));
        assert!(rendered.contains("Telegram подтвердил, что сообщение находится в расписании"));
        assert!(rendered.contains("сообщение 42"));
        assert!(keyboard_for_mutation(&approval).inline_keyboard.is_empty());
    }

    #[test]
    fn elapsed_scheduled_message_card_explains_that_nothing_was_sent() {
        let mut approval = sample_schedule_approval(MutationApprovalState::Expired);
        approval.last_error_code = Some("SCHEDULE_TIME_ELAPSED".to_string());
        let rendered = render_mutation_card(&MutationApprovalCard {
            approval: approval.clone(),
            invite_progress: None,
            media: None,
        })
        .expect("card");

        assert!(rendered.starts_with("⌛ <b>Время отправки прошло</b>"));
        assert!(rendered.contains("Ничего не отправлено"));
        assert!(rendered.contains("требуется новое точное подтверждение"));
        assert!(keyboard_for_mutation(&approval).inline_keyboard.is_empty());
    }
    #[test]
    fn send_file_card_is_readable_with_preview_details() {
        let caption = "Привет, Иркутск! Точный текст подписи.";
        let approval = sample_send_file_approval(MutationApprovalState::Pending, caption);
        let rendered = render_mutation_card(&MutationApprovalCard {
            approval: approval.clone(),
            invite_progress: None,
            media: None,
        })
        .expect("card");
        assert!(rendered.starts_with("📤 <b>Отправка в Telegram</b>"));
        assert!(rendered.contains("<b>Куда:</b> <code>900000007</code>"));
        assert!(rendered.contains("<b>Что:</b> фото"));
        assert!(rendered.contains("<code>праздник-на-байкале.jpg</code>"));
        assert!(rendered.contains("1.5 МБ"));
        assert!(rendered.contains("<b>Подпись ("));
        assert!(rendered.contains("<blockquote>Привет, Иркутск! Точный текст подписи.</blockquote>"));
        assert!(rendered.contains("🔐 <code>0123456789ab…</code>"));
        assert!(!rendered.contains("0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"));
        assert!(!rendered.contains("Режим:"));
        assert!(!rendered.contains("SHA-256:"));
        assert!(!rendered.contains("Mutation:"));
        assert!(!rendered.contains("Unix"));
        assert_eq!(mutation_card_parse_mode(&approval.action), Some("HTML"));
        let keyboard = keyboard_for_mutation(&approval);
        assert_eq!(keyboard.inline_keyboard.len(), 1);
        assert_eq!(keyboard.inline_keyboard[0][0].text, "✅ Опубликовать");
        assert_eq!(keyboard.inline_keyboard[0][1].text, "❌ Отменить");
    }

    #[test]
    fn send_story_card_is_readable_with_visibility_and_lifetime() {
        let approval =
            sample_send_story_approval(MutationApprovalState::Pending, "Сторис про приёмы");
        let rendered = render_mutation_card(&MutationApprovalCard {
            approval: approval.clone(),
            invite_progress: None,
            media: None,
        })
        .expect("card");
        assert!(rendered.starts_with("📸 <b>Публикация сторис</b>"));
        assert!(rendered.contains("<b>Куда:</b> <code>900000009</code>"));
        assert!(rendered.contains("Кадр:"));
        assert!(rendered.contains("<code>кадр-иркутск.jpg</code>"));
        assert!(rendered.contains("2.0 МБ"));
        assert!(rendered.contains("Видно:"));
        assert!(rendered.contains("всем"));
        assert!(rendered.contains("Время жизни:"));
        assert!(rendered.contains("24 часа"));
        assert!(rendered.contains("<blockquote>Сторис про приёмы</blockquote>"));
        assert!(rendered.contains("🔐 <code>fedcba987654…</code>"));
        assert!(!rendered.contains("Privacy:"));
        assert!(!rendered.contains("SHA-256:"));
        assert_eq!(mutation_card_parse_mode(&approval.action), Some("HTML"));
        let keyboard = keyboard_for_mutation(&approval);
        assert_eq!(keyboard.inline_keyboard.len(), 1);
        assert_eq!(keyboard.inline_keyboard[0][0].text, "✅ Опубликовать");
        assert_eq!(keyboard.inline_keyboard[0][1].text, "❌ Отменить");
    }

    #[tokio::test]
    async fn mutation_card_uses_media_preview_for_short_caption() {
        let approval = sample_send_file_approval(
            MutationApprovalState::Pending,
            "Короткая подпись",
        );
        let media = CardMedia::from_path(
            PathBuf::from("/tmp/outbox/photo.jpg"),
            "photo.jpg",
        );
        assert_eq!(media.kind, CardMediaKind::Photo);
        assert_eq!(media.mime, "image/jpeg");
        let card = MutationApprovalCard {
            approval: approval.clone(),
            invite_progress: None,
            media: Some(media.clone()),
        };
        let rendered = render_mutation_card(&card).expect("card");
        assert!(rendered.encode_utf16().count() <= MAX_MEDIA_CAPTION_UTF16);
        let bot = FakeApprovalBot::default();
        let receipt = bot.send_mutation_card(&card).await.expect("media card");
        let media_cards = bot.sent_media_cards().await;
        let text_cards = bot.sent_mutation_cards().await;
        assert_eq!(media_cards.len(), 1);
        assert!(text_cards.is_empty());
        assert_eq!(receipt.message_id, media_cards[0].message_id);
        assert!(media_cards[0].caption_html.contains("📤 <b>Отправка в Telegram</b>"));
        assert!(media_cards[0].has_keyboard);
        assert_eq!(media_cards[0].media, media);
    }

    #[tokio::test]
    async fn mutation_card_falls_back_to_preview_plus_reply_for_long_caption() {
        let long_caption = "я".repeat(1_100);
        let approval =
            sample_send_file_approval(MutationApprovalState::Pending, &long_caption);
        let media = CardMedia::from_path(
            PathBuf::from("/tmp/outbox/photo.jpg"),
            "photo.jpg",
        );
        let card = MutationApprovalCard {
            approval: approval.clone(),
            invite_progress: None,
            media: Some(media),
        };
        let rendered = render_mutation_card(&card).expect("card");
        assert!(rendered.encode_utf16().count() > MAX_MEDIA_CAPTION_UTF16);
        let bot = FakeApprovalBot::default();
        let receipt = bot.send_mutation_card(&card).await.expect("fallback card");
        let media_cards = bot.sent_media_cards().await;
        let text_cards = bot.sent_mutation_cards().await;
        assert_eq!(media_cards.len(), 1);
        assert_eq!(text_cards.len(), 1);
        let expected_preview = match &approval.action {
            MutationAction::SendFile { artifact, .. } => artifact.file_name.clone(),
            other => panic!("unexpected action: {other:?}"),
        };
        assert_eq!(media_cards[0].caption_html, format!("Предпросмотр: {expected_preview}"));
        assert!(!media_cards[0].has_keyboard);
        assert_eq!(receipt.message_id, text_cards[0].message_id);
        assert_eq!(text_cards[0].message_id, media_cards[0].message_id + 1);
        assert!(text_cards[0].rendered_text.contains(&long_caption));
    }

    fn create_fanout_database(path: &Path) -> Connection {
        let connection = Connection::open(path).expect("fanout database");
        connection
            .execute_batch(
                "CREATE TABLE telegram_ingress_runtime (
                    singleton INTEGER PRIMARY KEY NOT NULL CHECK(singleton=1),
                    schema_version INTEGER NOT NULL
                );
                INSERT INTO telegram_ingress_runtime(singleton,schema_version) VALUES(1,1);
                CREATE TABLE telegram_ingress_updates (
                    update_id INTEGER PRIMARY KEY NOT NULL,
                    kind TEXT NOT NULL,
                    from_user_id INTEGER NOT NULL,
                    chat_id INTEGER NOT NULL,
                    message_id INTEGER NOT NULL,
                    callback_query_id TEXT,
                    callback_data TEXT
                );",
            )
            .expect("fanout schema");
        connection
    }

    #[test]
    fn callback_data_accepts_only_the_typed_uuid_contract() {
        let id = Uuid::new_v4().to_string();
        assert_eq!(
            parse_callback_data(&format!("{APPROVE_PREFIX}{id}")),
            Some((ApprovalKind::Send, ApprovalAction::Approve, id.clone()))
        );
        assert_eq!(
            parse_callback_data(&format!("{CANCEL_PREFIX}{id}")),
            Some((ApprovalKind::Send, ApprovalAction::Cancel, id.clone()))
        );
        assert_eq!(
            parse_callback_data(&format!("{MUTATION_APPROVE_PREFIX}{id}")),
            Some((ApprovalKind::Mutation, ApprovalAction::Approve, id.clone()))
        );
        assert_eq!(
            parse_callback_data(&format!("{MUTATION_CANCEL_PREFIX}{id}")),
            Some((ApprovalKind::Mutation, ApprovalAction::Cancel, id))
        );
        assert_eq!(parse_callback_data("send:everything"), None);
        assert_eq!(parse_callback_data("tpa:a:not-a-uuid"), None);
    }

    #[test]
    fn fanout_reader_selects_only_typed_callback_rows() {
        let temporary = tempfile::tempdir().expect("temporary directory");
        let path = temporary.path().join("fanout.sqlite");
        let connection = create_fanout_database(&path);
        let approval_id = Uuid::new_v4().to_string();
        connection
            .execute(
                "INSERT INTO telegram_ingress_updates(
                    update_id,kind,from_user_id,chat_id,message_id,callback_query_id,callback_data
                 ) VALUES(?1,'message',7,7,11,NULL,NULL),
                         (?2,'callback',7,7,12,'callback-12',?3),
                         (?4,'callback',7,7,13,'callback-13','unrelated:data')",
                params![10_i64, 12_i64, format!("{APPROVE_PREFIX}{approval_id}"), 13_i64],
            )
            .expect("fanout rows");

        let updates = read_fanout_updates(&connection, 0).expect("fanout read");
        assert_eq!(updates.len(), 2);
        let decision = updates
            .first()
            .and_then(|update| update.decision.as_ref())
            .expect("typed approval decision");
        assert_eq!(decision.update_id, 12);
        assert_eq!(decision.callback_query_id, "callback-12");
        assert_eq!(decision.approval_id, approval_id);
        assert_eq!(decision.kind, ApprovalKind::Send);
        assert_eq!(decision.action, ApprovalAction::Approve);
        assert!(updates.get(1).is_some_and(|update| update.decision.is_none()));
    }

    #[test]
    fn direct_bot_api_pollers_share_the_hermes_update_lease() {
        let temporary = tempfile::tempdir().expect("temporary runtime directory");
        let first = acquire_bot_api_update_lease_in("123:secret", temporary.path())
            .expect("first update lease");
        let second = acquire_bot_api_update_lease_in("123:secret", temporary.path())
            .expect_err("same token must not gain a second poller");
        assert_eq!(second.code, "BOT_API_UPDATE_LEASE_BUSY");
        drop(first);
        acquire_bot_api_update_lease_in("123:secret", temporary.path())
            .expect("released update lease can be reacquired");
    }

    #[test]
    fn approval_proxy_uses_remote_dns_and_rejects_non_socks_routes() {
        assert_eq!(
            approval_proxy_url("socks5://127.0.0.1:1080").expect("SOCKS5 proxy"),
            "socks5h://127.0.0.1:1080"
        );
        assert_eq!(
            approval_proxy_url("socks5h://user:secret@proxy.example:1080").expect("SOCKS5H proxy"),
            "socks5h://user:secret@proxy.example:1080"
        );
        assert_eq!(
            approval_proxy_url("127.0.0.1:1080").expect("bare SOCKS5 authority"),
            "socks5h://127.0.0.1:1080"
        );
        assert_eq!(
            approval_proxy_url("https://proxy.example").expect_err("HTTP proxy must fail").code,
            "BOT_PROXY_INVALID"
        );
    }
}
