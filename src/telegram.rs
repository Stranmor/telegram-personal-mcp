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

use crate::Config;
use crate::backend::TelegramBackend;
use crate::domain::{
    AccountEmojiStatusSpec, AccountProfileSummary, AdminLogActionKind, AdminLogEventFilter,
    AdminLogEventSummary, AdminLogPage, AlbumUploadSpec, AudioTranscriptionSummary,
    AvailableStarGiftsPage, BirthdaySummary, BoostPage, BoostSlotSummary, BoostStatusSummary,
    BoostSummary, BotCallbackAnswerSummary, BotCommandSummary, BusinessAccountSettingsSummary,
    BusinessAwayMessageSummary, BusinessAwayScheduleSummary, BusinessChatLinkContent,
    BusinessChatLinkSummary, BusinessChatLinksPage, BusinessGreetingMessageSummary,
    BusinessIntroSpec, BusinessIntroSummary, BusinessLocationSummary, BusinessRecipientsSummary,
    BusinessWeeklyOpenSummary, BusinessWorkHoursConfig, BusinessWorkHoursSummary,
    CallbackButtonSnapshot, CaptionParseMode, ChannelSettingsSummary, ChannelStatsSummary,
    ChannelUpdatesDifferenceKind, ChannelUpdatesDifferenceSummary, ChannelUpdatesRequest,
    ChannelUpdatesRequestSummary, ChatAdminRightsSummary, ChatAutoDeleteSummary,
    ChatBannedRightsSummary, ChatDetailsSummary, ChatLifecycleState, ChatMemberRole,
    ChatMemberSummary, ChatModerationSettingsSummary, ChatReactionsPolicy, ChatSummary,
    Completeness, ContactSummary, CustomEmojiDocumentSummary, DefaultReactionSpec,
    DefaultReactionSummary, DeletedMessagesSummary, DialogDraftSummary, DialogFolderKind,
    DialogFolderList, DialogFolderSummary, DialogStateSnapshot, DiceEmojiCatalog,
    DisallowedGiftsSettingsSummary, DiscussionMessageSummary, DraftSummary, EmojiStatusCatalogKind,
    EmojiStatusCatalogPage, EmojiStatusKind, EmojiStatusSummary, FactCheckPage, FactCheckSummary,
    FileSendMode, ForumTopicSummary, GeoPointSpec, GlobalPrivacySettingsSummary,
    GroupCallInfoSummary, GroupCallPage, GroupCallParticipantSummary,
    GroupCallParticipantVideoSourceGroupSummary, GroupCallParticipantVideoSummary,
    GroupCallParticipantsPage, InlineBotGeoSummary, InlineBotResultSourceKind,
    InlineBotResultSummary, InlineBotResults, InlineBotSwitchPmSummary,
    InlineBotSwitchWebViewSummary, InviteJoinerPage, InviteJoinerSummary, InviteLinkKind,
    InviteLinkSummary, InviteSubscriptionPricingSummary, MessageButtonInventory, MessageButtonKind,
    MessageButtonMarkupType, MessageButtonStyleSummary, MessageButtonSummary, MessageLinkSummary,
    MessagePage, MessageReactionCount, MessageReactionPage, MessageReactionReadReceipt,
    MessageReactionSummary, MessageReadReceipt, MessageTranslationBatch, MessageTranslationSummary,
    MyBoostsSummary, NotificationSettingsPatch, NotificationSettingsSummary,
    NotificationSoundSummary, OutboxReadDateSummary, ParticipantFilter, ParticipantPage,
    ParticipantSummary, PeerColorSummary, PersonalChannelSummary, PollAnswerSummary,
    PollCreateSpec, PollOptionBinding, PollSummary, PollVoterPage, PollVoterSummary, PrivacyKey,
    PrivacyRuleSummary, ProfilePhotoPage, ProfilePhotoSummary, QuickReplyCatalogPage,
    QuickReplyMessagesPage, QuickReplyShortcutSummary, QuickReplyStoredMessageSummary,
    ReactionCatalogPage, ReactionIdentity, ReactionSpec, ResolvedBusinessChatLinkSummary,
    RichMediaSpec, RichMessageSnapshot, SavedDialogSummary, SavedDialogsPage, SavedHistoryPage,
    SavedMessagesSearchRequest, SavedReactionTagSummary, SavedReactionTagsPage, SavedStarGiftRef,
    SavedStarGiftSummary, SavedStarGiftsPage, SavedStarGiftsRequest, SearchMessagesRequest,
    SentDocumentMediaSummary, SentRichMediaSummary, SessionSummary, StarGiftAttributeSummary,
    StarGiftRaritySummary, StarGiftSummary, StarsAmountKind, StarsAmountSummary, StarsStatusPage,
    StarsSubscriptionPricingSummary, StarsSubscriptionSummary, StarsSubscriptionsPage,
    StarsTopupOptionSummary, StarsTopupOptionsPage, StarsTransactionGiftSummary,
    StarsTransactionPeerSummary, StarsTransactionSummary, StatsDateRangeSummary, StatsGraphSummary,
    StatsPercentSummary, StatsPostInteractionSummary, StatsTopAdminSummary, StatsTopInviterSummary,
    StatsTopPosterSummary, StatsValueSummary, StickerCatalogPage, StickerDocumentSummary,
    StickerEmojiPackSummary, StickerKeywordSummary, StickerSetDetailsPage, StickerSetSearchPage,
    StickerSetSummary, StoryCollectionPage, StoryLinkSummary, StoryListPage, StoryPeerPage,
    StoryPeerSummary, StoryPrivacySpec, StoryReportOption, StoryReportResult, StorySummary,
    StoryViewerSummary, StoryViewersPage, SuggestedDialogFolderList, SuggestedDialogFolderSummary,
    TypingAction, UnreadMessagesRequest, UpdateKindSummary, UpdatesCursorSummary,
    UpdatesDifferenceKind, UpdatesDifferenceRequest, UpdatesDifferenceRequestSummary,
    UpdatesDifferenceSummary, UpdatesStateSummary, UserDetails, WebPagePreviewSummary,
};
use crate::fs_security::{
    directory_is_owner_only, restrict_private_directory, restrict_private_file,
};
use crate::invite::opaque_invite_handle;
use crate::profile_photo::{opaque_chat_photo_handle, opaque_profile_photo_handle};
use crate::session_owner::LockedSessionBackend;
use crate::sticker::opaque_sticker_handle;
use async_trait::async_trait;
use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64_STANDARD};
use ferogram::tl;
use ferogram::{Client, InputMessage, SendCodeOutcome, ShutdownToken, SignInError};
use fs2::FileExt;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use std::fs::{File, OpenOptions};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use thiserror::Error;
use tokio::io::AsyncWriteExt;
use tokio::sync::Mutex;
use tokio::time::timeout;

const CONNECT_TIMEOUT: Duration = Duration::from_secs(30);
const MAX_LOCAL_PARTICIPANT_SCAN: usize = 10_000;

#[derive(Debug, Clone, Copy)]
enum UploadedDocumentKind {
    Voice,
    VideoNote { duration_seconds: i32, length_pixels: i32 },
}

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
    pub premium: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct MessageSummary {
    pub message_id: i32,
    pub chat_ref: Option<String>,
    pub sender_ref: Option<String>,
    pub outgoing: bool,
    pub pinned: bool,
    pub silent: bool,
    pub noforwards: bool,
    pub text: Option<String>,
    pub date: i32,
    pub edit_date: Option<i32>,
    pub reply_to_message_id: Option<i32>,
    pub views: Option<i32>,
    pub forwards: Option<i32>,
    pub reply_count: Option<i32>,
    pub media_type: Option<String>,
    pub forward_origin: Option<MessageForwardOrigin>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct MessageForwardOrigin {
    pub from_ref: Option<String>,
    pub source_message_id: Option<i32>,
    pub saved_from_ref: Option<String>,
    pub saved_from_message_id: Option<i32>,
    pub date: i32,
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
    pub pinned: bool,
    pub unread_mark: bool,
    pub unread_count: i32,
    pub archived: bool,
    pub folder_id: Option<i32>,
}

struct ConnectedSession {
    client: Client,
    _shutdown: ShutdownToken,
    path: PathBuf,
}

impl Drop for ConnectedSession {
    fn drop(&mut self) {
        self._shutdown.cancel();
    }
}

/// One live, exclusively locked Telegram connection used for interactive login.
///
/// The login code token and an optional 2FA challenge are intentionally kept in
/// this process and cannot be serialized or resumed by a different invocation.
pub struct LoginSession {
    client: Client,
    _shutdown: ShutdownToken,
    path: PathBuf,
}

impl Drop for LoginSession {
    fn drop(&mut self) {
        self._shutdown.cancel();
    }
}

/// Opaque challenge returned after Telegram has sent a login code.
pub struct LoginCodeChallenge(ferogram::LoginToken);

/// Opaque challenge returned when the account requires two-step verification.
pub struct LoginPasswordChallenge(Box<ferogram::PasswordToken>);

pub enum LoginRequestOutcome {
    CodeRequired(LoginCodeChallenge),
    Authorized,
}

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
    ) -> Result<LoginRequestOutcome, TelegramError> {
        match self
            .client
            .request_login_code(phone)
            .await
            .map_err(|error| TelegramError::Telegram(error.to_string()))?
        {
            SendCodeOutcome::CodeRequired(challenge) => {
                Ok(LoginRequestOutcome::CodeRequired(LoginCodeChallenge(challenge)))
            },
            SendCodeOutcome::AlreadyAuthorized(_) => Ok(LoginRequestOutcome::Authorized),
        }
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
        let (canonical_chat_id, _) = resolve_rich_message_peer(&connected.client, chat_id).await?;
        Ok(canonical_chat_id)
    }

    pub async fn send_typing_action(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        action: TypingAction,
    ) -> Result<(), TelegramError> {
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
        let accepted = connected
            .client
            .invoke(&tl::functions::messages::SetTyping {
                peer: input_peer,
                top_msg_id: None,
                action: typing_action_to_tl(action),
            })
            .await
            .map_err(send_invocation_error)?;
        if !accepted {
            return Err(TelegramError::SendRejected(
                "Telegram rejected the typing indicator".to_string(),
            ));
        }
        Ok(())
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
        let (client, shutdown) = self.connect_client(&selected_path, lock).await?;
        Ok(LoginSession { client, _shutdown: shutdown, path: selected_path })
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
            let (pinned, unread_mark, unread_count, folder_id) = match &dialog.raw {
                tl::enums::Dialog::Dialog(raw) => {
                    (raw.pinned, raw.unread_mark, raw.unread_count, raw.folder_id)
                },
                tl::enums::Dialog::Folder(_) | tl::enums::Dialog::Community(_) => {
                    (false, false, 0, None)
                },
            };
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
                pinned,
                unread_mark,
                unread_count,
                archived: folder_id == Some(1),
                folder_id,
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
        for message in raw_messages.messages {
            messages.push(summarize_message(&message));
            if messages.len() >= limit {
                break;
            }
        }

        Ok(messages)
    }

    pub async fn history_newer_than(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        min_message_id: i32,
        limit: usize,
        offset_id: Option<i32>,
    ) -> Result<Vec<MessageSummary>, TelegramError> {
        let connected = self.connected(session_path).await?;
        let peer = Self::resolve_peer_ref(&connected.client, chat_id).await?;
        let telegram_limit = i32::try_from(limit).map_err(|_| {
            TelegramError::InvalidPeer("history_newer_than.limit is too large".to_string())
        })?;
        let raw_messages = connected
            .client
            .get_message_history_range(
                peer.as_str(),
                telegram_limit,
                offset_id.unwrap_or_default(),
                0,
                min_message_id,
                0,
            )
            .await
            .map_err(|error| TelegramError::Telegram(error.to_string()))?;

        let mut messages = Vec::new();
        for message in raw_messages.messages {
            messages.push(summarize_message(&message));
            if messages.len() >= limit {
                break;
            }
        }
        Ok(messages)
    }

    pub async fn get_updates_state(
        &self,
        session_path: Option<PathBuf>,
    ) -> Result<UpdatesStateSummary, TelegramError> {
        let connected = self.connected(session_path).await?;
        let response = connected
            .client
            .invoke(&tl::functions::updates::GetState {})
            .await
            .map_err(|error| TelegramError::Telegram(error.to_string()))?;
        Ok(updates_state_summary(response))
    }

    pub async fn get_updates_difference(
        &self,
        request: UpdatesDifferenceRequest,
    ) -> Result<UpdatesDifferenceSummary, TelegramError> {
        let UpdatesDifferenceRequest {
            session_path,
            pts,
            qts,
            date,
            pts_limit,
            pts_total_limit,
            qts_limit,
        } = request;
        let connected = self.connected(session_path).await?;
        let response = connected
            .client
            .invoke(&tl::functions::updates::GetDifference {
                pts,
                pts_limit,
                pts_total_limit,
                date,
                qts,
                qts_limit,
            })
            .await
            .map_err(|error| TelegramError::Telegram(error.to_string()))?;
        updates_difference_summary(
            UpdatesDifferenceRequestSummary {
                pts,
                qts,
                date,
                pts_limit,
                pts_total_limit,
                qts_limit,
            },
            response,
        )
    }

    pub async fn get_channel_updates(
        &self,
        request: ChannelUpdatesRequest,
    ) -> Result<ChannelUpdatesDifferenceSummary, TelegramError> {
        let ChannelUpdatesRequest { session_path, chat_id, pts, limit, force } = request;
        let connected = self.connected(session_path).await?;
        let peer_ref = Self::resolve_peer_ref(&connected.client, &chat_id).await?;
        let peer = connected
            .client
            .resolve(peer_ref.as_str())
            .await
            .map_err(|error| TelegramError::Telegram(error.to_string()))?;
        let canonical_chat_id = canonical_peer_ref(&peer);
        let input_peer = connected
            .client
            .resolve_to_input_peer(&peer)
            .await
            .map_err(|error| TelegramError::Telegram(error.to_string()))?;
        let channel = input_channel_from_peer(input_peer, "get_channel_updates")?;
        let response = connected
            .client
            .invoke(&tl::functions::updates::GetChannelDifference {
                force,
                channel,
                filter: tl::enums::ChannelMessagesFilter::Empty,
                pts,
                limit,
            })
            .await
            .map_err(|error| TelegramError::Telegram(error.to_string()))?;
        channel_updates_difference_summary(
            ChannelUpdatesRequestSummary { chat_id: canonical_chat_id, pts, limit, force },
            response,
        )
    }

    pub async fn get_quick_replies(
        &self,
        session_path: Option<PathBuf>,
        hash: i64,
    ) -> Result<QuickReplyCatalogPage, TelegramError> {
        let connected = self.connected(session_path).await?;
        let response = connected
            .client
            .invoke(&tl::functions::messages::GetQuickReplies { hash })
            .await
            .map_err(|error| TelegramError::Telegram(error.to_string()))?;
        Ok(quick_reply_catalog_page(response))
    }

    pub async fn get_quick_reply_messages(
        &self,
        session_path: Option<PathBuf>,
        shortcut_id: i32,
        message_ids: Option<&[i32]>,
        hash: i64,
    ) -> Result<QuickReplyMessagesPage, TelegramError> {
        let connected = self.connected(session_path).await?;
        let response = connected
            .client
            .invoke(&tl::functions::messages::GetQuickReplyMessages {
                shortcut_id,
                id: message_ids.map(<[i32]>::to_vec),
                hash,
            })
            .await
            .map_err(|error| TelegramError::Telegram(error.to_string()))?;
        Ok(quick_reply_messages_page(shortcut_id, response))
    }

    pub async fn get_group_call(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        participant_limit: i32,
    ) -> Result<GroupCallPage, TelegramError> {
        let connected = self.connected(session_path).await?;
        let (canonical_chat_id, call) =
            resolve_active_group_call(&connected.client, chat_id).await?;
        let response = connected
            .client
            .invoke(&tl::functions::phone::GetGroupCall { call, limit: participant_limit })
            .await
            .map_err(|error| TelegramError::Telegram(error.to_string()))?;
        Ok(group_call_page(canonical_chat_id, response))
    }

    pub async fn get_group_call_participants(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        peer_refs: &[String],
        sources: &[i32],
        offset: &str,
        limit: i32,
    ) -> Result<GroupCallParticipantsPage, TelegramError> {
        let connected = self.connected(session_path).await?;
        let (canonical_chat_id, call) =
            resolve_active_group_call(&connected.client, chat_id).await?;
        let mut ids = Vec::with_capacity(peer_refs.len());
        for peer_ref in peer_refs {
            ids.push(resolve_input_peer(&connected.client, peer_ref).await?);
        }
        let response = connected
            .client
            .invoke(&tl::functions::phone::GetGroupParticipants {
                call,
                ids,
                sources: sources.to_vec(),
                offset: offset.to_string(),
                limit,
            })
            .await
            .map_err(|error| TelegramError::Telegram(error.to_string()))?;
        Ok(group_call_participants_page(canonical_chat_id, response))
    }

    pub async fn get_business_chat_links(
        &self,
        session_path: Option<PathBuf>,
    ) -> Result<BusinessChatLinksPage, TelegramError> {
        let connected = self.connected(session_path).await?;
        let response = connected
            .client
            .invoke(&tl::functions::account::GetBusinessChatLinks {})
            .await
            .map_err(|error| TelegramError::Telegram(error.to_string()))?;
        let tl::enums::account::BusinessChatLinks::BusinessChatLinks(response) = response;
        business_chat_links_page(response)
    }

    pub async fn resolve_business_chat_link(
        &self,
        session_path: Option<PathBuf>,
        slug: &str,
    ) -> Result<ResolvedBusinessChatLinkSummary, TelegramError> {
        let connected = self.connected(session_path).await?;
        let response = connected
            .client
            .invoke(&tl::functions::account::ResolveBusinessChatLink { slug: slug.to_string() })
            .await
            .map_err(|error| TelegramError::Telegram(error.to_string()))?;
        let tl::enums::account::ResolvedBusinessChatLinks::ResolvedBusinessChatLinks(response) =
            response;
        Ok(resolved_business_chat_link_summary(response))
    }

    pub async fn create_business_chat_link(
        &self,
        session_path: Option<PathBuf>,
        content: &BusinessChatLinkContent,
    ) -> Result<BusinessChatLinkSummary, TelegramError> {
        let connected = self.connected(session_path).await?;
        let link = business_chat_link_content_input(content)?;
        let response = connected
            .client
            .invoke(&tl::functions::account::CreateBusinessChatLink { link })
            .await
            .map_err(send_invocation_error)?;
        business_chat_link_summary(response)
    }

    pub async fn edit_business_chat_link(
        &self,
        session_path: Option<PathBuf>,
        slug: &str,
        content: &BusinessChatLinkContent,
    ) -> Result<BusinessChatLinkSummary, TelegramError> {
        let connected = self.connected(session_path).await?;
        let link = business_chat_link_content_input(content)?;
        let response = connected
            .client
            .invoke(&tl::functions::account::EditBusinessChatLink { slug: slug.to_string(), link })
            .await
            .map_err(send_invocation_error)?;
        business_chat_link_summary(response)
    }

    pub async fn delete_business_chat_link(
        &self,
        session_path: Option<PathBuf>,
        slug: &str,
    ) -> Result<(), TelegramError> {
        let connected = self.connected(session_path).await?;
        let deleted = connected
            .client
            .invoke(&tl::functions::account::DeleteBusinessChatLink { slug: slug.to_string() })
            .await
            .map_err(send_invocation_error)?;
        if !deleted {
            return Err(TelegramError::SendRejected(
                "Telegram returned false while deleting the exact Business chat link".to_string(),
            ));
        }
        Ok(())
    }

    pub async fn set_business_work_hours(
        &self,
        session_path: Option<PathBuf>,
        target: Option<&BusinessWorkHoursConfig>,
    ) -> Result<(), TelegramError> {
        let connected = self.connected(session_path).await?;
        let business_work_hours = target.map(|target| {
            tl::enums::BusinessWorkHours::BusinessWorkHours(tl::types::BusinessWorkHours {
                open_now: false,
                timezone_id: target.timezone_id.clone(),
                weekly_open: target
                    .weekly_open
                    .iter()
                    .map(|interval| {
                        tl::enums::BusinessWeeklyOpen::BusinessWeeklyOpen(
                            tl::types::BusinessWeeklyOpen {
                                start_minute: interval.start_minute,
                                end_minute: interval.end_minute,
                            },
                        )
                    })
                    .collect(),
            })
        });
        let accepted = connected
            .client
            .invoke(&tl::functions::account::UpdateBusinessWorkHours { business_work_hours })
            .await
            .map_err(send_invocation_error)?;
        if !accepted {
            return Err(TelegramError::SendRejected(
                "Telegram returned false while updating Business work hours".to_string(),
            ));
        }
        Ok(())
    }

    pub async fn set_business_location(
        &self,
        session_path: Option<PathBuf>,
        target: Option<&BusinessLocationSummary>,
    ) -> Result<(), TelegramError> {
        let connected = self.connected(session_path).await?;
        let accepted = connected
            .client
            .invoke(&tl::functions::account::UpdateBusinessLocation {
                geo_point: target.and_then(|target| target.point.as_ref()).map(input_geo_point),
                address: target.map(|target| target.address.clone()),
            })
            .await
            .map_err(send_invocation_error)?;
        if !accepted {
            return Err(TelegramError::SendRejected(
                "Telegram returned false while updating the Business location".to_string(),
            ));
        }
        Ok(())
    }

    pub async fn set_business_greeting(
        &self,
        session_path: Option<PathBuf>,
        target: Option<&BusinessGreetingMessageSummary>,
    ) -> Result<(), TelegramError> {
        let connected = self.connected(session_path).await?;
        let message = match target {
            Some(target) => {
                Some(tl::enums::InputBusinessGreetingMessage::InputBusinessGreetingMessage(
                    tl::types::InputBusinessGreetingMessage {
                        shortcut_id: target.shortcut_id,
                        recipients: input_business_recipients(
                            &connected.client,
                            &target.recipients,
                            "set_business_greeting",
                        )
                        .await?,
                        no_activity_days: target.no_activity_days,
                    },
                ))
            },
            None => None,
        };
        let accepted = connected
            .client
            .invoke(&tl::functions::account::UpdateBusinessGreetingMessage { message })
            .await
            .map_err(send_invocation_error)?;
        if !accepted {
            return Err(TelegramError::SendRejected(
                "Telegram returned false while updating the Business greeting".to_string(),
            ));
        }
        Ok(())
    }

    pub async fn set_business_away(
        &self,
        session_path: Option<PathBuf>,
        target: Option<&BusinessAwayMessageSummary>,
    ) -> Result<(), TelegramError> {
        let connected = self.connected(session_path).await?;
        let message = match target {
            Some(target) => Some(tl::enums::InputBusinessAwayMessage::InputBusinessAwayMessage(
                tl::types::InputBusinessAwayMessage {
                    offline_only: target.offline_only,
                    shortcut_id: target.shortcut_id,
                    schedule: business_away_schedule_input(&target.schedule),
                    recipients: input_business_recipients(
                        &connected.client,
                        &target.recipients,
                        "set_business_away",
                    )
                    .await?,
                },
            )),
            None => None,
        };
        let accepted = connected
            .client
            .invoke(&tl::functions::account::UpdateBusinessAwayMessage { message })
            .await
            .map_err(send_invocation_error)?;
        if !accepted {
            return Err(TelegramError::SendRejected(
                "Telegram returned false while updating the Business away message".to_string(),
            ));
        }
        Ok(())
    }

    pub async fn set_business_intro(
        &self,
        session_path: Option<PathBuf>,
        target: Option<&BusinessIntroSpec>,
    ) -> Result<(), TelegramError> {
        let connected = self.connected(session_path).await?;
        let intro = match target {
            Some(target) => {
                let sticker = match (
                    target.sticker_set_short_name.as_deref(),
                    target.sticker_handle.as_deref(),
                ) {
                    (None, None) => None,
                    (Some(set_short_name), Some(sticker_handle)) => {
                        let document = resolve_raw_sticker_document(
                            &connected.client,
                            set_short_name,
                            sticker_handle,
                        )
                        .await?;
                        Some(tl::enums::InputDocument::InputDocument(tl::types::InputDocument {
                            id: document.id,
                            access_hash: document.access_hash,
                            file_reference: document.file_reference,
                        }))
                    },
                    _ => {
                        return Err(TelegramError::InvalidPeer(
                            "Business intro sticker set and opaque handle must be supplied together"
                                .to_string(),
                        ));
                    },
                };
                Some(tl::enums::InputBusinessIntro::InputBusinessIntro(
                    tl::types::InputBusinessIntro {
                        title: target.title.clone(),
                        description: target.description.clone(),
                        sticker,
                    },
                ))
            },
            None => None,
        };
        let accepted = connected
            .client
            .invoke(&tl::functions::account::UpdateBusinessIntro { intro })
            .await
            .map_err(send_invocation_error)?;
        if !accepted {
            return Err(TelegramError::SendRejected(
                "Telegram returned false while updating the Business introduction".to_string(),
            ));
        }
        Ok(())
    }

    pub async fn get_stars_status(
        &self,
        session_path: Option<PathBuf>,
        peer_ref: &str,
    ) -> Result<StarsStatusPage, TelegramError> {
        let connected = self.connected(session_path).await?;
        let (canonical_peer_ref, peer) = resolve_stars_peer(&connected.client, peer_ref).await?;
        let response = connected
            .client
            .invoke(&tl::functions::payments::GetStarsStatus { ton: false, peer })
            .await
            .map_err(|error| TelegramError::Telegram(error.to_string()))?;
        Ok(stars_status_page(canonical_peer_ref, response))
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn get_stars_transactions(
        &self,
        session_path: Option<PathBuf>,
        peer_ref: &str,
        inbound: bool,
        outbound: bool,
        ascending: bool,
        subscription_id: Option<&str>,
        offset: &str,
        limit: i32,
    ) -> Result<StarsStatusPage, TelegramError> {
        let connected = self.connected(session_path).await?;
        let (canonical_peer_ref, peer) = resolve_stars_peer(&connected.client, peer_ref).await?;
        let response = connected
            .client
            .invoke(&tl::functions::payments::GetStarsTransactions {
                inbound,
                outbound,
                ascending,
                ton: false,
                subscription_id: subscription_id.map(str::to_string),
                peer,
                offset: offset.to_string(),
                limit,
            })
            .await
            .map_err(|error| TelegramError::Telegram(error.to_string()))?;
        Ok(stars_status_page(canonical_peer_ref, response))
    }

    pub async fn get_available_star_gifts(
        &self,
        session_path: Option<PathBuf>,
    ) -> Result<AvailableStarGiftsPage, TelegramError> {
        let connected = self.connected(session_path).await?;
        let response = connected
            .client
            .invoke(&tl::functions::payments::GetStarGifts { hash: 0 })
            .await
            .map_err(|error| TelegramError::Telegram(error.to_string()))?;
        Ok(available_star_gifts_page(response))
    }

    pub async fn get_saved_star_gifts(
        &self,
        request: SavedStarGiftsRequest,
    ) -> Result<SavedStarGiftsPage, TelegramError> {
        let connected = self.connected(request.session_path.clone()).await?;
        let (canonical_peer_ref, peer) =
            resolve_stars_peer(&connected.client, &request.peer_ref).await?;

        if !request.exclude_limited {
            let response = fetch_saved_star_gifts(
                &connected.client,
                peer,
                &request,
                request.exclude_unlimited,
                request.exclude_unique,
                request.offset.clone(),
                request.limit,
            )
            .await?;
            return Ok(saved_star_gifts_page(canonical_peer_ref, response));
        }

        // Current Telegram TL no longer has the competitor's legacy exclude_limited bit.
        // Preserve its exact behavior by scanning cursor-safe pages whose size never exceeds
        // the remaining requested output, then subtracting a server-counted limited-only slice.
        let limited_only = fetch_saved_star_gifts(
            &connected.client,
            peer.clone(),
            &request,
            true,
            true,
            String::new(),
            1,
        )
        .await?;
        let limited_count = limited_only.count;

        let mut output = SavedStarGiftsPage {
            peer_ref: canonical_peer_ref,
            count: 0,
            chat_notifications_enabled: None,
            gifts: Vec::new(),
            next_offset: None,
            related_chats: Vec::new(),
            related_users: Vec::new(),
        };
        let mut current_offset = request.offset.clone();
        let mut seen_offsets = HashSet::new();
        let mut source_count = None;
        while output.gifts.len() < usize::try_from(request.limit).unwrap_or_default() {
            if !seen_offsets.insert(current_offset.clone()) {
                return Err(TelegramError::Telegram(
                    "Telegram repeated a saved-Star-Gift pagination cursor".to_string(),
                ));
            }
            let remaining = i32::try_from(
                usize::try_from(request.limit)
                    .unwrap_or_default()
                    .saturating_sub(output.gifts.len()),
            )
            .unwrap_or(1)
            .max(1);
            let response = fetch_saved_star_gifts(
                &connected.client,
                peer.clone(),
                &request,
                request.exclude_unlimited,
                request.exclude_unique,
                current_offset.clone(),
                remaining,
            )
            .await?;
            source_count.get_or_insert(response.count);
            if output.chat_notifications_enabled.is_none() {
                output.chat_notifications_enabled = response.chat_notifications_enabled;
            }
            output.related_chats.extend(response.chats.into_iter().filter_map(chat_summary));
            output.related_users.extend(response.users.into_iter().filter_map(contact_summary));
            output.gifts.extend(
                response
                    .gifts
                    .into_iter()
                    .map(saved_star_gift_summary)
                    .filter(|gift| !star_gift_is_limited_regular(&gift.gift)),
            );
            output.next_offset = response.next_offset.filter(|offset| !offset.is_empty());
            let Some(next_offset) = output.next_offset.clone() else {
                break;
            };
            current_offset = next_offset;
        }
        output.count = source_count.unwrap_or_default().saturating_sub(limited_count);
        deduplicate_star_gift_related_peers(&mut output);
        Ok(output)
    }

    pub async fn get_stars_topup_options(
        &self,
        session_path: Option<PathBuf>,
    ) -> Result<StarsTopupOptionsPage, TelegramError> {
        let connected = self.connected(session_path).await?;
        let response = connected
            .client
            .invoke(&tl::functions::payments::GetStarsTopupOptions {})
            .await
            .map_err(|error| TelegramError::Telegram(error.to_string()))?;
        Ok(stars_topup_options_page(response))
    }

    pub async fn get_stars_subscriptions(
        &self,
        session_path: Option<PathBuf>,
        peer_ref: &str,
        missing_balance: bool,
        offset: &str,
    ) -> Result<StarsSubscriptionsPage, TelegramError> {
        let connected = self.connected(session_path).await?;
        let (canonical_peer_ref, peer) = resolve_stars_peer(&connected.client, peer_ref).await?;
        let response = connected
            .client
            .invoke(&tl::functions::payments::GetStarsSubscriptions {
                missing_balance,
                peer,
                offset: offset.to_string(),
            })
            .await
            .map_err(|error| TelegramError::Telegram(error.to_string()))?;
        Ok(stars_subscriptions_page(canonical_peer_ref, response))
    }

    pub async fn get_saved_star_gift(
        &self,
        session_path: Option<PathBuf>,
        reference: &SavedStarGiftRef,
    ) -> Result<Option<SavedStarGiftSummary>, TelegramError> {
        let connected = self.connected(session_path).await?;
        let input = input_saved_star_gift(&connected.client, reference).await?;
        let response = connected
            .client
            .invoke(&tl::functions::payments::GetSavedStarGift { stargift: vec![input] })
            .await
            .map_err(|error| TelegramError::Telegram(error.to_string()))?;
        let tl::enums::payments::SavedStarGifts::SavedStarGifts(response) = response;
        if response.gifts.len() > 1 {
            return Err(TelegramError::Telegram(
                "Telegram returned multiple gifts for one exact saved-gift reference".to_string(),
            ));
        }
        Ok(response.gifts.into_iter().next().map(saved_star_gift_summary))
    }

    pub async fn set_star_gift_unsaved(
        &self,
        session_path: Option<PathBuf>,
        reference: &SavedStarGiftRef,
        unsaved: bool,
    ) -> Result<(), TelegramError> {
        let connected = self.connected(session_path).await?;
        let stargift = input_saved_star_gift(&connected.client, reference).await?;
        let accepted = connected
            .client
            .invoke(&tl::functions::payments::SaveStarGift { unsave: unsaved, stargift })
            .await
            .map_err(send_invocation_error)?;
        if !accepted {
            return Err(TelegramError::Telegram(
                "Telegram rejected the saved Star Gift visibility change".to_string(),
            ));
        }
        Ok(())
    }

    pub async fn convert_star_gift(
        &self,
        session_path: Option<PathBuf>,
        reference: &SavedStarGiftRef,
    ) -> Result<(), TelegramError> {
        let connected = self.connected(session_path).await?;
        let stargift = input_saved_star_gift(&connected.client, reference).await?;
        let accepted = connected
            .client
            .invoke(&tl::functions::payments::ConvertStarGift { stargift })
            .await
            .map_err(send_invocation_error)?;
        if !accepted {
            return Err(TelegramError::Telegram(
                "Telegram rejected the irreversible Star Gift conversion".to_string(),
            ));
        }
        Ok(())
    }

    pub async fn get_stars_subscription(
        &self,
        session_path: Option<PathBuf>,
        peer_ref: &str,
        subscription_id: &str,
    ) -> Result<Option<StarsSubscriptionSummary>, TelegramError> {
        let connected = self.connected(session_path).await?;
        let (_, peer) = resolve_stars_peer(&connected.client, peer_ref).await?;
        let mut offset = String::new();
        let mut seen_offsets = HashSet::new();
        loop {
            if !seen_offsets.insert(offset.clone()) {
                return Err(TelegramError::Telegram(
                    "Telegram repeated a Stars subscription pagination cursor".to_string(),
                ));
            }
            let response = connected
                .client
                .invoke(&tl::functions::payments::GetStarsSubscriptions {
                    missing_balance: false,
                    peer: peer.clone(),
                    offset,
                })
                .await
                .map_err(|error| TelegramError::Telegram(error.to_string()))?;
            let tl::enums::payments::StarsStatus::StarsStatus(response) = response;
            if let Some(subscription) = response
                .subscriptions
                .unwrap_or_default()
                .into_iter()
                .map(stars_subscription_summary)
                .find(|subscription| subscription.subscription_id == subscription_id)
            {
                return Ok(Some(subscription));
            }
            let Some(next_offset) =
                response.subscriptions_next_offset.filter(|next_offset| !next_offset.is_empty())
            else {
                return Ok(None);
            };
            offset = next_offset;
        }
    }

    pub async fn change_stars_subscription(
        &self,
        session_path: Option<PathBuf>,
        peer_ref: &str,
        subscription_id: &str,
        canceled: bool,
    ) -> Result<(), TelegramError> {
        let connected = self.connected(session_path).await?;
        let (_, peer) = resolve_stars_peer(&connected.client, peer_ref).await?;
        let accepted = connected
            .client
            .invoke(&tl::functions::payments::ChangeStarsSubscription {
                peer,
                subscription_id: subscription_id.to_string(),
                canceled: Some(canceled),
            })
            .await
            .map_err(send_invocation_error)?;
        if !accepted {
            return Err(TelegramError::Telegram(
                "Telegram rejected the Stars subscription state change".to_string(),
            ));
        }
        Ok(())
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

    pub async fn search_saved_messages(
        &self,
        request: SavedMessagesSearchRequest,
    ) -> Result<MessagePage, TelegramError> {
        let connected = self.connected(request.session_path).await?;
        let limit = i32::try_from(request.limit).map_err(|_| {
            TelegramError::InvalidPeer("search_saved_messages.limit is too large".to_string())
        })?;
        let saved_peer_id = match request.saved_peer_id.as_deref() {
            Some(peer_ref) => Some(resolve_input_peer(&connected.client, peer_ref).await?),
            None => None,
        };
        let saved_reaction = if request.tag_reactions.is_empty() {
            None
        } else {
            Some(
                request
                    .tag_reactions
                    .iter()
                    .map(saved_tag_reaction_to_tl)
                    .collect::<Result<Vec<_>, _>>()?,
            )
        };
        let page = connected
            .client
            .invoke(&tl::functions::messages::Search {
                peer: tl::enums::InputPeer::PeerSelf,
                q: request.query,
                from_id: None,
                saved_peer_id,
                saved_reaction,
                top_msg_id: None,
                filter: tl::enums::MessagesFilter::InputMessagesFilterEmpty,
                min_date: request.min_date.unwrap_or_default(),
                max_date: request.max_date.unwrap_or_default(),
                offset_id: request.offset_id.unwrap_or_default(),
                add_offset: 0,
                limit,
                max_id: 0,
                min_id: 0,
                hash: 0,
            })
            .await
            .map_err(|error| TelegramError::Telegram(error.to_string()))?;
        Ok(message_page_from_tl(page, request.limit))
    }

    pub async fn get_saved_dialogs(
        &self,
        session_path: Option<PathBuf>,
        limit: usize,
        exclude_pinned: bool,
        offset_id: Option<i32>,
    ) -> Result<SavedDialogsPage, TelegramError> {
        let connected = self.connected(session_path).await?;
        let limit = i32::try_from(limit).map_err(|_| {
            TelegramError::InvalidPeer("get_saved_dialogs.limit is too large".to_string())
        })?;
        let page = connected
            .client
            .invoke(&tl::functions::messages::GetSavedDialogs {
                exclude_pinned,
                parent_peer: None,
                offset_date: 0,
                offset_id: offset_id.unwrap_or_default(),
                offset_peer: tl::enums::InputPeer::Empty,
                limit,
                hash: 0,
            })
            .await
            .map_err(|error| TelegramError::Telegram(error.to_string()))?;
        Ok(saved_dialogs_page(page, usize::try_from(limit).unwrap_or_default()))
    }

    pub async fn get_saved_dialog(
        &self,
        session_path: Option<PathBuf>,
        peer_ref: &str,
    ) -> Result<Option<SavedDialogSummary>, TelegramError> {
        let connected = self.connected(session_path).await?;
        let peer = resolve_input_peer(&connected.client, peer_ref).await?;
        let page = connected
            .client
            .invoke(&tl::functions::messages::GetSavedDialogsById {
                parent_peer: None,
                ids: vec![peer],
            })
            .await
            .map_err(|error| TelegramError::Telegram(error.to_string()))?;
        Ok(saved_dialogs_page(page, 1).dialogs.into_iter().next())
    }

    pub async fn get_saved_history(
        &self,
        session_path: Option<PathBuf>,
        peer_ref: &str,
        limit: usize,
        offset_id: Option<i32>,
    ) -> Result<SavedHistoryPage, TelegramError> {
        let connected = self.connected(session_path).await?;
        let resolved_ref = Self::resolve_peer_ref(&connected.client, peer_ref).await?;
        let peer = connected
            .client
            .resolve(resolved_ref.as_str())
            .await
            .map_err(|error| TelegramError::Telegram(error.to_string()))?;
        let canonical_peer_ref = canonical_peer_ref(&peer);
        let input_peer = connected
            .client
            .resolve_to_input_peer(&peer)
            .await
            .map_err(|error| TelegramError::Telegram(error.to_string()))?;
        let limit_i32 = i32::try_from(limit).map_err(|_| {
            TelegramError::InvalidPeer("get_saved_history.limit is too large".to_string())
        })?;
        let page = connected
            .client
            .invoke(&tl::functions::messages::GetSavedHistory {
                parent_peer: None,
                peer: input_peer,
                offset_id: offset_id.unwrap_or_default(),
                offset_date: 0,
                add_offset: 0,
                limit: limit_i32,
                max_id: 0,
                min_id: 0,
                hash: 0,
            })
            .await
            .map_err(|error| TelegramError::Telegram(error.to_string()))?;
        Ok(saved_history_page_from_tl(page, canonical_peer_ref, limit))
    }

    pub async fn delete_saved_history(
        &self,
        session_path: Option<PathBuf>,
        peer_ref: &str,
        max_id: i32,
        min_date: Option<i32>,
        max_date: Option<i32>,
    ) -> Result<(), TelegramError> {
        let connected = self.connected(session_path).await?;
        let peer = resolve_input_peer(&connected.client, peer_ref).await?;
        connected
            .client
            .invoke(&tl::functions::messages::DeleteSavedHistory {
                parent_peer: None,
                peer,
                max_id,
                min_date,
                max_date,
            })
            .await
            .map_err(send_invocation_error)?;
        Ok(())
    }

    pub async fn toggle_saved_dialog_pin(
        &self,
        session_path: Option<PathBuf>,
        peer_ref: &str,
        pinned: bool,
    ) -> Result<(), TelegramError> {
        let connected = self.connected(session_path).await?;
        let peer = resolve_input_peer(&connected.client, peer_ref).await?;
        let accepted = connected
            .client
            .invoke(&tl::functions::messages::ToggleSavedDialogPin {
                pinned,
                peer: tl::enums::InputDialogPeer::InputDialogPeer(tl::types::InputDialogPeer {
                    peer,
                }),
            })
            .await
            .map_err(send_invocation_error)?;
        if !accepted {
            return Err(TelegramError::SendRejected(
                "Telegram rejected the Saved Messages pin update".to_string(),
            ));
        }
        Ok(())
    }

    pub async fn get_saved_reaction_tags(
        &self,
        session_path: Option<PathBuf>,
        peer_ref: Option<&str>,
        hash: i64,
    ) -> Result<SavedReactionTagsPage, TelegramError> {
        let connected = self.connected(session_path).await?;
        let peer = match peer_ref {
            Some(peer_ref) => Some(resolve_input_peer(&connected.client, peer_ref).await?),
            None => None,
        };
        let tags = connected
            .client
            .invoke(&tl::functions::messages::GetSavedReactionTags { peer, hash })
            .await
            .map_err(|error| TelegramError::Telegram(error.to_string()))?;
        Ok(saved_reaction_tags_page(tags))
    }

    pub async fn update_saved_reaction_tag(
        &self,
        session_path: Option<PathBuf>,
        reaction: &ReactionIdentity,
        title: Option<&str>,
    ) -> Result<(), TelegramError> {
        let connected = self.connected(session_path).await?;
        let accepted = connected
            .client
            .invoke(&tl::functions::messages::UpdateSavedReactionTag {
                reaction: saved_tag_reaction_to_tl(reaction)?,
                title: title.map(str::to_string),
            })
            .await
            .map_err(send_invocation_error)?;
        if !accepted {
            return Err(TelegramError::SendRejected(
                "Telegram rejected the Saved Messages reaction-tag update".to_string(),
            ));
        }
        Ok(())
    }

    pub async fn get_default_tag_reactions(
        &self,
        session_path: Option<PathBuf>,
        hash: i64,
    ) -> Result<ReactionCatalogPage, TelegramError> {
        let connected = self.connected(session_path).await?;
        let reactions = connected
            .client
            .invoke(&tl::functions::messages::GetDefaultTagReactions { hash })
            .await
            .map_err(|error| TelegramError::Telegram(error.to_string()))?;
        Ok(reaction_catalog_page(reactions))
    }

    pub async fn get_unread_mentions(
        &self,
        request: UnreadMessagesRequest,
    ) -> Result<MessagePage, TelegramError> {
        let connected = self.connected(request.session_path).await?;
        let input_peer = resolve_input_peer(&connected.client, &request.chat_id).await?;
        let limit = i32::try_from(request.limit).map_err(|_| {
            TelegramError::InvalidPeer("get_unread_mentions.limit is too large".to_string())
        })?;
        let page = connected
            .client
            .invoke(&tl::functions::messages::GetUnreadMentions {
                peer: input_peer,
                top_msg_id: request.top_message_id,
                offset_id: request.offset_id,
                add_offset: request.add_offset,
                limit,
                max_id: request.max_id,
                min_id: request.min_id,
            })
            .await
            .map_err(|error| TelegramError::Telegram(error.to_string()))?;
        Ok(message_page_from_tl(page, request.limit))
    }

    pub async fn get_unread_reactions(
        &self,
        request: UnreadMessagesRequest,
    ) -> Result<MessagePage, TelegramError> {
        let connected = self.connected(request.session_path).await?;
        let input_peer = resolve_input_peer(&connected.client, &request.chat_id).await?;
        let saved_peer_id = match request.saved_peer_id.as_deref() {
            Some(peer) => Some(resolve_input_peer(&connected.client, peer).await?),
            None => None,
        };
        let limit = i32::try_from(request.limit).map_err(|_| {
            TelegramError::InvalidPeer("get_unread_reactions.limit is too large".to_string())
        })?;
        let page = connected
            .client
            .invoke(&tl::functions::messages::GetUnreadReactions {
                peer: input_peer,
                top_msg_id: request.top_message_id,
                saved_peer_id,
                offset_id: request.offset_id,
                add_offset: request.add_offset,
                limit,
                max_id: request.max_id,
                min_id: request.min_id,
            })
            .await
            .map_err(|error| TelegramError::Telegram(error.to_string()))?;
        Ok(message_page_from_tl(page, request.limit))
    }

    pub async fn get_unread_poll_votes(
        &self,
        request: UnreadMessagesRequest,
    ) -> Result<MessagePage, TelegramError> {
        let connected = self.connected(request.session_path).await?;
        let input_peer = resolve_input_peer(&connected.client, &request.chat_id).await?;
        let limit = i32::try_from(request.limit).map_err(|_| {
            TelegramError::InvalidPeer("get_unread_poll_votes.limit is too large".to_string())
        })?;
        let page = connected
            .client
            .invoke(&tl::functions::messages::GetUnreadPollVotes {
                peer: input_peer,
                top_msg_id: request.top_message_id,
                offset_id: request.offset_id,
                add_offset: request.add_offset,
                limit,
                max_id: request.max_id,
                min_id: request.min_id,
            })
            .await
            .map_err(|error| TelegramError::Telegram(error.to_string()))?;
        Ok(message_page_from_tl(page, request.limit))
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

    pub async fn get_message_html(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        message_id: i32,
    ) -> Result<String, TelegramError> {
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
        let text = message.text().unwrap_or_default();
        let entities = message.entities().map_or(&[][..], Vec::as_slice);
        Ok(html_with_phone_links(text, entities))
    }

    pub async fn get_message_buttons(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        message_id: Option<i32>,
        limit: usize,
    ) -> Result<MessageButtonInventory, TelegramError> {
        let connected = self.connected(session_path).await?;
        let peer_ref = Self::resolve_peer_ref(&connected.client, chat_id).await?;
        let peer = connected
            .client
            .resolve(peer_ref.as_str())
            .await
            .map_err(|error| TelegramError::Telegram(error.to_string()))?;
        let canonical_chat_id = canonical_peer_ref(&peer);
        let message = if let Some(message_id) = message_id {
            connected
                .client
                .get_messages(peer_ref.as_str(), &[message_id])
                .await
                .map_err(|error| TelegramError::Telegram(error.to_string()))?
                .into_iter()
                .find(is_concrete_message)
                .ok_or_else(|| {
                    TelegramError::MessageNotFound(format!("{canonical_chat_id}:{message_id}"))
                })?
        } else {
            let telegram_limit = i32::try_from(limit).map_err(|_| {
                TelegramError::InvalidPeer("get_message_buttons.limit is too large".to_string())
            })?;
            connected
                .client
                .get_message_history(peer_ref.as_str(), telegram_limit, 0, 0)
                .await
                .map_err(|error| TelegramError::Telegram(error.to_string()))?
                .messages
                .into_iter()
                .find(raw_message_has_buttons)
                .ok_or_else(|| {
                    TelegramError::MessageNotFound(format!(
                        "no message with buttons found in the latest {limit} messages of {canonical_chat_id}"
                    ))
                })?
        };
        message_button_inventory_from_raw(&canonical_chat_id, &message.raw).ok_or_else(|| {
            TelegramError::MessageNotFound(format!("{canonical_chat_id}:{}", message.id()))
        })
    }

    pub async fn press_callback_button(
        &self,
        session_path: Option<PathBuf>,
        button: &CallbackButtonSnapshot,
    ) -> Result<BotCallbackAnswerSummary, TelegramError> {
        let connected = self.connected(session_path).await?;
        let peer_ref = Self::resolve_peer_ref(&connected.client, &button.chat_id).await?;
        let peer = connected
            .client
            .resolve(peer_ref.as_str())
            .await
            .map_err(|error| TelegramError::Telegram(error.to_string()))?;
        let canonical_chat_id = canonical_peer_ref(&peer);
        if canonical_chat_id != button.chat_id {
            return Err(TelegramError::InvalidPeer(format!(
                "callback target resolved to {canonical_chat_id}, expected {}",
                button.chat_id
            )));
        }
        let message = connected
            .client
            .get_messages(peer_ref.as_str(), &[button.message_id])
            .await
            .map_err(|error| TelegramError::Telegram(error.to_string()))?
            .into_iter()
            .find(is_concrete_message)
            .ok_or_else(|| {
                TelegramError::MessageNotFound(format!("{}:{}", button.chat_id, button.message_id))
            })?;
        let inventory = message_button_inventory_from_raw(&button.chat_id, &message.raw)
            .ok_or_else(|| {
                TelegramError::MessageNotFound(format!("{}:{}", button.chat_id, button.message_id))
            })?;
        let current = inventory
            .buttons
            .iter()
            .find(|candidate| {
                candidate.row == button.row
                    && candidate.column == button.column
                    && candidate.index == button.index
            })
            .ok_or_else(|| {
                TelegramError::InvalidPeer(
                    "the approved callback button no longer exists at its bound position"
                        .to_string(),
                )
            })?;
        if current.kind != MessageButtonKind::Callback
            || current.requires_password
            || current.label != button.label
            || current.callback_data_sha256.as_deref() != Some(button.callback_data_sha256.as_str())
            || current.callback_data_length != Some(button.callback_data_length)
            || inventory.message_date_unix != button.message_date_unix
        {
            return Err(TelegramError::InvalidPeer(
                "the callback button changed after approval preparation".to_string(),
            ));
        }
        let data_base64 = current.callback_data_base64.as_deref().ok_or_else(|| {
            TelegramError::InvalidPeer("callback button omitted callback data".to_string())
        })?;
        let data = BASE64_STANDARD.decode(data_base64).map_err(|_| {
            TelegramError::InvalidPeer("callback button data was not valid base64".to_string())
        })?;
        let input_peer = connected
            .client
            .resolve_to_input_peer(&peer)
            .await
            .map_err(|error| TelegramError::Telegram(error.to_string()))?;
        let answer = connected
            .client
            .invoke(&callback_answer_request(input_peer, button.message_id, data))
            .await
            .map_err(send_invocation_error)?;
        let tl::enums::messages::BotCallbackAnswer::BotCallbackAnswer(answer) = answer;
        Ok(BotCallbackAnswerSummary {
            alert: answer.alert,
            has_url: answer.has_url,
            native_ui: answer.native_ui,
            message: answer.message,
            url: answer.url,
            cache_time_seconds: answer.cache_time,
        })
    }

    pub async fn get_inline_bot_results(
        &self,
        session_path: Option<PathBuf>,
        bot_ref: &str,
        context_chat_id: &str,
        query: &str,
        offset: &str,
    ) -> Result<InlineBotResults, TelegramError> {
        let bot_details = self.get_user_info(session_path.clone(), bot_ref).await?;
        if !bot_details.user.bot {
            return Err(TelegramError::InvalidPeer(format!(
                "{bot_ref} resolved to a Telegram user that is not a bot"
            )));
        }
        let connected = self.connected(session_path).await?;
        let bot = resolve_input_user(&connected.client, bot_ref, "get_inline_bot_results").await?;
        let context_ref = Self::resolve_peer_ref(&connected.client, context_chat_id).await?;
        let context_peer = connected
            .client
            .resolve(context_ref.as_str())
            .await
            .map_err(|error| TelegramError::Telegram(error.to_string()))?;
        let canonical_context_chat_id = canonical_peer_ref(&context_peer);
        let peer = connected
            .client
            .resolve_to_input_peer(&context_peer)
            .await
            .map_err(|error| TelegramError::Telegram(error.to_string()))?;
        let response = connected
            .client
            .invoke(&inline_bot_results_request(bot, peer, query.to_string(), offset.to_string()))
            .await
            .map_err(|error| TelegramError::Telegram(error.to_string()))?;
        let tl::enums::messages::BotResults::BotResults(response) = response;
        let switch_pm = response.switch_pm.map(|switch| match switch {
            tl::enums::InlineBotSwitchPm::InlineBotSwitchPm(switch) => {
                InlineBotSwitchPmSummary { text: switch.text, start_param: switch.start_param }
            },
        });
        let switch_webview = response.switch_webview.map(|switch| match switch {
            tl::enums::InlineBotWebView::InlineBotWebView(switch) => {
                InlineBotSwitchWebViewSummary { text: switch.text, url: switch.url }
            },
        });
        Ok(InlineBotResults {
            bot: bot_details.user,
            context_chat_id: canonical_context_chat_id,
            query: query.to_string(),
            offset: offset.to_string(),
            query_id: response.query_id.to_string(),
            next_offset: response.next_offset,
            cache_time_seconds: response.cache_time,
            gallery: response.gallery,
            switch_pm,
            switch_webview,
            results: response
                .results
                .into_iter()
                .map(inline_bot_result_summary)
                .collect::<Result<Vec<_>, _>>()?,
        })
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn send_inline_bot_result_with_random_id(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        query_id: &str,
        result_id: &str,
        reply_to_message_id: Option<i32>,
        silent: bool,
        hide_via: bool,
        clear_draft: bool,
        random_id: i64,
    ) -> Result<i32, TelegramError> {
        let query_id_value = query_id.parse::<i64>().map_err(|_| {
            TelegramError::InvalidPeer("inline query id is not a signed 64-bit integer".to_string())
        })?;
        if query_id_value.to_string() != query_id {
            return Err(TelegramError::InvalidPeer(
                "inline query id is not canonical decimal text".to_string(),
            ));
        }
        let connected = self.connected(session_path).await?;
        let peer_ref = Self::resolve_peer_ref(&connected.client, chat_id).await?;
        let resolved = connected
            .client
            .resolve(peer_ref.as_str())
            .await
            .map_err(|error| TelegramError::Telegram(error.to_string()))?;
        let peer = connected
            .client
            .resolve_to_input_peer(&resolved)
            .await
            .map_err(|error| TelegramError::Telegram(error.to_string()))?;
        let request = inline_bot_result_send_request(
            peer,
            query_id_value,
            result_id.to_string(),
            reply_to_message_id,
            silent,
            hide_via,
            clear_draft,
            random_id,
        );
        let updates = connected.client.invoke(&request).await.map_err(send_invocation_error)?;
        sent_message_id(&updates, random_id).ok_or_else(|| {
            TelegramError::DeliveryUncertain(
                "Telegram accepted inline-result send request but returned no exact message ID"
                    .to_string(),
            )
        })
    }

    pub async fn translate_messages(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        message_ids: &[i32],
        to_language: &str,
    ) -> Result<MessageTranslationBatch, TelegramError> {
        let connected = self.connected(session_path).await?;
        let peer_ref = Self::resolve_peer_ref(&connected.client, chat_id).await?;
        let peer = connected
            .client
            .resolve(peer_ref.as_str())
            .await
            .map_err(|error| TelegramError::Telegram(error.to_string()))?;
        let canonical_chat_id = canonical_peer_ref(&peer);
        let input_peer = connected
            .client
            .resolve_to_input_peer(&peer)
            .await
            .map_err(|error| TelegramError::Telegram(error.to_string()))?;
        let translated = connected
            .client
            .invoke(&tl::functions::messages::TranslateText {
                peer: Some(input_peer),
                id: Some(message_ids.to_vec()),
                text: None,
                to_lang: to_language.to_string(),
                tone: None,
            })
            .await
            .map_err(send_invocation_error)?;
        let tl::enums::messages::TranslatedText::TranslateResult(translated) = translated;
        if translated.result.len() != message_ids.len() {
            return Err(TelegramError::DeliveryUncertain(format!(
                "Telegram returned {} translations for {} requested messages",
                translated.result.len(),
                message_ids.len()
            )));
        }
        let translations = message_ids
            .iter()
            .copied()
            .zip(translated.result.iter())
            .map(|(message_id, text)| MessageTranslationSummary {
                message_id,
                text: text_with_entities_text(text).to_string(),
            })
            .collect();
        Ok(MessageTranslationBatch {
            chat_id: canonical_chat_id,
            to_language: to_language.to_string(),
            translations,
        })
    }

    pub async fn transcribe_audio(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        message_id: i32,
    ) -> Result<AudioTranscriptionSummary, TelegramError> {
        let connected = self.connected(session_path).await?;
        let peer_ref = Self::resolve_peer_ref(&connected.client, chat_id).await?;
        let peer = connected
            .client
            .resolve(peer_ref.as_str())
            .await
            .map_err(|error| TelegramError::Telegram(error.to_string()))?;
        let canonical_chat_id = canonical_peer_ref(&peer);
        let input_peer = connected
            .client
            .resolve_to_input_peer(&peer)
            .await
            .map_err(|error| TelegramError::Telegram(error.to_string()))?;
        let transcription = connected
            .client
            .invoke(&tl::functions::messages::TranscribeAudio {
                peer: input_peer,
                msg_id: message_id,
            })
            .await
            .map_err(send_invocation_error)?;
        let tl::enums::messages::TranscribedAudio::TranscribedAudio(transcription) = transcription;
        Ok(AudioTranscriptionSummary {
            chat_id: canonical_chat_id,
            message_id,
            transcription_id: transcription.transcription_id.to_string(),
            text: transcription.text,
            pending: transcription.pending,
            trial_remains_num: transcription.trial_remains_num,
            trial_remains_until_unix: transcription.trial_remains_until_date,
        })
    }

    pub async fn rate_transcription(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        message_id: i32,
        transcription_id: i64,
        good: bool,
    ) -> Result<(), TelegramError> {
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
        let accepted = connected
            .client
            .invoke(&tl::functions::messages::RateTranscribedAudio {
                peer: input_peer,
                msg_id: message_id,
                transcription_id,
                good,
            })
            .await
            .map_err(send_invocation_error)?;
        if accepted {
            Ok(())
        } else {
            Err(TelegramError::DeliveryUncertain(
                "Telegram returned false while rating the transcription".to_string(),
            ))
        }
    }

    pub async fn get_fact_checks(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        message_ids: &[i32],
    ) -> Result<FactCheckPage, TelegramError> {
        let connected = self.connected(session_path).await?;
        let peer_ref = Self::resolve_peer_ref(&connected.client, chat_id).await?;
        let peer = connected
            .client
            .resolve(peer_ref.as_str())
            .await
            .map_err(|error| TelegramError::Telegram(error.to_string()))?;
        let canonical_chat_id = canonical_peer_ref(&peer);
        let input_peer = connected
            .client
            .resolve_to_input_peer(&peer)
            .await
            .map_err(|error| TelegramError::Telegram(error.to_string()))?;
        let returned = connected
            .client
            .invoke(&tl::functions::messages::GetFactCheck {
                peer: input_peer,
                msg_id: message_ids.to_vec(),
            })
            .await
            .map_err(|error| TelegramError::Telegram(error.to_string()))?;
        if returned.len() != message_ids.len() {
            return Err(TelegramError::Telegram(format!(
                "Telegram returned {} fact-check records for {} requested messages",
                returned.len(),
                message_ids.len()
            )));
        }
        let fact_checks = message_ids
            .iter()
            .copied()
            .zip(returned)
            .map(|(message_id, fact_check)| {
                let tl::enums::FactCheck::FactCheck(fact_check) = fact_check;
                FactCheckSummary {
                    message_id,
                    need_check: fact_check.need_check,
                    country: fact_check.country,
                    text: fact_check
                        .text
                        .as_ref()
                        .map(|text| text_with_entities_text(text).to_string()),
                    hash: fact_check.hash.to_string(),
                }
            })
            .collect::<Vec<_>>();
        Ok(FactCheckPage {
            chat_id: canonical_chat_id,
            returned_count: fact_checks.len(),
            fact_checks,
        })
    }

    pub async fn edit_fact_check(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        message_id: i32,
        text: &str,
    ) -> Result<(), TelegramError> {
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
        connected
            .client
            .invoke(&tl::functions::messages::EditFactCheck {
                peer: input_peer,
                msg_id: message_id,
                text: plain_text_with_entities(text.to_string()),
            })
            .await
            .map_err(send_invocation_error)?;
        Ok(())
    }

    pub async fn delete_fact_check(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        message_id: i32,
    ) -> Result<(), TelegramError> {
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
        connected
            .client
            .invoke(&tl::functions::messages::DeleteFactCheck {
                peer: input_peer,
                msg_id: message_id,
            })
            .await
            .map_err(send_invocation_error)?;
        Ok(())
    }

    pub async fn get_discussion_message(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        message_id: i32,
    ) -> Result<DiscussionMessageSummary, TelegramError> {
        let connected = self.connected(session_path).await?;
        let peer = Self::resolve_peer_ref(&connected.client, chat_id).await?;
        let discussion = connected
            .client
            .get_discussion_message(peer.as_str(), message_id)
            .await
            .map_err(|error| TelegramError::Telegram(error.to_string()))?;
        Ok(DiscussionMessageSummary {
            source_chat_id: chat_id.to_string(),
            source_message_id: message_id,
            messages: discussion.messages.iter().filter_map(summarize_raw_message).collect(),
            max_id: discussion.max_id,
            read_inbox_max_id: discussion.read_inbox_max_id,
            read_outbox_max_id: discussion.read_outbox_max_id,
            unread_count: discussion.unread_count,
        })
    }

    pub async fn export_message_link(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        message_id: i32,
        grouped: bool,
        thread: bool,
    ) -> Result<MessageLinkSummary, TelegramError> {
        let connected = self.connected(session_path).await?;
        let peer_ref = Self::resolve_peer_ref(&connected.client, chat_id).await?;
        let peer = connected
            .client
            .resolve(peer_ref.as_str())
            .await
            .map_err(|error| TelegramError::Telegram(error.to_string()))?;
        let canonical_chat_id = canonical_peer_ref(&peer);
        let input_peer = connected
            .client
            .resolve_to_input_peer(&peer)
            .await
            .map_err(|error| TelegramError::Telegram(error.to_string()))?;
        let tl::enums::InputPeer::Channel(channel) = input_peer else {
            return Err(TelegramError::InvalidPeer(
                "message links require a channel or supergroup".to_string(),
            ));
        };
        let response = connected
            .client
            .invoke(&tl::functions::channels::ExportMessageLink {
                grouped,
                thread,
                channel: tl::enums::InputChannel::InputChannel(tl::types::InputChannel {
                    channel_id: channel.channel_id,
                    access_hash: channel.access_hash,
                }),
                id: message_id,
            })
            .await
            .map_err(|error| TelegramError::Telegram(error.to_string()))?;
        let tl::enums::ExportedMessageLink::ExportedMessageLink(response) = response;
        Ok(MessageLinkSummary {
            chat_id: canonical_chat_id,
            message_id,
            grouped,
            thread,
            link: response.link,
            html: response.html,
        })
    }

    pub async fn get_web_page_preview(
        &self,
        session_path: Option<PathBuf>,
        message: &str,
    ) -> Result<WebPagePreviewSummary, TelegramError> {
        let connected = self.connected(session_path).await?;
        let response = connected
            .client
            .invoke(&tl::functions::messages::GetWebPagePreview {
                message: message.to_string(),
                entities: None,
            })
            .await
            .map_err(|error| TelegramError::Telegram(error.to_string()))?;
        let tl::enums::messages::WebPagePreview::WebPagePreview(response) = response;
        Ok(web_page_preview_summary(response))
    }

    pub async fn get_groups_for_discussion(
        &self,
        session_path: Option<PathBuf>,
    ) -> Result<Vec<ChatSummary>, TelegramError> {
        let connected = self.connected(session_path).await?;
        let response = connected
            .client
            .invoke(&tl::functions::channels::GetGroupsForDiscussion {})
            .await
            .map_err(|error| TelegramError::Telegram(error.to_string()))?;
        let chats = match response {
            tl::enums::messages::Chats::Chats(chats) => chats.chats,
            tl::enums::messages::Chats::Slice(chats) => chats.chats,
        };
        Ok(chats.into_iter().filter_map(chat_summary).collect())
    }

    pub async fn list_contacts(
        &self,
        session_path: Option<PathBuf>,
        limit: usize,
    ) -> Result<Vec<ContactSummary>, TelegramError> {
        let connected = self.connected(session_path).await?;
        let contacts = connected
            .client
            .get_contacts()
            .await
            .map_err(|error| TelegramError::Telegram(error.to_string()))?;
        Ok(contacts.into_iter().filter_map(contact_summary).take(limit).collect())
    }

    pub async fn search_contacts(
        &self,
        session_path: Option<PathBuf>,
        query: &str,
        limit: usize,
    ) -> Result<Vec<ContactSummary>, TelegramError> {
        let connected = self.connected(session_path).await?;
        let telegram_limit = i32::try_from(limit).map_err(|_| {
            TelegramError::InvalidPeer("search_contacts.limit is too large".to_string())
        })?;
        let found = connected
            .client
            .invoke(&tl::functions::contacts::Search {
                q: query.to_string(),
                limit: telegram_limit,
                bots: false,
                broadcasts: false,
            })
            .await
            .map_err(|error| TelegramError::Telegram(error.to_string()))?;
        let tl::enums::contacts::Found::Found(found) = found;
        let mut by_user_id = found
            .users
            .into_iter()
            .filter_map(contact_summary)
            .map(|contact| (contact.user_id, contact))
            .collect::<HashMap<_, _>>();
        let mut peers = found.my_results;
        for peer in found.results {
            if !peers.contains(&peer) {
                peers.push(peer);
            }
        }
        Ok(peers
            .into_iter()
            .filter_map(|peer| match peer {
                tl::enums::Peer::User(user) => by_user_id.remove(&user.user_id),
                tl::enums::Peer::Chat(_) | tl::enums::Peer::Channel(_) => None,
            })
            .take(limit)
            .collect())
    }

    pub async fn get_pinned_message(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
    ) -> Result<Option<MessageSummary>, TelegramError> {
        let connected = self.connected(session_path).await?;
        let peer = Self::resolve_peer_ref(&connected.client, chat_id).await?;
        connected
            .client
            .get_pinned_message(peer.as_str())
            .await
            .map(|message| message.as_ref().map(summarize_message))
            .map_err(|error| TelegramError::Telegram(error.to_string()))
    }

    pub async fn get_scheduled_messages(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
    ) -> Result<Vec<MessageSummary>, TelegramError> {
        let connected = self.connected(session_path).await?;
        let peer = Self::resolve_peer_ref(&connected.client, chat_id).await?;
        let messages = connected
            .client
            .get_scheduled_messages(peer.as_str())
            .await
            .map_err(|error| TelegramError::Telegram(error.to_string()))?;
        Ok(messages.iter().map(summarize_message).collect())
    }

    pub async fn get_poll(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        message_id: i32,
    ) -> Result<Option<PollSummary>, TelegramError> {
        let connected = self.connected(session_path).await?;
        let peer_ref = Self::resolve_peer_ref(&connected.client, chat_id).await?;
        let peer = connected
            .client
            .resolve(peer_ref.as_str())
            .await
            .map_err(|error| TelegramError::Telegram(error.to_string()))?;
        let messages = connected
            .client
            .get_messages(peer_ref.as_str(), &[message_id])
            .await
            .map_err(|error| TelegramError::Telegram(error.to_string()))?;
        let Some(message) = messages.iter().find(|message| is_concrete_message(message)) else {
            return Ok(None);
        };
        let Some(media) = poll_media_from_message(message) else {
            return Ok(None);
        };
        let mut poll = media.poll.clone();
        let mut results = media.results.clone();
        let input_peer = connected
            .client
            .resolve_to_input_peer(&peer)
            .await
            .map_err(|error| TelegramError::Telegram(error.to_string()))?;
        let updates = connected
            .client
            .invoke(&tl::functions::messages::GetPollResults {
                peer: input_peer,
                msg_id: message_id,
                poll_hash: 0,
            })
            .await
            .map_err(|error| TelegramError::Telegram(error.to_string()))?;
        let poll_id = match &poll {
            tl::enums::Poll::Poll(poll) => poll.id,
        };
        if let Some(update) = poll_update_from_updates(&updates, poll_id, message_id) {
            if let Some(updated_poll) = &update.poll {
                poll = updated_poll.clone();
            }
            results = update.results.clone();
        }
        poll_summary_from_parts(
            canonical_peer_ref(&peer),
            message_id,
            &poll,
            &results,
            media.attached_media.as_ref(),
        )
        .map(Some)
    }

    pub async fn get_poll_votes(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        message_id: i32,
        option_index: Option<usize>,
        limit: usize,
        offset: Option<&str>,
    ) -> Result<PollVoterPage, TelegramError> {
        let selected_path = session_path.clone();
        let poll = self
            .get_poll(session_path, chat_id, message_id)
            .await?
            .ok_or_else(|| TelegramError::MessageNotFound(format!("{chat_id}:{message_id}")))?;
        let option = option_index
            .map(|index| {
                poll.answers
                    .iter()
                    .find(|answer| answer.option.index == index)
                    .ok_or_else(|| {
                        TelegramError::InvalidPeer(format!(
                            "poll option index {index} was not found"
                        ))
                    })
                    .and_then(|answer| decode_hex(&answer.option.option_hex))
            })
            .transpose()?;
        let connected = self.connected(selected_path).await?;
        let peer_ref = Self::resolve_peer_ref(&connected.client, chat_id).await?;
        let telegram_limit = i32::try_from(limit)
            .map_err(|_| TelegramError::InvalidPeer("poll voter limit is too large".to_string()))?;
        let page = connected
            .client
            .get_poll_votes(
                peer_ref.as_str(),
                message_id,
                option,
                telegram_limit,
                offset.map(str::to_string),
            )
            .await
            .map_err(|error| TelegramError::Telegram(error.to_string()))?;
        let option_index_by_hex = poll
            .answers
            .iter()
            .map(|answer| (answer.option.option_hex.as_str(), answer.option.index))
            .collect::<HashMap<_, _>>();
        let voters = page
            .votes
            .into_iter()
            .map(|vote| {
                let (peer, options, submitted_open_answer, date_unix) = match vote {
                    tl::enums::MessagePeerVote::MessagePeerVote(vote) => {
                        (vote.peer, vec![vote.option], false, vote.date)
                    },
                    tl::enums::MessagePeerVote::InputOption(vote) => {
                        (vote.peer, Vec::new(), true, vote.date)
                    },
                    tl::enums::MessagePeerVote::Multiple(vote) => {
                        (vote.peer, vote.options, false, vote.date)
                    },
                };
                let mut option_indices = Vec::new();
                let mut unknown_option_hex = Vec::new();
                for option in options {
                    let encoded = hex_encode(&option);
                    if let Some(index) = option_index_by_hex.get(encoded.as_str()) {
                        option_indices.push(*index);
                    } else {
                        unknown_option_hex.push(encoded);
                    }
                }
                option_indices.sort_unstable();
                PollVoterSummary {
                    peer_ref: canonical_peer_ref(&peer),
                    option_indices,
                    unknown_option_hex,
                    submitted_open_answer,
                    date_unix,
                }
            })
            .collect();
        Ok(PollVoterPage { total_count: page.count, voters, next_offset: page.next_offset })
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn create_poll_with_random_id(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        poll: &PollCreateSpec,
        reply_to_message_id: Option<i32>,
        silent: bool,
        noforwards: bool,
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
        let answers = poll
            .answers
            .iter()
            .enumerate()
            .map(|(index, answer)| {
                let option = u8::try_from(index).map_err(|_| {
                    TelegramError::InvalidPeer("poll has too many answers".to_string())
                })?;
                Ok(tl::enums::PollAnswer::PollAnswer(tl::types::PollAnswer {
                    text: plain_text_with_entities(answer.clone()),
                    option: vec![option],
                    media: None,
                    added_by: None,
                    date: None,
                }))
            })
            .collect::<Result<Vec<_>, TelegramError>>()?;
        let correct_answers = poll
            .correct_option_index
            .map(|index| {
                i32::try_from(index).map(|index| vec![index]).map_err(|_| {
                    TelegramError::InvalidPeer("quiz answer index is too large".into())
                })
            })
            .transpose()?;
        let media = tl::enums::InputMedia::Poll(Box::new(tl::types::InputMediaPoll {
            poll: tl::enums::Poll::Poll(tl::types::Poll {
                id: 0,
                closed: false,
                public_voters: poll.public_voters,
                multiple_choice: poll.multiple_choice,
                quiz: poll.quiz,
                open_answers: poll.open_answers,
                revoting_disabled: poll.revoting_disabled,
                shuffle_answers: poll.shuffle_answers,
                hide_results_until_close: poll.hide_results_until_close,
                creator: false,
                subscribers_only: poll.subscribers_only,
                question: plain_text_with_entities(poll.question.clone()),
                answers,
                close_period: poll.close_period_seconds,
                close_date: poll.close_date_unix,
                countries_iso2: (!poll.countries_iso2.is_empty())
                    .then(|| poll.countries_iso2.clone()),
                hash: 0,
            }),
            correct_answers,
            attached_media: None,
            solution: poll.solution.clone(),
            solution_entities: poll.solution.as_ref().map(|_| Vec::new()),
            solution_media: None,
        }));
        let reply_to = reply_to_message_id.map(|message_id| {
            tl::enums::InputReplyTo::Message(tl::types::InputReplyToMessage {
                reply_to_msg_id: message_id,
                top_msg_id: None,
                reply_to_peer_id: None,
                quote_text: None,
                quote_entities: None,
                quote_offset: None,
                monoforum_peer_id: None,
                poll_option: None,
                todo_item_id: None,
            })
        });
        let updates = connected
            .client
            .invoke(&tl::functions::messages::SendMedia {
                silent,
                background: false,
                clear_draft: false,
                noforwards,
                update_stickersets_order: false,
                invert_media: false,
                allow_paid_floodskip: false,
                peer: input_peer,
                reply_to,
                media,
                message: String::new(),
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
            })
            .await
            .map_err(send_invocation_error)?;
        sent_message_id(&updates, random_id).ok_or_else(|| {
            TelegramError::DeliveryUncertain(
                "Telegram accepted poll creation but returned no exact message ID".to_string(),
            )
        })
    }

    pub async fn set_poll_vote(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        message_id: i32,
        options: &[Vec<u8>],
    ) -> Result<(), TelegramError> {
        let connected = self.connected(session_path).await?;
        let peer_ref = Self::resolve_peer_ref(&connected.client, chat_id).await?;
        connected
            .client
            .send_vote(peer_ref.as_str(), message_id, options.to_vec())
            .await
            .map_err(send_invocation_error)
    }

    pub async fn stop_poll(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        message_id: i32,
    ) -> Result<(), TelegramError> {
        let connected = self.connected(session_path).await?;
        let peer_ref = Self::resolve_peer_ref(&connected.client, chat_id).await?;
        let peer = connected
            .client
            .resolve(peer_ref.as_str())
            .await
            .map_err(|error| TelegramError::Telegram(error.to_string()))?;
        let messages = connected
            .client
            .get_messages(peer_ref.as_str(), &[message_id])
            .await
            .map_err(|error| TelegramError::Telegram(error.to_string()))?;
        let message = messages
            .iter()
            .find(|message| is_concrete_message(message))
            .ok_or_else(|| TelegramError::MessageNotFound(format!("{chat_id}:{message_id}")))?;
        let media = poll_media_from_message(message)
            .ok_or_else(|| TelegramError::InvalidPeer("message is not a poll".to_string()))?;
        if media.attached_media.is_some() {
            return Err(TelegramError::InvalidPeer(
                "stopping polls with attached media is unavailable until exact media preservation is supported"
                    .to_string(),
            ));
        }
        let tl::enums::Poll::Poll(mut poll) = media.poll.clone();
        poll.closed = true;
        let tl::enums::PollResults::PollResults(results) = &media.results;
        if results.solution_media.is_some() {
            return Err(TelegramError::InvalidPeer(
                "stopping quizzes with solution media is unavailable until exact media preservation is supported"
                    .to_string(),
            ));
        }
        let correct_answers = results.results.as_ref().and_then(|results| {
            let indices = results
                .iter()
                .filter_map(|result| {
                    let tl::enums::PollAnswerVoters::PollAnswerVoters(result) = result;
                    if !result.correct {
                        return None;
                    }
                    poll.answers.iter().position(|answer| match answer {
                        tl::enums::PollAnswer::PollAnswer(answer) => answer.option == result.option,
                        tl::enums::PollAnswer::InputPollAnswer(_) => false,
                    })
                })
                .filter_map(|index| i32::try_from(index).ok())
                .collect::<Vec<_>>();
            (!indices.is_empty()).then_some(indices)
        });
        let input_peer = connected
            .client
            .resolve_to_input_peer(&peer)
            .await
            .map_err(|error| TelegramError::Telegram(error.to_string()))?;
        connected
            .client
            .invoke(&tl::functions::messages::EditMessage {
                no_webpage: false,
                invert_media: false,
                peer: input_peer,
                id: message_id,
                message: None,
                media: Some(tl::enums::InputMedia::Poll(Box::new(tl::types::InputMediaPoll {
                    poll: tl::enums::Poll::Poll(poll),
                    correct_answers,
                    attached_media: None,
                    solution: results.solution.clone(),
                    solution_entities: results.solution_entities.clone(),
                    solution_media: None,
                }))),
                reply_markup: None,
                entities: None,
                schedule_date: None,
                schedule_repeat_period: None,
                quick_reply_shortcut_id: None,
                rich_message: None,
            })
            .await
            .map_err(send_invocation_error)?;
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn schedule_text_with_random_id(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        text: &str,
        parse_mode: CaptionParseMode,
        reply_to_message_id: Option<i32>,
        schedule_date_unix: i32,
        silent: bool,
        noforwards: bool,
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
        let content = caption_input(text, parse_mode);
        let updates = connected
            .client
            .invoke(&tl::functions::messages::SendMessage {
                no_webpage: false,
                silent,
                background: false,
                clear_draft: false,
                noforwards,
                update_stickersets_order: false,
                invert_media: false,
                allow_paid_floodskip: false,
                peer: input_peer,
                reply_to,
                message: content.text,
                random_id,
                reply_markup: None,
                entities: content.entities,
                schedule_date: Some(schedule_date_unix),
                schedule_repeat_period: None,
                send_as: None,
                quick_reply_shortcut: None,
                effect: None,
                allow_paid_stars: None,
                suggested_post: None,
                rich_message: content.rich_message,
            })
            .await
            .map_err(send_invocation_error)?;
        sent_message_id(&updates, random_id).ok_or_else(|| {
            TelegramError::DeliveryUncertain(
                "Telegram accepted scheduled send request but response contained no scheduled message ID"
                    .to_string(),
            )
        })
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn schedule_file_with_random_id(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        file_path: &Path,
        caption: &str,
        caption_parse_mode: CaptionParseMode,
        reply_to_message_id: Option<i32>,
        schedule_date_unix: i32,
        silent: bool,
        noforwards: bool,
        mode: FileSendMode,
        random_id: i64,
    ) -> Result<i32, TelegramError> {
        if matches!(caption_parse_mode, CaptionParseMode::RichMarkdown) {
            return self
                .schedule_rich_file_with_random_id(
                    session_path,
                    chat_id,
                    file_path,
                    caption,
                    reply_to_message_id,
                    schedule_date_unix,
                    silent,
                    noforwards,
                    mode,
                    random_id,
                )
                .await;
        }
        reject_rich_media_caption(caption_parse_mode, "scheduled media")?;
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
        let uploaded =
            connected.client.upload_file(file_path).await.map_err(send_invocation_error)?;
        let media = match mode {
            FileSendMode::Auto => uploaded.as_auto_media(),
            FileSendMode::Document => uploaded.as_document_media(),
            FileSendMode::Photo => uploaded.as_photo_media(),
            FileSendMode::Gif => {
                if uploaded.mime_type() != "image/gif" {
                    return Err(TelegramError::InvalidPeer(format!(
                        "GIF mode requires image/gif bytes; detected {}",
                        uploaded.mime_type()
                    )));
                }
                uploaded.as_auto_media()
            },
        };
        let caption = caption_input(caption, caption_parse_mode);
        let updates = connected
            .client
            .invoke(&tl::functions::messages::SendMedia {
                silent,
                background: false,
                clear_draft: false,
                noforwards,
                update_stickersets_order: false,
                invert_media: false,
                allow_paid_floodskip: false,
                peer: input_peer,
                reply_to: input_reply_to(reply_to_message_id, None),
                media,
                message: caption.text,
                random_id,
                reply_markup: None,
                entities: caption.entities,
                schedule_date: Some(schedule_date_unix),
                schedule_repeat_period: None,
                send_as: None,
                quick_reply_shortcut: None,
                effect: None,
                allow_paid_stars: None,
                suggested_post: None,
            })
            .await
            .map_err(send_invocation_error)?;
        sent_message_id(&updates, random_id).ok_or_else(|| {
            TelegramError::DeliveryUncertain(
                "Telegram accepted scheduled file send but response contained no scheduled message ID"
                    .to_string(),
            )
        })
    }

    #[allow(clippy::too_many_arguments)]
    async fn schedule_rich_file_with_random_id(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        file_path: &Path,
        caption: &str,
        reply_to_message_id: Option<i32>,
        schedule_date_unix: i32,
        silent: bool,
        noforwards: bool,
        mode: FileSendMode,
        random_id: i64,
    ) -> Result<i32, TelegramError> {
        if !matches!(mode, FileSendMode::Auto | FileSendMode::Photo) {
            return Err(TelegramError::SendRejected(
                "native RichMessage media currently supports photo uploads only".to_string(),
            ));
        }
        let normalized_caption = normalize_rich_markdown(caption);
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
        let uploaded =
            connected.client.upload_file(file_path).await.map_err(send_invocation_error)?;
        if !uploaded.mime_type().starts_with("image/") || uploaded.mime_type() == "image/gif" {
            return Err(TelegramError::SendRejected(format!(
                "native RichMessage media requires a static image; detected {}",
                uploaded.mime_type()
            )));
        }
        let tl::enums::InputMedia::UploadedPhoto(uploaded_photo) = uploaded.as_photo_media() else {
            return Err(TelegramError::Telegram(
                "Ferogram did not produce uploaded photo media for RichMessage".to_string(),
            ));
        };
        let uploaded_media = connected
            .client
            .invoke(&tl::functions::messages::UploadMedia {
                business_connection_id: None,
                peer: input_peer.clone(),
                media: tl::enums::InputMedia::UploadedPhoto(uploaded_photo),
            })
            .await
            .map_err(send_invocation_error)?;
        let photo = match uploaded_media {
            tl::enums::MessageMedia::Photo(media) => match media.photo {
                Some(tl::enums::Photo::Photo(photo)) => photo,
                _ => {
                    return Err(TelegramError::Telegram(
                        "Telegram returned an empty uploaded photo for RichMessage".to_string(),
                    ));
                },
            },
            _ => {
                return Err(TelegramError::Telegram(
                    "Telegram returned non-photo media for RichMessage upload".to_string(),
                ));
            },
        };
        let photo_input = tl::enums::InputPhoto::InputPhoto(tl::types::InputPhoto {
            id: photo.id,
            access_hash: photo.access_hash,
            file_reference: photo.file_reference,
        });
        let rich_message =
            tl::enums::InputRichMessage::Markdown(tl::types::InputRichMessageMarkdown {
                rtl: false,
                noautolink: false,
                markdown: format!("![](tg://photo?id=visual)\n\n{normalized_caption}"),
                files: Some(vec![tl::enums::InputRichFile::Photo(tl::types::InputRichFilePhoto {
                    id: "visual".to_string(),
                    photo: photo_input,
                })]),
            });
        let updates = connected
            .client
            .invoke(&tl::functions::messages::SendMessage {
                no_webpage: false,
                silent,
                background: false,
                clear_draft: false,
                noforwards,
                update_stickersets_order: false,
                invert_media: false,
                allow_paid_floodskip: false,
                peer: input_peer,
                reply_to: input_reply_to(reply_to_message_id, None),
                message: String::new(),
                random_id,
                reply_markup: None,
                entities: None,
                schedule_date: Some(schedule_date_unix),
                schedule_repeat_period: None,
                send_as: None,
                quick_reply_shortcut: None,
                effect: None,
                allow_paid_stars: None,
                suggested_post: None,
                rich_message: Some(rich_message),
            })
            .await
            .map_err(send_invocation_error)?;
        sent_message_id(&updates, random_id).ok_or_else(|| {
            TelegramError::DeliveryUncertain(
                "Telegram accepted scheduled RichMessage media but response contained no scheduled message ID"
                    .to_string(),
            )
        })
    }

    pub async fn edit_scheduled_message(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        message_id: i32,
        text: &str,
        parse_mode: CaptionParseMode,
        schedule_date_unix: i32,
    ) -> Result<(), TelegramError> {
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
        let content = caption_input(text, parse_mode);
        connected
            .client
            .invoke(&tl::functions::messages::EditMessage {
                no_webpage: false,
                invert_media: false,
                peer: input_peer,
                id: message_id,
                message: Some(content.text),
                media: None,
                reply_markup: None,
                entities: content.entities,
                schedule_date: Some(schedule_date_unix),
                schedule_repeat_period: None,
                quick_reply_shortcut_id: None,
                rich_message: content.rich_message,
            })
            .await
            .map_err(send_invocation_error)
            .map(|_| ())
    }

    pub async fn delete_scheduled_messages(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        message_ids: &[i32],
    ) -> Result<(), TelegramError> {
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
        let updates = connected
            .client
            .invoke(&tl::functions::messages::DeleteScheduledMessages {
                peer: input_peer,
                id: message_ids.to_vec(),
            })
            .await
            .map_err(send_invocation_error)?;
        match scheduled_transition_receipt(&updates, &peer, message_ids) {
            Some(None) => Ok(()),
            _ => Err(TelegramError::DeliveryUncertain(
                "Telegram accepted scheduled deletion but returned no exact deletion receipt"
                    .to_string(),
            )),
        }
    }

    pub async fn send_scheduled_messages_now(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        message_ids: &[i32],
    ) -> Result<Vec<i32>, TelegramError> {
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
        let updates = connected
            .client
            .invoke(&tl::functions::messages::SendScheduledMessages {
                peer: input_peer,
                id: message_ids.to_vec(),
            })
            .await
            .map_err(send_invocation_error)?;
        scheduled_transition_receipt(&updates, &peer, message_ids)
            .flatten()
            .filter(|sent| sent.len() == message_ids.len())
            .ok_or_else(|| {
                TelegramError::DeliveryUncertain(
                    "Telegram accepted send-scheduled-now but did not bind every scheduled message ID to a sent message ID"
                        .to_string(),
                )
            })
    }

    pub async fn list_drafts(
        &self,
        session_path: Option<PathBuf>,
        limit: usize,
    ) -> Result<Vec<DialogDraftSummary>, TelegramError> {
        let connected = self.connected(session_path).await?;
        let mut iter = connected.client.iter_dialogs();
        let mut drafts = Vec::new();
        while let Some(dialog) = iter
            .next(&connected.client)
            .await
            .map_err(|error| TelegramError::Telegram(error.to_string()))?
        {
            let tl::enums::Dialog::Dialog(raw_dialog) = &dialog.raw else {
                continue;
            };
            let Some(draft) = raw_dialog.draft.clone().and_then(draft_summary) else {
                continue;
            };
            drafts.push(DialogDraftSummary {
                chat_id: canonical_peer_ref(&raw_dialog.peer),
                title: dialog.title(),
                draft,
            });
            if drafts.len() >= limit {
                break;
            }
        }
        Ok(drafts)
    }

    pub async fn get_draft(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
    ) -> Result<Option<DraftSummary>, TelegramError> {
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
        let response = connected
            .client
            .invoke(&tl::functions::messages::GetPeerDialogs {
                peers: vec![tl::enums::InputDialogPeer::InputDialogPeer(
                    tl::types::InputDialogPeer { peer: input_peer },
                )],
            })
            .await
            .map_err(|error| TelegramError::Telegram(error.to_string()))?;
        let tl::enums::messages::PeerDialogs::PeerDialogs(response) = response;
        Ok(response.dialogs.into_iter().find_map(|dialog| match dialog {
            tl::enums::Dialog::Dialog(dialog) => dialog.draft.and_then(draft_summary),
            tl::enums::Dialog::Folder(_) | tl::enums::Dialog::Community(_) => None,
        }))
    }

    pub async fn save_draft(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        text: Option<&str>,
    ) -> Result<(), TelegramError> {
        let connected = self.connected(session_path).await?;
        let peer = Self::resolve_peer_ref(&connected.client, chat_id).await?;
        connected
            .client
            .save_draft(peer.as_str(), text.unwrap_or_default())
            .await
            .map_err(|error| TelegramError::Telegram(error.to_string()))
    }

    pub async fn clear_all_drafts(
        &self,
        session_path: Option<PathBuf>,
    ) -> Result<(), TelegramError> {
        let connected = self.connected(session_path).await?;
        let cleared = connected
            .client
            .invoke(&tl::functions::messages::ClearAllDrafts {})
            .await
            .map_err(send_invocation_error)?;
        if !cleared {
            return Err(TelegramError::Telegram(
                "Telegram returned false for messages.clearAllDrafts".to_string(),
            ));
        }
        Ok(())
    }

    pub async fn get_dialog_state(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
    ) -> Result<DialogStateSnapshot, TelegramError> {
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
        let response = connected
            .client
            .invoke(&tl::functions::messages::GetPeerDialogs {
                peers: vec![tl::enums::InputDialogPeer::InputDialogPeer(
                    tl::types::InputDialogPeer { peer: input_peer },
                )],
            })
            .await
            .map_err(|error| TelegramError::Telegram(error.to_string()))?;
        let tl::enums::messages::PeerDialogs::PeerDialogs(response) = response;
        response
            .dialogs
            .iter()
            .find_map(dialog_state_from_raw)
            .ok_or_else(|| TelegramError::PeerNotFound(chat_id.to_string()))
    }

    pub async fn set_dialog_unread(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        unread: bool,
    ) -> Result<(), TelegramError> {
        let connected = self.connected(session_path).await?;
        let peer = Self::resolve_peer_ref(&connected.client, chat_id).await?;
        if unread {
            connected
                .client
                .mark_dialog_unread(peer.as_str())
                .await
                .map_err(|error| TelegramError::Telegram(error.to_string()))
        } else {
            connected
                .client
                .mark_read(peer.as_str())
                .await
                .map_err(|error| TelegramError::Telegram(error.to_string()))
        }
    }

    pub async fn mark_messages_read(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        max_id: i32,
    ) -> Result<(), TelegramError> {
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
        match input_peer {
            tl::enums::InputPeer::Channel(channel) => {
                connected
                    .client
                    .invoke(&tl::functions::channels::ReadHistory {
                        channel: tl::enums::InputChannel::InputChannel(tl::types::InputChannel {
                            channel_id: channel.channel_id,
                            access_hash: channel.access_hash,
                        }),
                        max_id,
                    })
                    .await
                    .map_err(send_invocation_error)?;
            },
            input_peer => {
                connected
                    .client
                    .invoke(&tl::functions::messages::ReadHistory { peer: input_peer, max_id })
                    .await
                    .map_err(send_invocation_error)?;
            },
        }
        Ok(())
    }

    pub async fn mark_reactions_read(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
    ) -> Result<MessageReactionReadReceipt, TelegramError> {
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
        let response = connected
            .client
            .invoke(&tl::functions::messages::ReadReactions {
                peer: input_peer,
                top_msg_id: None,
                saved_peer_id: None,
            })
            .await
            .map_err(send_invocation_error)?;
        let tl::enums::messages::AffectedHistory::AffectedHistory(response) = response;
        Ok(MessageReactionReadReceipt {
            pts: response.pts,
            pts_count: response.pts_count,
            offset: response.offset,
        })
    }

    pub async fn set_dialog_archived(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        archived: bool,
    ) -> Result<(), TelegramError> {
        let connected = self.connected(session_path).await?;
        let peer = Self::resolve_peer_ref(&connected.client, chat_id).await?;
        connected
            .client
            .archive(peer.as_str(), archived)
            .await
            .map_err(|error| TelegramError::Telegram(error.to_string()))
    }

    pub async fn set_dialog_pinned(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        pinned: bool,
    ) -> Result<(), TelegramError> {
        let connected = self.connected(session_path).await?;
        let peer = Self::resolve_peer_ref(&connected.client, chat_id).await?;
        connected
            .client
            .pin_dialog(peer.as_str(), pinned)
            .await
            .map_err(|error| TelegramError::Telegram(error.to_string()))
    }

    pub async fn list_dialog_folders(
        &self,
        session_path: Option<PathBuf>,
    ) -> Result<DialogFolderList, TelegramError> {
        let connected = self.connected(session_path).await?;
        let me = connected
            .client
            .get_me()
            .await
            .map_err(|error| TelegramError::Telegram(error.to_string()))?;
        let (tags_enabled, filters) = get_raw_dialog_filters(&connected.client).await?;
        let folders = filters
            .iter()
            .map(|filter| dialog_folder_summary(filter, me.id))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(DialogFolderList { tags_enabled, folders })
    }

    pub async fn get_suggested_dialog_folders(
        &self,
        session_path: Option<PathBuf>,
    ) -> Result<SuggestedDialogFolderList, TelegramError> {
        let connected = self.connected(session_path).await?;
        let me = connected
            .client
            .get_me()
            .await
            .map_err(|error| TelegramError::Telegram(error.to_string()))?;
        let response = connected
            .client
            .invoke(&tl::functions::messages::GetSuggestedDialogFilters {})
            .await
            .map_err(|error| TelegramError::Telegram(error.to_string()))?;
        let suggestions = response
            .into_iter()
            .map(|suggestion| {
                let tl::enums::DialogFilterSuggested::DialogFilterSuggested(suggestion) =
                    suggestion;
                Ok(SuggestedDialogFolderSummary {
                    description: suggestion.description,
                    folder: dialog_folder_summary(&suggestion.filter, me.id)?,
                })
            })
            .collect::<Result<Vec<_>, TelegramError>>()?;
        Ok(SuggestedDialogFolderList { suggestions })
    }

    pub async fn set_dialog_folder_tags(
        &self,
        session_path: Option<PathBuf>,
        enabled: bool,
    ) -> Result<(), TelegramError> {
        let connected = self.connected(session_path).await?;
        let updated = connected
            .client
            .invoke(&tl::functions::messages::ToggleDialogFilterTags { enabled })
            .await
            .map_err(send_invocation_error)?;
        if updated {
            Ok(())
        } else {
            Err(TelegramError::SendRejected(
                "Telegram returned false while updating dialog-folder tags".into(),
            ))
        }
    }

    pub async fn get_global_privacy_settings(
        &self,
        session_path: Option<PathBuf>,
    ) -> Result<GlobalPrivacySettingsSummary, TelegramError> {
        let connected = self.connected(session_path).await?;
        let settings = connected
            .client
            .invoke(&tl::functions::account::GetGlobalPrivacySettings {})
            .await
            .map_err(|error| TelegramError::Telegram(error.to_string()))?;
        Ok(global_privacy_settings_summary(settings))
    }

    pub async fn set_global_privacy_settings(
        &self,
        session_path: Option<PathBuf>,
        settings: &GlobalPrivacySettingsSummary,
    ) -> Result<(), TelegramError> {
        let connected = self.connected(session_path).await?;
        connected
            .client
            .invoke(&tl::functions::account::SetGlobalPrivacySettings {
                settings: global_privacy_settings_input(settings)?,
            })
            .await
            .map_err(send_invocation_error)?;
        Ok(())
    }

    pub async fn upsert_dialog_folder(
        &self,
        session_path: Option<PathBuf>,
        folder: &DialogFolderSummary,
        preserve_title_entities: bool,
    ) -> Result<(), TelegramError> {
        let connected = self.connected(session_path).await?;
        let preserved_title = if preserve_title_entities {
            let (_, filters) = get_raw_dialog_filters(&connected.client).await?;
            filters.iter().find_map(|filter| match filter {
                tl::enums::DialogFilter::DialogFilter(existing)
                    if existing.id == folder.folder_id
                        && text_with_entities_text(&existing.title) == folder.title =>
                {
                    Some(existing.title.clone())
                },
                tl::enums::DialogFilter::Chatlist(existing)
                    if existing.id == folder.folder_id
                        && text_with_entities_text(&existing.title) == folder.title =>
                {
                    Some(existing.title.clone())
                },
                _ => None,
            })
        } else {
            None
        };
        let filter = build_dialog_filter(&connected.client, folder, preserved_title).await?;
        let updated = connected
            .client
            .invoke(&tl::functions::messages::UpdateDialogFilter {
                id: folder.folder_id,
                filter: Some(filter),
            })
            .await
            .map_err(send_invocation_error)?;
        if updated {
            Ok(())
        } else {
            Err(TelegramError::DeliveryUncertain(
                "Telegram returned false while updating the dialog folder".into(),
            ))
        }
    }

    pub async fn delete_dialog_folder(
        &self,
        session_path: Option<PathBuf>,
        folder_id: i32,
    ) -> Result<(), TelegramError> {
        let connected = self.connected(session_path).await?;
        let deleted = connected
            .client
            .invoke(&tl::functions::messages::UpdateDialogFilter { id: folder_id, filter: None })
            .await
            .map_err(send_invocation_error)?;
        if deleted {
            Ok(())
        } else {
            Err(TelegramError::DeliveryUncertain(
                "Telegram returned false while deleting the dialog folder".into(),
            ))
        }
    }

    pub async fn reorder_dialog_folders(
        &self,
        session_path: Option<PathBuf>,
        folder_ids: &[i32],
    ) -> Result<(), TelegramError> {
        let connected = self.connected(session_path).await?;
        let updated = connected
            .client
            .invoke(&tl::functions::messages::UpdateDialogFiltersOrder {
                order: folder_ids.to_vec(),
            })
            .await
            .map_err(send_invocation_error)?;
        if updated {
            Ok(())
        } else {
            Err(TelegramError::DeliveryUncertain(
                "Telegram returned false while reordering dialog folders".into(),
            ))
        }
    }

    pub async fn set_dialog_folder_membership(
        &self,
        session_path: Option<PathBuf>,
        folder_id: i32,
        chat_id: &str,
        included: bool,
    ) -> Result<(), TelegramError> {
        let connected = self.connected(session_path).await?;
        let me = connected
            .client
            .get_me()
            .await
            .map_err(|error| TelegramError::Telegram(error.to_string()))?;
        let (_, filters) = get_raw_dialog_filters(&connected.client).await?;
        let filter = filters
            .into_iter()
            .find(|filter| dialog_filter_id(filter) == folder_id)
            .ok_or_else(|| TelegramError::InvalidPeer(format!("dialog folder {folder_id}")))?;
        let peer = connected
            .client
            .resolve(chat_id)
            .await
            .map_err(|error| TelegramError::Telegram(error.to_string()))?;
        let input_peer = connected
            .client
            .resolve_to_input_peer(&peer)
            .await
            .map_err(|error| TelegramError::Telegram(error.to_string()))?;
        let target_ref = input_peer_ref(&input_peer, me.id)?;
        let filter =
            mutate_dialog_filter_membership(filter, input_peer, &target_ref, me.id, included)?;
        let updated = connected
            .client
            .invoke(&tl::functions::messages::UpdateDialogFilter {
                id: folder_id,
                filter: Some(filter),
            })
            .await
            .map_err(send_invocation_error)?;
        if updated {
            Ok(())
        } else {
            Err(TelegramError::DeliveryUncertain(
                "Telegram returned false while updating dialog-folder membership".into(),
            ))
        }
    }

    pub async fn set_user_blocked(
        &self,
        session_path: Option<PathBuf>,
        user_ref: &str,
        blocked: bool,
    ) -> Result<(), TelegramError> {
        let connected = self.connected(session_path).await?;
        let peer = Self::resolve_peer_ref(&connected.client, user_ref).await?;
        connected
            .client
            .block(peer.as_str(), blocked)
            .await
            .map_err(|error| TelegramError::Telegram(error.to_string()))
    }

    pub async fn get_replies(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        message_id: i32,
        limit: usize,
        offset_id: Option<i32>,
    ) -> Result<MessagePage, TelegramError> {
        let connected = self.connected(session_path).await?;
        let peer = Self::resolve_peer_ref(&connected.client, chat_id).await?;
        let telegram_limit = i32::try_from(limit).map_err(|_| {
            TelegramError::InvalidPeer("get_replies.limit is too large".to_string())
        })?;
        let page = connected
            .client
            .get_replies(
                peer.as_str(),
                message_id,
                telegram_limit,
                offset_id.unwrap_or_default(),
                0,
            )
            .await
            .map_err(|error| TelegramError::Telegram(error.to_string()))?;
        let has_more = page.has_more();
        let messages = page.messages.iter().map(summarize_message).collect::<Vec<_>>();
        let next_offset_id = (has_more || messages.len() == limit)
            .then(|| messages.last().map(|message| message.message_id))
            .flatten();
        let completeness =
            if next_offset_id.is_some() { Completeness::Page } else { Completeness::Complete };
        Ok(MessagePage { messages, next_offset_id, completeness })
    }

    pub async fn get_message_read_receipts(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        message_id: i32,
    ) -> Result<Vec<MessageReadReceipt>, TelegramError> {
        let connected = self.connected(session_path).await?;
        let peer = Self::resolve_peer_ref(&connected.client, chat_id).await?;
        let receipts = connected
            .client
            .get_message_read_participants(peer.as_str(), message_id)
            .await
            .map_err(|error| TelegramError::Telegram(error.to_string()))?;
        Ok(receipts
            .into_iter()
            .map(|receipt| MessageReadReceipt {
                user_id: receipt.user_id,
                read_at_unix: receipt.date,
            })
            .collect())
    }

    pub async fn get_outbox_read_date(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        message_id: i32,
    ) -> Result<OutboxReadDateSummary, TelegramError> {
        let connected = self.connected(session_path).await?;
        let peer = Self::resolve_peer_ref(&connected.client, chat_id).await?;
        let resolved = connected
            .client
            .resolve(peer.as_str())
            .await
            .map_err(|error| TelegramError::Telegram(error.to_string()))?;
        if !matches!(resolved, tl::enums::Peer::User(_)) {
            return Err(TelegramError::InvalidPeer(
                "outbox read dates are available only for private chats".to_string(),
            ));
        }
        let input_peer = connected
            .client
            .resolve_to_input_peer(&resolved)
            .await
            .map_err(|error| TelegramError::Telegram(error.to_string()))?;
        let read_date = connected
            .client
            .invoke(&tl::functions::messages::GetOutboxReadDate {
                peer: input_peer,
                msg_id: message_id,
            })
            .await
            .map_err(|error| TelegramError::Telegram(error.to_string()))?;
        let tl::enums::OutboxReadDate::OutboxReadDate(read_date) = read_date;
        Ok(OutboxReadDateSummary {
            chat_id: canonical_peer_ref(&resolved),
            message_id,
            read_at_unix: read_date.date,
        })
    }

    pub async fn get_user_info(
        &self,
        session_path: Option<PathBuf>,
        user_ref: &str,
    ) -> Result<UserDetails, TelegramError> {
        let connected = self.connected(session_path).await?;
        let peer_ref = Self::resolve_peer_ref(&connected.client, user_ref).await?;
        let peer = connected
            .client
            .resolve(peer_ref.as_str())
            .await
            .map_err(|error| TelegramError::Telegram(error.to_string()))?;
        let user_id = match peer {
            tl::enums::Peer::User(user) => user.user_id,
            tl::enums::Peer::Chat(_) | tl::enums::Peer::Channel(_) => {
                return Err(TelegramError::InvalidPeer(
                    "get_user_info requires a user reference".to_string(),
                ));
            },
        };
        let full = connected
            .client
            .get_user_full(user_id)
            .await
            .map_err(|error| TelegramError::Telegram(error.to_string()))?;
        let user = full.user().ok_or_else(|| {
            TelegramError::PeerNotFound(format!("Telegram returned no user object for {user_ref}"))
        })?;
        let (status, status_timestamp_unix) = user_status(full.status().as_ref());
        Ok(UserDetails {
            user: contact_summary_from_user(&user),
            about: full.about().map(str::to_string),
            blocked: full.blocked(),
            common_chats_count: full.common_chats_count(),
            phone_calls_available: full.phone_calls_available(),
            phone_calls_private: full.phone_calls_private(),
            status,
            status_timestamp_unix,
        })
    }

    pub async fn list_participants(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        limit: usize,
    ) -> Result<Vec<ParticipantSummary>, TelegramError> {
        let page = self
            .list_chat_members(session_path, chat_id, ParticipantFilter::Recent, None, 0, limit)
            .await?;
        Ok(page
            .participants
            .into_iter()
            .map(|member| ParticipantSummary {
                user: member.user,
                role: chat_member_role_name(&member.role).to_string(),
            })
            .collect())
    }

    pub async fn list_chat_members(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        filter: ParticipantFilter,
        query: Option<&str>,
        offset: usize,
        limit: usize,
    ) -> Result<ParticipantPage, TelegramError> {
        if limit == 0 {
            return Err(TelegramError::InvalidPeer(
                "list_participants.limit must be at least 1".to_string(),
            ));
        }
        let connected = self.connected(session_path).await?;
        let chat_ref = Self::resolve_peer_ref(&connected.client, chat_id).await?;
        let chat_peer = connected
            .client
            .resolve(chat_ref.as_str())
            .await
            .map_err(|error| TelegramError::Telegram(error.to_string()))?;
        let input_chat = connected
            .client
            .resolve_to_input_peer(&chat_peer)
            .await
            .map_err(|error| TelegramError::Telegram(error.to_string()))?;
        let canonical_chat_id = canonical_peer_ref(&chat_peer);
        let query = query.map(str::trim).filter(|value| !value.is_empty());
        match input_chat {
            tl::enums::InputPeer::Channel(channel) => {
                let input_channel =
                    tl::enums::InputChannel::InputChannel(tl::types::InputChannel {
                        channel_id: channel.channel_id,
                        access_hash: channel.access_hash,
                    });
                let local_query = query.is_some()
                    && matches!(filter, ParticipantFilter::Admins | ParticipantFilter::Bots);
                if local_query {
                    let mut members = Vec::new();
                    let mut raw_offset = 0_usize;
                    let mut expected_count = None;
                    loop {
                        let response = connected
                            .client
                            .invoke(&tl::functions::channels::GetParticipants {
                                channel: input_channel.clone(),
                                filter: channel_participants_filter(filter, None),
                                offset: i32::try_from(raw_offset).map_err(|_| {
                                    TelegramError::InvalidPeer(
                                        "list_participants scan offset is too large".to_string(),
                                    )
                                })?,
                                limit: 200,
                                hash: 0,
                            })
                            .await
                            .map_err(|error| TelegramError::Telegram(error.to_string()))?;
                        let response = match response {
                            tl::enums::channels::ChannelParticipants::ChannelParticipants(
                                response,
                            ) => response,
                            tl::enums::channels::ChannelParticipants::NotModified => {
                                return Err(TelegramError::Telegram(
                                    "Telegram returned an unusable not-modified participant page for hash=0"
                                        .to_string(),
                                ));
                            },
                        };
                        let count = usize::try_from(response.count).map_err(|_| {
                            TelegramError::Telegram(
                                "Telegram returned a negative participant count".to_string(),
                            )
                        })?;
                        if count > MAX_LOCAL_PARTICIPANT_SCAN {
                            return Err(TelegramError::InvalidPeer(format!(
                                "list_participants query for {filter:?} requires an exact local scan, but Telegram reports {count} entries; omit query or narrow the filter"
                            )));
                        }
                        if expected_count.replace(count).is_some_and(|expected| expected != count) {
                            return Err(TelegramError::Telegram(
                                "Telegram participant inventory changed during the exact scan; retry"
                                    .to_string(),
                            ));
                        }
                        let raw_len = response.participants.len();
                        members.extend(channel_member_summaries_exact(
                            &canonical_chat_id,
                            response.participants,
                            &response.users,
                        )?);
                        raw_offset = raw_offset.saturating_add(raw_len);
                        if raw_offset >= count {
                            break;
                        }
                        if raw_len == 0 {
                            return Err(TelegramError::Telegram(
                                "Telegram returned an empty participant page before its advertised count"
                                    .to_string(),
                            ));
                        }
                    }
                    ensure_unique_participant_ids(&members)?;
                    let query = query.ok_or_else(|| {
                        TelegramError::Telegram(
                            "participant local-search state lost its query".to_string(),
                        )
                    })?;
                    members.retain(|member| participant_query_matches(member, query));
                    return Ok(participant_page(
                        canonical_chat_id,
                        filter,
                        Some(query),
                        offset,
                        limit,
                        members,
                    ));
                }

                let telegram_offset = i32::try_from(offset).map_err(|_| {
                    TelegramError::InvalidPeer("list_participants.offset is too large".to_string())
                })?;
                let telegram_limit = i32::try_from(limit).map_err(|_| {
                    TelegramError::InvalidPeer("list_participants.limit is too large".to_string())
                })?;
                let response = connected
                    .client
                    .invoke(&tl::functions::channels::GetParticipants {
                        channel: input_channel,
                        filter: channel_participants_filter(filter, query),
                        offset: telegram_offset,
                        limit: telegram_limit,
                        hash: 0,
                    })
                    .await
                    .map_err(|error| TelegramError::Telegram(error.to_string()))?;
                let response = match response {
                    tl::enums::channels::ChannelParticipants::ChannelParticipants(response) => {
                        response
                    },
                    tl::enums::channels::ChannelParticipants::NotModified => {
                        return Err(TelegramError::Telegram(
                            "Telegram returned an unusable not-modified participant page for hash=0"
                                .to_string(),
                        ));
                    },
                };
                let total_count = usize::try_from(response.count).map_err(|_| {
                    TelegramError::Telegram(
                        "Telegram returned a negative participant count".to_string(),
                    )
                })?;
                let participants = channel_member_summaries_exact(
                    &canonical_chat_id,
                    response.participants,
                    &response.users,
                )?;
                let returned_count = participants.len();
                if returned_count == 0 && offset < total_count {
                    return Err(TelegramError::Telegram(
                        "Telegram returned an empty participant page before its advertised count"
                            .to_string(),
                    ));
                }
                let next = offset.saturating_add(returned_count);
                let has_more = next < total_count;
                Ok(ParticipantPage {
                    chat_id: canonical_chat_id,
                    filter,
                    query: query.map(str::to_string),
                    offset,
                    limit,
                    total_count,
                    returned_count,
                    participants,
                    next_offset: has_more.then_some(next),
                    has_more,
                    completeness: if has_more {
                        Completeness::Page
                    } else {
                        Completeness::Complete
                    },
                })
            },
            tl::enums::InputPeer::Chat(_) => {
                let full = connected
                    .client
                    .get_chat_full(chat_ref.as_str())
                    .await
                    .map_err(|error| TelegramError::Telegram(error.to_string()))?;
                let tl::enums::messages::ChatFull::ChatFull(full) = full;
                let participants = match full.full_chat {
                    tl::enums::ChatFull::ChatFull(chat) => match chat.participants {
                        tl::enums::ChatParticipants::ChatParticipants(participants) => {
                            participants.participants
                        },
                        tl::enums::ChatParticipants::Forbidden(_) => {
                            return Err(TelegramError::InvalidPeer(
                                "Telegram does not expose this basic group's participant inventory"
                                    .to_string(),
                            ));
                        },
                    },
                    tl::enums::ChatFull::ChannelFull(_) | tl::enums::ChatFull::CommunityFull(_) => {
                        return Err(TelegramError::Telegram(
                            "Telegram returned channel metadata for a basic group".to_string(),
                        ));
                    },
                };
                let mut members = participants
                    .into_iter()
                    .map(|participant| {
                        basic_chat_member_summary(&canonical_chat_id, participant, &full.users)
                            .ok_or_else(|| {
                                TelegramError::Telegram(
                                    "Telegram participant inventory omitted a matching user"
                                        .to_string(),
                                )
                            })
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                ensure_unique_participant_ids(&members)?;
                members.retain(|member| participant_filter_matches(member, filter));
                if let Some(query) = query {
                    members.retain(|member| participant_query_matches(member, query));
                }
                Ok(participant_page(canonical_chat_id, filter, query, offset, limit, members))
            },
            tl::enums::InputPeer::User(_)
            | tl::enums::InputPeer::PeerSelf
            | tl::enums::InputPeer::Empty
            | tl::enums::InputPeer::UserFromMessage(_)
            | tl::enums::InputPeer::ChannelFromMessage(_) => Err(TelegramError::InvalidPeer(
                "list_participants requires a group, supergroup, or channel".to_string(),
            )),
        }
    }

    pub async fn get_chat_member(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        user_ref: &str,
    ) -> Result<Option<ChatMemberSummary>, TelegramError> {
        let connected = self.connected(session_path).await?;
        let chat_ref = Self::resolve_peer_ref(&connected.client, chat_id).await?;
        let chat_peer = connected
            .client
            .resolve(chat_ref.as_str())
            .await
            .map_err(|error| TelegramError::Telegram(error.to_string()))?;
        let input_chat = connected
            .client
            .resolve_to_input_peer(&chat_peer)
            .await
            .map_err(|error| TelegramError::Telegram(error.to_string()))?;
        let canonical_chat_id = canonical_peer_ref(&chat_peer);
        let user_peer_ref = Self::resolve_peer_ref(&connected.client, user_ref).await?;
        let user_peer = connected
            .client
            .resolve(user_peer_ref.as_str())
            .await
            .map_err(|error| TelegramError::Telegram(error.to_string()))?;
        let tl::enums::Peer::User(user_peer_id) = user_peer else {
            return Err(TelegramError::InvalidPeer(
                "get_chat_member.user_ref must identify a Telegram user".to_string(),
            ));
        };
        let input_user_peer = connected
            .client
            .resolve_to_input_peer(&tl::enums::Peer::User(user_peer_id.clone()))
            .await
            .map_err(|error| TelegramError::Telegram(error.to_string()))?;
        match input_chat {
            tl::enums::InputPeer::Channel(channel) => {
                let response = match connected
                    .client
                    .invoke(&tl::functions::channels::GetParticipant {
                        channel: tl::enums::InputChannel::InputChannel(tl::types::InputChannel {
                            channel_id: channel.channel_id,
                            access_hash: channel.access_hash,
                        }),
                        participant: input_user_peer,
                    })
                    .await
                {
                    Ok(response) => response,
                    Err(error) if error.to_string().contains("USER_NOT_PARTICIPANT") => {
                        return Ok(None);
                    },
                    Err(error) => return Err(TelegramError::Telegram(error.to_string())),
                };
                let tl::enums::channels::ChannelParticipant::ChannelParticipant(response) =
                    response;
                Ok(channel_member_summary(
                    &canonical_chat_id,
                    response.participant,
                    &response.users,
                ))
            },
            tl::enums::InputPeer::Chat(_) => {
                let full = connected
                    .client
                    .get_chat_full(chat_ref.as_str())
                    .await
                    .map_err(|error| TelegramError::Telegram(error.to_string()))?;
                let tl::enums::messages::ChatFull::ChatFull(full) = full;
                let participant = match &full.full_chat {
                    tl::enums::ChatFull::ChatFull(chat) => match &chat.participants {
                        tl::enums::ChatParticipants::ChatParticipants(participants) => participants
                            .participants
                            .iter()
                            .find(|participant| {
                                basic_chat_participant_user_id(participant)
                                    == Some(user_peer_id.user_id)
                            })
                            .cloned(),
                        tl::enums::ChatParticipants::Forbidden(_) => None,
                    },
                    tl::enums::ChatFull::ChannelFull(_) | tl::enums::ChatFull::CommunityFull(_) => {
                        None
                    },
                };
                Ok(participant.and_then(|participant| {
                    basic_chat_member_summary(&canonical_chat_id, participant, &full.users)
                }))
            },
            tl::enums::InputPeer::User(_)
            | tl::enums::InputPeer::PeerSelf
            | tl::enums::InputPeer::Empty
            | tl::enums::InputPeer::UserFromMessage(_)
            | tl::enums::InputPeer::ChannelFromMessage(_) => Err(TelegramError::InvalidPeer(
                "get_chat_member.chat_id must identify a group, supergroup, or channel".to_string(),
            )),
        }
    }

    pub async fn add_chat_member(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        user_ref: &str,
    ) -> Result<(), TelegramError> {
        let connected = self.connected(session_path).await?;
        let chat_peer = Self::resolve_peer_ref(&connected.client, chat_id).await?;
        let user_peer_ref = Self::resolve_peer_ref(&connected.client, user_ref).await?;
        let user_peer = connected
            .client
            .resolve(user_peer_ref.as_str())
            .await
            .map_err(|error| TelegramError::Telegram(error.to_string()))?;
        let tl::enums::Peer::User(user) = user_peer else {
            return Err(TelegramError::InvalidPeer(
                "add_chat_member.user_ref must identify a Telegram user".to_string(),
            ));
        };
        connected
            .client
            .invite_users(chat_peer.as_str(), &[user.user_id])
            .await
            .map_err(send_invocation_error)
    }

    pub async fn remove_chat_member(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        user_ref: &str,
    ) -> Result<(), TelegramError> {
        let connected = self.connected(session_path).await?;
        let chat_peer = Self::resolve_peer_ref(&connected.client, chat_id).await?;
        let user_peer_ref = Self::resolve_peer_ref(&connected.client, user_ref).await?;
        let user_peer = connected
            .client
            .resolve(user_peer_ref.as_str())
            .await
            .map_err(|error| TelegramError::Telegram(error.to_string()))?;
        let tl::enums::Peer::User(user) = user_peer else {
            return Err(TelegramError::InvalidPeer(
                "remove_chat_member.user_ref must identify a Telegram user".to_string(),
            ));
        };
        connected.client.kick(chat_peer.as_str(), user.user_id).await.map_err(send_invocation_error)
    }

    pub async fn set_chat_member_admin(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        user_ref: &str,
        rights: &ChatAdminRightsSummary,
        rank: Option<&str>,
    ) -> Result<(), TelegramError> {
        let connected = self.connected(session_path).await?;
        let chat_peer_ref = Self::resolve_peer_ref(&connected.client, chat_id).await?;
        let chat_peer = connected
            .client
            .resolve(chat_peer_ref.as_str())
            .await
            .map_err(|error| TelegramError::Telegram(error.to_string()))?;
        let input_chat = connected
            .client
            .resolve_to_input_peer(&chat_peer)
            .await
            .map_err(|error| TelegramError::Telegram(error.to_string()))?;
        let tl::enums::InputPeer::Channel(channel) = input_chat else {
            return Err(TelegramError::InvalidPeer(
                "granular admin rights require a channel or supergroup".to_string(),
            ));
        };
        let user_peer_ref = Self::resolve_peer_ref(&connected.client, user_ref).await?;
        let user_peer = connected
            .client
            .resolve(user_peer_ref.as_str())
            .await
            .map_err(|error| TelegramError::Telegram(error.to_string()))?;
        let input_user_peer = connected
            .client
            .resolve_to_input_peer(&user_peer)
            .await
            .map_err(|error| TelegramError::Telegram(error.to_string()))?;
        let tl::enums::InputPeer::User(user) = input_user_peer else {
            return Err(TelegramError::InvalidPeer(
                "set_chat_member_admin.user_ref must identify a Telegram user".to_string(),
            ));
        };
        connected
            .client
            .invoke(&tl::functions::channels::EditAdmin {
                channel: tl::enums::InputChannel::InputChannel(tl::types::InputChannel {
                    channel_id: channel.channel_id,
                    access_hash: channel.access_hash,
                }),
                user_id: tl::enums::InputUser::InputUser(tl::types::InputUser {
                    user_id: user.user_id,
                    access_hash: user.access_hash,
                }),
                admin_rights: chat_admin_rights_to_tl(rights),
                rank: rank.map(str::to_string),
            })
            .await
            .map_err(send_invocation_error)?;
        Ok(())
    }

    pub async fn set_chat_member_restrictions(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        user_ref: &str,
        rights: &ChatBannedRightsSummary,
    ) -> Result<(), TelegramError> {
        let connected = self.connected(session_path).await?;
        let chat_peer_ref = Self::resolve_peer_ref(&connected.client, chat_id).await?;
        let chat_peer = connected
            .client
            .resolve(chat_peer_ref.as_str())
            .await
            .map_err(|error| TelegramError::Telegram(error.to_string()))?;
        let input_chat = connected
            .client
            .resolve_to_input_peer(&chat_peer)
            .await
            .map_err(|error| TelegramError::Telegram(error.to_string()))?;
        let tl::enums::InputPeer::Channel(channel) = input_chat else {
            return Err(TelegramError::InvalidPeer(
                "granular member restrictions require a channel or supergroup".to_string(),
            ));
        };
        let user_peer_ref = Self::resolve_peer_ref(&connected.client, user_ref).await?;
        let user_peer = connected
            .client
            .resolve(user_peer_ref.as_str())
            .await
            .map_err(|error| TelegramError::Telegram(error.to_string()))?;
        let input_user = connected
            .client
            .resolve_to_input_peer(&user_peer)
            .await
            .map_err(|error| TelegramError::Telegram(error.to_string()))?;
        connected
            .client
            .invoke(&tl::functions::channels::EditBanned {
                channel: tl::enums::InputChannel::InputChannel(tl::types::InputChannel {
                    channel_id: channel.channel_id,
                    access_hash: channel.access_hash,
                }),
                participant: input_user,
                banned_rights: chat_banned_rights_to_tl(rights),
            })
            .await
            .map_err(send_invocation_error)?;
        Ok(())
    }

    pub async fn list_invite_links(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        admin_user_id: i64,
        revoked: bool,
        limit: usize,
    ) -> Result<Vec<InviteLinkSummary>, TelegramError> {
        let connected = self.connected(session_path).await?;
        let chat_ref = Self::resolve_peer_ref(&connected.client, chat_id).await?;
        let chat_peer = connected
            .client
            .resolve(chat_ref.as_str())
            .await
            .map_err(|error| TelegramError::Telegram(error.to_string()))?;
        let input_peer = connected
            .client
            .resolve_to_input_peer(&chat_peer)
            .await
            .map_err(|error| TelegramError::Telegram(error.to_string()))?;
        let me = connected
            .client
            .get_me()
            .await
            .map_err(|error| TelegramError::Telegram(error.to_string()))?;
        let admin_id = if admin_user_id == me.id {
            tl::enums::InputUser::UserSelf
        } else {
            resolve_input_user(
                &connected.client,
                &admin_user_id.to_string(),
                "list_invite_links.admin_id",
            )
            .await?
        };
        let limit = i32::try_from(limit).map_err(|_| {
            TelegramError::InvalidPeer("invite-link limit is too large".to_string())
        })?;
        let page = connected
            .client
            .invoke(&tl::functions::messages::GetExportedChatInvites {
                revoked,
                peer: input_peer,
                admin_id,
                offset_date: None,
                offset_link: None,
                limit,
            })
            .await
            .map_err(|error| TelegramError::Telegram(error.to_string()))?;
        let tl::enums::messages::ExportedChatInvites::ExportedChatInvites(page) = page;
        Ok(page.invites.into_iter().map(invite_link_summary).collect())
    }

    pub async fn get_invite_link(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        admin_user_id: i64,
        invite_handle: &str,
    ) -> Result<Option<InviteLinkSummary>, TelegramError> {
        let connected = self.connected(session_path).await?;
        let chat_ref = Self::resolve_peer_ref(&connected.client, chat_id).await?;
        for revoked in [false, true] {
            if let Some(invite) = find_raw_invite_link(
                &connected.client,
                chat_ref.as_str(),
                admin_user_id,
                invite_handle,
                revoked,
            )
            .await?
            {
                return Ok(Some(invite_link_summary(invite)));
            }
        }
        Ok(None)
    }

    pub async fn export_invite_link(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        expire_date_unix: Option<i32>,
        usage_limit: Option<i32>,
        request_needed: bool,
        title: Option<&str>,
    ) -> Result<InviteLinkSummary, TelegramError> {
        let connected = self.connected(session_path).await?;
        let chat_ref = Self::resolve_peer_ref(&connected.client, chat_id).await?;
        let invite = connected
            .client
            .export_invite_link(
                chat_ref.as_str(),
                expire_date_unix,
                usage_limit,
                request_needed,
                title.map(str::to_string),
            )
            .await
            .map_err(send_invocation_error)?;
        Ok(invite_link_summary(invite))
    }

    pub async fn revoke_invite_link(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        admin_user_id: i64,
        invite_handle: &str,
    ) -> Result<InviteLinkSummary, TelegramError> {
        let connected = self.connected(session_path).await?;
        let chat_ref = Self::resolve_peer_ref(&connected.client, chat_id).await?;
        if let Some(existing) = find_raw_invite_link(
            &connected.client,
            chat_ref.as_str(),
            admin_user_id,
            invite_handle,
            true,
        )
        .await?
        {
            return Ok(invite_link_summary(existing));
        }
        let invite = find_raw_invite_link(
            &connected.client,
            chat_ref.as_str(),
            admin_user_id,
            invite_handle,
            false,
        )
        .await?
        .ok_or_else(|| TelegramError::InvalidPeer("invite handle was not found".to_string()))?;
        let raw_link = raw_invite_link(&invite).ok_or_else(|| {
            TelegramError::InvalidPeer("the selected invite kind cannot be revoked".to_string())
        })?;
        let revoked = connected
            .client
            .revoke_invite_link(chat_ref.as_str(), raw_link)
            .await
            .map_err(send_invocation_error)?;
        Ok(invite_link_summary(revoked))
    }

    pub async fn delete_revoked_invite_link(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        admin_user_id: i64,
        invite_handle: &str,
    ) -> Result<(), TelegramError> {
        let connected = self.connected(session_path).await?;
        let chat_ref = Self::resolve_peer_ref(&connected.client, chat_id).await?;
        let invite = find_raw_invite_link(
            &connected.client,
            chat_ref.as_str(),
            admin_user_id,
            invite_handle,
            true,
        )
        .await?
        .ok_or_else(|| {
            TelegramError::InvalidPeer("revoked invite handle was not found".to_string())
        })?;
        let raw_link = raw_invite_link(&invite).ok_or_else(|| {
            TelegramError::InvalidPeer("the selected invite kind cannot be deleted".to_string())
        })?;
        connected
            .client
            .delete_invite_link(chat_ref.as_str(), raw_link)
            .await
            .map_err(send_invocation_error)
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn list_invite_joiners(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        admin_user_id: i64,
        invite_handle: Option<&str>,
        requested: bool,
        limit: usize,
        offset_date_unix: i32,
        offset_user_id: i64,
    ) -> Result<InviteJoinerPage, TelegramError> {
        let connected = self.connected(session_path).await?;
        let chat_ref = Self::resolve_peer_ref(&connected.client, chat_id).await?;
        let chat_peer = connected
            .client
            .resolve(chat_ref.as_str())
            .await
            .map_err(|error| TelegramError::Telegram(error.to_string()))?;
        let input_peer = connected
            .client
            .resolve_to_input_peer(&chat_peer)
            .await
            .map_err(|error| TelegramError::Telegram(error.to_string()))?;
        let raw_link = match invite_handle {
            Some(handle) => Some(
                resolve_raw_invite_filter(
                    &connected.client,
                    chat_ref.as_str(),
                    admin_user_id,
                    handle,
                )
                .await?,
            ),
            None => None,
        };
        let offset_user = if offset_user_id == 0 {
            tl::enums::InputUser::Empty
        } else {
            let input_peer = connected
                .client
                .resolve_to_input_peer(&tl::enums::Peer::User(tl::types::PeerUser {
                    user_id: offset_user_id,
                }))
                .await
                .map_err(|error| TelegramError::Telegram(error.to_string()))?;
            match input_peer {
                tl::enums::InputPeer::User(user) => {
                    tl::enums::InputUser::InputUser(tl::types::InputUser {
                        user_id: user.user_id,
                        access_hash: user.access_hash,
                    })
                },
                tl::enums::InputPeer::PeerSelf => tl::enums::InputUser::UserSelf,
                tl::enums::InputPeer::UserFromMessage(user) => {
                    tl::enums::InputUser::FromMessage(tl::types::InputUserFromMessage {
                        peer: user.peer,
                        msg_id: user.msg_id,
                        user_id: user.user_id,
                    })
                },
                _ => {
                    return Err(TelegramError::InvalidPeer(format!(
                        "invite-joiner offset user {offset_user_id} did not resolve to a Telegram user"
                    )));
                },
            }
        };
        let telegram_limit = i32::try_from(limit).map_err(|_| {
            TelegramError::InvalidPeer("invite-joiner limit is too large".to_string())
        })?;
        let response = connected
            .client
            .invoke(&tl::functions::messages::GetChatInviteImporters {
                requested,
                subscription_expired: false,
                peer: input_peer,
                link: raw_link,
                q: None,
                offset_date: offset_date_unix,
                offset_user,
                limit: telegram_limit,
            })
            .await
            .map_err(|error| TelegramError::Telegram(error.to_string()))?;
        let tl::enums::messages::ChatInviteImporters::ChatInviteImporters(response) = response;
        let total_count = usize::try_from(response.count.max(0)).unwrap_or_default();
        let source_invite_handle = invite_handle.map(str::to_string);
        let entries = response
            .importers
            .into_iter()
            .map(|importer| {
                let tl::enums::ChatInviteImporter::ChatInviteImporter(importer) = importer;
                InviteJoinerSummary {
                    user_id: importer.user_id,
                    user: contact_summary_by_user_id(&response.users, importer.user_id),
                    requested: importer.requested,
                    via_chatlist: importer.via_chatlist,
                    date_unix: importer.date,
                    about: importer.about,
                    approved_by_user_id: importer.approved_by,
                    source_invite_handle: source_invite_handle.clone(),
                }
            })
            .collect::<Vec<_>>();
        let has_more = entries.len() == limit && !entries.is_empty();
        let (next_offset_date_unix, next_offset_user_id) = if has_more {
            entries
                .last()
                .map(|entry| (Some(entry.date_unix), Some(entry.user_id)))
                .unwrap_or((None, None))
        } else {
            (None, None)
        };
        Ok(InviteJoinerPage {
            total_count,
            entries,
            next_offset_date_unix,
            next_offset_user_id,
            completeness: if has_more { Completeness::Page } else { Completeness::Complete },
        })
    }

    pub async fn process_join_request(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        user_id: i64,
        approve: bool,
    ) -> Result<(), TelegramError> {
        let connected = self.connected(session_path).await?;
        let chat_ref = Self::resolve_peer_ref(&connected.client, chat_id).await?;
        connected
            .client
            .join_request(chat_ref.as_str(), user_id, approve)
            .await
            .map_err(send_invocation_error)
    }

    pub async fn get_account_profile(
        &self,
        session_path: Option<PathBuf>,
    ) -> Result<AccountProfileSummary, TelegramError> {
        let connected = self.connected(session_path).await?;
        let response = connected
            .client
            .invoke(&tl::functions::users::GetFullUser { id: tl::enums::InputUser::UserSelf })
            .await
            .map_err(|error| TelegramError::Telegram(error.to_string()))?;
        let tl::enums::users::UserFull::UserFull(response) = response;
        let tl::enums::UserFull::UserFull(full) = response.full_user;
        let raw_user = response.users.iter().find_map(|user| match user {
            tl::enums::User::User(user) if user.id == full.id => Some(user.clone()),
            _ => None,
        });
        let raw_user = raw_user.ok_or_else(|| {
            TelegramError::PeerNotFound(
                "Telegram returned no authenticated user object with the full profile".to_string(),
            )
        })?;
        let user = contact_summary(tl::enums::User::User(raw_user.clone())).ok_or_else(|| {
            TelegramError::PeerNotFound(
                "Telegram returned an unusable authenticated user object".to_string(),
            )
        })?;
        let business = business_account_settings_summary(&full)?;
        let personal_channel = match (full.personal_channel_id, full.personal_channel_message) {
            (None, None) => None,
            (Some(channel_id), Some(message_id)) if channel_id > 0 => {
                let canonical_channel_id = format!("-100{channel_id}");
                let chat = response
                    .chats
                    .iter()
                    .cloned()
                    .filter_map(chat_summary)
                    .find(|chat| chat.chat_id == canonical_channel_id)
                    .ok_or_else(|| {
                        TelegramError::PeerNotFound(format!(
                            "Telegram returned personal channel {canonical_channel_id} without its exact chat object"
                        ))
                    })?;
                Some(PersonalChannelSummary { chat, message_id })
            },
            _ => {
                return Err(TelegramError::Telegram(
                    "Telegram returned an inconsistent personal-channel identity/message pair"
                        .to_string(),
                ));
            },
        };
        Ok(AccountProfileSummary {
            user_id: user.user_id,
            first_name: user.first_name,
            last_name: user.last_name,
            username: user.username,
            about: full.about,
            birthday: full.birthday.map(|birthday| {
                let tl::enums::Birthday::Birthday(birthday) = birthday;
                BirthdaySummary { day: birthday.day, month: birthday.month, year: birthday.year }
            }),
            name_color: raw_user.color.map(peer_color_summary),
            profile_color: raw_user.profile_color.map(peer_color_summary),
            personal_channel,
            business,
        })
    }

    pub async fn get_bot_commands(
        &self,
        session_path: Option<PathBuf>,
        language_code: &str,
    ) -> Result<Vec<BotCommandSummary>, TelegramError> {
        let connected = self.connected(session_path).await?;
        let commands = connected
            .client
            .invoke(&bot_commands_get_request(language_code))
            .await
            .map_err(|error| TelegramError::Telegram(error.to_string()))?;
        Ok(commands
            .into_iter()
            .map(|command| {
                let tl::enums::BotCommand::BotCommand(command) = command;
                BotCommandSummary {
                    command: command.command,
                    description: command.description,
                    ephemeral: command.ephemeral,
                }
            })
            .collect())
    }

    pub async fn set_bot_commands(
        &self,
        session_path: Option<PathBuf>,
        language_code: &str,
        commands: &[BotCommandSummary],
    ) -> Result<(), TelegramError> {
        let connected = self.connected(session_path).await?;
        if commands.is_empty() {
            connected
                .client
                .invoke(&bot_commands_reset_request(language_code))
                .await
                .map_err(send_invocation_error)?;
        } else {
            connected
                .client
                .invoke(&bot_commands_set_request(language_code, commands))
                .await
                .map_err(send_invocation_error)?;
        }
        Ok(())
    }

    pub async fn get_account_emoji_status(
        &self,
        session_path: Option<PathBuf>,
    ) -> Result<EmojiStatusSummary, TelegramError> {
        let connected = self.connected(session_path).await?;
        let me = connected
            .client
            .get_me()
            .await
            .map_err(|error| TelegramError::Telegram(error.to_string()))?;
        Ok(me.emoji_status.map(emoji_status_summary).unwrap_or_else(empty_emoji_status_summary))
    }

    pub async fn list_emoji_statuses(
        &self,
        session_path: Option<PathBuf>,
        kind: EmojiStatusCatalogKind,
        hash: i64,
    ) -> Result<EmojiStatusCatalogPage, TelegramError> {
        let connected = self.connected(session_path).await?;
        let response = match kind {
            EmojiStatusCatalogKind::Default => {
                connected
                    .client
                    .invoke(&tl::functions::account::GetDefaultEmojiStatuses { hash })
                    .await
            },
            EmojiStatusCatalogKind::Recent => {
                connected
                    .client
                    .invoke(&tl::functions::account::GetRecentEmojiStatuses { hash })
                    .await
            },
            EmojiStatusCatalogKind::ChannelDefault => {
                connected
                    .client
                    .invoke(&tl::functions::account::GetChannelDefaultEmojiStatuses { hash })
                    .await
            },
            EmojiStatusCatalogKind::Collectible => {
                connected
                    .client
                    .invoke(&tl::functions::account::GetCollectibleEmojiStatuses { hash })
                    .await
            },
        }
        .map_err(|error| TelegramError::Telegram(error.to_string()))?;
        match response {
            tl::enums::account::EmojiStatuses::NotModified => Ok(EmojiStatusCatalogPage {
                kind,
                request_hash: hash.to_string(),
                response_hash: None,
                not_modified: true,
                available_count: None,
                returned_count: 0,
                truncated: false,
                statuses: Vec::new(),
            }),
            tl::enums::account::EmojiStatuses::EmojiStatuses(response) => {
                let statuses =
                    response.statuses.into_iter().map(emoji_status_summary).collect::<Vec<_>>();
                let count = statuses.len();
                Ok(EmojiStatusCatalogPage {
                    kind,
                    request_hash: hash.to_string(),
                    response_hash: Some(response.hash.to_string()),
                    not_modified: false,
                    available_count: Some(count),
                    returned_count: count,
                    truncated: false,
                    statuses,
                })
            },
        }
    }

    pub async fn set_account_emoji_status(
        &self,
        session_path: Option<PathBuf>,
        status: &AccountEmojiStatusSpec,
    ) -> Result<(), TelegramError> {
        let connected = self.connected(session_path).await?;
        let emoji_status = emoji_status_input(status)?;
        let accepted = connected
            .client
            .invoke(&tl::functions::account::UpdateEmojiStatus { emoji_status })
            .await
            .map_err(send_invocation_error)?;
        if !accepted {
            return Err(TelegramError::SendRejected(
                "Telegram returned false while updating the account emoji status".to_string(),
            ));
        }
        Ok(())
    }

    pub async fn clear_recent_emoji_statuses(
        &self,
        session_path: Option<PathBuf>,
    ) -> Result<(), TelegramError> {
        let connected = self.connected(session_path).await?;
        let accepted = connected
            .client
            .invoke(&tl::functions::account::ClearRecentEmojiStatuses {})
            .await
            .map_err(send_invocation_error)?;
        if !accepted {
            return Err(TelegramError::SendRejected(
                "Telegram returned false while clearing recent emoji statuses".to_string(),
            ));
        }
        Ok(())
    }

    pub async fn update_account_profile(
        &self,
        session_path: Option<PathBuf>,
        first_name: Option<&str>,
        last_name: Option<&str>,
        about: Option<&str>,
    ) -> Result<(), TelegramError> {
        let connected = self.connected(session_path).await?;
        connected
            .client
            .invoke(&tl::functions::account::UpdateProfile {
                first_name: first_name.map(str::to_string),
                last_name: last_name.map(str::to_string),
                about: about.map(str::to_string),
            })
            .await
            .map_err(send_invocation_error)?;
        Ok(())
    }

    pub async fn update_account_username(
        &self,
        session_path: Option<PathBuf>,
        username: Option<&str>,
    ) -> Result<(), TelegramError> {
        let connected = self.connected(session_path).await?;
        connected
            .client
            .invoke(&tl::functions::account::UpdateUsername {
                username: username.unwrap_or_default().to_string(),
            })
            .await
            .map_err(send_invocation_error)?;
        Ok(())
    }

    pub async fn update_account_birthday(
        &self,
        session_path: Option<PathBuf>,
        birthday: Option<&BirthdaySummary>,
    ) -> Result<(), TelegramError> {
        let connected = self.connected(session_path).await?;
        let birthday = birthday.map(|birthday| {
            tl::enums::Birthday::Birthday(tl::types::Birthday {
                day: birthday.day,
                month: birthday.month,
                year: birthday.year,
            })
        });
        connected
            .client
            .invoke(&tl::functions::account::UpdateBirthday { birthday })
            .await
            .map_err(send_invocation_error)?;
        Ok(())
    }

    pub async fn update_account_color(
        &self,
        session_path: Option<PathBuf>,
        for_profile: bool,
        color: Option<i32>,
        background_emoji_id: Option<&str>,
    ) -> Result<(), TelegramError> {
        let connected = self.connected(session_path).await?;
        let request = account_color_update_request(for_profile, color, background_emoji_id)?;
        let accepted = connected.client.invoke(&request).await.map_err(send_invocation_error)?;
        if !accepted {
            return Err(TelegramError::SendRejected(
                "Telegram returned false while updating the account peer color".to_string(),
            ));
        }
        Ok(())
    }

    pub async fn update_personal_channel(
        &self,
        session_path: Option<PathBuf>,
        channel_id: Option<&str>,
    ) -> Result<(), TelegramError> {
        let connected = self.connected(session_path).await?;
        let channel = if let Some(channel_id) = channel_id {
            let peer_ref = Self::resolve_peer_ref(&connected.client, channel_id).await?;
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
            input_channel_for_personal_channel(input_peer)?
        } else {
            tl::enums::InputChannel::Empty
        };
        let accepted = connected
            .client
            .invoke(&tl::functions::account::UpdatePersonalChannel { channel })
            .await
            .map_err(send_invocation_error)?;
        if !accepted {
            return Err(TelegramError::SendRejected(
                "Telegram returned false while updating the personal channel".to_string(),
            ));
        }
        Ok(())
    }

    pub async fn set_account_presence(
        &self,
        session_path: Option<PathBuf>,
        online: bool,
    ) -> Result<(), TelegramError> {
        let connected = self.connected(session_path).await?;
        connected.client.set_presence(online).await.map_err(send_invocation_error)
    }

    pub async fn list_profile_photos(
        &self,
        session_path: Option<PathBuf>,
        offset: usize,
        limit: usize,
    ) -> Result<ProfilePhotoPage, TelegramError> {
        let connected = self.connected(session_path).await?;
        let offset = i32::try_from(offset).map_err(|_| {
            TelegramError::Telegram("profile photo offset exceeds Telegram's int range".into())
        })?;
        let limit = i32::try_from(limit).map_err(|_| {
            TelegramError::Telegram("profile photo limit exceeds Telegram's int range".into())
        })?;
        let response = connected
            .client
            .invoke(&tl::functions::photos::GetUserPhotos {
                user_id: tl::enums::InputUser::UserSelf,
                offset,
                max_id: 0,
                limit,
            })
            .await
            .map_err(|error| TelegramError::Telegram(error.to_string()))?;
        let (total_count, raw_photos) = unpack_profile_photos(response, offset)?;
        let photos = raw_photos.iter().filter_map(profile_photo_summary).collect::<Vec<_>>();
        let offset = usize::try_from(offset).unwrap_or_default();
        let next_offset = (offset.saturating_add(raw_photos.len()) < total_count)
            .then_some(offset.saturating_add(raw_photos.len()));
        Ok(ProfilePhotoPage {
            photos,
            offset,
            next_offset,
            total_count,
            completeness: if next_offset.is_some() {
                Completeness::Page
            } else {
                Completeness::Complete
            },
        })
    }

    pub async fn get_profile_photo(
        &self,
        session_path: Option<PathBuf>,
        photo_handle: &str,
    ) -> Result<Option<ProfilePhotoSummary>, TelegramError> {
        let connected = self.connected(session_path).await?;
        Ok(find_raw_profile_photo(&connected.client, photo_handle)
            .await?
            .as_ref()
            .and_then(|photo| profile_photo_summary(&tl::enums::Photo::Photo(photo.clone()))))
    }

    pub async fn upload_profile_photo(
        &self,
        session_path: Option<PathBuf>,
        file_path: &Path,
        fallback: bool,
    ) -> Result<ProfilePhotoSummary, TelegramError> {
        let connected = self.connected(session_path).await?;
        let uploaded = connected
            .client
            .upload_file(file_path)
            .await
            .map_err(|error| TelegramError::Telegram(error.to_string()))?;
        if !uploaded.mime_type().starts_with("image/") || uploaded.mime_type() == "image/gif" {
            return Err(TelegramError::SendRejected(format!(
                "profile photo must be a static image; detected {}",
                uploaded.mime_type()
            )));
        }
        let tl::enums::InputMedia::UploadedPhoto(uploaded_photo) = uploaded.as_photo_media() else {
            return Err(TelegramError::Telegram(
                "Ferogram did not produce uploaded photo media".to_string(),
            ));
        };
        let response = connected
            .client
            .invoke(&tl::functions::photos::UploadProfilePhoto {
                fallback,
                bot: None,
                file: Some(uploaded_photo.file),
                video: None,
                video_start_ts: None,
                video_emoji_markup: None,
            })
            .await
            .map_err(send_invocation_error)?;
        let tl::enums::photos::Photo::Photo(response) = response;
        profile_photo_summary(&response.photo).ok_or_else(|| {
            TelegramError::Telegram("Telegram returned an empty uploaded profile photo".into())
        })
    }

    pub async fn delete_profile_photos(
        &self,
        session_path: Option<PathBuf>,
        photo_handles: &[String],
    ) -> Result<(), TelegramError> {
        let connected = self.connected(session_path).await?;
        let mut photos = Vec::with_capacity(photo_handles.len());
        for handle in photo_handles {
            let Some(photo) = find_raw_profile_photo(&connected.client, handle).await? else {
                continue;
            };
            photos.push(tl::enums::InputPhoto::InputPhoto(tl::types::InputPhoto {
                id: photo.id,
                access_hash: photo.access_hash,
                file_reference: photo.file_reference,
            }));
        }
        if photos.is_empty() {
            return Ok(());
        }
        let expected_ids = photos
            .iter()
            .filter_map(|photo| match photo {
                tl::enums::InputPhoto::InputPhoto(photo) => Some(photo.id),
                tl::enums::InputPhoto::Empty => None,
            })
            .collect::<HashSet<_>>();
        let deleted_ids = connected
            .client
            .invoke(&tl::functions::photos::DeletePhotos { id: photos })
            .await
            .map_err(send_invocation_error)?;
        let deleted_ids = deleted_ids.into_iter().collect::<HashSet<_>>();
        if expected_ids.is_subset(&deleted_ids) {
            Ok(())
        } else {
            Err(TelegramError::DeliveryUncertain(
                "Telegram returned an incomplete profile-photo deletion receipt".into(),
            ))
        }
    }

    pub async fn get_sessions(
        &self,
        session_path: Option<PathBuf>,
    ) -> Result<Vec<SessionSummary>, TelegramError> {
        let connected = self.connected(session_path).await?;
        let sessions = connected
            .client
            .get_authorizations()
            .await
            .map_err(|error| TelegramError::Telegram(error.to_string()))?;
        Ok(sessions
            .into_iter()
            .map(|session| SessionSummary {
                authorization_id: session.hash,
                current: session.current,
                official_app: session.official_app,
                password_pending: session.password_pending,
                encrypted_requests_disabled: session.encrypted_requests_disabled,
                call_requests_disabled: session.call_requests_disabled,
                unconfirmed: session.unconfirmed,
                device_model: session.device_model,
                platform: session.platform,
                system_version: session.system_version,
                api_id: session.api_id,
                app_name: session.app_name,
                app_version: session.app_version,
                date_created_unix: session.date_created,
                date_active_unix: session.date_active,
                ip: session.ip,
                country: session.country,
                region: session.region,
            })
            .collect())
    }

    pub async fn terminate_session(
        &self,
        session_path: Option<PathBuf>,
        authorization_id: i64,
    ) -> Result<(), TelegramError> {
        let connected = self.connected(session_path).await?;
        connected
            .client
            .terminate_session(authorization_id)
            .await
            .map_err(|error| TelegramError::Telegram(error.to_string()))
    }

    pub async fn terminate_all_other_sessions(
        &self,
        session_path: Option<PathBuf>,
    ) -> Result<(), TelegramError> {
        let connected = self.connected(session_path).await?;
        let accepted = connected
            .client
            .invoke(&tl::functions::auth::ResetAuthorizations {})
            .await
            .map_err(send_invocation_error)?;
        if !accepted {
            return Err(TelegramError::SendRejected(
                "Telegram returned false while terminating all other authorizations".to_string(),
            ));
        }
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn set_contact(
        &self,
        session_path: Option<PathBuf>,
        user_id: i64,
        contact: bool,
        first_name: &str,
        last_name: &str,
        phone: &str,
        add_phone_privacy_exception: bool,
    ) -> Result<(), TelegramError> {
        let connected = self.connected(session_path).await?;
        if contact {
            connected
                .client
                .add_contact(user_id, first_name, last_name, phone, add_phone_privacy_exception)
                .await
                .map_err(|error| TelegramError::Telegram(error.to_string()))
        } else {
            connected
                .client
                .delete_contacts(&[user_id])
                .await
                .map_err(|error| TelegramError::Telegram(error.to_string()))
        }
    }

    pub async fn get_common_chats(
        &self,
        session_path: Option<PathBuf>,
        user_ref: &str,
        max_id: i64,
        limit: usize,
    ) -> Result<Vec<ChatSummary>, TelegramError> {
        let connected = self.connected(session_path).await?;
        let peer_ref = Self::resolve_peer_ref(&connected.client, user_ref).await?;
        let peer = connected
            .client
            .resolve(peer_ref.as_str())
            .await
            .map_err(|error| TelegramError::Telegram(error.to_string()))?;
        let user_id = match peer {
            tl::enums::Peer::User(user) => user.user_id,
            tl::enums::Peer::Chat(_) | tl::enums::Peer::Channel(_) => {
                return Err(TelegramError::InvalidPeer(
                    "get_common_chats requires a user reference".to_string(),
                ));
            },
        };
        let telegram_limit = i32::try_from(limit).map_err(|_| {
            TelegramError::InvalidPeer("get_common_chats.limit is too large".to_string())
        })?;
        let chats = connected
            .client
            .get_common_chats(user_id, max_id, telegram_limit)
            .await
            .map_err(|error| TelegramError::Telegram(error.to_string()))?;
        Ok(chats.into_iter().filter_map(chat_summary).collect())
    }

    pub async fn get_chat_lifecycle_state(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
    ) -> Result<ChatLifecycleState, TelegramError> {
        let connected = self.connected(session_path).await?;
        chat_lifecycle_state(&connected.client, chat_id).await
    }

    pub async fn get_chat_auto_delete(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
    ) -> Result<ChatAutoDeleteSummary, TelegramError> {
        let connected = self.connected(session_path).await?;
        chat_auto_delete_summary(&connected.client, chat_id).await
    }

    pub async fn set_chat_auto_delete(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        period_seconds: i32,
    ) -> Result<(), TelegramError> {
        if period_seconds < 0 {
            return Err(TelegramError::InvalidPeer(
                "auto-delete period must be non-negative".to_string(),
            ));
        }
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
        connected
            .client
            .invoke(&tl::functions::messages::SetHistoryTtl {
                peer: input_peer,
                period: period_seconds,
            })
            .await
            .map_err(send_invocation_error)?;
        Ok(())
    }

    pub async fn create_group(
        &self,
        session_path: Option<PathBuf>,
        title: &str,
        user_ids: &[i64],
    ) -> Result<ChatDetailsSummary, TelegramError> {
        let connected = self.connected(session_path).await?;
        let raw = connected
            .client
            .create_group(title, user_ids.to_vec())
            .await
            .map_err(send_invocation_error)?;
        connected.client.cache_entities(&[], std::slice::from_ref(&raw)).await;
        let chat = chat_summary(raw).ok_or_else(|| {
            TelegramError::DeliveryUncertain(
                "Telegram created a group but returned no usable chat identity".to_string(),
            )
        })?;
        chat_lifecycle_state(&connected.client, &chat.chat_id)
            .await
            .map(|state| state.details)
            .map_err(|error| TelegramError::DeliveryUncertain(error.to_string()))
    }

    pub async fn create_channel(
        &self,
        session_path: Option<PathBuf>,
        title: &str,
        about: &str,
        broadcast: bool,
    ) -> Result<ChatDetailsSummary, TelegramError> {
        let connected = self.connected(session_path).await?;
        let raw = connected
            .client
            .create_channel(title, about, broadcast)
            .await
            .map_err(send_invocation_error)?;
        connected.client.cache_entities(&[], std::slice::from_ref(&raw)).await;
        let chat = chat_summary(raw).ok_or_else(|| {
            TelegramError::DeliveryUncertain(
                "Telegram created a channel but returned no usable chat identity".to_string(),
            )
        })?;
        chat_lifecycle_state(&connected.client, &chat.chat_id)
            .await
            .map(|state| state.details)
            .map_err(|error| TelegramError::DeliveryUncertain(error.to_string()))
    }

    pub async fn join_chat(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
    ) -> Result<ChatLifecycleState, TelegramError> {
        let connected = self.connected(session_path).await?;
        let peer = Self::resolve_peer_ref(&connected.client, chat_id).await?;
        connected.client.join_chat(peer.as_str()).await.map_err(send_invocation_error)?;
        let state = chat_lifecycle_state(&connected.client, peer.as_str())
            .await
            .map_err(|error| TelegramError::DeliveryUncertain(error.to_string()))?;
        if state.in_dialogs {
            Ok(state)
        } else {
            Err(TelegramError::DeliveryUncertain(format!(
                "Telegram did not return {chat_id} in the exact dialog readback after joining"
            )))
        }
    }

    pub async fn leave_chat(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
    ) -> Result<ChatLifecycleState, TelegramError> {
        let connected = self.connected(session_path).await?;
        let before = chat_lifecycle_state(&connected.client, chat_id).await?;
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
        connected.client.leave_chat(peer_ref.as_str()).await.map_err(send_invocation_error)?;
        if input_peer_is_in_dialogs(&connected.client, input_peer).await? {
            return Err(TelegramError::DeliveryUncertain(format!(
                "Telegram still returns {chat_id} in dialogs after leaving"
            )));
        }
        Ok(ChatLifecycleState { details: before.details, in_dialogs: false })
    }

    pub async fn delete_chat(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
    ) -> Result<ChatLifecycleState, TelegramError> {
        let connected = self.connected(session_path).await?;
        let before = chat_lifecycle_state(&connected.client, chat_id).await?;
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
        connected.client.delete_chat(peer_ref.as_str()).await.map_err(send_invocation_error)?;
        if input_peer_is_in_dialogs(&connected.client, input_peer).await? {
            return Err(TelegramError::DeliveryUncertain(format!(
                "Telegram still returns {chat_id} in dialogs after deletion"
            )));
        }
        Ok(ChatLifecycleState { details: before.details, in_dialogs: false })
    }

    pub async fn update_chat_title(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        title: &str,
    ) -> Result<ChatDetailsSummary, TelegramError> {
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
        match input_peer {
            tl::enums::InputPeer::Channel(channel) => {
                connected
                    .client
                    .invoke(&tl::functions::channels::EditTitle {
                        channel: tl::enums::InputChannel::InputChannel(tl::types::InputChannel {
                            channel_id: channel.channel_id,
                            access_hash: channel.access_hash,
                        }),
                        title: title.to_string(),
                    })
                    .await
                    .map_err(send_invocation_error)?;
            },
            tl::enums::InputPeer::Chat(chat) => {
                connected
                    .client
                    .invoke(&tl::functions::messages::EditChatTitle {
                        chat_id: chat.chat_id,
                        title: title.to_string(),
                    })
                    .await
                    .map_err(send_invocation_error)?;
            },
            _ => {
                return Err(TelegramError::InvalidPeer(
                    "update_chat_title requires a group, supergroup, or channel".to_string(),
                ));
            },
        }
        let details = chat_lifecycle_state(&connected.client, peer_ref.as_str())
            .await
            .map_err(|error| TelegramError::DeliveryUncertain(error.to_string()))?
            .details;
        if details.chat.title == title {
            Ok(details)
        } else {
            Err(TelegramError::DeliveryUncertain(format!(
                "Telegram did not return the requested title for {chat_id}"
            )))
        }
    }

    pub async fn update_chat_about(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        about: &str,
    ) -> Result<ChatDetailsSummary, TelegramError> {
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
        let updated = connected
            .client
            .invoke(&tl::functions::messages::EditChatAbout {
                peer: input_peer,
                about: about.to_string(),
            })
            .await
            .map_err(send_invocation_error)?;
        if !updated {
            return Err(TelegramError::SendRejected(
                "Telegram rejected the chat-about update".to_string(),
            ));
        }
        let details = chat_lifecycle_state(&connected.client, peer_ref.as_str())
            .await
            .map_err(|error| TelegramError::DeliveryUncertain(error.to_string()))?
            .details;
        if details.about.as_deref().unwrap_or_default() == about {
            Ok(details)
        } else {
            Err(TelegramError::DeliveryUncertain(format!(
                "Telegram did not return the requested about text for {chat_id}"
            )))
        }
    }

    pub async fn update_chat_photo(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        file_path: &Path,
    ) -> Result<ChatDetailsSummary, TelegramError> {
        let connected = self.connected(session_path).await?;
        let before = chat_lifecycle_state(&connected.client, chat_id).await?.details;
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
        let uploaded = connected
            .client
            .upload_file(file_path)
            .await
            .map_err(|error| TelegramError::Telegram(error.to_string()))?;
        if !uploaded.mime_type().starts_with("image/") || uploaded.mime_type() == "image/gif" {
            return Err(TelegramError::SendRejected(format!(
                "chat photo must be a static image; detected {}",
                uploaded.mime_type()
            )));
        }
        let tl::enums::InputMedia::UploadedPhoto(uploaded_photo) = uploaded.as_photo_media() else {
            return Err(TelegramError::Telegram(
                "Ferogram did not produce uploaded chat-photo media".to_string(),
            ));
        };
        let photo =
            tl::enums::InputChatPhoto::InputChatUploadedPhoto(tl::types::InputChatUploadedPhoto {
                video: None,
                file: Some(uploaded_photo.file),
                video_start_ts: None,
                video_emoji_markup: None,
            });
        match input_peer {
            tl::enums::InputPeer::Channel(channel) => {
                connected
                    .client
                    .invoke(&tl::functions::channels::EditPhoto {
                        channel: tl::enums::InputChannel::InputChannel(tl::types::InputChannel {
                            channel_id: channel.channel_id,
                            access_hash: channel.access_hash,
                        }),
                        photo,
                    })
                    .await
                    .map_err(send_invocation_error)?;
            },
            tl::enums::InputPeer::Chat(chat) => {
                connected
                    .client
                    .invoke(&tl::functions::messages::EditChatPhoto {
                        chat_id: chat.chat_id,
                        photo,
                    })
                    .await
                    .map_err(send_invocation_error)?;
            },
            _ => {
                return Err(TelegramError::InvalidPeer(
                    "update_chat_photo requires a group, supergroup, or channel".to_string(),
                ));
            },
        }
        let details = chat_lifecycle_state(&connected.client, peer_ref.as_str())
            .await
            .map_err(|error| TelegramError::DeliveryUncertain(error.to_string()))?
            .details;
        if details.photo_handle.is_some() && details.photo_handle != before.photo_handle {
            Ok(details)
        } else {
            Err(TelegramError::DeliveryUncertain(format!(
                "Telegram did not return a new opaque chat-photo identity for {chat_id}"
            )))
        }
    }

    pub async fn delete_chat_photo(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
    ) -> Result<ChatDetailsSummary, TelegramError> {
        let connected = self.connected(session_path).await?;
        let before = chat_lifecycle_state(&connected.client, chat_id).await?.details;
        if before.photo_handle.is_none() {
            return Ok(before);
        }
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
        match input_peer {
            tl::enums::InputPeer::Channel(channel) => {
                connected
                    .client
                    .invoke(&tl::functions::channels::EditPhoto {
                        channel: tl::enums::InputChannel::InputChannel(tl::types::InputChannel {
                            channel_id: channel.channel_id,
                            access_hash: channel.access_hash,
                        }),
                        photo: tl::enums::InputChatPhoto::Empty,
                    })
                    .await
                    .map_err(send_invocation_error)?;
            },
            tl::enums::InputPeer::Chat(chat) => {
                connected
                    .client
                    .invoke(&tl::functions::messages::EditChatPhoto {
                        chat_id: chat.chat_id,
                        photo: tl::enums::InputChatPhoto::Empty,
                    })
                    .await
                    .map_err(send_invocation_error)?;
            },
            _ => {
                return Err(TelegramError::InvalidPeer(
                    "delete_chat_photo requires a group, supergroup, or channel".to_string(),
                ));
            },
        }
        let details = chat_lifecycle_state(&connected.client, peer_ref.as_str())
            .await
            .map_err(|error| TelegramError::DeliveryUncertain(error.to_string()))?
            .details;
        if details.photo_handle.is_none() {
            Ok(details)
        } else {
            Err(TelegramError::DeliveryUncertain(format!(
                "Telegram still returns a chat photo for {chat_id} after deletion"
            )))
        }
    }

    pub async fn get_channel_settings(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
    ) -> Result<ChannelSettingsSummary, TelegramError> {
        let connected = self.connected(session_path).await?;
        let peer_ref = Self::resolve_peer_ref(&connected.client, chat_id).await?;
        let peer = connected
            .client
            .resolve(peer_ref.as_str())
            .await
            .map_err(|error| TelegramError::Telegram(error.to_string()))?;
        let canonical_chat_id = canonical_peer_ref(&peer);
        let full = connected
            .client
            .get_chat_full(peer_ref.as_str())
            .await
            .map_err(|error| TelegramError::Telegram(error.to_string()))?;
        let tl::enums::messages::ChatFull::ChatFull(full) = full;
        let raw_channel = full.chats.iter().find_map(|raw| match raw {
            tl::enums::Chat::Channel(channel)
                if format!("-100{}", channel.id) == canonical_chat_id =>
            {
                Some(channel)
            },
            _ => None,
        });
        let Some(raw_channel) = raw_channel else {
            return Err(TelegramError::InvalidPeer(
                "channel settings require a channel or supergroup".to_string(),
            ));
        };
        let tl::enums::ChatFull::ChannelFull(channel_full) = &full.full_chat else {
            return Err(TelegramError::InvalidPeer(
                "Telegram did not return channel settings for this peer".to_string(),
            ));
        };
        let chat =
            chat_summary(tl::enums::Chat::Channel(raw_channel.clone())).ok_or_else(|| {
                TelegramError::DeliveryUncertain(
                    "Telegram returned no usable channel identity".to_string(),
                )
            })?;
        let photo_handle = opaque_chat_photo_from_raw(&canonical_chat_id, &channel_full.chat_photo);
        Ok(ChannelSettingsSummary {
            details: ChatDetailsSummary {
                chat,
                about: nonempty_owned(&channel_full.about),
                photo_handle,
            },
            can_set_username: channel_full.can_set_username,
            can_view_stats: channel_full.can_view_stats,
            signatures_enabled: raw_channel.signatures,
            signature_profiles_enabled: raw_channel.signature_profiles,
            pre_history_hidden: channel_full.hidden_prehistory,
            join_request_required: raw_channel.join_request,
            join_to_send_required: raw_channel.join_to_send,
            slow_mode_seconds: channel_full.slowmode_seconds.unwrap_or_default(),
            no_forwards: raw_channel.noforwards,
            forum_enabled: raw_channel.forum,
            history_ttl_seconds: channel_full.ttl_period,
            linked_chat_id: channel_full.linked_chat_id.map(|linked_id| format!("-100{linked_id}")),
        })
    }

    pub async fn get_chat_moderation_settings(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
    ) -> Result<ChatModerationSettingsSummary, TelegramError> {
        let connected = self.connected(session_path).await?;
        let peer_ref = Self::resolve_peer_ref(&connected.client, chat_id).await?;
        let peer = connected
            .client
            .resolve(peer_ref.as_str())
            .await
            .map_err(|error| TelegramError::Telegram(error.to_string()))?;
        let canonical_chat_id = canonical_peer_ref(&peer);
        let full = connected
            .client
            .get_chat_full(peer_ref.as_str())
            .await
            .map_err(|error| TelegramError::Telegram(error.to_string()))?;
        let tl::enums::messages::ChatFull::ChatFull(full) = full;
        let raw = full
            .chats
            .iter()
            .find(|raw| {
                chat_summary((*raw).clone())
                    .is_some_and(|summary| summary.chat_id == canonical_chat_id)
            })
            .ok_or_else(|| {
                TelegramError::PeerNotFound(format!(
                    "Telegram returned no chat object for {canonical_chat_id}"
                ))
            })?;
        let chat = chat_summary(raw.clone()).ok_or_else(|| {
            TelegramError::DeliveryUncertain(
                "Telegram returned no usable chat identity for moderation settings".to_string(),
            )
        })?;
        let (about, photo_handle, available_reactions, reactions_limit) = match &full.full_chat {
            tl::enums::ChatFull::ChatFull(details) => (
                nonempty_owned(&details.about),
                details
                    .chat_photo
                    .as_ref()
                    .and_then(|photo| opaque_chat_photo_from_raw(&canonical_chat_id, photo)),
                details.available_reactions.clone().map(chat_reactions_policy_from_tl),
                details.reactions_limit,
            ),
            tl::enums::ChatFull::ChannelFull(details) => (
                nonempty_owned(&details.about),
                opaque_chat_photo_from_raw(&canonical_chat_id, &details.chat_photo),
                details.available_reactions.clone().map(chat_reactions_policy_from_tl),
                details.reactions_limit,
            ),
            tl::enums::ChatFull::CommunityFull(details) => (
                nonempty_owned(&details.about),
                opaque_chat_photo_from_raw(&canonical_chat_id, &details.chat_photo),
                None,
                None,
            ),
        };
        let (default_banned_rights, anti_spam_enabled, forum_enabled, forum_tabs_enabled) =
            match raw {
                tl::enums::Chat::Chat(chat) => (
                    chat.default_banned_rights.clone().map(chat_banned_rights_summary),
                    None,
                    None,
                    None,
                ),
                tl::enums::Chat::Channel(channel) => {
                    let supergroup = channel.megagroup || channel.gigagroup;
                    let anti_spam = match &full.full_chat {
                        tl::enums::ChatFull::ChannelFull(details) if supergroup => {
                            Some(details.antispam)
                        },
                        _ => None,
                    };
                    (
                        channel.default_banned_rights.clone().map(chat_banned_rights_summary),
                        anti_spam,
                        supergroup.then_some(channel.forum),
                        supergroup.then_some(channel.forum_tabs),
                    )
                },
                tl::enums::Chat::Community(community) => (
                    community.default_banned_rights.clone().map(chat_banned_rights_summary),
                    None,
                    None,
                    None,
                ),
                _ => {
                    return Err(TelegramError::InvalidPeer(
                        "chat moderation settings require a group, supergroup, or channel"
                            .to_string(),
                    ));
                },
            };
        Ok(ChatModerationSettingsSummary {
            details: ChatDetailsSummary { chat, about, photo_handle },
            default_banned_rights,
            available_reactions,
            reactions_limit,
            anti_spam_enabled,
            forum_enabled,
            forum_tabs_enabled,
        })
    }

    pub async fn get_channel_stats(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
    ) -> Result<ChannelStatsSummary, TelegramError> {
        let connected = self.connected(session_path).await?;
        let peer_ref = Self::resolve_peer_ref(&connected.client, chat_id).await?;
        let peer = connected
            .client
            .resolve(peer_ref.as_str())
            .await
            .map_err(|error| TelegramError::Telegram(error.to_string()))?;
        let canonical_chat_id = canonical_peer_ref(&peer);
        let stats = connected
            .client
            .stats(peer_ref.as_str())
            .await
            .map_err(|error| TelegramError::Telegram(error.to_string()))?;
        Ok(channel_stats_summary(canonical_chat_id, stats))
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn get_admin_log(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        query: &str,
        events: Option<AdminLogEventFilter>,
        admin_refs: &[String],
        max_id: i64,
        min_id: i64,
        limit: usize,
    ) -> Result<AdminLogPage, TelegramError> {
        let connected = self.connected(session_path).await?;
        let (canonical_chat_id, input_peer) =
            resolve_channel_input_peer(&connected.client, chat_id, "get_admin_log").await?;
        let tl::enums::InputPeer::Channel(channel) = input_peer else {
            unreachable!("resolve_channel_input_peer returned a non-channel peer")
        };
        let input_channel = tl::enums::InputChannel::InputChannel(tl::types::InputChannel {
            channel_id: channel.channel_id,
            access_hash: channel.access_hash,
        });
        let admins = if admin_refs.is_empty() {
            None
        } else {
            let mut admins = Vec::with_capacity(admin_refs.len());
            for admin_ref in admin_refs {
                admins.push(
                    resolve_input_user(&connected.client, admin_ref, "get_admin_log.admins")
                        .await?,
                );
            }
            Some(admins)
        };
        let events_filter = events.map(admin_log_filter_to_tl);
        let response = connected
            .client
            .invoke(&tl::functions::channels::GetAdminLog {
                channel: input_channel,
                q: query.to_string(),
                events_filter,
                admins,
                max_id,
                min_id,
                limit: i32::try_from(limit).unwrap_or(i32::MAX),
            })
            .await
            .map_err(|error| TelegramError::Telegram(error.to_string()))?;
        let tl::enums::channels::AdminLogResults::AdminLogResults(response) = response;
        let raw_count = response.events.len();
        let mut summaries = Vec::with_capacity(raw_count);
        for raw_event in response.events {
            let tl::enums::ChannelAdminLogEvent::ChannelAdminLogEvent(event) = raw_event;
            let (action, details, mut target_user_id, message_id, topic_id) =
                admin_log_action_summary(event.action);
            if target_user_id.is_none()
                && matches!(
                    action,
                    AdminLogActionKind::ParticipantJoin
                        | AdminLogActionKind::ParticipantLeave
                        | AdminLogActionKind::ParticipantJoinByInvite
                        | AdminLogActionKind::ParticipantJoinByRequest
                )
            {
                target_user_id = Some(event.user_id);
            }
            summaries.push(AdminLogEventSummary {
                event_id: event.id,
                date_unix: event.date,
                actor_user_id: event.user_id,
                actor: contact_summary_by_user_id(&response.users, event.user_id),
                action,
                details,
                target_user_id,
                target_user: target_user_id
                    .and_then(|user_id| contact_summary_by_user_id(&response.users, user_id)),
                message_id,
                topic_id,
            });
        }
        let next_max_id =
            (raw_count == limit).then(|| summaries.last().map(|event| event.event_id)).flatten();
        let completeness =
            if next_max_id.is_some() { Completeness::Page } else { Completeness::Complete };
        Ok(AdminLogPage {
            chat_id: canonical_chat_id,
            returned_count: summaries.len(),
            events: summaries,
            next_max_id,
            completeness,
        })
    }

    pub async fn get_my_boosts(
        &self,
        session_path: Option<PathBuf>,
    ) -> Result<MyBoostsSummary, TelegramError> {
        let connected = self.connected(session_path).await?;
        let response = connected
            .client
            .invoke(&tl::functions::premium::GetMyBoosts {})
            .await
            .map_err(|error| TelegramError::Telegram(error.to_string()))?;
        Ok(my_boosts_summary(response))
    }

    pub async fn get_boost_status(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
    ) -> Result<BoostStatusSummary, TelegramError> {
        let connected = self.connected(session_path).await?;
        let (canonical_chat_id, input_peer) =
            resolve_channel_input_peer(&connected.client, chat_id, "get_boost_status").await?;
        let response = connected
            .client
            .invoke(&tl::functions::premium::GetBoostsStatus { peer: input_peer })
            .await
            .map_err(|error| TelegramError::Telegram(error.to_string()))?;
        let tl::enums::premium::BoostsStatus::BoostsStatus(response) = response;
        Ok(BoostStatusSummary {
            chat_id: canonical_chat_id,
            level: response.level,
            current_level_boosts: response.current_level_boosts,
            boosts: response.boosts,
            gift_boosts: response.gift_boosts,
            next_level_boosts: response.next_level_boosts,
            premium_audience: response.premium_audience.as_ref().map(stats_percent_summary),
            boost_url: response.boost_url,
            prepaid_giveaway_count: response.prepaid_giveaways.map_or(0, |items| items.len()),
            my_boost: response.my_boost,
            my_boost_slots: response.my_boost_slots.unwrap_or_default(),
        })
    }

    pub async fn list_boosts(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        gifts_only: bool,
        offset: &str,
        limit: usize,
    ) -> Result<BoostPage, TelegramError> {
        let connected = self.connected(session_path).await?;
        let (_, input_peer) =
            resolve_channel_input_peer(&connected.client, chat_id, "list_boosts").await?;
        let response = connected
            .client
            .invoke(&tl::functions::premium::GetBoostsList {
                gifts: gifts_only,
                peer: input_peer,
                offset: offset.to_string(),
                limit: i32::try_from(limit).unwrap_or(i32::MAX),
            })
            .await
            .map_err(|error| TelegramError::Telegram(error.to_string()))?;
        let tl::enums::premium::BoostsList::BoostsList(response) = response;
        let boosts = response.boosts.into_iter().map(boost_summary).collect::<Vec<_>>();
        Ok(BoostPage {
            total_count: response.count,
            returned_count: boosts.len(),
            boosts,
            next_offset: response.next_offset,
        })
    }

    pub async fn apply_boosts(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        slots: &[i32],
    ) -> Result<MyBoostsSummary, TelegramError> {
        let connected = self.connected(session_path).await?;
        let (_, input_peer) =
            resolve_channel_input_peer(&connected.client, chat_id, "apply_boosts").await?;
        let response = connected
            .client
            .invoke(&tl::functions::premium::ApplyBoost {
                slots: Some(slots.to_vec()),
                peer: input_peer,
            })
            .await
            .map_err(send_invocation_error)?;
        Ok(my_boosts_summary(response))
    }

    pub async fn check_chat_username(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        username: &str,
    ) -> Result<bool, TelegramError> {
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
        let tl::enums::InputPeer::Channel(channel) = input_peer else {
            return Err(TelegramError::InvalidPeer(
                "check_chat_username requires a channel or supergroup".to_string(),
            ));
        };
        connected
            .client
            .invoke(&tl::functions::channels::CheckUsername {
                channel: tl::enums::InputChannel::InputChannel(tl::types::InputChannel {
                    channel_id: channel.channel_id,
                    access_hash: channel.access_hash,
                }),
                username: username.to_string(),
            })
            .await
            .map_err(send_invocation_error)
    }

    pub async fn update_chat_username(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        username: Option<&str>,
    ) -> Result<ChannelSettingsSummary, TelegramError> {
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
        let tl::enums::InputPeer::Channel(channel) = input_peer else {
            return Err(TelegramError::InvalidPeer(
                "update_chat_username requires a channel or supergroup".to_string(),
            ));
        };
        let requested = username.unwrap_or_default().to_string();
        let updated = connected
            .client
            .invoke(&tl::functions::channels::UpdateUsername {
                channel: tl::enums::InputChannel::InputChannel(tl::types::InputChannel {
                    channel_id: channel.channel_id,
                    access_hash: channel.access_hash,
                }),
                username: requested.clone(),
            })
            .await
            .map_err(send_invocation_error)?;
        if !updated {
            return Err(TelegramError::SendRejected(
                "Telegram rejected the channel username update".to_string(),
            ));
        }
        let readback_path = connected.path.clone();
        drop(connected);
        let settings = self
            .get_channel_settings(Some(readback_path), peer_ref.as_str())
            .await
            .map_err(|error| TelegramError::DeliveryUncertain(error.to_string()))?;
        if settings.details.chat.username.as_deref().unwrap_or_default() == requested {
            Ok(settings)
        } else {
            Err(TelegramError::DeliveryUncertain(
                "Telegram did not return the requested channel username".to_string(),
            ))
        }
    }

    pub async fn set_chat_slow_mode(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        seconds: i32,
    ) -> Result<ChannelSettingsSummary, TelegramError> {
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
        let tl::enums::InputPeer::Channel(channel) = input_peer else {
            return Err(TelegramError::InvalidPeer(
                "set_chat_slow_mode requires a supergroup".to_string(),
            ));
        };
        connected
            .client
            .invoke(&tl::functions::channels::ToggleSlowMode {
                channel: tl::enums::InputChannel::InputChannel(tl::types::InputChannel {
                    channel_id: channel.channel_id,
                    access_hash: channel.access_hash,
                }),
                seconds,
            })
            .await
            .map_err(send_invocation_error)?;
        let readback_path = connected.path.clone();
        drop(connected);
        let settings = self
            .get_channel_settings(Some(readback_path), peer_ref.as_str())
            .await
            .map_err(|error| TelegramError::DeliveryUncertain(error.to_string()))?;
        if settings.slow_mode_seconds == seconds {
            Ok(settings)
        } else {
            Err(TelegramError::DeliveryUncertain(
                "Telegram did not return the requested slow-mode interval".to_string(),
            ))
        }
    }

    pub async fn toggle_chat_signatures(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        signatures_enabled: bool,
        profiles_enabled: bool,
    ) -> Result<ChannelSettingsSummary, TelegramError> {
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
        let tl::enums::InputPeer::Channel(channel) = input_peer else {
            return Err(TelegramError::InvalidPeer(
                "toggle_chat_signatures requires a broadcast channel".to_string(),
            ));
        };
        connected
            .client
            .invoke(&tl::functions::channels::ToggleSignatures {
                signatures_enabled,
                profiles_enabled,
                channel: tl::enums::InputChannel::InputChannel(tl::types::InputChannel {
                    channel_id: channel.channel_id,
                    access_hash: channel.access_hash,
                }),
            })
            .await
            .map_err(send_invocation_error)?;
        let readback_path = connected.path.clone();
        drop(connected);
        let settings = self
            .get_channel_settings(Some(readback_path), peer_ref.as_str())
            .await
            .map_err(|error| TelegramError::DeliveryUncertain(error.to_string()))?;
        if settings.signatures_enabled == signatures_enabled
            && settings.signature_profiles_enabled == profiles_enabled
        {
            Ok(settings)
        } else {
            Err(TelegramError::DeliveryUncertain(
                "Telegram did not return the requested signature settings".to_string(),
            ))
        }
    }

    pub async fn toggle_chat_pre_history_hidden(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        enabled: bool,
    ) -> Result<ChannelSettingsSummary, TelegramError> {
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
        let tl::enums::InputPeer::Channel(channel) = input_peer else {
            return Err(TelegramError::InvalidPeer(
                "toggle_chat_pre_history_hidden requires a supergroup".to_string(),
            ));
        };
        connected
            .client
            .invoke(&tl::functions::channels::TogglePreHistoryHidden {
                channel: tl::enums::InputChannel::InputChannel(tl::types::InputChannel {
                    channel_id: channel.channel_id,
                    access_hash: channel.access_hash,
                }),
                enabled,
            })
            .await
            .map_err(send_invocation_error)?;
        let readback_path = connected.path.clone();
        drop(connected);
        let settings = self
            .get_channel_settings(Some(readback_path), peer_ref.as_str())
            .await
            .map_err(|error| TelegramError::DeliveryUncertain(error.to_string()))?;
        if settings.pre_history_hidden == enabled {
            Ok(settings)
        } else {
            Err(TelegramError::DeliveryUncertain(
                "Telegram did not return the requested pre-history setting".to_string(),
            ))
        }
    }

    pub async fn toggle_chat_join_request(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        enabled: bool,
    ) -> Result<ChannelSettingsSummary, TelegramError> {
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
        let tl::enums::InputPeer::Channel(channel) = input_peer else {
            return Err(TelegramError::InvalidPeer(
                "toggle_chat_join_request requires a channel or supergroup".to_string(),
            ));
        };
        connected
            .client
            .invoke(&tl::functions::channels::ToggleJoinRequest {
                apply_to_invites: false,
                channel: tl::enums::InputChannel::InputChannel(tl::types::InputChannel {
                    channel_id: channel.channel_id,
                    access_hash: channel.access_hash,
                }),
                enabled,
                guard_bot: None,
            })
            .await
            .map_err(send_invocation_error)?;
        let readback_path = connected.path.clone();
        drop(connected);
        let settings = self
            .get_channel_settings(Some(readback_path), peer_ref.as_str())
            .await
            .map_err(|error| TelegramError::DeliveryUncertain(error.to_string()))?;
        if settings.join_request_required == enabled {
            Ok(settings)
        } else {
            Err(TelegramError::DeliveryUncertain(
                "Telegram did not return the requested join-request setting".to_string(),
            ))
        }
    }

    pub async fn set_chat_default_permissions(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        target: &ChatBannedRightsSummary,
    ) -> Result<ChatModerationSettingsSummary, TelegramError> {
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
        if matches!(input_peer, tl::enums::InputPeer::User(_) | tl::enums::InputPeer::PeerSelf) {
            return Err(TelegramError::InvalidPeer(
                "default permissions require a group, supergroup, or channel".to_string(),
            ));
        }
        connected
            .client
            .invoke(&tl::functions::messages::EditChatDefaultBannedRights {
                peer: input_peer,
                banned_rights: chat_banned_rights_to_tl(target),
            })
            .await
            .map_err(send_invocation_error)?;
        let readback_path = connected.path.clone();
        drop(connected);
        let settings = self
            .get_chat_moderation_settings(Some(readback_path), peer_ref.as_str())
            .await
            .map_err(|error| TelegramError::DeliveryUncertain(error.to_string()))?;
        if settings.default_banned_rights.as_ref() == Some(target) {
            Ok(settings)
        } else {
            Err(TelegramError::DeliveryUncertain(
                "Telegram did not return the requested default chat permissions".to_string(),
            ))
        }
    }

    pub async fn set_chat_available_reactions(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        target: &ChatReactionsPolicy,
    ) -> Result<ChatModerationSettingsSummary, TelegramError> {
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
        if matches!(input_peer, tl::enums::InputPeer::User(_) | tl::enums::InputPeer::PeerSelf) {
            return Err(TelegramError::InvalidPeer(
                "available reactions require a group, supergroup, or channel".to_string(),
            ));
        }
        connected
            .client
            .invoke(&tl::functions::messages::SetChatAvailableReactions {
                peer: input_peer,
                available_reactions: chat_reactions_policy_to_tl(target)?,
                reactions_limit: None,
                paid_enabled: None,
            })
            .await
            .map_err(send_invocation_error)?;
        let readback_path = connected.path.clone();
        drop(connected);
        let settings = self
            .get_chat_moderation_settings(Some(readback_path), peer_ref.as_str())
            .await
            .map_err(|error| TelegramError::DeliveryUncertain(error.to_string()))?;
        if settings.available_reactions.as_ref() == Some(target) {
            Ok(settings)
        } else {
            Err(TelegramError::DeliveryUncertain(
                "Telegram did not return the requested chat reaction policy".to_string(),
            ))
        }
    }

    pub async fn toggle_chat_anti_spam(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        enabled: bool,
    ) -> Result<ChatModerationSettingsSummary, TelegramError> {
        let before = self.get_chat_moderation_settings(session_path.clone(), chat_id).await?;
        if before.anti_spam_enabled.is_none() {
            return Err(TelegramError::InvalidPeer(
                "native anti-spam requires a supergroup, not a basic group or broadcast channel"
                    .to_string(),
            ));
        }
        let canonical_chat_id = before.details.chat.chat_id.clone();
        let connected = self.connected(session_path).await?;
        let peer = connected
            .client
            .resolve(canonical_chat_id.as_str())
            .await
            .map_err(|error| TelegramError::Telegram(error.to_string()))?;
        let input_peer = connected
            .client
            .resolve_to_input_peer(&peer)
            .await
            .map_err(|error| TelegramError::Telegram(error.to_string()))?;
        let tl::enums::InputPeer::Channel(channel) = input_peer else {
            return Err(TelegramError::InvalidPeer(
                "native anti-spam requires a supergroup".to_string(),
            ));
        };
        connected
            .client
            .invoke(&tl::functions::channels::ToggleAntiSpam {
                channel: tl::enums::InputChannel::InputChannel(tl::types::InputChannel {
                    channel_id: channel.channel_id,
                    access_hash: channel.access_hash,
                }),
                enabled,
            })
            .await
            .map_err(send_invocation_error)?;
        let readback_path = connected.path.clone();
        drop(connected);
        let settings = self
            .get_chat_moderation_settings(Some(readback_path), canonical_chat_id.as_str())
            .await
            .map_err(|error| TelegramError::DeliveryUncertain(error.to_string()))?;
        if settings.anti_spam_enabled == Some(enabled) {
            Ok(settings)
        } else {
            Err(TelegramError::DeliveryUncertain(
                "Telegram did not return the requested native anti-spam state".to_string(),
            ))
        }
    }

    pub async fn toggle_chat_forum_mode(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        enabled: bool,
        tabs_enabled: bool,
    ) -> Result<ChatModerationSettingsSummary, TelegramError> {
        let before = self.get_chat_moderation_settings(session_path.clone(), chat_id).await?;
        if before.forum_enabled.is_none() {
            return Err(TelegramError::InvalidPeer(
                "forum mode requires a supergroup, not a basic group or broadcast channel"
                    .to_string(),
            ));
        }
        let target_tabs = enabled && tabs_enabled;
        let canonical_chat_id = before.details.chat.chat_id.clone();
        let connected = self.connected(session_path).await?;
        let peer = connected
            .client
            .resolve(canonical_chat_id.as_str())
            .await
            .map_err(|error| TelegramError::Telegram(error.to_string()))?;
        let input_peer = connected
            .client
            .resolve_to_input_peer(&peer)
            .await
            .map_err(|error| TelegramError::Telegram(error.to_string()))?;
        let tl::enums::InputPeer::Channel(channel) = input_peer else {
            return Err(TelegramError::InvalidPeer("forum mode requires a supergroup".to_string()));
        };
        connected
            .client
            .invoke(&tl::functions::channels::ToggleForum {
                channel: tl::enums::InputChannel::InputChannel(tl::types::InputChannel {
                    channel_id: channel.channel_id,
                    access_hash: channel.access_hash,
                }),
                enabled,
                tabs: target_tabs,
            })
            .await
            .map_err(send_invocation_error)?;
        let readback_path = connected.path.clone();
        drop(connected);
        let settings = self
            .get_chat_moderation_settings(Some(readback_path), canonical_chat_id.as_str())
            .await
            .map_err(|error| TelegramError::DeliveryUncertain(error.to_string()))?;
        if settings.forum_enabled == Some(enabled)
            && settings.forum_tabs_enabled == Some(target_tabs)
        {
            Ok(settings)
        } else {
            Err(TelegramError::DeliveryUncertain(
                "Telegram did not return the requested forum-mode state".to_string(),
            ))
        }
    }

    pub async fn list_stories(
        &self,
        session_path: Option<PathBuf>,
        next: bool,
        hidden: bool,
        state: Option<&str>,
    ) -> Result<StoryListPage, TelegramError> {
        let connected = self.connected(session_path).await?;
        let response = connected
            .client
            .invoke(&tl::functions::stories::GetAllStories {
                next,
                hidden,
                state: state.map(str::to_string),
            })
            .await
            .map_err(send_invocation_error)?;
        match response {
            tl::enums::stories::AllStories::NotModified(response) => {
                let (active_until, cooldown_until) = stealth_mode_dates(&response.stealth_mode);
                Ok(StoryListPage {
                    state: Some(response.state),
                    has_more: false,
                    count: 0,
                    peers: Vec::new(),
                    stealth_mode_active_until_unix: active_until,
                    stealth_mode_cooldown_until_unix: cooldown_until,
                })
            },
            tl::enums::stories::AllStories::AllStories(response) => {
                let (active_until, cooldown_until) = stealth_mode_dates(&response.stealth_mode);
                let peers = response
                    .peer_stories
                    .into_iter()
                    .map(|peer| match peer {
                        tl::enums::PeerStories::PeerStories(peer) => story_peer_summary(peer),
                    })
                    .collect();
                Ok(StoryListPage {
                    state: Some(response.state),
                    has_more: response.has_more,
                    count: response.count,
                    peers,
                    stealth_mode_active_until_unix: active_until,
                    stealth_mode_cooldown_until_unix: cooldown_until,
                })
            },
        }
    }

    pub async fn get_peer_stories(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
    ) -> Result<StoryPeerPage, TelegramError> {
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
        let tl::enums::stories::PeerStories::PeerStories(response) = connected
            .client
            .invoke(&tl::functions::stories::GetPeerStories { peer: input_peer })
            .await
            .map_err(send_invocation_error)?;
        let peer = match response.stories {
            tl::enums::PeerStories::PeerStories(peer) => story_peer_summary(peer),
        };
        Ok(StoryPeerPage { count: peer.stories.len(), peer })
    }

    pub async fn get_stories_by_ids(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        story_ids: &[i32],
    ) -> Result<StoryCollectionPage, TelegramError> {
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
        let response = connected
            .client
            .invoke(&tl::functions::stories::GetStoriesById {
                peer: input_peer,
                id: story_ids.to_vec(),
            })
            .await
            .map_err(send_invocation_error)?;
        Ok(story_collection_page(peer_ref, response, None))
    }

    pub async fn get_stories_archive(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        offset_id: i32,
        limit: usize,
    ) -> Result<StoryCollectionPage, TelegramError> {
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
        let response = connected
            .client
            .invoke(&tl::functions::stories::GetStoriesArchive {
                peer: input_peer,
                offset_id,
                limit: i32::try_from(limit).map_err(|_| {
                    TelegramError::InvalidPeer("get_stories_archive.limit is too large".into())
                })?,
            })
            .await
            .map_err(send_invocation_error)?;
        Ok(story_collection_page(peer_ref, response, Some(limit)))
    }

    pub async fn export_story_link(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        story_id: i32,
    ) -> Result<StoryLinkSummary, TelegramError> {
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
        let tl::enums::ExportedStoryLink::ExportedStoryLink(response) = connected
            .client
            .invoke(&tl::functions::stories::ExportStoryLink { peer: input_peer, id: story_id })
            .await
            .map_err(send_invocation_error)?;
        Ok(StoryLinkSummary { peer_ref, story_id, link: response.link })
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn get_story_viewers(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        story_id: i32,
        offset: &str,
        limit: usize,
        just_contacts: bool,
        reactions_first: bool,
        forwards_first: bool,
        query: Option<&str>,
    ) -> Result<StoryViewersPage, TelegramError> {
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
        let response = connected
            .client
            .invoke(&tl::functions::stories::GetStoryViewsList {
                just_contacts,
                reactions_first,
                forwards_first,
                peer: input_peer,
                q: query.map(str::to_string),
                id: story_id,
                offset: offset.to_string(),
                limit: i32::try_from(limit).map_err(|_| {
                    TelegramError::InvalidPeer("get_story_viewers.limit is too large".into())
                })?,
            })
            .await
            .map_err(send_invocation_error)?;
        let tl::enums::stories::StoryViewsList::StoryViewsList(response) = response;
        let viewers = response
            .views
            .into_iter()
            .map(|view| story_viewer_summary(view, &response.users))
            .collect::<Vec<_>>();
        Ok(StoryViewersPage {
            story_id,
            count: response.count,
            views_count: response.views_count,
            forwards_count: response.forwards_count,
            reactions_count: response.reactions_count,
            viewers,
            next_offset: response.next_offset,
        })
    }

    pub async fn view_story(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        story_ids: &[i32],
    ) -> Result<(), TelegramError> {
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
        let viewed_ids = connected
            .client
            .invoke(&tl::functions::stories::IncrementStoryViews {
                peer: input_peer,
                id: story_ids.to_vec(),
            })
            .await
            .map_err(send_invocation_error)?;
        if !viewed_ids {
            return Err(TelegramError::SendRejected(
                "Telegram did not accept the story view acknowledgement".into(),
            ));
        }
        Ok(())
    }

    pub async fn read_stories(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        max_id: i32,
    ) -> Result<Vec<i32>, TelegramError> {
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
        connected
            .client
            .invoke(&tl::functions::stories::ReadStories { peer: input_peer, max_id })
            .await
            .map_err(send_invocation_error)
    }

    pub async fn set_story_reaction(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        story_id: i32,
        reaction: &ReactionSpec,
    ) -> Result<(), TelegramError> {
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
        let (reaction, add_to_recent) = match reaction {
            ReactionSpec::Remove => (tl::enums::Reaction::Empty, false),
            ReactionSpec::Emoji { emoji, add_to_recent, .. } => (
                tl::enums::Reaction::Emoji(tl::types::ReactionEmoji { emoticon: emoji.clone() }),
                *add_to_recent,
            ),
            ReactionSpec::CustomEmoji { document_id, add_to_recent, .. } => (
                tl::enums::Reaction::CustomEmoji(tl::types::ReactionCustomEmoji {
                    document_id: *document_id,
                }),
                *add_to_recent,
            ),
        };
        connected
            .client
            .invoke(&tl::functions::stories::SendReaction {
                add_to_recent,
                peer: input_peer,
                story_id,
                reaction,
            })
            .await
            .map_err(send_invocation_error)?;
        Ok(())
    }

    pub async fn set_stories_pinned(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        story_ids: &[i32],
        pinned: bool,
    ) -> Result<Vec<i32>, TelegramError> {
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
        connected
            .client
            .invoke(&tl::functions::stories::TogglePinned {
                peer: input_peer,
                id: story_ids.to_vec(),
                pinned,
            })
            .await
            .map_err(send_invocation_error)
    }

    pub async fn toggle_stories_pinned_to_top(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        story_ids: &[i32],
    ) -> Result<bool, TelegramError> {
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
        connected
            .client
            .invoke(&tl::functions::stories::TogglePinnedToTop {
                peer: input_peer,
                id: story_ids.to_vec(),
            })
            .await
            .map_err(send_invocation_error)
    }

    pub async fn activate_story_stealth_mode(
        &self,
        session_path: Option<PathBuf>,
        past: bool,
        future: bool,
    ) -> Result<(), TelegramError> {
        let connected = self.connected(session_path).await?;
        connected
            .client
            .invoke(&tl::functions::stories::ActivateStealthMode { past, future })
            .await
            .map_err(send_invocation_error)?;
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn send_story_with_random_id(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        file_path: &Path,
        caption: &str,
        privacy: &StoryPrivacySpec,
        pinned: bool,
        noforwards: bool,
        period_seconds: Option<i32>,
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
        let uploaded = connected
            .client
            .upload_file(file_path)
            .await
            .map_err(|error| TelegramError::Telegram(error.to_string()))?;
        let mime = uploaded.mime_type();
        if (!mime.starts_with("image/") && !mime.starts_with("video/")) || mime == "image/gif" {
            return Err(TelegramError::SendRejected(format!(
                "story media must be a supported image or video; detected {mime}"
            )));
        }
        let privacy_rules = story_privacy_to_tl(&connected.client, privacy).await?;
        let response = connected
            .client
            .invoke(&tl::functions::stories::SendStory {
                pinned,
                noforwards,
                fwd_modified: false,
                peer: input_peer,
                media: uploaded.as_auto_media(),
                media_areas: None,
                caption: (!caption.is_empty()).then(|| caption.to_string()),
                entities: None,
                privacy_rules,
                random_id,
                period: period_seconds,
                fwd_from_id: None,
                fwd_from_story: None,
                albums: None,
                music: None,
            })
            .await
            .map_err(send_invocation_error)?;
        sent_story_id(&response, random_id, &peer).ok_or_else(|| {
            TelegramError::DeliveryUncertain(
                "Telegram accepted story send request but returned no story ID".to_string(),
            )
        })
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn edit_story(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        story_id: i32,
        file_path: Option<&Path>,
        caption: Option<&str>,
        privacy: Option<&StoryPrivacySpec>,
    ) -> Result<(), TelegramError> {
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
        let media = if let Some(file_path) = file_path {
            let uploaded = connected
                .client
                .upload_file(file_path)
                .await
                .map_err(|error| TelegramError::Telegram(error.to_string()))?;
            let mime = uploaded.mime_type();
            if (!mime.starts_with("image/") && !mime.starts_with("video/")) || mime == "image/gif" {
                return Err(TelegramError::SendRejected(format!(
                    "story media must be a supported image or video; detected {mime}"
                )));
            }
            Some(uploaded.as_auto_media())
        } else {
            None
        };
        let privacy_rules = if let Some(privacy) = privacy {
            Some(story_privacy_to_tl(&connected.client, privacy).await?)
        } else {
            None
        };
        connected
            .client
            .invoke(&tl::functions::stories::EditStory {
                peer: input_peer,
                id: story_id,
                media,
                media_areas: None,
                caption: caption.map(str::to_string),
                entities: caption.map(|_| Vec::new()),
                privacy_rules,
                music: None,
            })
            .await
            .map_err(send_invocation_error)?;
        Ok(())
    }

    pub async fn report_stories(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        story_ids: &[i32],
        option_hex: &str,
        message: &str,
    ) -> Result<StoryReportResult, TelegramError> {
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
        let response = connected
            .client
            .invoke(&tl::functions::stories::Report {
                peer: input_peer,
                id: story_ids.to_vec(),
                option: decode_hex(option_hex)?,
                message: message.to_string(),
            })
            .await
            .map_err(send_invocation_error)?;
        Ok(match response {
            tl::enums::ReportResult::ChooseOption(result) => StoryReportResult::ChooseOption {
                title: result.title,
                options: result
                    .options
                    .into_iter()
                    .map(|option| {
                        let tl::enums::MessageReportOption::MessageReportOption(option) = option;
                        StoryReportOption {
                            text: option.text,
                            option_hex: hex_encode(&option.option),
                        }
                    })
                    .collect(),
            },
            tl::enums::ReportResult::AddComment(result) => StoryReportResult::AddComment {
                optional: result.optional,
                option_hex: hex_encode(&result.option),
            },
            tl::enums::ReportResult::Reported => StoryReportResult::Reported,
        })
    }

    pub async fn delete_stories(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        story_ids: &[i32],
    ) -> Result<Vec<i32>, TelegramError> {
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
        connected
            .client
            .invoke(&tl::functions::stories::DeleteStories {
                peer: input_peer,
                id: story_ids.to_vec(),
            })
            .await
            .map_err(send_invocation_error)
    }

    pub async fn get_story(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        story_id: i32,
    ) -> Result<Option<StorySummary>, TelegramError> {
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
        let tl::enums::stories::Stories::Stories(response) = connected
            .client
            .invoke(&tl::functions::stories::GetStoriesById {
                peer: input_peer,
                id: vec![story_id],
            })
            .await
            .map_err(send_invocation_error)?;
        Ok(response.stories.into_iter().map(story_summary).find(|story| story.story_id == story_id))
    }

    pub async fn get_chat_administrators(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
    ) -> Result<Vec<ParticipantSummary>, TelegramError> {
        let connected = self.connected(session_path).await?;
        let peer = Self::resolve_peer_ref(&connected.client, chat_id).await?;
        let participants = connected
            .client
            .get_chat_administrators(peer.as_str())
            .await
            .map_err(|error| TelegramError::Telegram(error.to_string()))?;
        Ok(participants
            .into_iter()
            .filter_map(|participant| {
                let user = contact_summary(tl::enums::User::User(participant.user))?;
                let role = match participant.status {
                    ferogram::participants::ParticipantStatus::Creator => "creator",
                    ferogram::participants::ParticipantStatus::Admin => "admin",
                    ferogram::participants::ParticipantStatus::Member
                    | ferogram::participants::ParticipantStatus::Restricted
                    | ferogram::participants::ParticipantStatus::Left
                    | ferogram::participants::ParticipantStatus::Banned => return None,
                };
                Some(ParticipantSummary { user, role: role.to_string() })
            })
            .collect())
    }

    pub async fn get_online_count(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
    ) -> Result<i32, TelegramError> {
        let connected = self.connected(session_path).await?;
        let peer = Self::resolve_peer_ref(&connected.client, chat_id).await?;
        connected
            .client
            .get_online_count(peer.as_str())
            .await
            .map_err(|error| TelegramError::Telegram(error.to_string()))
    }

    pub async fn get_blocked_users(
        &self,
        session_path: Option<PathBuf>,
        offset: usize,
        limit: usize,
    ) -> Result<Vec<ContactSummary>, TelegramError> {
        let connected = self.connected(session_path).await?;
        let telegram_offset = i32::try_from(offset).map_err(|_| {
            TelegramError::InvalidPeer("get_blocked_users.offset is too large".to_string())
        })?;
        let telegram_limit = i32::try_from(limit).map_err(|_| {
            TelegramError::InvalidPeer("get_blocked_users.limit is too large".to_string())
        })?;
        let peers = connected
            .client
            .get_blocked_users(telegram_offset, telegram_limit)
            .await
            .map_err(|error| TelegramError::Telegram(error.to_string()))?;
        let user_ids = peers
            .into_iter()
            .filter_map(|peer| match peer {
                tl::enums::Peer::User(user) => Some(user.user_id),
                tl::enums::Peer::Chat(_) | tl::enums::Peer::Channel(_) => None,
            })
            .collect::<Vec<_>>();
        let users = connected
            .client
            .get_users_by_id(&user_ids)
            .await
            .map_err(|error| TelegramError::Telegram(error.to_string()))?;
        Ok(users.into_iter().flatten().map(|user| contact_summary_from_user(&user)).collect())
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn list_forum_topics(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        query: Option<&str>,
        limit: usize,
        offset_date: i32,
        offset_id: i32,
        offset_topic: i32,
    ) -> Result<Vec<ForumTopicSummary>, TelegramError> {
        let connected = self.connected(session_path).await?;
        let peer = Self::resolve_peer_ref(&connected.client, chat_id).await?;
        let telegram_limit = i32::try_from(limit).map_err(|_| {
            TelegramError::InvalidPeer("list_forum_topics.limit is too large".to_string())
        })?;
        let topics = connected
            .client
            .get_forum_topics(
                peer.as_str(),
                query.map(str::to_string),
                telegram_limit,
                offset_date,
                offset_id,
                offset_topic,
            )
            .await
            .map_err(|error| TelegramError::Telegram(error.to_string()))?;
        Ok(topics.into_iter().map(forum_topic_summary).collect())
    }

    pub async fn get_forum_topic(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        topic_id: i32,
    ) -> Result<Option<ForumTopicSummary>, TelegramError> {
        let connected = self.connected(session_path).await?;
        let peer = Self::resolve_peer_ref(&connected.client, chat_id).await?;
        let topics = connected
            .client
            .get_forum_topics_by_id(peer.as_str(), vec![topic_id])
            .await
            .map_err(|error| TelegramError::Telegram(error.to_string()))?;
        Ok(topics.into_iter().map(forum_topic_summary).find(|topic| topic.topic_id == topic_id))
    }

    pub async fn create_forum_topic_with_random_id(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        title: &str,
        icon_color: Option<i32>,
        icon_emoji_id: Option<i64>,
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
        let updates = connected
            .client
            .invoke(&tl::functions::messages::CreateForumTopic {
                title_missing: false,
                peer: input_peer,
                title: title.to_string(),
                icon_color,
                icon_emoji_id,
                random_id,
                send_as: None,
            })
            .await
            .map_err(send_invocation_error)?;
        sent_message_id(&updates, random_id).ok_or_else(|| {
            TelegramError::DeliveryUncertain(
                "Telegram accepted forum-topic creation but returned no exact topic ID".to_string(),
            )
        })
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn edit_forum_topic(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        topic_id: i32,
        title: Option<&str>,
        icon_emoji_id: Option<i64>,
        closed: Option<bool>,
        hidden: Option<bool>,
    ) -> Result<(), TelegramError> {
        let connected = self.connected(session_path).await?;
        let peer = Self::resolve_peer_ref(&connected.client, chat_id).await?;
        connected
            .client
            .edit_forum_topic(
                peer.as_str(),
                topic_id,
                title.map(str::to_string),
                icon_emoji_id,
                closed,
                hidden,
            )
            .await
            .map_err(send_invocation_error)
    }

    pub async fn set_forum_topic_pinned(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        topic_id: i32,
        pinned: bool,
    ) -> Result<(), TelegramError> {
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
        connected
            .client
            .invoke(&tl::functions::messages::UpdatePinnedForumTopic {
                peer: input_peer,
                topic_id,
                pinned,
            })
            .await
            .map_err(send_invocation_error)?;
        Ok(())
    }

    pub async fn delete_forum_topic(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        topic_id: i32,
    ) -> Result<(), TelegramError> {
        let connected = self.connected(session_path).await?;
        let peer = Self::resolve_peer_ref(&connected.client, chat_id).await?;
        connected
            .client
            .delete_forum_topic_history(peer.as_str(), topic_id)
            .await
            .map_err(send_invocation_error)
    }

    pub async fn get_privacy_rules(
        &self,
        session_path: Option<PathBuf>,
        key: PrivacyKey,
    ) -> Result<Vec<PrivacyRuleSummary>, TelegramError> {
        let connected = self.connected(session_path).await?;
        let rules = connected
            .client
            .get_privacy(privacy_key_to_tl(key))
            .await
            .map_err(|error| TelegramError::Telegram(error.to_string()))?;
        Ok(rules.into_iter().map(privacy_rule_summary).collect())
    }

    pub async fn set_privacy_rules(
        &self,
        session_path: Option<PathBuf>,
        key: PrivacyKey,
        rules: &[PrivacyRuleSummary],
    ) -> Result<Vec<PrivacyRuleSummary>, TelegramError> {
        let connected = self.connected(session_path).await?;
        let mut input_rules = Vec::with_capacity(rules.len());
        for rule in rules {
            input_rules.push(privacy_rule_to_tl(&connected.client, rule).await?);
        }
        let rules = connected
            .client
            .set_privacy(privacy_key_to_tl(key), input_rules)
            .await
            .map_err(send_invocation_error)?;
        Ok(rules.into_iter().map(privacy_rule_summary).collect())
    }

    pub async fn get_notification_settings(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
    ) -> Result<NotificationSettingsSummary, TelegramError> {
        let connected = self.connected(session_path).await?;
        let peer = Self::resolve_peer_ref(&connected.client, chat_id).await?;
        let settings = connected
            .client
            .get_notify_settings(peer.as_str())
            .await
            .map_err(|error| TelegramError::Telegram(error.to_string()))?;
        Ok(notification_settings_summary(settings))
    }

    pub async fn set_notification_settings(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        patch: &NotificationSettingsPatch,
    ) -> Result<(), TelegramError> {
        let connected = self.connected(session_path).await?;
        let peer = Self::resolve_peer_ref(&connected.client, chat_id).await?;
        let settings = tl::enums::InputPeerNotifySettings::InputPeerNotifySettings(
            tl::types::InputPeerNotifySettings {
                show_previews: patch.show_previews,
                silent: patch.silent,
                mute_until: patch.mute_until_unix,
                sound: patch.sound.as_ref().map(notification_sound_to_tl).transpose()?,
                stories_muted: patch.stories_muted,
                stories_hide_sender: patch.stories_hide_sender,
                stories_sound: patch
                    .stories_sound
                    .as_ref()
                    .map(notification_sound_to_tl)
                    .transpose()?,
            },
        );
        connected
            .client
            .update_notify_settings(peer.as_str(), settings)
            .await
            .map_err(send_invocation_error)
    }

    pub async fn list_sticker_sets(
        &self,
        session_path: Option<PathBuf>,
        hash: i64,
    ) -> Result<Option<Vec<StickerSetSummary>>, TelegramError> {
        let connected = self.connected(session_path).await?;
        let sets = connected
            .client
            .get_all_stickers(hash)
            .await
            .map_err(|error| TelegramError::Telegram(error.to_string()))?;
        Ok(sets.map(|sets| sets.into_iter().map(sticker_set_summary_from_tl).collect()))
    }

    pub async fn get_recent_stickers(
        &self,
        session_path: Option<PathBuf>,
        attached: bool,
        limit: usize,
        hash: i64,
    ) -> Result<StickerCatalogPage, TelegramError> {
        let connected = self.connected(session_path).await?;
        let page = connected
            .client
            .invoke(&tl::functions::messages::GetRecentStickers { attached, hash })
            .await
            .map_err(|error| TelegramError::Telegram(error.to_string()))?;
        let tl::enums::messages::RecentStickers::RecentStickers(page) = page else {
            return Ok(StickerCatalogPage {
                not_modified: true,
                hash: Some(hash),
                attached,
                total_count: 0,
                returned_count: 0,
                has_more: false,
                stickers: Vec::new(),
            });
        };
        let set_names = installed_sticker_set_names(&connected.client).await?;
        let total_count = page.stickers.len();
        let mut stickers = page
            .stickers
            .iter()
            .enumerate()
            .filter_map(|(index, document)| {
                sticker_document_summary(document, None, &set_names, page.dates.get(index).copied())
            })
            .take(limit)
            .collect::<Vec<_>>();
        let returned_count = stickers.len();
        let has_more = returned_count < total_count;
        stickers.shrink_to_fit();
        Ok(StickerCatalogPage {
            not_modified: false,
            hash: Some(page.hash),
            attached,
            total_count,
            returned_count,
            has_more,
            stickers,
        })
    }

    pub async fn get_sticker_set(
        &self,
        session_path: Option<PathBuf>,
        short_name: &str,
        hash: i32,
    ) -> Result<StickerSetDetailsPage, TelegramError> {
        let connected = self.connected(session_path).await?;
        let page = connected
            .client
            .invoke(&tl::functions::messages::GetStickerSet {
                stickerset: tl::enums::InputStickerSet::ShortName(
                    tl::types::InputStickerSetShortName { short_name: short_name.to_string() },
                ),
                hash,
            })
            .await
            .map_err(|error| TelegramError::Telegram(error.to_string()))?;
        let tl::enums::messages::StickerSet::StickerSet(page) = page else {
            return Ok(StickerSetDetailsPage {
                not_modified: true,
                sticker_set: None,
                packs: Vec::new(),
                keywords: Vec::new(),
                stickers: Vec::new(),
            });
        };
        Ok(sticker_set_details_page(page))
    }

    pub async fn search_sticker_sets(
        &self,
        session_path: Option<PathBuf>,
        query: &str,
        exclude_featured: bool,
        limit: usize,
        hash: i64,
    ) -> Result<StickerSetSearchPage, TelegramError> {
        let connected = self.connected(session_path).await?;
        let page = connected
            .client
            .invoke(&tl::functions::messages::SearchStickerSets {
                exclude_featured,
                q: query.to_string(),
                hash,
            })
            .await
            .map_err(|error| TelegramError::Telegram(error.to_string()))?;
        let tl::enums::messages::FoundStickerSets::FoundStickerSets(page) = page else {
            return Ok(StickerSetSearchPage {
                not_modified: true,
                hash: Some(hash),
                total_count: 0,
                returned_count: 0,
                has_more: false,
                sticker_sets: Vec::new(),
            });
        };
        let total_count = page.sets.len();
        let sticker_sets = page
            .sets
            .iter()
            .take(limit)
            .map(|covered| sticker_set_summary_from_tl(covered_sticker_set(covered)))
            .collect::<Vec<_>>();
        Ok(StickerSetSearchPage {
            not_modified: false,
            hash: Some(page.hash),
            total_count,
            returned_count: sticker_sets.len(),
            has_more: sticker_sets.len() < total_count,
            sticker_sets,
        })
    }

    pub async fn get_favorite_stickers(
        &self,
        session_path: Option<PathBuf>,
        limit: usize,
        hash: i64,
    ) -> Result<StickerCatalogPage, TelegramError> {
        let connected = self.connected(session_path).await?;
        let page = connected
            .client
            .invoke(&tl::functions::messages::GetFavedStickers { hash })
            .await
            .map_err(|error| TelegramError::Telegram(error.to_string()))?;
        let tl::enums::messages::FavedStickers::FavedStickers(page) = page else {
            return Ok(StickerCatalogPage {
                not_modified: true,
                hash: Some(hash),
                attached: false,
                total_count: 0,
                returned_count: 0,
                has_more: false,
                stickers: Vec::new(),
            });
        };
        let set_names = installed_sticker_set_names(&connected.client).await?;
        let total_count = page.stickers.len();
        let stickers = page
            .stickers
            .iter()
            .filter_map(|document| sticker_document_summary(document, None, &set_names, None))
            .take(limit)
            .collect::<Vec<_>>();
        Ok(StickerCatalogPage {
            not_modified: false,
            hash: Some(page.hash),
            attached: false,
            total_count,
            returned_count: stickers.len(),
            has_more: stickers.len() < total_count,
            stickers,
        })
    }

    pub async fn set_sticker_favorite(
        &self,
        session_path: Option<PathBuf>,
        set_short_name: &str,
        sticker_handle: &str,
        favorite: bool,
    ) -> Result<(), TelegramError> {
        let connected = self.connected(session_path).await?;
        let document =
            resolve_raw_sticker_document(&connected.client, set_short_name, sticker_handle).await?;
        let accepted = connected
            .client
            .invoke(&tl::functions::messages::FaveSticker {
                id: tl::enums::InputDocument::InputDocument(tl::types::InputDocument {
                    id: document.id,
                    access_hash: document.access_hash,
                    file_reference: document.file_reference,
                }),
                unfave: !favorite,
            })
            .await
            .map_err(send_invocation_error)?;
        if !accepted {
            return Err(TelegramError::Telegram(
                "Telegram rejected the sticker favourite update".to_string(),
            ));
        }
        Ok(())
    }

    pub async fn set_recent_sticker(
        &self,
        session_path: Option<PathBuf>,
        set_short_name: &str,
        sticker_handle: &str,
        attached: bool,
        saved: bool,
    ) -> Result<(), TelegramError> {
        let connected = self.connected(session_path).await?;
        let document =
            resolve_raw_sticker_document(&connected.client, set_short_name, sticker_handle).await?;
        let accepted = connected
            .client
            .invoke(&tl::functions::messages::SaveRecentSticker {
                attached,
                id: tl::enums::InputDocument::InputDocument(tl::types::InputDocument {
                    id: document.id,
                    access_hash: document.access_hash,
                    file_reference: document.file_reference,
                }),
                unsave: !saved,
            })
            .await
            .map_err(send_invocation_error)?;
        if !accepted {
            return Err(TelegramError::Telegram(
                "Telegram rejected the recent-sticker update".to_string(),
            ));
        }
        Ok(())
    }

    pub async fn install_sticker_set(
        &self,
        session_path: Option<PathBuf>,
        short_name: &str,
        archived: bool,
    ) -> Result<(), TelegramError> {
        let connected = self.connected(session_path).await?;
        connected
            .client
            .invoke(&tl::functions::messages::InstallStickerSet {
                stickerset: tl::enums::InputStickerSet::ShortName(
                    tl::types::InputStickerSetShortName { short_name: short_name.to_string() },
                ),
                archived,
            })
            .await
            .map_err(send_invocation_error)?;
        Ok(())
    }

    pub async fn uninstall_sticker_set(
        &self,
        session_path: Option<PathBuf>,
        short_name: &str,
    ) -> Result<(), TelegramError> {
        let connected = self.connected(session_path).await?;
        let accepted = connected
            .client
            .invoke(&tl::functions::messages::UninstallStickerSet {
                stickerset: tl::enums::InputStickerSet::ShortName(
                    tl::types::InputStickerSetShortName { short_name: short_name.to_string() },
                ),
            })
            .await
            .map_err(send_invocation_error)?;
        if !accepted {
            return Err(TelegramError::Telegram(
                "Telegram rejected the sticker-set uninstall".to_string(),
            ));
        }
        Ok(())
    }

    pub async fn get_custom_emoji_documents(
        &self,
        session_path: Option<PathBuf>,
        document_ids: &[i64],
    ) -> Result<Vec<CustomEmojiDocumentSummary>, TelegramError> {
        let connected = self.connected(session_path).await?;
        let documents = connected
            .client
            .get_custom_emoji_documents(document_ids.to_vec())
            .await
            .map_err(|error| TelegramError::Telegram(error.to_string()))?;
        Ok(documents.iter().filter_map(custom_emoji_document_summary).collect())
    }

    pub async fn download_custom_emoji_to(
        &self,
        session_path: Option<PathBuf>,
        document_id: i64,
        destination: &Path,
        max_bytes: u64,
    ) -> Result<u64, TelegramError> {
        let connected = self.connected(session_path).await?;
        let documents = connected
            .client
            .get_custom_emoji_documents(vec![document_id])
            .await
            .map_err(|error| TelegramError::Telegram(error.to_string()))?;
        let document = documents
            .into_iter()
            .find_map(|document| match document {
                tl::enums::Document::Document(document)
                    if document.id == document_id
                        && custom_emoji_document_summary(&tl::enums::Document::Document(
                            document.clone(),
                        ))
                        .is_some() =>
                {
                    Some(document)
                },
                _ => None,
            })
            .ok_or_else(|| {
                TelegramError::AttachmentNotFound(format!("custom emoji {document_id}"))
            })?;
        let expected_size = u64::try_from(document.size).map_err(|_| {
            TelegramError::Telegram(format!(
                "custom emoji {document_id} reported a negative byte size"
            ))
        })?;
        if expected_size > max_bytes {
            return Err(TelegramError::AttachmentTooLarge { max_bytes });
        }
        let downloadable = ferogram::media::Document::from_raw(document);
        let mut download = connected.client.iter_download(&downloadable).ok_or_else(|| {
            TelegramError::AttachmentNotFound(format!("custom emoji {document_id}"))
        })?;
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

    pub async fn get_dice_emojis(
        &self,
        session_path: Option<PathBuf>,
        hash: i32,
    ) -> Result<Option<DiceEmojiCatalog>, TelegramError> {
        let connected = self.connected(session_path).await?;
        let config = connected
            .client
            .invoke(&tl::functions::help::GetAppConfig { hash })
            .await
            .map_err(|error| TelegramError::Telegram(error.to_string()))?;
        let tl::enums::help::AppConfig::AppConfig(config) = config else {
            return Ok(None);
        };
        let emojis = tl_json_object_field(&config.config, "emojies_send_dice")
            .and_then(tl_json_string_array)
            .ok_or_else(|| {
                TelegramError::Telegram(
                    "Telegram app config omitted a valid emojies_send_dice string array"
                        .to_string(),
                )
            })?;
        if emojis.is_empty() || emojis.iter().any(String::is_empty) {
            return Err(TelegramError::Telegram(
                "Telegram app config returned an empty or invalid dice emoji catalog".to_string(),
            ));
        }
        let mut unique = HashSet::with_capacity(emojis.len());
        if !emojis.iter().all(|emoji| unique.insert(emoji.as_str())) {
            return Err(TelegramError::Telegram(
                "Telegram app config returned duplicate dice emoji entries".to_string(),
            ));
        }
        Ok(Some(DiceEmojiCatalog { hash: config.hash, emojis }))
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn send_rich_media_with_random_id(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        media: &RichMediaSpec,
        caption: &str,
        reply_to_message_id: Option<i32>,
        topic_id: Option<i32>,
        silent: bool,
        noforwards: bool,
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
        let reply_to = input_reply_to(reply_to_message_id, topic_id);
        let updates = connected
            .client
            .invoke(&tl::functions::messages::SendMedia {
                silent,
                background: false,
                clear_draft: false,
                noforwards,
                update_stickersets_order: false,
                invert_media: false,
                allow_paid_floodskip: false,
                peer: input_peer,
                reply_to,
                media: rich_media_to_input(media),
                message: caption.to_string(),
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
            })
            .await
            .map_err(send_invocation_error)?;
        sent_message_id(&updates, random_id).ok_or_else(|| {
            TelegramError::DeliveryUncertain(
                "Telegram accepted rich-media send request but response contained no exact message ID"
                    .to_string(),
            )
        })
    }

    pub async fn get_message_rich_media(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        message_id: i32,
    ) -> Result<Option<SentRichMediaSummary>, TelegramError> {
        let connected = self.connected(session_path).await?;
        let peer = Self::resolve_peer_ref(&connected.client, chat_id).await?;
        let messages = connected
            .client
            .get_messages(peer.as_str(), &[message_id])
            .await
            .map_err(|error| TelegramError::Telegram(error.to_string()))?;
        let Some(message) = messages.iter().find(|message| is_concrete_message(message)) else {
            return Ok(None);
        };
        message.media().map(rich_media_summary_from_tl).transpose().map(Option::flatten)
    }

    /// List who reacted to one message.
    ///
    /// Broadcast channels answer `403 BROADCAST_FORBIDDEN` to Telegram's
    /// per-viewer reaction list, so this returns the message's own summary
    /// there instead of failing: per-reaction totals in `counts` (with
    /// `chosen` marking this account's own reaction), the recent reactors the
    /// summary exposes in `reactions`, and the refusal in
    /// `viewer_list_forbidden`. The summary is not paginated, so the fallback
    /// ignores `limit`/`offset` and leaves `next_offset` empty. Any other
    /// failure stays an error.
    pub async fn list_message_reactions(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        message_id: i32,
        limit: usize,
        offset: Option<&str>,
    ) -> Result<MessageReactionPage, TelegramError> {
        let connected = self.connected(session_path).await?;
        let peer = Self::resolve_peer_ref(&connected.client, chat_id).await?;
        let telegram_limit = i32::try_from(limit).map_err(|_| {
            TelegramError::InvalidPeer("list_message_reactions.limit is too large".to_string())
        })?;
        let listed = connected
            .client
            .iter_reaction_users(
                peer.as_str(),
                message_id,
                None,
                telegram_limit,
                offset.map(str::to_string),
            )
            .await;
        reaction_read_outcome(listed, async || {
            connected.client.get_messages(peer.as_str(), &[message_id]).await
        })
        .await
    }

    pub async fn get_default_reaction(
        &self,
        session_path: Option<PathBuf>,
    ) -> Result<DefaultReactionSummary, TelegramError> {
        let connected = self.connected(session_path).await?;
        let config = connected
            .client
            .invoke(&tl::functions::help::GetConfig {})
            .await
            .map_err(|error| TelegramError::Telegram(error.to_string()))?;
        let tl::enums::Config::Config(config) = config;
        Ok(DefaultReactionSummary {
            reaction: config.reactions_default.map(reaction_identity_from_tl),
        })
    }

    pub async fn get_top_reactions(
        &self,
        session_path: Option<PathBuf>,
        limit: usize,
        hash: i64,
    ) -> Result<ReactionCatalogPage, TelegramError> {
        let connected = self.connected(session_path).await?;
        let limit = i32::try_from(limit).map_err(|_| {
            TelegramError::InvalidPeer("get_top_reactions.limit is too large".to_string())
        })?;
        let reactions = connected
            .client
            .invoke(&tl::functions::messages::GetTopReactions { limit, hash })
            .await
            .map_err(|error| TelegramError::Telegram(error.to_string()))?;
        Ok(reaction_catalog_page(reactions))
    }

    pub async fn get_recent_reactions(
        &self,
        session_path: Option<PathBuf>,
        limit: usize,
        hash: i64,
    ) -> Result<ReactionCatalogPage, TelegramError> {
        let connected = self.connected(session_path).await?;
        let limit = i32::try_from(limit).map_err(|_| {
            TelegramError::InvalidPeer("get_recent_reactions.limit is too large".to_string())
        })?;
        let reactions = connected
            .client
            .invoke(&tl::functions::messages::GetRecentReactions { limit, hash })
            .await
            .map_err(|error| TelegramError::Telegram(error.to_string()))?;
        Ok(reaction_catalog_page(reactions))
    }

    pub async fn set_default_reaction(
        &self,
        session_path: Option<PathBuf>,
        reaction: &DefaultReactionSpec,
    ) -> Result<(), TelegramError> {
        let connected = self.connected(session_path).await?;
        let accepted = connected
            .client
            .invoke(&tl::functions::messages::SetDefaultReaction {
                reaction: default_reaction_to_tl(reaction),
            })
            .await
            .map_err(send_invocation_error)?;
        if !accepted {
            return Err(TelegramError::SendRejected(
                "Telegram rejected the default reaction update".to_string(),
            ));
        }
        Ok(())
    }

    /// The current account's own reaction on one message.
    ///
    /// Read from the message's own `reactions` summary, never from the per-user
    /// reaction list: Telegram answers `403 BROADCAST_FORBIDDEN` to
    /// `messages.getMessageReactionsList` in broadcast channels, and reaching
    /// for that list made every reaction change in a channel fail before it
    /// started — both preparation and application call this function. The
    /// summary carries the same fact everywhere: `chosen_order` on a
    /// `reactionCount` is set for the reaction this account chose, and `my` on
    /// a `messagePeerReaction` marks it in the recent-reactions list.
    pub async fn get_my_reaction(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        message_id: i32,
    ) -> Result<Option<ReactionSpec>, TelegramError> {
        let connected = self.connected(session_path).await?;
        let peer = Self::resolve_peer_ref(&connected.client, chat_id).await?;
        let messages = connected
            .client
            .get_messages(peer.as_str(), &[message_id])
            .await
            .map_err(|error| TelegramError::Telegram(error.to_string()))?;
        my_reaction_from_reactions(message_reactions_of(&messages))
    }

    pub async fn set_message_reaction(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        message_id: i32,
        reaction: &ReactionSpec,
    ) -> Result<(), TelegramError> {
        let connected = self.connected(session_path).await?;
        let peer = Self::resolve_peer_ref(&connected.client, chat_id).await?;
        let input = match reaction {
            ReactionSpec::Remove => ferogram::reactions::InputReactions::remove(),
            ReactionSpec::Emoji { emoji, big, add_to_recent } => {
                let mut input = ferogram::reactions::InputReactions::emoticon(emoji.clone());
                if *big {
                    input = input.big();
                }
                if *add_to_recent {
                    input = input.add_to_recent();
                }
                input
            },
            ReactionSpec::CustomEmoji { document_id, big, add_to_recent } => {
                let mut input = ferogram::reactions::InputReactions::custom_emoji(*document_id);
                if *big {
                    input = input.big();
                }
                if *add_to_recent {
                    input = input.add_to_recent();
                }
                input
            },
        };
        connected
            .client
            .send_reaction(peer.as_str(), message_id, input)
            .await
            .map_err(|error| TelegramError::Telegram(error.to_string()))
    }

    pub async fn edit_message(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        message_id: i32,
        text: &str,
        parse_mode: CaptionParseMode,
    ) -> Result<i32, TelegramError> {
        let connected = self.connected(session_path).await?;
        let peer = Self::resolve_peer_ref(&connected.client, chat_id).await?;
        connected
            .client
            .edit_message(peer.as_str(), message_id, edit_text_input(text, parse_mode)?)
            .await
            .map_err(send_invocation_error)?;
        Ok(message_id)
    }

    pub async fn delete_messages(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        message_ids: &[i32],
        revoke: bool,
    ) -> Result<(), TelegramError> {
        let connected = self.connected(session_path).await?;
        let peer_ref = Self::resolve_peer_ref(&connected.client, chat_id).await?;
        let peer = connected
            .client
            .resolve(peer_ref.as_str())
            .await
            .map_err(|error| TelegramError::Telegram(error.to_string()))?;
        if matches!(peer, tl::enums::Peer::Channel(_)) {
            if !revoke {
                return Err(TelegramError::InvalidPeer(
                    "channel and supergroup deletions always revoke for every participant"
                        .to_string(),
                ));
            }
            let input_peer = connected
                .client
                .resolve_to_input_peer(&peer)
                .await
                .map_err(|error| TelegramError::Telegram(error.to_string()))?;
            let tl::enums::InputPeer::Channel(channel) = input_peer else {
                return Err(TelegramError::InvalidPeer(
                    "resolved channel did not produce an input channel".to_string(),
                ));
            };
            connected
                .client
                .invoke(&tl::functions::channels::DeleteMessages {
                    channel: tl::enums::InputChannel::InputChannel(tl::types::InputChannel {
                        channel_id: channel.channel_id,
                        access_hash: channel.access_hash,
                    }),
                    id: message_ids.to_vec(),
                })
                .await
                .map_err(send_invocation_error)?;
        } else {
            connected
                .client
                .delete_messages(message_ids, revoke)
                .await
                .map_err(send_invocation_error)?;
        }
        Ok(())
    }

    pub async fn set_message_pinned(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        message_id: i32,
        pinned: bool,
        silent: bool,
        one_sided: bool,
    ) -> Result<(), TelegramError> {
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
        connected
            .client
            .invoke(&tl::functions::messages::UpdatePinnedMessage {
                silent,
                unpin: !pinned,
                pm_oneside: one_sided,
                peer: input_peer,
                id: message_id,
            })
            .await
            .map_err(send_invocation_error)?;
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn forward_messages_with_random_ids(
        &self,
        session_path: Option<PathBuf>,
        source_chat_id: &str,
        message_ids: &[i32],
        destination_chat_id: &str,
        silent: bool,
        drop_author: bool,
        drop_media_captions: bool,
        noforwards: bool,
        random_ids: &[i64],
    ) -> Result<Vec<i32>, TelegramError> {
        if message_ids.is_empty() || message_ids.len() != random_ids.len() {
            return Err(TelegramError::InvalidPeer(
                "forward message ids and random ids must be non-empty and have equal length"
                    .to_string(),
            ));
        }
        let connected = self.connected(session_path).await?;
        let source_ref = Self::resolve_peer_ref(&connected.client, source_chat_id).await?;
        let destination_ref =
            Self::resolve_peer_ref(&connected.client, destination_chat_id).await?;
        let source_peer = connected
            .client
            .resolve(source_ref.as_str())
            .await
            .map_err(|error| TelegramError::Telegram(error.to_string()))?;
        let destination_peer = connected
            .client
            .resolve(destination_ref.as_str())
            .await
            .map_err(|error| TelegramError::Telegram(error.to_string()))?;
        let from_peer = connected
            .client
            .resolve_to_input_peer(&source_peer)
            .await
            .map_err(|error| TelegramError::Telegram(error.to_string()))?;
        let to_peer = connected
            .client
            .resolve_to_input_peer(&destination_peer)
            .await
            .map_err(|error| TelegramError::Telegram(error.to_string()))?;
        let updates = connected
            .client
            .invoke(&tl::functions::messages::ForwardMessages {
                silent,
                background: false,
                with_my_score: false,
                drop_author,
                drop_media_captions,
                noforwards,
                allow_paid_floodskip: false,
                from_peer,
                id: message_ids.to_vec(),
                random_id: random_ids.to_vec(),
                to_peer,
                top_msg_id: None,
                reply_to: None,
                schedule_date: None,
                schedule_repeat_period: None,
                send_as: None,
                quick_reply_shortcut: None,
                effect: None,
                video_timestamp: None,
                allow_paid_stars: None,
                suggested_post: None,
            })
            .await
            .map_err(send_invocation_error)?;
        sent_message_ids(&updates, random_ids).ok_or_else(|| {
            TelegramError::DeliveryUncertain(
                "Telegram accepted forward request but response did not explicitly bind every random ID to a destination message ID"
                    .to_string(),
            )
        })
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
            FileSendMode::Gif => {
                if uploaded.mime_type() != "image/gif" {
                    return Err(TelegramError::InvalidPeer(format!(
                        "GIF mode requires image/gif bytes; detected {}",
                        uploaded.mime_type()
                    )));
                }
                uploaded.as_auto_media()
            },
        };
        let options = InputMessage::text(caption).reply_to(reply_to_message_id);
        let sent = connected
            .client
            .send_file(peer.as_str(), media, &options)
            .await
            .map_err(send_invocation_error)?;
        Ok(sent.id())
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn send_file_with_random_id(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        file_path: &Path,
        caption: &str,
        reply_to_message_id: Option<i32>,
        mode: FileSendMode,
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
        let uploaded = connected
            .client
            .upload_file(file_path)
            .await
            .map_err(|error| TelegramError::Telegram(error.to_string()))?;
        let media = match mode {
            FileSendMode::Auto => uploaded.as_auto_media(),
            FileSendMode::Document => uploaded.as_document_media(),
            FileSendMode::Photo => uploaded.as_photo_media(),
            FileSendMode::Gif => {
                if uploaded.mime_type() != "image/gif" {
                    return Err(TelegramError::InvalidPeer(format!(
                        "GIF mode requires image/gif bytes; detected {}",
                        uploaded.mime_type()
                    )));
                }
                uploaded.as_auto_media()
            },
        };
        let reply_to = reply_to_message_id.map(|message_id| {
            tl::enums::InputReplyTo::Message(tl::types::InputReplyToMessage {
                reply_to_msg_id: message_id,
                top_msg_id: None,
                reply_to_peer_id: None,
                quote_text: None,
                quote_entities: None,
                quote_offset: None,
                monoforum_peer_id: None,
                poll_option: None,
                todo_item_id: None,
            })
        });
        let request = tl::functions::messages::SendMedia {
            silent: false,
            background: false,
            clear_draft: false,
            noforwards: false,
            update_stickersets_order: false,
            invert_media: false,
            allow_paid_floodskip: false,
            peer: input_peer,
            reply_to,
            media,
            message: caption.to_string(),
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
        };
        let updates = connected.client.invoke(&request).await.map_err(send_invocation_error)?;
        sent_message_id(&updates, random_id).ok_or_else(|| {
            TelegramError::DeliveryUncertain(
                "Telegram accepted file send request but response contained no message ID"
                    .to_string(),
            )
        })
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn send_file_html_with_random_id(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        file_path: &Path,
        caption_html: &str,
        reply_to_message_id: Option<i32>,
        topic_id: Option<i32>,
        silent: bool,
        noforwards: bool,
        mode: FileSendMode,
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
        let uploaded = connected
            .client
            .upload_file(file_path)
            .await
            .map_err(|error| TelegramError::Telegram(error.to_string()))?;
        let media = match mode {
            FileSendMode::Auto => uploaded.as_auto_media(),
            FileSendMode::Document => uploaded.as_document_media(),
            FileSendMode::Photo => uploaded.as_photo_media(),
            FileSendMode::Gif => {
                if uploaded.mime_type() != "image/gif" {
                    return Err(TelegramError::InvalidPeer(format!(
                        "GIF mode requires image/gif bytes; detected {}",
                        uploaded.mime_type()
                    )));
                }
                uploaded.as_auto_media()
            },
        };
        let caption = caption_input(caption_html, CaptionParseMode::Html);
        let updates = connected
            .client
            .invoke(&tl::functions::messages::SendMedia {
                silent,
                background: false,
                clear_draft: false,
                noforwards,
                update_stickersets_order: false,
                invert_media: false,
                allow_paid_floodskip: false,
                peer: input_peer,
                reply_to: input_reply_to(reply_to_message_id, topic_id),
                media,
                message: caption.text,
                random_id,
                reply_markup: None,
                entities: caption.entities,
                schedule_date: None,
                schedule_repeat_period: None,
                send_as: None,
                quick_reply_shortcut: None,
                effect: None,
                allow_paid_stars: None,
                suggested_post: None,
            })
            .await
            .map_err(send_invocation_error)?;
        sent_message_id(&updates, random_id).ok_or_else(|| {
            TelegramError::DeliveryUncertain(
                "Telegram accepted recoverable file send but returned no exact message ID"
                    .to_string(),
            )
        })
    }

    #[allow(clippy::too_many_arguments)]
    async fn send_uploaded_document_with_random_id(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        file_path: &Path,
        caption: &str,
        caption_parse_mode: CaptionParseMode,
        reply_to_message_id: Option<i32>,
        topic_id: Option<i32>,
        silent: bool,
        noforwards: bool,
        random_id: i64,
        kind: UploadedDocumentKind,
    ) -> Result<i32, TelegramError> {
        reject_rich_media_caption(caption_parse_mode, "media")?;
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
        let uploaded = connected
            .client
            .upload_file(file_path)
            .await
            .map_err(|error| TelegramError::Telegram(error.to_string()))?;
        match kind {
            UploadedDocumentKind::Voice
                if !(uploaded.mime_type().starts_with("audio/")
                    || uploaded.mime_type() == "application/ogg") =>
            {
                return Err(TelegramError::InvalidPeer(format!(
                    "voice send requires audio bytes; detected {}",
                    uploaded.mime_type()
                )));
            },
            UploadedDocumentKind::VideoNote { .. } if uploaded.mime_type() != "video/mp4" => {
                return Err(TelegramError::InvalidPeer(format!(
                    "video-note send requires video/mp4 bytes; detected {}",
                    uploaded.mime_type()
                )));
            },
            _ => {},
        }
        let mut media = uploaded.as_auto_media();
        let tl::enums::InputMedia::UploadedDocument(document) = &mut media else {
            return Err(TelegramError::InvalidPeer(
                "special document media unexpectedly resolved to a non-document upload".to_string(),
            ));
        };
        let attribute_updated =
            document.attributes.iter_mut().any(|attribute| match (kind, attribute) {
                (UploadedDocumentKind::Voice, tl::enums::DocumentAttribute::Audio(attribute)) => {
                    attribute.voice = true;
                    attribute.title = None;
                    attribute.performer = None;
                    true
                },
                (
                    UploadedDocumentKind::VideoNote { duration_seconds, length_pixels },
                    tl::enums::DocumentAttribute::Video(attribute),
                ) => {
                    attribute.round_message = true;
                    attribute.supports_streaming = true;
                    attribute.duration = f64::from(duration_seconds);
                    attribute.w = length_pixels;
                    attribute.h = length_pixels;
                    true
                },
                _ => false,
            });
        if !attribute_updated {
            return Err(TelegramError::InvalidPeer(
                "special document media did not contain the required audio/video attribute"
                    .to_string(),
            ));
        }
        let caption = caption_input(caption, caption_parse_mode);
        let reply_to = input_reply_to(reply_to_message_id, topic_id);
        let updates = connected
            .client
            .invoke(&tl::functions::messages::SendMedia {
                silent,
                background: false,
                clear_draft: false,
                noforwards,
                update_stickersets_order: false,
                invert_media: false,
                allow_paid_floodskip: false,
                peer: input_peer,
                reply_to,
                media,
                message: caption.text,
                random_id,
                reply_markup: None,
                entities: caption.entities,
                schedule_date: None,
                schedule_repeat_period: None,
                send_as: None,
                quick_reply_shortcut: None,
                effect: None,
                allow_paid_stars: None,
                suggested_post: None,
            })
            .await
            .map_err(send_invocation_error)?;
        sent_message_id(&updates, random_id).ok_or_else(|| {
            TelegramError::DeliveryUncertain(
                "Telegram accepted special document send request but response contained no exact message ID"
                    .to_string(),
            )
        })
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn send_voice_with_random_id(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        file_path: &Path,
        caption: &str,
        caption_parse_mode: CaptionParseMode,
        reply_to_message_id: Option<i32>,
        topic_id: Option<i32>,
        silent: bool,
        noforwards: bool,
        random_id: i64,
    ) -> Result<i32, TelegramError> {
        self.send_uploaded_document_with_random_id(
            session_path,
            chat_id,
            file_path,
            caption,
            caption_parse_mode,
            reply_to_message_id,
            topic_id,
            silent,
            noforwards,
            random_id,
            UploadedDocumentKind::Voice,
        )
        .await
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn send_video_note_with_random_id(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        file_path: &Path,
        duration_seconds: i32,
        length_pixels: i32,
        reply_to_message_id: Option<i32>,
        topic_id: Option<i32>,
        silent: bool,
        noforwards: bool,
        random_id: i64,
    ) -> Result<i32, TelegramError> {
        self.send_uploaded_document_with_random_id(
            session_path,
            chat_id,
            file_path,
            "",
            CaptionParseMode::Plain,
            reply_to_message_id,
            topic_id,
            silent,
            noforwards,
            random_id,
            UploadedDocumentKind::VideoNote { duration_seconds, length_pixels },
        )
        .await
    }

    pub async fn get_message_document_media(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        message_id: i32,
    ) -> Result<Option<SentDocumentMediaSummary>, TelegramError> {
        let connected = self.connected(session_path).await?;
        let peer = Self::resolve_peer_ref(&connected.client, chat_id).await?;
        let messages = connected
            .client
            .get_messages(peer.as_str(), &[message_id])
            .await
            .map_err(|error| TelegramError::Telegram(error.to_string()))?;
        let Some(message) = messages.iter().find(|message| is_concrete_message(message)) else {
            return Ok(None);
        };
        message.media().map(document_media_summary_from_tl).transpose().map(Option::flatten)
    }

    pub async fn get_message_topic_id(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        message_id: i32,
    ) -> Result<Option<i32>, TelegramError> {
        let connected = self.connected(session_path).await?;
        let peer = Self::resolve_peer_ref(&connected.client, chat_id).await?;
        let messages = connected
            .client
            .get_messages(peer.as_str(), &[message_id])
            .await
            .map_err(|error| TelegramError::Telegram(error.to_string()))?;
        let Some(message) = messages.iter().find(|message| is_concrete_message(message)) else {
            return Ok(None);
        };
        Ok(match &message.raw {
            tl::enums::Message::Message(message) => {
                message.reply_to.as_ref().and_then(|reply| match reply {
                    tl::enums::MessageReplyHeader::MessageReplyHeader(header) => {
                        header.reply_to_top_id
                    },
                    _ => None,
                })
            },
            _ => None,
        })
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn send_album_with_random_ids(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        items: &[crate::domain::AlbumUploadSpec],
        caption_parse_mode: CaptionParseMode,
        reply_to_message_id: Option<i32>,
        topic_id: Option<i32>,
        silent: bool,
        noforwards: bool,
        random_ids: &[i64],
    ) -> Result<Vec<i32>, TelegramError> {
        reject_rich_media_caption(caption_parse_mode, "album")?;
        if !(2..=10).contains(&items.len()) || items.len() != random_ids.len() {
            return Err(TelegramError::InvalidPeer(
                "album requires 2-10 items and one random id per item".to_string(),
            ));
        }
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
        let mut multi_media = Vec::with_capacity(items.len());
        for (item, random_id) in items.iter().zip(random_ids.iter().copied()) {
            let uploaded = connected
                .client
                .upload_file(&item.file_path)
                .await
                .map_err(|error| TelegramError::Telegram(error.to_string()))?;
            let media = match item.media_kind {
                crate::domain::AlbumMediaKind::Photo => {
                    if !uploaded.mime_type().starts_with("image/")
                        || uploaded.mime_type() == "image/gif"
                    {
                        return Err(TelegramError::InvalidPeer(format!(
                            "album photo item requires a static image; detected {}",
                            uploaded.mime_type()
                        )));
                    }
                    let finalized = connected
                        .client
                        .invoke(&tl::functions::messages::UploadMedia {
                            business_connection_id: None,
                            peer: input_peer.clone(),
                            media: uploaded.as_photo_media(),
                        })
                        .await
                        .map_err(send_invocation_error)?;
                    finalized_album_photo_media(finalized)?
                },
                crate::domain::AlbumMediaKind::Video => {
                    if !uploaded.mime_type().starts_with("video/") {
                        return Err(TelegramError::InvalidPeer(format!(
                            "album video item requires video bytes; detected {}",
                            uploaded.mime_type()
                        )));
                    }
                    let finalized = connected
                        .client
                        .invoke(&tl::functions::messages::UploadMedia {
                            business_connection_id: None,
                            peer: input_peer.clone(),
                            media: uploaded.as_auto_media(),
                        })
                        .await
                        .map_err(send_invocation_error)?;
                    finalized_album_document_media(finalized)?
                },
            };
            let caption = caption_input(&item.caption, caption_parse_mode);
            multi_media.push(tl::enums::InputSingleMedia::InputSingleMedia(
                tl::types::InputSingleMedia {
                    media,
                    random_id,
                    message: caption.text,
                    entities: caption.entities,
                },
            ));
        }
        let updates = connected
            .client
            .invoke(&tl::functions::messages::SendMultiMedia {
                silent,
                background: false,
                clear_draft: false,
                noforwards,
                update_stickersets_order: false,
                invert_media: false,
                allow_paid_floodskip: false,
                peer: input_peer,
                reply_to: input_reply_to(reply_to_message_id, topic_id),
                multi_media,
                schedule_date: None,
                send_as: None,
                quick_reply_shortcut: None,
                effect: None,
                allow_paid_stars: None,
            })
            .await
            .map_err(send_invocation_error)?;
        sent_message_ids(&updates, random_ids).ok_or_else(|| {
            TelegramError::DeliveryUncertain(
                "Telegram accepted album send request but response did not explicitly bind every random ID to a message ID"
                    .to_string(),
            )
        })
    }

    pub async fn get_message_grouped_id(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        message_id: i32,
    ) -> Result<Option<i64>, TelegramError> {
        let connected = self.connected(session_path).await?;
        let peer = Self::resolve_peer_ref(&connected.client, chat_id).await?;
        let messages = connected
            .client
            .get_messages(peer.as_str(), &[message_id])
            .await
            .map_err(|error| TelegramError::Telegram(error.to_string()))?;
        let Some(message) = messages.iter().find(|message| is_concrete_message(message)) else {
            return Ok(None);
        };
        Ok(message.grouped_id())
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn send_sticker_with_random_id(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        set_short_name: &str,
        sticker_handle: &str,
        reply_to_message_id: Option<i32>,
        silent: bool,
        noforwards: bool,
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
        let document =
            resolve_raw_sticker_document(&connected.client, set_short_name, sticker_handle).await?;
        let media = tl::enums::InputMedia::Document(tl::types::InputMediaDocument {
            spoiler: false,
            id: tl::enums::InputDocument::InputDocument(tl::types::InputDocument {
                id: document.id,
                access_hash: document.access_hash,
                file_reference: document.file_reference,
            }),
            video_cover: None,
            video_timestamp: None,
            ttl_seconds: None,
            query: None,
        });
        let reply_to = reply_to_message_id.map(|message_id| {
            tl::enums::InputReplyTo::Message(tl::types::InputReplyToMessage {
                reply_to_msg_id: message_id,
                top_msg_id: None,
                reply_to_peer_id: None,
                quote_text: None,
                quote_entities: None,
                quote_offset: None,
                monoforum_peer_id: None,
                poll_option: None,
                todo_item_id: None,
            })
        });
        let updates = connected
            .client
            .invoke(&tl::functions::messages::SendMedia {
                silent,
                background: false,
                clear_draft: false,
                noforwards,
                update_stickersets_order: true,
                invert_media: false,
                allow_paid_floodskip: false,
                peer: input_peer,
                reply_to,
                media,
                message: String::new(),
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
            })
            .await
            .map_err(send_invocation_error)?;
        sent_message_id(&updates, random_id).ok_or_else(|| {
            TelegramError::DeliveryUncertain(
                "Telegram accepted sticker send request but response contained no message ID"
                    .to_string(),
            )
        })
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

    #[allow(clippy::too_many_arguments)]
    pub async fn send_text_html_with_random_id(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        html: &str,
        reply_to_message_id: Option<i32>,
        topic_id: Option<i32>,
        silent: bool,
        noforwards: bool,
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
        let message = InputMessage::html(html);
        let updates = connected
            .client
            .invoke(&tl::functions::messages::SendMessage {
                no_webpage: false,
                silent,
                background: false,
                clear_draft: false,
                noforwards,
                update_stickersets_order: false,
                invert_media: false,
                allow_paid_floodskip: false,
                peer: input_peer,
                reply_to: input_reply_to(reply_to_message_id, topic_id),
                message: message.text,
                random_id,
                reply_markup: None,
                entities: message.entities,
                schedule_date: None,
                schedule_repeat_period: None,
                send_as: None,
                quick_reply_shortcut: None,
                effect: None,
                allow_paid_stars: None,
                suggested_post: None,
                rich_message: None,
            })
            .await
            .map_err(send_invocation_error)?;
        sent_message_id(&updates, random_id).ok_or_else(|| {
            TelegramError::DeliveryUncertain(
                "Telegram accepted recoverable text send but returned no exact message ID"
                    .to_string(),
            )
        })
    }

    pub async fn send_rich_markdown_with_random_id(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        markdown: &str,
        reply_to_message_id: Option<i32>,
        random_id: i64,
    ) -> Result<i32, TelegramError> {
        let normalized_markdown = normalize_rich_markdown(markdown);
        let blocks = ferogram::parsers::parse_rich_markdown(&normalized_markdown);
        if blocks.is_empty() {
            return Err(TelegramError::SendRejected(
                "rich Markdown produced no Telegram PageBlock content".to_string(),
            ));
        }
        let connected = self.connected(session_path).await?;
        let (_, input_peer) = resolve_rich_message_peer(&connected.client, chat_id).await?;
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
        let rich_message =
            tl::enums::InputRichMessage::InputRichMessage(tl::types::InputRichMessage {
                rtl: false,
                noautolink: false,
                blocks,
                photos: None,
                documents: None,
                users: None,
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
            message: String::new(),
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
            rich_message: Some(rich_message),
        };
        let updates = connected.client.invoke(&request).await.map_err(send_invocation_error)?;
        sent_message_id(&updates, random_id).ok_or_else(|| {
            TelegramError::DeliveryUncertain(
                "Telegram accepted rich-message send but response contained no message ID"
                    .to_string(),
            )
        })
    }

    pub async fn get_rich_message_snapshot(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        message_id: i32,
    ) -> Result<Option<RichMessageSnapshot>, TelegramError> {
        let connected = self.connected(session_path).await?;
        let (canonical_chat_id, input_peer) =
            resolve_rich_message_peer(&connected.client, chat_id).await?;
        let direct_result = connected
            .client
            .invoke(&tl::functions::messages::GetRichMessage { peer: input_peer, id: message_id })
            .await;
        let direct_error = direct_result.as_ref().err().map(ToString::to_string);
        let direct_rich_message = direct_result.ok().and_then(|page| {
            let messages = match page {
                tl::enums::messages::Messages::Messages(page) => page.messages,
                tl::enums::messages::Messages::Slice(page) => page.messages,
                tl::enums::messages::Messages::ChannelMessages(page) => page.messages,
                tl::enums::messages::Messages::NotModified(_) => Vec::new(),
            };
            messages.into_iter().find_map(|message| match message {
                tl::enums::Message::Message(message) if message.id == message_id => {
                    message.rich_message
                },
                _ => None,
            })
        });
        if let Some(rich_message) = direct_rich_message {
            return Ok(Some(rich_message_snapshot(canonical_chat_id, message_id, &rich_message)));
        }

        // Telegram's GetRichMessage may omit queued messages. GetScheduledHistory carries the
        // same native payload and is the authoritative readback while a message is scheduled.
        let scheduled = connected
            .client
            .get_scheduled_messages(canonical_chat_id.clone())
            .await
            .map_err(|error| {
                let error = error.to_string();
                match direct_error.as_deref() {
                    Some(direct_error) => TelegramError::Telegram(format!(
                        "GetRichMessage failed: {direct_error}; scheduled-history fallback failed: {error}"
                    )),
                    None => TelegramError::Telegram(error),
                }
            })?;
        let scheduled_rich_message = scheduled.into_iter().find_map(|message| match message.raw {
            tl::enums::Message::Message(message) if message.id == message_id => {
                message.rich_message
            },
            _ => None,
        });
        if let Some(rich_message) = scheduled_rich_message {
            return Ok(Some(rich_message_snapshot(canonical_chat_id, message_id, &rich_message)));
        }
        if let Some(direct_error) = direct_error {
            return Err(TelegramError::Telegram(direct_error));
        }
        Ok(None)
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
        let (client, shutdown) = self.connect_client(&selected_path, lock).await?;
        let connected =
            Arc::new(ConnectedSession { client, _shutdown: shutdown, path: selected_path });
        if !connected
            .client
            .is_authorized()
            .await
            .map_err(|error| TelegramError::Telegram(error.to_string()))?
        {
            return Err(TelegramError::Unauthorized);
        }

        *guard = Some(connected.clone());
        Ok(connected)
    }

    async fn connect_client(
        &self,
        session_path: &Path,
        lock: File,
    ) -> Result<(Client, ShutdownToken), TelegramError> {
        let mut builder = Client::builder()
            .api_id(self.config.api_id)
            .api_hash(self.config.api_hash.clone())
            .session_backend(Arc::new(LockedSessionBackend::new(session_path.to_path_buf(), lock)))
            // This MCP exposes bounded request/response tools and does not expose
            // Ferogram's realtime update stream. Passive gap recovery therefore
            // adds schema-sensitive background work without serving a consumer.
            .process_updates(false);
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

    async fn send_typing_action(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        action: TypingAction,
    ) -> Result<(), TelegramError> {
        TelegramClient::send_typing_action(self, session_path, chat_id, action).await
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

    async fn history_newer_than(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        min_message_id: i32,
        limit: usize,
        offset_id: Option<i32>,
    ) -> Result<Vec<MessageSummary>, TelegramError> {
        TelegramClient::history_newer_than(
            self,
            session_path,
            chat_id,
            min_message_id,
            limit,
            offset_id,
        )
        .await
    }

    async fn get_updates_state(
        &self,
        session_path: Option<PathBuf>,
    ) -> Result<UpdatesStateSummary, TelegramError> {
        TelegramClient::get_updates_state(self, session_path).await
    }

    async fn get_updates_difference(
        &self,
        request: UpdatesDifferenceRequest,
    ) -> Result<UpdatesDifferenceSummary, TelegramError> {
        TelegramClient::get_updates_difference(self, request).await
    }

    async fn get_channel_updates(
        &self,
        request: ChannelUpdatesRequest,
    ) -> Result<ChannelUpdatesDifferenceSummary, TelegramError> {
        TelegramClient::get_channel_updates(self, request).await
    }

    async fn get_quick_replies(
        &self,
        session_path: Option<PathBuf>,
        hash: i64,
    ) -> Result<QuickReplyCatalogPage, TelegramError> {
        TelegramClient::get_quick_replies(self, session_path, hash).await
    }

    async fn get_quick_reply_messages(
        &self,
        session_path: Option<PathBuf>,
        shortcut_id: i32,
        message_ids: Option<&[i32]>,
        hash: i64,
    ) -> Result<QuickReplyMessagesPage, TelegramError> {
        TelegramClient::get_quick_reply_messages(self, session_path, shortcut_id, message_ids, hash)
            .await
    }

    async fn get_group_call(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        participant_limit: i32,
    ) -> Result<GroupCallPage, TelegramError> {
        TelegramClient::get_group_call(self, session_path, chat_id, participant_limit).await
    }

    async fn get_group_call_participants(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        peer_refs: &[String],
        sources: &[i32],
        offset: &str,
        limit: i32,
    ) -> Result<GroupCallParticipantsPage, TelegramError> {
        TelegramClient::get_group_call_participants(
            self,
            session_path,
            chat_id,
            peer_refs,
            sources,
            offset,
            limit,
        )
        .await
    }

    async fn get_business_chat_links(
        &self,
        session_path: Option<PathBuf>,
    ) -> Result<BusinessChatLinksPage, TelegramError> {
        TelegramClient::get_business_chat_links(self, session_path).await
    }

    async fn resolve_business_chat_link(
        &self,
        session_path: Option<PathBuf>,
        slug: &str,
    ) -> Result<ResolvedBusinessChatLinkSummary, TelegramError> {
        TelegramClient::resolve_business_chat_link(self, session_path, slug).await
    }

    async fn create_business_chat_link(
        &self,
        session_path: Option<PathBuf>,
        content: &BusinessChatLinkContent,
    ) -> Result<BusinessChatLinkSummary, TelegramError> {
        TelegramClient::create_business_chat_link(self, session_path, content).await
    }

    async fn edit_business_chat_link(
        &self,
        session_path: Option<PathBuf>,
        slug: &str,
        content: &BusinessChatLinkContent,
    ) -> Result<BusinessChatLinkSummary, TelegramError> {
        TelegramClient::edit_business_chat_link(self, session_path, slug, content).await
    }

    async fn delete_business_chat_link(
        &self,
        session_path: Option<PathBuf>,
        slug: &str,
    ) -> Result<(), TelegramError> {
        TelegramClient::delete_business_chat_link(self, session_path, slug).await
    }

    async fn set_business_work_hours(
        &self,
        session_path: Option<PathBuf>,
        target: Option<&BusinessWorkHoursConfig>,
    ) -> Result<(), TelegramError> {
        TelegramClient::set_business_work_hours(self, session_path, target).await
    }

    async fn set_business_location(
        &self,
        session_path: Option<PathBuf>,
        target: Option<&BusinessLocationSummary>,
    ) -> Result<(), TelegramError> {
        TelegramClient::set_business_location(self, session_path, target).await
    }

    async fn set_business_greeting(
        &self,
        session_path: Option<PathBuf>,
        target: Option<&BusinessGreetingMessageSummary>,
    ) -> Result<(), TelegramError> {
        TelegramClient::set_business_greeting(self, session_path, target).await
    }

    async fn set_business_away(
        &self,
        session_path: Option<PathBuf>,
        target: Option<&BusinessAwayMessageSummary>,
    ) -> Result<(), TelegramError> {
        TelegramClient::set_business_away(self, session_path, target).await
    }

    async fn set_business_intro(
        &self,
        session_path: Option<PathBuf>,
        target: Option<&BusinessIntroSpec>,
    ) -> Result<(), TelegramError> {
        TelegramClient::set_business_intro(self, session_path, target).await
    }

    async fn get_stars_status(
        &self,
        session_path: Option<PathBuf>,
        peer_ref: &str,
    ) -> Result<StarsStatusPage, TelegramError> {
        TelegramClient::get_stars_status(self, session_path, peer_ref).await
    }

    async fn get_stars_transactions(
        &self,
        session_path: Option<PathBuf>,
        peer_ref: &str,
        inbound: bool,
        outbound: bool,
        ascending: bool,
        subscription_id: Option<&str>,
        offset: &str,
        limit: i32,
    ) -> Result<StarsStatusPage, TelegramError> {
        TelegramClient::get_stars_transactions(
            self,
            session_path,
            peer_ref,
            inbound,
            outbound,
            ascending,
            subscription_id,
            offset,
            limit,
        )
        .await
    }

    async fn get_available_star_gifts(
        &self,
        session_path: Option<PathBuf>,
    ) -> Result<AvailableStarGiftsPage, TelegramError> {
        TelegramClient::get_available_star_gifts(self, session_path).await
    }

    async fn get_saved_star_gifts(
        &self,
        request: SavedStarGiftsRequest,
    ) -> Result<SavedStarGiftsPage, TelegramError> {
        TelegramClient::get_saved_star_gifts(self, request).await
    }

    async fn get_stars_topup_options(
        &self,
        session_path: Option<PathBuf>,
    ) -> Result<StarsTopupOptionsPage, TelegramError> {
        TelegramClient::get_stars_topup_options(self, session_path).await
    }

    async fn get_stars_subscriptions(
        &self,
        session_path: Option<PathBuf>,
        peer_ref: &str,
        missing_balance: bool,
        offset: &str,
    ) -> Result<StarsSubscriptionsPage, TelegramError> {
        TelegramClient::get_stars_subscriptions(
            self,
            session_path,
            peer_ref,
            missing_balance,
            offset,
        )
        .await
    }

    async fn get_saved_star_gift(
        &self,
        session_path: Option<PathBuf>,
        reference: &SavedStarGiftRef,
    ) -> Result<Option<SavedStarGiftSummary>, TelegramError> {
        TelegramClient::get_saved_star_gift(self, session_path, reference).await
    }

    async fn set_star_gift_unsaved(
        &self,
        session_path: Option<PathBuf>,
        reference: &SavedStarGiftRef,
        unsaved: bool,
    ) -> Result<(), TelegramError> {
        TelegramClient::set_star_gift_unsaved(self, session_path, reference, unsaved).await
    }

    async fn convert_star_gift(
        &self,
        session_path: Option<PathBuf>,
        reference: &SavedStarGiftRef,
    ) -> Result<(), TelegramError> {
        TelegramClient::convert_star_gift(self, session_path, reference).await
    }

    async fn get_stars_subscription(
        &self,
        session_path: Option<PathBuf>,
        peer_ref: &str,
        subscription_id: &str,
    ) -> Result<Option<StarsSubscriptionSummary>, TelegramError> {
        TelegramClient::get_stars_subscription(self, session_path, peer_ref, subscription_id).await
    }

    async fn change_stars_subscription(
        &self,
        session_path: Option<PathBuf>,
        peer_ref: &str,
        subscription_id: &str,
        canceled: bool,
    ) -> Result<(), TelegramError> {
        TelegramClient::change_stars_subscription(
            self,
            session_path,
            peer_ref,
            subscription_id,
            canceled,
        )
        .await
    }

    async fn search_messages(
        &self,
        request: SearchMessagesRequest,
    ) -> Result<MessagePage, TelegramError> {
        TelegramClient::search_messages(self, request).await
    }

    async fn search_saved_messages(
        &self,
        request: SavedMessagesSearchRequest,
    ) -> Result<MessagePage, TelegramError> {
        TelegramClient::search_saved_messages(self, request).await
    }

    async fn get_saved_dialogs(
        &self,
        session_path: Option<PathBuf>,
        limit: usize,
        exclude_pinned: bool,
        offset_id: Option<i32>,
    ) -> Result<SavedDialogsPage, TelegramError> {
        TelegramClient::get_saved_dialogs(self, session_path, limit, exclude_pinned, offset_id)
            .await
    }

    async fn get_saved_dialog(
        &self,
        session_path: Option<PathBuf>,
        peer_ref: &str,
    ) -> Result<Option<SavedDialogSummary>, TelegramError> {
        TelegramClient::get_saved_dialog(self, session_path, peer_ref).await
    }

    async fn get_saved_history(
        &self,
        session_path: Option<PathBuf>,
        peer_ref: &str,
        limit: usize,
        offset_id: Option<i32>,
    ) -> Result<SavedHistoryPage, TelegramError> {
        TelegramClient::get_saved_history(self, session_path, peer_ref, limit, offset_id).await
    }

    async fn delete_saved_history(
        &self,
        session_path: Option<PathBuf>,
        peer_ref: &str,
        max_id: i32,
        min_date: Option<i32>,
        max_date: Option<i32>,
    ) -> Result<(), TelegramError> {
        TelegramClient::delete_saved_history(
            self,
            session_path,
            peer_ref,
            max_id,
            min_date,
            max_date,
        )
        .await
    }

    async fn toggle_saved_dialog_pin(
        &self,
        session_path: Option<PathBuf>,
        peer_ref: &str,
        pinned: bool,
    ) -> Result<(), TelegramError> {
        TelegramClient::toggle_saved_dialog_pin(self, session_path, peer_ref, pinned).await
    }

    async fn get_saved_reaction_tags(
        &self,
        session_path: Option<PathBuf>,
        peer_ref: Option<&str>,
        hash: i64,
    ) -> Result<SavedReactionTagsPage, TelegramError> {
        TelegramClient::get_saved_reaction_tags(self, session_path, peer_ref, hash).await
    }

    async fn update_saved_reaction_tag(
        &self,
        session_path: Option<PathBuf>,
        reaction: &ReactionIdentity,
        title: Option<&str>,
    ) -> Result<(), TelegramError> {
        TelegramClient::update_saved_reaction_tag(self, session_path, reaction, title).await
    }

    async fn get_default_tag_reactions(
        &self,
        session_path: Option<PathBuf>,
        hash: i64,
    ) -> Result<ReactionCatalogPage, TelegramError> {
        TelegramClient::get_default_tag_reactions(self, session_path, hash).await
    }

    async fn get_unread_mentions(
        &self,
        request: UnreadMessagesRequest,
    ) -> Result<MessagePage, TelegramError> {
        TelegramClient::get_unread_mentions(self, request).await
    }

    async fn get_unread_reactions(
        &self,
        request: UnreadMessagesRequest,
    ) -> Result<MessagePage, TelegramError> {
        TelegramClient::get_unread_reactions(self, request).await
    }

    async fn get_unread_poll_votes(
        &self,
        request: UnreadMessagesRequest,
    ) -> Result<MessagePage, TelegramError> {
        TelegramClient::get_unread_poll_votes(self, request).await
    }

    async fn get_message(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        message_id: i32,
    ) -> Result<Option<MessageSummary>, TelegramError> {
        TelegramClient::get_message(self, session_path, chat_id, message_id).await
    }

    async fn get_message_html(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        message_id: i32,
    ) -> Result<String, TelegramError> {
        TelegramClient::get_message_html(self, session_path, chat_id, message_id).await
    }

    async fn get_message_buttons(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        message_id: Option<i32>,
        limit: usize,
    ) -> Result<MessageButtonInventory, TelegramError> {
        TelegramClient::get_message_buttons(self, session_path, chat_id, message_id, limit).await
    }

    async fn press_callback_button(
        &self,
        session_path: Option<PathBuf>,
        button: &CallbackButtonSnapshot,
    ) -> Result<BotCallbackAnswerSummary, TelegramError> {
        TelegramClient::press_callback_button(self, session_path, button).await
    }

    async fn get_inline_bot_results(
        &self,
        session_path: Option<PathBuf>,
        bot_ref: &str,
        context_chat_id: &str,
        query: &str,
        offset: &str,
    ) -> Result<InlineBotResults, TelegramError> {
        TelegramClient::get_inline_bot_results(
            self,
            session_path,
            bot_ref,
            context_chat_id,
            query,
            offset,
        )
        .await
    }

    async fn send_inline_bot_result_with_random_id(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        query_id: &str,
        result_id: &str,
        reply_to_message_id: Option<i32>,
        silent: bool,
        hide_via: bool,
        clear_draft: bool,
        random_id: i64,
    ) -> Result<i32, TelegramError> {
        TelegramClient::send_inline_bot_result_with_random_id(
            self,
            session_path,
            chat_id,
            query_id,
            result_id,
            reply_to_message_id,
            silent,
            hide_via,
            clear_draft,
            random_id,
        )
        .await
    }

    async fn translate_messages(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        message_ids: &[i32],
        to_language: &str,
    ) -> Result<MessageTranslationBatch, TelegramError> {
        TelegramClient::translate_messages(self, session_path, chat_id, message_ids, to_language)
            .await
    }

    async fn transcribe_audio(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        message_id: i32,
    ) -> Result<AudioTranscriptionSummary, TelegramError> {
        TelegramClient::transcribe_audio(self, session_path, chat_id, message_id).await
    }

    async fn rate_transcription(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        message_id: i32,
        transcription_id: i64,
        good: bool,
    ) -> Result<(), TelegramError> {
        TelegramClient::rate_transcription(
            self,
            session_path,
            chat_id,
            message_id,
            transcription_id,
            good,
        )
        .await
    }

    async fn get_fact_checks(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        message_ids: &[i32],
    ) -> Result<FactCheckPage, TelegramError> {
        TelegramClient::get_fact_checks(self, session_path, chat_id, message_ids).await
    }

    async fn edit_fact_check(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        message_id: i32,
        text: &str,
    ) -> Result<(), TelegramError> {
        TelegramClient::edit_fact_check(self, session_path, chat_id, message_id, text).await
    }

    async fn delete_fact_check(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        message_id: i32,
    ) -> Result<(), TelegramError> {
        TelegramClient::delete_fact_check(self, session_path, chat_id, message_id).await
    }

    async fn get_discussion_message(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        message_id: i32,
    ) -> Result<DiscussionMessageSummary, TelegramError> {
        TelegramClient::get_discussion_message(self, session_path, chat_id, message_id).await
    }

    async fn export_message_link(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        message_id: i32,
        grouped: bool,
        thread: bool,
    ) -> Result<MessageLinkSummary, TelegramError> {
        TelegramClient::export_message_link(
            self,
            session_path,
            chat_id,
            message_id,
            grouped,
            thread,
        )
        .await
    }

    async fn get_web_page_preview(
        &self,
        session_path: Option<PathBuf>,
        message: &str,
    ) -> Result<WebPagePreviewSummary, TelegramError> {
        TelegramClient::get_web_page_preview(self, session_path, message).await
    }

    async fn get_groups_for_discussion(
        &self,
        session_path: Option<PathBuf>,
    ) -> Result<Vec<ChatSummary>, TelegramError> {
        TelegramClient::get_groups_for_discussion(self, session_path).await
    }

    async fn list_contacts(
        &self,
        session_path: Option<PathBuf>,
        limit: usize,
    ) -> Result<Vec<ContactSummary>, TelegramError> {
        TelegramClient::list_contacts(self, session_path, limit).await
    }

    async fn search_contacts(
        &self,
        session_path: Option<PathBuf>,
        query: &str,
        limit: usize,
    ) -> Result<Vec<ContactSummary>, TelegramError> {
        TelegramClient::search_contacts(self, session_path, query, limit).await
    }

    async fn get_pinned_message(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
    ) -> Result<Option<MessageSummary>, TelegramError> {
        TelegramClient::get_pinned_message(self, session_path, chat_id).await
    }

    async fn get_scheduled_messages(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
    ) -> Result<Vec<MessageSummary>, TelegramError> {
        TelegramClient::get_scheduled_messages(self, session_path, chat_id).await
    }

    async fn get_poll(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        message_id: i32,
    ) -> Result<Option<PollSummary>, TelegramError> {
        TelegramClient::get_poll(self, session_path, chat_id, message_id).await
    }

    async fn get_poll_votes(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        message_id: i32,
        option_index: Option<usize>,
        limit: usize,
        offset: Option<&str>,
    ) -> Result<PollVoterPage, TelegramError> {
        TelegramClient::get_poll_votes(
            self,
            session_path,
            chat_id,
            message_id,
            option_index,
            limit,
            offset,
        )
        .await
    }

    async fn create_poll_with_random_id(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        poll: &PollCreateSpec,
        reply_to_message_id: Option<i32>,
        silent: bool,
        noforwards: bool,
        random_id: i64,
    ) -> Result<i32, TelegramError> {
        TelegramClient::create_poll_with_random_id(
            self,
            session_path,
            chat_id,
            poll,
            reply_to_message_id,
            silent,
            noforwards,
            random_id,
        )
        .await
    }

    async fn set_poll_vote(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        message_id: i32,
        options: &[Vec<u8>],
    ) -> Result<(), TelegramError> {
        TelegramClient::set_poll_vote(self, session_path, chat_id, message_id, options).await
    }

    async fn stop_poll(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        message_id: i32,
    ) -> Result<(), TelegramError> {
        TelegramClient::stop_poll(self, session_path, chat_id, message_id).await
    }

    async fn schedule_text_with_random_id(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        text: &str,
        parse_mode: CaptionParseMode,
        reply_to_message_id: Option<i32>,
        schedule_date_unix: i32,
        silent: bool,
        noforwards: bool,
        random_id: i64,
    ) -> Result<i32, TelegramError> {
        TelegramClient::schedule_text_with_random_id(
            self,
            session_path,
            chat_id,
            text,
            parse_mode,
            reply_to_message_id,
            schedule_date_unix,
            silent,
            noforwards,
            random_id,
        )
        .await
    }

    #[allow(clippy::too_many_arguments)]
    async fn schedule_file_with_random_id(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        file_path: &Path,
        caption: &str,
        caption_parse_mode: CaptionParseMode,
        reply_to_message_id: Option<i32>,
        schedule_date_unix: i32,
        silent: bool,
        noforwards: bool,
        mode: FileSendMode,
        random_id: i64,
    ) -> Result<i32, TelegramError> {
        TelegramClient::schedule_file_with_random_id(
            self,
            session_path,
            chat_id,
            file_path,
            caption,
            caption_parse_mode,
            reply_to_message_id,
            schedule_date_unix,
            silent,
            noforwards,
            mode,
            random_id,
        )
        .await
    }

    async fn edit_scheduled_message(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        message_id: i32,
        text: &str,
        parse_mode: CaptionParseMode,
        schedule_date_unix: i32,
    ) -> Result<(), TelegramError> {
        TelegramClient::edit_scheduled_message(
            self,
            session_path,
            chat_id,
            message_id,
            text,
            parse_mode,
            schedule_date_unix,
        )
        .await
    }

    async fn delete_scheduled_messages(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        message_ids: &[i32],
    ) -> Result<(), TelegramError> {
        TelegramClient::delete_scheduled_messages(self, session_path, chat_id, message_ids).await
    }

    async fn send_scheduled_messages_now(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        message_ids: &[i32],
    ) -> Result<Vec<i32>, TelegramError> {
        TelegramClient::send_scheduled_messages_now(self, session_path, chat_id, message_ids).await
    }

    async fn list_drafts(
        &self,
        session_path: Option<PathBuf>,
        limit: usize,
    ) -> Result<Vec<DialogDraftSummary>, TelegramError> {
        TelegramClient::list_drafts(self, session_path, limit).await
    }

    async fn get_draft(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
    ) -> Result<Option<DraftSummary>, TelegramError> {
        TelegramClient::get_draft(self, session_path, chat_id).await
    }

    async fn save_draft(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        text: Option<&str>,
    ) -> Result<(), TelegramError> {
        TelegramClient::save_draft(self, session_path, chat_id, text).await
    }

    async fn clear_all_drafts(&self, session_path: Option<PathBuf>) -> Result<(), TelegramError> {
        TelegramClient::clear_all_drafts(self, session_path).await
    }

    async fn get_dialog_state(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
    ) -> Result<DialogStateSnapshot, TelegramError> {
        TelegramClient::get_dialog_state(self, session_path, chat_id).await
    }

    async fn set_dialog_unread(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        unread: bool,
    ) -> Result<(), TelegramError> {
        TelegramClient::set_dialog_unread(self, session_path, chat_id, unread).await
    }

    async fn mark_messages_read(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        max_id: i32,
    ) -> Result<(), TelegramError> {
        TelegramClient::mark_messages_read(self, session_path, chat_id, max_id).await
    }

    async fn mark_reactions_read(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
    ) -> Result<MessageReactionReadReceipt, TelegramError> {
        TelegramClient::mark_reactions_read(self, session_path, chat_id).await
    }

    async fn set_dialog_archived(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        archived: bool,
    ) -> Result<(), TelegramError> {
        TelegramClient::set_dialog_archived(self, session_path, chat_id, archived).await
    }

    async fn set_dialog_pinned(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        pinned: bool,
    ) -> Result<(), TelegramError> {
        TelegramClient::set_dialog_pinned(self, session_path, chat_id, pinned).await
    }

    async fn list_dialog_folders(
        &self,
        session_path: Option<PathBuf>,
    ) -> Result<DialogFolderList, TelegramError> {
        TelegramClient::list_dialog_folders(self, session_path).await
    }

    async fn get_suggested_dialog_folders(
        &self,
        session_path: Option<PathBuf>,
    ) -> Result<SuggestedDialogFolderList, TelegramError> {
        TelegramClient::get_suggested_dialog_folders(self, session_path).await
    }

    async fn set_dialog_folder_tags(
        &self,
        session_path: Option<PathBuf>,
        enabled: bool,
    ) -> Result<(), TelegramError> {
        TelegramClient::set_dialog_folder_tags(self, session_path, enabled).await
    }

    async fn get_global_privacy_settings(
        &self,
        session_path: Option<PathBuf>,
    ) -> Result<GlobalPrivacySettingsSummary, TelegramError> {
        TelegramClient::get_global_privacy_settings(self, session_path).await
    }

    async fn set_global_privacy_settings(
        &self,
        session_path: Option<PathBuf>,
        settings: &GlobalPrivacySettingsSummary,
    ) -> Result<(), TelegramError> {
        TelegramClient::set_global_privacy_settings(self, session_path, settings).await
    }

    async fn upsert_dialog_folder(
        &self,
        session_path: Option<PathBuf>,
        folder: &DialogFolderSummary,
        preserve_title_entities: bool,
    ) -> Result<(), TelegramError> {
        TelegramClient::upsert_dialog_folder(self, session_path, folder, preserve_title_entities)
            .await
    }

    async fn delete_dialog_folder(
        &self,
        session_path: Option<PathBuf>,
        folder_id: i32,
    ) -> Result<(), TelegramError> {
        TelegramClient::delete_dialog_folder(self, session_path, folder_id).await
    }

    async fn reorder_dialog_folders(
        &self,
        session_path: Option<PathBuf>,
        folder_ids: &[i32],
    ) -> Result<(), TelegramError> {
        TelegramClient::reorder_dialog_folders(self, session_path, folder_ids).await
    }

    async fn set_dialog_folder_membership(
        &self,
        session_path: Option<PathBuf>,
        folder_id: i32,
        chat_id: &str,
        included: bool,
    ) -> Result<(), TelegramError> {
        TelegramClient::set_dialog_folder_membership(
            self,
            session_path,
            folder_id,
            chat_id,
            included,
        )
        .await
    }

    async fn set_user_blocked(
        &self,
        session_path: Option<PathBuf>,
        user_ref: &str,
        blocked: bool,
    ) -> Result<(), TelegramError> {
        TelegramClient::set_user_blocked(self, session_path, user_ref, blocked).await
    }

    async fn get_replies(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        message_id: i32,
        limit: usize,
        offset_id: Option<i32>,
    ) -> Result<MessagePage, TelegramError> {
        TelegramClient::get_replies(self, session_path, chat_id, message_id, limit, offset_id).await
    }

    async fn get_message_read_receipts(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        message_id: i32,
    ) -> Result<Vec<MessageReadReceipt>, TelegramError> {
        TelegramClient::get_message_read_receipts(self, session_path, chat_id, message_id).await
    }

    async fn get_outbox_read_date(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        message_id: i32,
    ) -> Result<OutboxReadDateSummary, TelegramError> {
        TelegramClient::get_outbox_read_date(self, session_path, chat_id, message_id).await
    }

    async fn get_user_info(
        &self,
        session_path: Option<PathBuf>,
        user_ref: &str,
    ) -> Result<UserDetails, TelegramError> {
        TelegramClient::get_user_info(self, session_path, user_ref).await
    }

    async fn list_participants(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        limit: usize,
    ) -> Result<Vec<ParticipantSummary>, TelegramError> {
        TelegramClient::list_participants(self, session_path, chat_id, limit).await
    }

    async fn list_chat_members(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        filter: ParticipantFilter,
        query: Option<&str>,
        offset: usize,
        limit: usize,
    ) -> Result<ParticipantPage, TelegramError> {
        TelegramClient::list_chat_members(self, session_path, chat_id, filter, query, offset, limit)
            .await
    }

    async fn get_chat_member(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        user_ref: &str,
    ) -> Result<Option<ChatMemberSummary>, TelegramError> {
        TelegramClient::get_chat_member(self, session_path, chat_id, user_ref).await
    }

    async fn add_chat_member(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        user_ref: &str,
    ) -> Result<(), TelegramError> {
        TelegramClient::add_chat_member(self, session_path, chat_id, user_ref).await
    }

    async fn remove_chat_member(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        user_ref: &str,
    ) -> Result<(), TelegramError> {
        TelegramClient::remove_chat_member(self, session_path, chat_id, user_ref).await
    }

    async fn set_chat_member_admin(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        user_ref: &str,
        rights: &ChatAdminRightsSummary,
        rank: Option<&str>,
    ) -> Result<(), TelegramError> {
        TelegramClient::set_chat_member_admin(self, session_path, chat_id, user_ref, rights, rank)
            .await
    }

    async fn set_chat_member_restrictions(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        user_ref: &str,
        rights: &ChatBannedRightsSummary,
    ) -> Result<(), TelegramError> {
        TelegramClient::set_chat_member_restrictions(self, session_path, chat_id, user_ref, rights)
            .await
    }

    async fn list_invite_links(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        admin_user_id: i64,
        revoked: bool,
        limit: usize,
    ) -> Result<Vec<InviteLinkSummary>, TelegramError> {
        TelegramClient::list_invite_links(
            self,
            session_path,
            chat_id,
            admin_user_id,
            revoked,
            limit,
        )
        .await
    }

    async fn get_invite_link(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        admin_user_id: i64,
        invite_handle: &str,
    ) -> Result<Option<InviteLinkSummary>, TelegramError> {
        TelegramClient::get_invite_link(self, session_path, chat_id, admin_user_id, invite_handle)
            .await
    }

    async fn export_invite_link(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        expire_date_unix: Option<i32>,
        usage_limit: Option<i32>,
        request_needed: bool,
        title: Option<&str>,
    ) -> Result<InviteLinkSummary, TelegramError> {
        TelegramClient::export_invite_link(
            self,
            session_path,
            chat_id,
            expire_date_unix,
            usage_limit,
            request_needed,
            title,
        )
        .await
    }

    async fn revoke_invite_link(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        admin_user_id: i64,
        invite_handle: &str,
    ) -> Result<InviteLinkSummary, TelegramError> {
        TelegramClient::revoke_invite_link(
            self,
            session_path,
            chat_id,
            admin_user_id,
            invite_handle,
        )
        .await
    }

    async fn delete_revoked_invite_link(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        admin_user_id: i64,
        invite_handle: &str,
    ) -> Result<(), TelegramError> {
        TelegramClient::delete_revoked_invite_link(
            self,
            session_path,
            chat_id,
            admin_user_id,
            invite_handle,
        )
        .await
    }

    async fn list_invite_joiners(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        admin_user_id: i64,
        invite_handle: Option<&str>,
        requested: bool,
        limit: usize,
        offset_date_unix: i32,
        offset_user_id: i64,
    ) -> Result<InviteJoinerPage, TelegramError> {
        TelegramClient::list_invite_joiners(
            self,
            session_path,
            chat_id,
            admin_user_id,
            invite_handle,
            requested,
            limit,
            offset_date_unix,
            offset_user_id,
        )
        .await
    }

    async fn process_join_request(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        user_id: i64,
        approve: bool,
    ) -> Result<(), TelegramError> {
        TelegramClient::process_join_request(self, session_path, chat_id, user_id, approve).await
    }

    async fn get_account_profile(
        &self,
        session_path: Option<PathBuf>,
    ) -> Result<AccountProfileSummary, TelegramError> {
        TelegramClient::get_account_profile(self, session_path).await
    }

    async fn get_bot_commands(
        &self,
        session_path: Option<PathBuf>,
        language_code: &str,
    ) -> Result<Vec<BotCommandSummary>, TelegramError> {
        TelegramClient::get_bot_commands(self, session_path, language_code).await
    }

    async fn set_bot_commands(
        &self,
        session_path: Option<PathBuf>,
        language_code: &str,
        commands: &[BotCommandSummary],
    ) -> Result<(), TelegramError> {
        TelegramClient::set_bot_commands(self, session_path, language_code, commands).await
    }

    async fn get_account_emoji_status(
        &self,
        session_path: Option<PathBuf>,
    ) -> Result<EmojiStatusSummary, TelegramError> {
        TelegramClient::get_account_emoji_status(self, session_path).await
    }

    async fn list_emoji_statuses(
        &self,
        session_path: Option<PathBuf>,
        kind: EmojiStatusCatalogKind,
        hash: i64,
    ) -> Result<EmojiStatusCatalogPage, TelegramError> {
        TelegramClient::list_emoji_statuses(self, session_path, kind, hash).await
    }

    async fn set_account_emoji_status(
        &self,
        session_path: Option<PathBuf>,
        status: &AccountEmojiStatusSpec,
    ) -> Result<(), TelegramError> {
        TelegramClient::set_account_emoji_status(self, session_path, status).await
    }

    async fn clear_recent_emoji_statuses(
        &self,
        session_path: Option<PathBuf>,
    ) -> Result<(), TelegramError> {
        TelegramClient::clear_recent_emoji_statuses(self, session_path).await
    }

    async fn update_account_profile(
        &self,
        session_path: Option<PathBuf>,
        first_name: Option<&str>,
        last_name: Option<&str>,
        about: Option<&str>,
    ) -> Result<(), TelegramError> {
        TelegramClient::update_account_profile(self, session_path, first_name, last_name, about)
            .await
    }

    async fn update_account_username(
        &self,
        session_path: Option<PathBuf>,
        username: Option<&str>,
    ) -> Result<(), TelegramError> {
        TelegramClient::update_account_username(self, session_path, username).await
    }

    async fn update_account_birthday(
        &self,
        session_path: Option<PathBuf>,
        birthday: Option<&BirthdaySummary>,
    ) -> Result<(), TelegramError> {
        TelegramClient::update_account_birthday(self, session_path, birthday).await
    }

    async fn update_account_color(
        &self,
        session_path: Option<PathBuf>,
        for_profile: bool,
        color: Option<i32>,
        background_emoji_id: Option<&str>,
    ) -> Result<(), TelegramError> {
        TelegramClient::update_account_color(
            self,
            session_path,
            for_profile,
            color,
            background_emoji_id,
        )
        .await
    }

    async fn update_personal_channel(
        &self,
        session_path: Option<PathBuf>,
        channel_id: Option<&str>,
    ) -> Result<(), TelegramError> {
        TelegramClient::update_personal_channel(self, session_path, channel_id).await
    }

    async fn set_account_presence(
        &self,
        session_path: Option<PathBuf>,
        online: bool,
    ) -> Result<(), TelegramError> {
        TelegramClient::set_account_presence(self, session_path, online).await
    }

    async fn list_profile_photos(
        &self,
        session_path: Option<PathBuf>,
        offset: usize,
        limit: usize,
    ) -> Result<ProfilePhotoPage, TelegramError> {
        TelegramClient::list_profile_photos(self, session_path, offset, limit).await
    }

    async fn get_profile_photo(
        &self,
        session_path: Option<PathBuf>,
        photo_handle: &str,
    ) -> Result<Option<ProfilePhotoSummary>, TelegramError> {
        TelegramClient::get_profile_photo(self, session_path, photo_handle).await
    }

    async fn upload_profile_photo(
        &self,
        session_path: Option<PathBuf>,
        file_path: &Path,
        fallback: bool,
    ) -> Result<ProfilePhotoSummary, TelegramError> {
        TelegramClient::upload_profile_photo(self, session_path, file_path, fallback).await
    }

    async fn delete_profile_photos(
        &self,
        session_path: Option<PathBuf>,
        photo_handles: &[String],
    ) -> Result<(), TelegramError> {
        TelegramClient::delete_profile_photos(self, session_path, photo_handles).await
    }

    async fn get_sessions(
        &self,
        session_path: Option<PathBuf>,
    ) -> Result<Vec<SessionSummary>, TelegramError> {
        TelegramClient::get_sessions(self, session_path).await
    }

    async fn terminate_session(
        &self,
        session_path: Option<PathBuf>,
        authorization_id: i64,
    ) -> Result<(), TelegramError> {
        TelegramClient::terminate_session(self, session_path, authorization_id).await
    }

    async fn terminate_all_other_sessions(
        &self,
        session_path: Option<PathBuf>,
    ) -> Result<(), TelegramError> {
        TelegramClient::terminate_all_other_sessions(self, session_path).await
    }

    async fn get_chat_auto_delete(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
    ) -> Result<ChatAutoDeleteSummary, TelegramError> {
        TelegramClient::get_chat_auto_delete(self, session_path, chat_id).await
    }

    async fn set_chat_auto_delete(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        period_seconds: i32,
    ) -> Result<(), TelegramError> {
        TelegramClient::set_chat_auto_delete(self, session_path, chat_id, period_seconds).await
    }

    async fn set_contact(
        &self,
        session_path: Option<PathBuf>,
        user_id: i64,
        contact: bool,
        first_name: &str,
        last_name: &str,
        phone: &str,
        add_phone_privacy_exception: bool,
    ) -> Result<(), TelegramError> {
        TelegramClient::set_contact(
            self,
            session_path,
            user_id,
            contact,
            first_name,
            last_name,
            phone,
            add_phone_privacy_exception,
        )
        .await
    }

    async fn get_common_chats(
        &self,
        session_path: Option<PathBuf>,
        user_ref: &str,
        max_id: i64,
        limit: usize,
    ) -> Result<Vec<ChatSummary>, TelegramError> {
        TelegramClient::get_common_chats(self, session_path, user_ref, max_id, limit).await
    }

    async fn get_chat_lifecycle_state(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
    ) -> Result<ChatLifecycleState, TelegramError> {
        TelegramClient::get_chat_lifecycle_state(self, session_path, chat_id).await
    }

    async fn create_group(
        &self,
        session_path: Option<PathBuf>,
        title: &str,
        user_ids: &[i64],
    ) -> Result<ChatDetailsSummary, TelegramError> {
        TelegramClient::create_group(self, session_path, title, user_ids).await
    }

    async fn create_channel(
        &self,
        session_path: Option<PathBuf>,
        title: &str,
        about: &str,
        broadcast: bool,
    ) -> Result<ChatDetailsSummary, TelegramError> {
        TelegramClient::create_channel(self, session_path, title, about, broadcast).await
    }

    async fn join_chat(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
    ) -> Result<ChatLifecycleState, TelegramError> {
        TelegramClient::join_chat(self, session_path, chat_id).await
    }

    async fn leave_chat(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
    ) -> Result<ChatLifecycleState, TelegramError> {
        TelegramClient::leave_chat(self, session_path, chat_id).await
    }

    async fn delete_chat(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
    ) -> Result<ChatLifecycleState, TelegramError> {
        TelegramClient::delete_chat(self, session_path, chat_id).await
    }

    async fn update_chat_title(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        title: &str,
    ) -> Result<ChatDetailsSummary, TelegramError> {
        TelegramClient::update_chat_title(self, session_path, chat_id, title).await
    }

    async fn update_chat_about(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        about: &str,
    ) -> Result<ChatDetailsSummary, TelegramError> {
        TelegramClient::update_chat_about(self, session_path, chat_id, about).await
    }

    async fn update_chat_photo(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        file_path: &Path,
    ) -> Result<ChatDetailsSummary, TelegramError> {
        TelegramClient::update_chat_photo(self, session_path, chat_id, file_path).await
    }

    async fn delete_chat_photo(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
    ) -> Result<ChatDetailsSummary, TelegramError> {
        TelegramClient::delete_chat_photo(self, session_path, chat_id).await
    }

    async fn get_channel_settings(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
    ) -> Result<ChannelSettingsSummary, TelegramError> {
        TelegramClient::get_channel_settings(self, session_path, chat_id).await
    }

    async fn get_chat_moderation_settings(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
    ) -> Result<ChatModerationSettingsSummary, TelegramError> {
        TelegramClient::get_chat_moderation_settings(self, session_path, chat_id).await
    }

    async fn get_channel_stats(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
    ) -> Result<ChannelStatsSummary, TelegramError> {
        TelegramClient::get_channel_stats(self, session_path, chat_id).await
    }

    async fn get_admin_log(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        query: &str,
        events: Option<AdminLogEventFilter>,
        admin_refs: &[String],
        max_id: i64,
        min_id: i64,
        limit: usize,
    ) -> Result<AdminLogPage, TelegramError> {
        TelegramClient::get_admin_log(
            self,
            session_path,
            chat_id,
            query,
            events,
            admin_refs,
            max_id,
            min_id,
            limit,
        )
        .await
    }

    async fn get_my_boosts(
        &self,
        session_path: Option<PathBuf>,
    ) -> Result<MyBoostsSummary, TelegramError> {
        TelegramClient::get_my_boosts(self, session_path).await
    }

    async fn get_boost_status(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
    ) -> Result<BoostStatusSummary, TelegramError> {
        TelegramClient::get_boost_status(self, session_path, chat_id).await
    }

    async fn list_boosts(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        gifts_only: bool,
        offset: &str,
        limit: usize,
    ) -> Result<BoostPage, TelegramError> {
        TelegramClient::list_boosts(self, session_path, chat_id, gifts_only, offset, limit).await
    }

    async fn apply_boosts(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        slots: &[i32],
    ) -> Result<MyBoostsSummary, TelegramError> {
        TelegramClient::apply_boosts(self, session_path, chat_id, slots).await
    }

    async fn check_chat_username(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        username: &str,
    ) -> Result<bool, TelegramError> {
        TelegramClient::check_chat_username(self, session_path, chat_id, username).await
    }

    async fn update_chat_username(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        username: Option<&str>,
    ) -> Result<ChannelSettingsSummary, TelegramError> {
        TelegramClient::update_chat_username(self, session_path, chat_id, username).await
    }

    async fn set_chat_slow_mode(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        seconds: i32,
    ) -> Result<ChannelSettingsSummary, TelegramError> {
        TelegramClient::set_chat_slow_mode(self, session_path, chat_id, seconds).await
    }

    async fn toggle_chat_signatures(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        signatures_enabled: bool,
        profiles_enabled: bool,
    ) -> Result<ChannelSettingsSummary, TelegramError> {
        TelegramClient::toggle_chat_signatures(
            self,
            session_path,
            chat_id,
            signatures_enabled,
            profiles_enabled,
        )
        .await
    }

    async fn toggle_chat_pre_history_hidden(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        enabled: bool,
    ) -> Result<ChannelSettingsSummary, TelegramError> {
        TelegramClient::toggle_chat_pre_history_hidden(self, session_path, chat_id, enabled).await
    }

    async fn toggle_chat_join_request(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        enabled: bool,
    ) -> Result<ChannelSettingsSummary, TelegramError> {
        TelegramClient::toggle_chat_join_request(self, session_path, chat_id, enabled).await
    }

    async fn set_chat_default_permissions(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        target: &ChatBannedRightsSummary,
    ) -> Result<ChatModerationSettingsSummary, TelegramError> {
        TelegramClient::set_chat_default_permissions(self, session_path, chat_id, target).await
    }

    async fn set_chat_available_reactions(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        target: &ChatReactionsPolicy,
    ) -> Result<ChatModerationSettingsSummary, TelegramError> {
        TelegramClient::set_chat_available_reactions(self, session_path, chat_id, target).await
    }

    async fn toggle_chat_anti_spam(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        enabled: bool,
    ) -> Result<ChatModerationSettingsSummary, TelegramError> {
        TelegramClient::toggle_chat_anti_spam(self, session_path, chat_id, enabled).await
    }

    async fn toggle_chat_forum_mode(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        enabled: bool,
        tabs_enabled: bool,
    ) -> Result<ChatModerationSettingsSummary, TelegramError> {
        TelegramClient::toggle_chat_forum_mode(self, session_path, chat_id, enabled, tabs_enabled)
            .await
    }

    async fn list_stories(
        &self,
        session_path: Option<PathBuf>,
        next: bool,
        hidden: bool,
        state: Option<&str>,
    ) -> Result<StoryListPage, TelegramError> {
        TelegramClient::list_stories(self, session_path, next, hidden, state).await
    }

    async fn get_peer_stories(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
    ) -> Result<StoryPeerPage, TelegramError> {
        TelegramClient::get_peer_stories(self, session_path, chat_id).await
    }

    async fn get_stories_by_ids(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        story_ids: &[i32],
    ) -> Result<StoryCollectionPage, TelegramError> {
        TelegramClient::get_stories_by_ids(self, session_path, chat_id, story_ids).await
    }

    async fn get_stories_archive(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        offset_id: i32,
        limit: usize,
    ) -> Result<StoryCollectionPage, TelegramError> {
        TelegramClient::get_stories_archive(self, session_path, chat_id, offset_id, limit).await
    }

    async fn export_story_link(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        story_id: i32,
    ) -> Result<StoryLinkSummary, TelegramError> {
        TelegramClient::export_story_link(self, session_path, chat_id, story_id).await
    }

    #[allow(clippy::too_many_arguments)]
    async fn get_story_viewers(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        story_id: i32,
        offset: &str,
        limit: usize,
        just_contacts: bool,
        reactions_first: bool,
        forwards_first: bool,
        query: Option<&str>,
    ) -> Result<StoryViewersPage, TelegramError> {
        TelegramClient::get_story_viewers(
            self,
            session_path,
            chat_id,
            story_id,
            offset,
            limit,
            just_contacts,
            reactions_first,
            forwards_first,
            query,
        )
        .await
    }

    async fn view_story(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        story_ids: &[i32],
    ) -> Result<(), TelegramError> {
        TelegramClient::view_story(self, session_path, chat_id, story_ids).await
    }

    async fn read_stories(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        max_id: i32,
    ) -> Result<Vec<i32>, TelegramError> {
        TelegramClient::read_stories(self, session_path, chat_id, max_id).await
    }

    async fn set_story_reaction(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        story_id: i32,
        reaction: &ReactionSpec,
    ) -> Result<(), TelegramError> {
        TelegramClient::set_story_reaction(self, session_path, chat_id, story_id, reaction).await
    }

    async fn set_stories_pinned(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        story_ids: &[i32],
        pinned: bool,
    ) -> Result<Vec<i32>, TelegramError> {
        TelegramClient::set_stories_pinned(self, session_path, chat_id, story_ids, pinned).await
    }

    async fn toggle_stories_pinned_to_top(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        story_ids: &[i32],
    ) -> Result<bool, TelegramError> {
        TelegramClient::toggle_stories_pinned_to_top(self, session_path, chat_id, story_ids).await
    }

    async fn activate_story_stealth_mode(
        &self,
        session_path: Option<PathBuf>,
        past: bool,
        future: bool,
    ) -> Result<(), TelegramError> {
        TelegramClient::activate_story_stealth_mode(self, session_path, past, future).await
    }

    #[allow(clippy::too_many_arguments)]
    async fn send_story_with_random_id(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        file_path: &Path,
        caption: &str,
        privacy: &StoryPrivacySpec,
        pinned: bool,
        noforwards: bool,
        period_seconds: Option<i32>,
        random_id: i64,
    ) -> Result<i32, TelegramError> {
        TelegramClient::send_story_with_random_id(
            self,
            session_path,
            chat_id,
            file_path,
            caption,
            privacy,
            pinned,
            noforwards,
            period_seconds,
            random_id,
        )
        .await
    }

    #[allow(clippy::too_many_arguments)]
    async fn edit_story(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        story_id: i32,
        file_path: Option<&Path>,
        caption: Option<&str>,
        privacy: Option<&StoryPrivacySpec>,
    ) -> Result<(), TelegramError> {
        TelegramClient::edit_story(
            self,
            session_path,
            chat_id,
            story_id,
            file_path,
            caption,
            privacy,
        )
        .await
    }

    async fn report_stories(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        story_ids: &[i32],
        option_hex: &str,
        message: &str,
    ) -> Result<StoryReportResult, TelegramError> {
        TelegramClient::report_stories(self, session_path, chat_id, story_ids, option_hex, message)
            .await
    }

    async fn delete_stories(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        story_ids: &[i32],
    ) -> Result<Vec<i32>, TelegramError> {
        TelegramClient::delete_stories(self, session_path, chat_id, story_ids).await
    }

    async fn get_story(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        story_id: i32,
    ) -> Result<Option<StorySummary>, TelegramError> {
        TelegramClient::get_story(self, session_path, chat_id, story_id).await
    }

    async fn get_chat_administrators(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
    ) -> Result<Vec<ParticipantSummary>, TelegramError> {
        TelegramClient::get_chat_administrators(self, session_path, chat_id).await
    }

    async fn get_online_count(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
    ) -> Result<i32, TelegramError> {
        TelegramClient::get_online_count(self, session_path, chat_id).await
    }

    async fn get_blocked_users(
        &self,
        session_path: Option<PathBuf>,
        offset: usize,
        limit: usize,
    ) -> Result<Vec<ContactSummary>, TelegramError> {
        TelegramClient::get_blocked_users(self, session_path, offset, limit).await
    }

    async fn list_forum_topics(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        query: Option<&str>,
        limit: usize,
        offset_date: i32,
        offset_id: i32,
        offset_topic: i32,
    ) -> Result<Vec<ForumTopicSummary>, TelegramError> {
        TelegramClient::list_forum_topics(
            self,
            session_path,
            chat_id,
            query,
            limit,
            offset_date,
            offset_id,
            offset_topic,
        )
        .await
    }

    async fn get_forum_topic(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        topic_id: i32,
    ) -> Result<Option<ForumTopicSummary>, TelegramError> {
        TelegramClient::get_forum_topic(self, session_path, chat_id, topic_id).await
    }

    async fn create_forum_topic_with_random_id(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        title: &str,
        icon_color: Option<i32>,
        icon_emoji_id: Option<i64>,
        random_id: i64,
    ) -> Result<i32, TelegramError> {
        TelegramClient::create_forum_topic_with_random_id(
            self,
            session_path,
            chat_id,
            title,
            icon_color,
            icon_emoji_id,
            random_id,
        )
        .await
    }

    async fn edit_forum_topic(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        topic_id: i32,
        title: Option<&str>,
        icon_emoji_id: Option<i64>,
        closed: Option<bool>,
        hidden: Option<bool>,
    ) -> Result<(), TelegramError> {
        TelegramClient::edit_forum_topic(
            self,
            session_path,
            chat_id,
            topic_id,
            title,
            icon_emoji_id,
            closed,
            hidden,
        )
        .await
    }

    async fn set_forum_topic_pinned(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        topic_id: i32,
        pinned: bool,
    ) -> Result<(), TelegramError> {
        TelegramClient::set_forum_topic_pinned(self, session_path, chat_id, topic_id, pinned).await
    }

    async fn delete_forum_topic(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        topic_id: i32,
    ) -> Result<(), TelegramError> {
        TelegramClient::delete_forum_topic(self, session_path, chat_id, topic_id).await
    }

    async fn get_privacy_rules(
        &self,
        session_path: Option<PathBuf>,
        key: PrivacyKey,
    ) -> Result<Vec<PrivacyRuleSummary>, TelegramError> {
        TelegramClient::get_privacy_rules(self, session_path, key).await
    }

    async fn set_privacy_rules(
        &self,
        session_path: Option<PathBuf>,
        key: PrivacyKey,
        rules: &[PrivacyRuleSummary],
    ) -> Result<Vec<PrivacyRuleSummary>, TelegramError> {
        TelegramClient::set_privacy_rules(self, session_path, key, rules).await
    }

    async fn get_notification_settings(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
    ) -> Result<NotificationSettingsSummary, TelegramError> {
        TelegramClient::get_notification_settings(self, session_path, chat_id).await
    }

    async fn set_notification_settings(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        patch: &NotificationSettingsPatch,
    ) -> Result<(), TelegramError> {
        TelegramClient::set_notification_settings(self, session_path, chat_id, patch).await
    }

    async fn list_sticker_sets(
        &self,
        session_path: Option<PathBuf>,
        hash: i64,
    ) -> Result<Option<Vec<StickerSetSummary>>, TelegramError> {
        TelegramClient::list_sticker_sets(self, session_path, hash).await
    }

    async fn get_recent_stickers(
        &self,
        session_path: Option<PathBuf>,
        attached: bool,
        limit: usize,
        hash: i64,
    ) -> Result<StickerCatalogPage, TelegramError> {
        TelegramClient::get_recent_stickers(self, session_path, attached, limit, hash).await
    }

    async fn get_sticker_set(
        &self,
        session_path: Option<PathBuf>,
        short_name: &str,
        hash: i32,
    ) -> Result<StickerSetDetailsPage, TelegramError> {
        TelegramClient::get_sticker_set(self, session_path, short_name, hash).await
    }

    async fn search_sticker_sets(
        &self,
        session_path: Option<PathBuf>,
        query: &str,
        exclude_featured: bool,
        limit: usize,
        hash: i64,
    ) -> Result<StickerSetSearchPage, TelegramError> {
        TelegramClient::search_sticker_sets(
            self,
            session_path,
            query,
            exclude_featured,
            limit,
            hash,
        )
        .await
    }

    async fn get_favorite_stickers(
        &self,
        session_path: Option<PathBuf>,
        limit: usize,
        hash: i64,
    ) -> Result<StickerCatalogPage, TelegramError> {
        TelegramClient::get_favorite_stickers(self, session_path, limit, hash).await
    }

    async fn set_sticker_favorite(
        &self,
        session_path: Option<PathBuf>,
        set_short_name: &str,
        sticker_handle: &str,
        favorite: bool,
    ) -> Result<(), TelegramError> {
        TelegramClient::set_sticker_favorite(
            self,
            session_path,
            set_short_name,
            sticker_handle,
            favorite,
        )
        .await
    }

    async fn set_recent_sticker(
        &self,
        session_path: Option<PathBuf>,
        set_short_name: &str,
        sticker_handle: &str,
        attached: bool,
        saved: bool,
    ) -> Result<(), TelegramError> {
        TelegramClient::set_recent_sticker(
            self,
            session_path,
            set_short_name,
            sticker_handle,
            attached,
            saved,
        )
        .await
    }

    async fn install_sticker_set(
        &self,
        session_path: Option<PathBuf>,
        short_name: &str,
        archived: bool,
    ) -> Result<(), TelegramError> {
        TelegramClient::install_sticker_set(self, session_path, short_name, archived).await
    }

    async fn uninstall_sticker_set(
        &self,
        session_path: Option<PathBuf>,
        short_name: &str,
    ) -> Result<(), TelegramError> {
        TelegramClient::uninstall_sticker_set(self, session_path, short_name).await
    }

    async fn get_custom_emoji_documents(
        &self,
        session_path: Option<PathBuf>,
        document_ids: &[i64],
    ) -> Result<Vec<CustomEmojiDocumentSummary>, TelegramError> {
        TelegramClient::get_custom_emoji_documents(self, session_path, document_ids).await
    }

    async fn download_custom_emoji_to(
        &self,
        session_path: Option<PathBuf>,
        document_id: i64,
        destination: &Path,
        max_bytes: u64,
    ) -> Result<u64, TelegramError> {
        TelegramClient::download_custom_emoji_to(
            self,
            session_path,
            document_id,
            destination,
            max_bytes,
        )
        .await
    }

    async fn get_dice_emojis(
        &self,
        session_path: Option<PathBuf>,
        hash: i32,
    ) -> Result<Option<DiceEmojiCatalog>, TelegramError> {
        TelegramClient::get_dice_emojis(self, session_path, hash).await
    }

    async fn send_rich_media_with_random_id(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        media: &RichMediaSpec,
        caption: &str,
        reply_to_message_id: Option<i32>,
        topic_id: Option<i32>,
        silent: bool,
        noforwards: bool,
        random_id: i64,
    ) -> Result<i32, TelegramError> {
        TelegramClient::send_rich_media_with_random_id(
            self,
            session_path,
            chat_id,
            media,
            caption,
            reply_to_message_id,
            topic_id,
            silent,
            noforwards,
            random_id,
        )
        .await
    }

    async fn get_message_rich_media(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        message_id: i32,
    ) -> Result<Option<SentRichMediaSummary>, TelegramError> {
        TelegramClient::get_message_rich_media(self, session_path, chat_id, message_id).await
    }

    async fn list_message_reactions(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        message_id: i32,
        limit: usize,
        offset: Option<&str>,
    ) -> Result<MessageReactionPage, TelegramError> {
        TelegramClient::list_message_reactions(
            self,
            session_path,
            chat_id,
            message_id,
            limit,
            offset,
        )
        .await
    }

    async fn get_default_reaction(
        &self,
        session_path: Option<PathBuf>,
    ) -> Result<DefaultReactionSummary, TelegramError> {
        TelegramClient::get_default_reaction(self, session_path).await
    }

    async fn get_top_reactions(
        &self,
        session_path: Option<PathBuf>,
        limit: usize,
        hash: i64,
    ) -> Result<ReactionCatalogPage, TelegramError> {
        TelegramClient::get_top_reactions(self, session_path, limit, hash).await
    }

    async fn get_recent_reactions(
        &self,
        session_path: Option<PathBuf>,
        limit: usize,
        hash: i64,
    ) -> Result<ReactionCatalogPage, TelegramError> {
        TelegramClient::get_recent_reactions(self, session_path, limit, hash).await
    }

    async fn set_default_reaction(
        &self,
        session_path: Option<PathBuf>,
        reaction: &DefaultReactionSpec,
    ) -> Result<(), TelegramError> {
        TelegramClient::set_default_reaction(self, session_path, reaction).await
    }

    async fn get_my_reaction(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        message_id: i32,
    ) -> Result<Option<ReactionSpec>, TelegramError> {
        TelegramClient::get_my_reaction(self, session_path, chat_id, message_id).await
    }

    async fn set_message_reaction(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        message_id: i32,
        reaction: &ReactionSpec,
    ) -> Result<(), TelegramError> {
        TelegramClient::set_message_reaction(self, session_path, chat_id, message_id, reaction)
            .await
    }

    async fn edit_message(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        message_id: i32,
        text: &str,
        parse_mode: CaptionParseMode,
    ) -> Result<i32, TelegramError> {
        TelegramClient::edit_message(self, session_path, chat_id, message_id, text, parse_mode)
            .await
    }

    async fn delete_messages(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        message_ids: &[i32],
        revoke: bool,
    ) -> Result<(), TelegramError> {
        TelegramClient::delete_messages(self, session_path, chat_id, message_ids, revoke).await
    }

    async fn set_message_pinned(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        message_id: i32,
        pinned: bool,
        silent: bool,
        one_sided: bool,
    ) -> Result<(), TelegramError> {
        TelegramClient::set_message_pinned(
            self,
            session_path,
            chat_id,
            message_id,
            pinned,
            silent,
            one_sided,
        )
        .await
    }

    async fn forward_messages_with_random_ids(
        &self,
        session_path: Option<PathBuf>,
        source_chat_id: &str,
        message_ids: &[i32],
        destination_chat_id: &str,
        silent: bool,
        drop_author: bool,
        drop_media_captions: bool,
        noforwards: bool,
        random_ids: &[i64],
    ) -> Result<Vec<i32>, TelegramError> {
        TelegramClient::forward_messages_with_random_ids(
            self,
            session_path,
            source_chat_id,
            message_ids,
            destination_chat_id,
            silent,
            drop_author,
            drop_media_captions,
            noforwards,
            random_ids,
        )
        .await
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

    async fn send_file_with_random_id(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        file_path: &Path,
        caption: &str,
        reply_to_message_id: Option<i32>,
        mode: FileSendMode,
        random_id: i64,
    ) -> Result<i32, TelegramError> {
        TelegramClient::send_file_with_random_id(
            self,
            session_path,
            chat_id,
            file_path,
            caption,
            reply_to_message_id,
            mode,
            random_id,
        )
        .await
    }

    async fn send_file_html_with_random_id(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        file_path: &Path,
        caption_html: &str,
        reply_to_message_id: Option<i32>,
        topic_id: Option<i32>,
        silent: bool,
        noforwards: bool,
        mode: FileSendMode,
        random_id: i64,
    ) -> Result<i32, TelegramError> {
        TelegramClient::send_file_html_with_random_id(
            self,
            session_path,
            chat_id,
            file_path,
            caption_html,
            reply_to_message_id,
            topic_id,
            silent,
            noforwards,
            mode,
            random_id,
        )
        .await
    }

    async fn send_voice_with_random_id(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        file_path: &Path,
        caption: &str,
        caption_parse_mode: CaptionParseMode,
        reply_to_message_id: Option<i32>,
        topic_id: Option<i32>,
        silent: bool,
        noforwards: bool,
        random_id: i64,
    ) -> Result<i32, TelegramError> {
        TelegramClient::send_voice_with_random_id(
            self,
            session_path,
            chat_id,
            file_path,
            caption,
            caption_parse_mode,
            reply_to_message_id,
            topic_id,
            silent,
            noforwards,
            random_id,
        )
        .await
    }

    async fn send_video_note_with_random_id(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        file_path: &Path,
        duration_seconds: i32,
        length_pixels: i32,
        reply_to_message_id: Option<i32>,
        topic_id: Option<i32>,
        silent: bool,
        noforwards: bool,
        random_id: i64,
    ) -> Result<i32, TelegramError> {
        TelegramClient::send_video_note_with_random_id(
            self,
            session_path,
            chat_id,
            file_path,
            duration_seconds,
            length_pixels,
            reply_to_message_id,
            topic_id,
            silent,
            noforwards,
            random_id,
        )
        .await
    }

    async fn get_message_document_media(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        message_id: i32,
    ) -> Result<Option<SentDocumentMediaSummary>, TelegramError> {
        TelegramClient::get_message_document_media(self, session_path, chat_id, message_id).await
    }

    async fn get_message_topic_id(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        message_id: i32,
    ) -> Result<Option<i32>, TelegramError> {
        TelegramClient::get_message_topic_id(self, session_path, chat_id, message_id).await
    }

    async fn send_album_with_random_ids(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        items: &[AlbumUploadSpec],
        caption_parse_mode: CaptionParseMode,
        reply_to_message_id: Option<i32>,
        topic_id: Option<i32>,
        silent: bool,
        noforwards: bool,
        random_ids: &[i64],
    ) -> Result<Vec<i32>, TelegramError> {
        TelegramClient::send_album_with_random_ids(
            self,
            session_path,
            chat_id,
            items,
            caption_parse_mode,
            reply_to_message_id,
            topic_id,
            silent,
            noforwards,
            random_ids,
        )
        .await
    }

    async fn get_message_grouped_id(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        message_id: i32,
    ) -> Result<Option<i64>, TelegramError> {
        TelegramClient::get_message_grouped_id(self, session_path, chat_id, message_id).await
    }

    async fn send_sticker_with_random_id(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        set_short_name: &str,
        sticker_handle: &str,
        reply_to_message_id: Option<i32>,
        silent: bool,
        noforwards: bool,
        random_id: i64,
    ) -> Result<i32, TelegramError> {
        TelegramClient::send_sticker_with_random_id(
            self,
            session_path,
            chat_id,
            set_short_name,
            sticker_handle,
            reply_to_message_id,
            silent,
            noforwards,
            random_id,
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

    async fn send_text_html_with_random_id(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        html: &str,
        reply_to_message_id: Option<i32>,
        topic_id: Option<i32>,
        silent: bool,
        noforwards: bool,
        random_id: i64,
    ) -> Result<i32, TelegramError> {
        TelegramClient::send_text_html_with_random_id(
            self,
            session_path,
            chat_id,
            html,
            reply_to_message_id,
            topic_id,
            silent,
            noforwards,
            random_id,
        )
        .await
    }

    async fn send_rich_markdown_with_random_id(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        markdown: &str,
        reply_to_message_id: Option<i32>,
        random_id: i64,
    ) -> Result<i32, TelegramError> {
        TelegramClient::send_rich_markdown_with_random_id(
            self,
            session_path,
            chat_id,
            markdown,
            reply_to_message_id,
            random_id,
        )
        .await
    }

    async fn get_rich_message_snapshot(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        message_id: i32,
    ) -> Result<Option<RichMessageSnapshot>, TelegramError> {
        TelegramClient::get_rich_message_snapshot(self, session_path, chat_id, message_id).await
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
        premium: me.premium,
    }
}

fn contact_summary(raw: tl::enums::User) -> Option<ContactSummary> {
    let user = ferogram::User::from_raw(raw)?;
    Some(contact_summary_from_user(&user))
}

fn contact_summary_from_user(user: &ferogram::User) -> ContactSummary {
    ContactSummary {
        user_id: user.id(),
        first_name: user.first_name().unwrap_or_default().to_string(),
        last_name: user.last_name().map(str::to_string),
        display_name: user.full_name(),
        username: user.username().map(str::to_string),
        phone: user.phone().map(str::to_string),
        is_contact: user.contact(),
        mutual_contact: user.mutual_contact(),
        bot: user.bot(),
        verified: user.verified(),
        premium: user.premium(),
        scam: user.scam(),
        restricted: user.restricted(),
    }
}

fn contact_summary_by_user_id(users: &[tl::enums::User], user_id: i64) -> Option<ContactSummary> {
    users.iter().cloned().find_map(|raw| {
        let user = contact_summary(raw)?;
        (user.user_id == user_id).then_some(user)
    })
}

fn chat_admin_rights_summary(rights: tl::enums::ChatAdminRights) -> ChatAdminRightsSummary {
    let tl::enums::ChatAdminRights::ChatAdminRights(rights) = rights;
    ChatAdminRightsSummary {
        change_info: rights.change_info,
        post_messages: rights.post_messages,
        edit_messages: rights.edit_messages,
        delete_messages: rights.delete_messages,
        ban_users: rights.ban_users,
        invite_users: rights.invite_users,
        pin_messages: rights.pin_messages,
        add_admins: rights.add_admins,
        anonymous: rights.anonymous,
        manage_call: rights.manage_call,
        other: rights.other,
        manage_topics: rights.manage_topics,
        post_stories: rights.post_stories,
        edit_stories: rights.edit_stories,
        delete_stories: rights.delete_stories,
        manage_direct_messages: rights.manage_direct_messages,
        manage_ranks: rights.manage_ranks,
        manage_linked_peers: rights.manage_linked_peers,
    }
}

fn chat_admin_rights_to_tl(rights: &ChatAdminRightsSummary) -> tl::enums::ChatAdminRights {
    tl::enums::ChatAdminRights::ChatAdminRights(tl::types::ChatAdminRights {
        change_info: rights.change_info,
        post_messages: rights.post_messages,
        edit_messages: rights.edit_messages,
        delete_messages: rights.delete_messages,
        ban_users: rights.ban_users,
        invite_users: rights.invite_users,
        pin_messages: rights.pin_messages,
        add_admins: rights.add_admins,
        anonymous: rights.anonymous,
        manage_call: rights.manage_call,
        other: rights.other,
        manage_topics: rights.manage_topics,
        post_stories: rights.post_stories,
        edit_stories: rights.edit_stories,
        delete_stories: rights.delete_stories,
        manage_direct_messages: rights.manage_direct_messages,
        manage_ranks: rights.manage_ranks,
        manage_linked_peers: rights.manage_linked_peers,
    })
}

fn chat_banned_rights_summary(rights: tl::enums::ChatBannedRights) -> ChatBannedRightsSummary {
    let tl::enums::ChatBannedRights::ChatBannedRights(rights) = rights;
    ChatBannedRightsSummary {
        view_messages: rights.view_messages,
        send_messages: rights.send_messages,
        send_media: rights.send_media,
        send_stickers: rights.send_stickers,
        send_gifs: rights.send_gifs,
        send_games: rights.send_games,
        send_inline: rights.send_inline,
        embed_links: rights.embed_links,
        send_polls: rights.send_polls,
        change_info: rights.change_info,
        invite_users: rights.invite_users,
        pin_messages: rights.pin_messages,
        manage_topics: rights.manage_topics,
        send_photos: rights.send_photos,
        send_videos: rights.send_videos,
        send_roundvideos: rights.send_roundvideos,
        send_audios: rights.send_audios,
        send_voices: rights.send_voices,
        send_docs: rights.send_docs,
        send_plain: rights.send_plain,
        edit_rank: rights.edit_rank,
        send_reactions: rights.send_reactions,
        manage_linked_peers: rights.manage_linked_peers,
        until_date_unix: rights.until_date,
    }
}

fn chat_banned_rights_to_tl(rights: &ChatBannedRightsSummary) -> tl::enums::ChatBannedRights {
    tl::enums::ChatBannedRights::ChatBannedRights(tl::types::ChatBannedRights {
        view_messages: rights.view_messages,
        send_messages: rights.send_messages,
        send_media: rights.send_media,
        send_stickers: rights.send_stickers,
        send_gifs: rights.send_gifs,
        send_games: rights.send_games,
        send_inline: rights.send_inline,
        embed_links: rights.embed_links,
        send_polls: rights.send_polls,
        change_info: rights.change_info,
        invite_users: rights.invite_users,
        pin_messages: rights.pin_messages,
        manage_topics: rights.manage_topics,
        send_photos: rights.send_photos,
        send_videos: rights.send_videos,
        send_roundvideos: rights.send_roundvideos,
        send_audios: rights.send_audios,
        send_voices: rights.send_voices,
        send_docs: rights.send_docs,
        send_plain: rights.send_plain,
        edit_rank: rights.edit_rank,
        send_reactions: rights.send_reactions,
        manage_linked_peers: rights.manage_linked_peers,
        until_date: rights.until_date_unix,
    })
}

fn peer_user_id(peer: &tl::enums::Peer) -> Option<i64> {
    match peer {
        tl::enums::Peer::User(peer) => Some(peer.user_id),
        tl::enums::Peer::Chat(_) | tl::enums::Peer::Channel(_) => None,
    }
}

fn chat_member_role_name(role: &ChatMemberRole) -> &'static str {
    match role {
        ChatMemberRole::Member => "member",
        ChatMemberRole::Creator => "creator",
        ChatMemberRole::Admin => "admin",
        ChatMemberRole::Restricted => "restricted",
        ChatMemberRole::Banned => "banned",
        ChatMemberRole::Left => "left",
    }
}

fn participant_filter_matches(member: &ChatMemberSummary, filter: ParticipantFilter) -> bool {
    match filter {
        ParticipantFilter::Recent => {
            !member.left && !matches!(member.role, ChatMemberRole::Banned | ChatMemberRole::Left)
        },
        ParticipantFilter::Admins => {
            !member.left && matches!(member.role, ChatMemberRole::Creator | ChatMemberRole::Admin)
        },
        ParticipantFilter::Kicked => member.left && member.role == ChatMemberRole::Banned,
        ParticipantFilter::Banned => {
            !member.left
                && matches!(member.role, ChatMemberRole::Restricted | ChatMemberRole::Banned)
        },
        ParticipantFilter::Bots => {
            !member.left
                && !matches!(member.role, ChatMemberRole::Banned | ChatMemberRole::Left)
                && member.user.bot
        },
        ParticipantFilter::Contacts => {
            !member.left
                && !matches!(member.role, ChatMemberRole::Banned | ChatMemberRole::Left)
                && member.user.is_contact
        },
    }
}

fn participant_query_matches(member: &ChatMemberSummary, query: &str) -> bool {
    let query = query.trim().trim_start_matches('@').to_lowercase();
    let user = &member.user;
    user.user_id.to_string().contains(&query)
        || user.first_name.to_lowercase().contains(&query)
        || user.last_name.as_deref().is_some_and(|value| value.to_lowercase().contains(&query))
        || user.display_name.to_lowercase().contains(&query)
        || user.username.as_deref().is_some_and(|value| value.to_lowercase().contains(&query))
        || user.phone.as_deref().is_some_and(|value| value.to_lowercase().contains(&query))
}

fn participant_page(
    chat_id: String,
    filter: ParticipantFilter,
    query: Option<&str>,
    offset: usize,
    limit: usize,
    members: Vec<ChatMemberSummary>,
) -> ParticipantPage {
    let total_count = members.len();
    let start = offset.min(total_count);
    let end = start.saturating_add(limit).min(total_count);
    let participants = members.get(start..end).map_or_else(Vec::new, ToOwned::to_owned);
    let returned_count = participants.len();
    let has_more = end < total_count;
    ParticipantPage {
        chat_id,
        filter,
        query: query.map(str::to_string),
        offset,
        limit,
        total_count,
        returned_count,
        participants,
        next_offset: has_more.then_some(end),
        has_more,
        completeness: if has_more { Completeness::Page } else { Completeness::Complete },
    }
}

fn channel_participants_filter(
    filter: ParticipantFilter,
    query: Option<&str>,
) -> tl::enums::ChannelParticipantsFilter {
    match filter {
        ParticipantFilter::Recent => match query {
            Some(query) => tl::enums::ChannelParticipantsFilter::ChannelParticipantsSearch(
                tl::types::ChannelParticipantsSearch { q: query.to_string() },
            ),
            None => tl::enums::ChannelParticipantsFilter::ChannelParticipantsRecent,
        },
        ParticipantFilter::Admins => {
            tl::enums::ChannelParticipantsFilter::ChannelParticipantsAdmins
        },
        ParticipantFilter::Kicked => {
            tl::enums::ChannelParticipantsFilter::ChannelParticipantsKicked(
                tl::types::ChannelParticipantsKicked { q: query.unwrap_or_default().to_string() },
            )
        },
        ParticipantFilter::Banned => {
            tl::enums::ChannelParticipantsFilter::ChannelParticipantsBanned(
                tl::types::ChannelParticipantsBanned { q: query.unwrap_or_default().to_string() },
            )
        },
        ParticipantFilter::Bots => tl::enums::ChannelParticipantsFilter::ChannelParticipantsBots,
        ParticipantFilter::Contacts => {
            tl::enums::ChannelParticipantsFilter::ChannelParticipantsContacts(
                tl::types::ChannelParticipantsContacts { q: query.unwrap_or_default().to_string() },
            )
        },
    }
}

fn channel_member_summaries_exact(
    chat_id: &str,
    participants: Vec<tl::enums::ChannelParticipant>,
    users: &[tl::enums::User],
) -> Result<Vec<ChatMemberSummary>, TelegramError> {
    let mut seen = HashSet::new();
    participants
        .into_iter()
        .map(|participant| {
            let member = channel_member_summary(chat_id, participant, users).ok_or_else(|| {
                TelegramError::Telegram(
                    "Telegram participant inventory omitted a matching user".to_string(),
                )
            })?;
            if !seen.insert(member.user.user_id) {
                return Err(TelegramError::Telegram(format!(
                    "Telegram participant page repeated user {}",
                    member.user.user_id
                )));
            }
            Ok(member)
        })
        .collect()
}

fn ensure_unique_participant_ids(members: &[ChatMemberSummary]) -> Result<(), TelegramError> {
    let mut seen = HashSet::with_capacity(members.len());
    for member in members {
        if !seen.insert(member.user.user_id) {
            return Err(TelegramError::Telegram(format!(
                "Telegram participant scan repeated user {} across pages",
                member.user.user_id
            )));
        }
    }
    Ok(())
}

fn channel_member_summary(
    chat_id: &str,
    participant: tl::enums::ChannelParticipant,
    users: &[tl::enums::User],
) -> Option<ChatMemberSummary> {
    let (
        user_id,
        role,
        can_edit,
        is_self,
        left,
        joined_date_unix,
        inviter_user_id,
        promoted_by_user_id,
        kicked_by_user_id,
        rank,
        subscription_until_date_unix,
        via_request,
        admin_rights,
        banned_rights,
    ) = match participant {
        tl::enums::ChannelParticipant::ChannelParticipant(participant) => (
            participant.user_id,
            ChatMemberRole::Member,
            false,
            false,
            false,
            Some(participant.date),
            None,
            None,
            None,
            participant.rank,
            participant.subscription_until_date,
            false,
            None,
            None,
        ),
        tl::enums::ChannelParticipant::ParticipantSelf(participant) => (
            participant.user_id,
            ChatMemberRole::Member,
            false,
            true,
            false,
            Some(participant.date),
            Some(participant.inviter_id),
            None,
            None,
            participant.rank,
            participant.subscription_until_date,
            participant.via_request,
            None,
            None,
        ),
        tl::enums::ChannelParticipant::Creator(participant) => (
            participant.user_id,
            ChatMemberRole::Creator,
            false,
            false,
            false,
            None,
            None,
            None,
            None,
            participant.rank,
            None,
            false,
            Some(chat_admin_rights_summary(participant.admin_rights)),
            None,
        ),
        tl::enums::ChannelParticipant::Admin(participant) => (
            participant.user_id,
            ChatMemberRole::Admin,
            participant.can_edit,
            participant.is_self,
            false,
            Some(participant.date),
            participant.inviter_id,
            Some(participant.promoted_by),
            None,
            participant.rank,
            None,
            false,
            Some(chat_admin_rights_summary(participant.admin_rights)),
            None,
        ),
        tl::enums::ChannelParticipant::Banned(participant) => {
            let user_id = peer_user_id(&participant.peer)?;
            let rights = chat_banned_rights_summary(participant.banned_rights);
            let role = if participant.left || rights.view_messages {
                ChatMemberRole::Banned
            } else {
                ChatMemberRole::Restricted
            };
            (
                user_id,
                role,
                false,
                false,
                participant.left,
                Some(participant.date),
                None,
                None,
                Some(participant.kicked_by),
                participant.rank,
                None,
                false,
                None,
                Some(rights),
            )
        },
        tl::enums::ChannelParticipant::Left(participant) => (
            peer_user_id(&participant.peer)?,
            ChatMemberRole::Left,
            false,
            false,
            true,
            None,
            None,
            None,
            None,
            None,
            None,
            false,
            None,
            None,
        ),
    };
    let user = contact_summary_by_user_id(users, user_id)?;
    Some(ChatMemberSummary {
        chat_id: chat_id.to_string(),
        user,
        role,
        can_edit,
        is_self,
        left,
        joined_date_unix,
        inviter_user_id,
        promoted_by_user_id,
        kicked_by_user_id,
        rank,
        subscription_until_date_unix,
        via_request,
        admin_rights,
        banned_rights,
    })
}

fn basic_chat_participant_user_id(participant: &tl::enums::ChatParticipant) -> Option<i64> {
    Some(match participant {
        tl::enums::ChatParticipant::ChatParticipant(participant) => participant.user_id,
        tl::enums::ChatParticipant::Creator(participant) => participant.user_id,
        tl::enums::ChatParticipant::Admin(participant) => participant.user_id,
    })
}

fn basic_chat_member_summary(
    chat_id: &str,
    participant: tl::enums::ChatParticipant,
    users: &[tl::enums::User],
) -> Option<ChatMemberSummary> {
    let (user_id, role, joined_date_unix, inviter_user_id, rank) = match participant {
        tl::enums::ChatParticipant::ChatParticipant(participant) => (
            participant.user_id,
            ChatMemberRole::Member,
            Some(participant.date),
            Some(participant.inviter_id),
            participant.rank,
        ),
        tl::enums::ChatParticipant::Creator(participant) => {
            (participant.user_id, ChatMemberRole::Creator, None, None, participant.rank)
        },
        tl::enums::ChatParticipant::Admin(participant) => (
            participant.user_id,
            ChatMemberRole::Admin,
            Some(participant.date),
            Some(participant.inviter_id),
            participant.rank,
        ),
    };
    let user = contact_summary_by_user_id(users, user_id)?;
    Some(ChatMemberSummary {
        chat_id: chat_id.to_string(),
        user,
        role,
        can_edit: false,
        is_self: false,
        left: false,
        joined_date_unix,
        inviter_user_id,
        promoted_by_user_id: None,
        kicked_by_user_id: None,
        rank,
        subscription_until_date_unix: None,
        via_request: false,
        admin_rights: None,
        banned_rights: None,
    })
}

fn raw_invite_link(invite: &tl::enums::ExportedChatInvite) -> Option<String> {
    match invite {
        tl::enums::ExportedChatInvite::ChatInviteExported(invite) => Some(invite.link.clone()),
        tl::enums::ExportedChatInvite::ChatInvitePublicJoinRequests => None,
    }
}

fn unpack_profile_photos(
    response: tl::enums::photos::Photos,
    offset: i32,
) -> Result<(usize, Vec<tl::enums::Photo>), TelegramError> {
    match response {
        tl::enums::photos::Photos::Photos(response) => {
            let offset = usize::try_from(offset).map_err(|_| {
                TelegramError::Telegram("Telegram returned a negative profile-photo offset".into())
            })?;
            let total_count = offset.saturating_add(response.photos.len());
            Ok((total_count, response.photos))
        },
        tl::enums::photos::Photos::Slice(response) => {
            let total_count = usize::try_from(response.count).map_err(|_| {
                TelegramError::Telegram(
                    "Telegram returned a negative profile-photo total count".into(),
                )
            })?;
            Ok((total_count, response.photos))
        },
    }
}

fn profile_photo_summary(photo: &tl::enums::Photo) -> Option<ProfilePhotoSummary> {
    let tl::enums::Photo::Photo(photo) = photo else {
        return None;
    };
    Some(ProfilePhotoSummary {
        photo_handle: opaque_profile_photo_handle(photo.id),
        date_unix: photo.date,
        has_stickers: photo.has_stickers,
        has_video: photo.video_sizes.as_ref().is_some_and(|sizes| !sizes.is_empty()),
    })
}

async fn find_raw_profile_photo(
    client: &Client,
    photo_handle: &str,
) -> Result<Option<tl::types::Photo>, TelegramError> {
    let mut offset = 0_i32;
    for _ in 0..100 {
        let response = client
            .invoke(&tl::functions::photos::GetUserPhotos {
                user_id: tl::enums::InputUser::UserSelf,
                offset,
                max_id: 0,
                limit: 100,
            })
            .await
            .map_err(|error| TelegramError::Telegram(error.to_string()))?;
        let (total_count, photos) = unpack_profile_photos(response, offset)?;
        let page_len = photos.len();
        for photo in photos {
            if let tl::enums::Photo::Photo(photo) = photo
                && opaque_profile_photo_handle(photo.id) == photo_handle
            {
                return Ok(Some(photo));
            }
        }
        let next_offset = usize::try_from(offset).unwrap_or_default().saturating_add(page_len);
        if page_len == 0 || next_offset >= total_count || next_offset >= 10_000 {
            break;
        }
        offset = i32::try_from(next_offset).map_err(|_| {
            TelegramError::Telegram("profile photo pagination exceeded Telegram's int range".into())
        })?;
    }
    Ok(None)
}

fn invite_link_summary(invite: tl::enums::ExportedChatInvite) -> InviteLinkSummary {
    match invite {
        tl::enums::ExportedChatInvite::ChatInviteExported(invite) => {
            let subscription_pricing = invite.subscription_pricing.map(|pricing| {
                let tl::enums::StarsSubscriptionPricing::StarsSubscriptionPricing(pricing) =
                    pricing;
                InviteSubscriptionPricingSummary {
                    period_seconds: pricing.period,
                    amount_stars: pricing.amount,
                }
            });
            InviteLinkSummary {
                invite_handle: opaque_invite_handle(&invite.link),
                kind: InviteLinkKind::Exported,
                revoked: invite.revoked,
                permanent: invite.permanent,
                request_needed: invite.request_needed,
                admin_user_id: Some(invite.admin_id),
                date_unix: Some(invite.date),
                start_date_unix: invite.start_date,
                expire_date_unix: invite.expire_date,
                usage_limit: invite.usage_limit,
                usage_count: invite.usage,
                requested_count: invite.requested,
                subscription_expired_count: invite.subscription_expired,
                title: invite.title,
                subscription_pricing,
            }
        },
        tl::enums::ExportedChatInvite::ChatInvitePublicJoinRequests => InviteLinkSummary {
            invite_handle: "public_join_requests".to_string(),
            kind: InviteLinkKind::PublicJoinRequests,
            revoked: false,
            permanent: true,
            request_needed: true,
            admin_user_id: None,
            date_unix: None,
            start_date_unix: None,
            expire_date_unix: None,
            usage_limit: None,
            usage_count: None,
            requested_count: None,
            subscription_expired_count: None,
            title: None,
            subscription_pricing: None,
        },
    }
}

async fn find_raw_invite_link(
    client: &Client,
    chat_ref: &str,
    admin_user_id: i64,
    invite_handle: &str,
    revoked: bool,
) -> Result<Option<tl::enums::ExportedChatInvite>, TelegramError> {
    let chat_peer = client
        .resolve(chat_ref)
        .await
        .map_err(|error| TelegramError::Telegram(error.to_string()))?;
    let input_peer = client
        .resolve_to_input_peer(&chat_peer)
        .await
        .map_err(|error| TelegramError::Telegram(error.to_string()))?;
    let me = client.get_me().await.map_err(|error| TelegramError::Telegram(error.to_string()))?;
    let admin_id = if admin_user_id == me.id {
        tl::enums::InputUser::UserSelf
    } else {
        resolve_input_user(client, &admin_user_id.to_string(), "invite-link administrator").await?
    };
    let mut offset_date = None;
    let mut offset_link = None;
    for _ in 0..10 {
        let page = client
            .invoke(&tl::functions::messages::GetExportedChatInvites {
                revoked,
                peer: input_peer.clone(),
                admin_id: admin_id.clone(),
                offset_date,
                offset_link: offset_link.clone(),
                limit: 100,
            })
            .await
            .map_err(|error| TelegramError::Telegram(error.to_string()))?;
        let tl::enums::messages::ExportedChatInvites::ExportedChatInvites(page) = page;
        let page = page.invites;
        if let Some(invite) = page.iter().find(|invite| {
            raw_invite_link(invite)
                .as_deref()
                .is_some_and(|link| opaque_invite_handle(link) == invite_handle)
        }) {
            return Ok(Some(invite.clone()));
        }
        let page_len = page.len();
        let next = page.iter().rev().find_map(|invite| match invite {
            tl::enums::ExportedChatInvite::ChatInviteExported(invite) => {
                Some((Some(invite.date), Some(invite.link.clone())))
            },
            tl::enums::ExportedChatInvite::ChatInvitePublicJoinRequests => None,
        });
        if page_len < 100 || next.is_none() || next == Some((offset_date, offset_link.clone())) {
            break;
        }
        let (next_date, next_link) = next.expect("checked above");
        offset_date = next_date;
        offset_link = next_link;
    }
    Ok(None)
}

async fn resolve_raw_invite_filter(
    client: &Client,
    chat_ref: &str,
    admin_user_id: i64,
    invite_handle: &str,
) -> Result<String, TelegramError> {
    if invite_handle == "public_join_requests" {
        return Err(TelegramError::InvalidPeer(
            "Telegram cannot filter invite importers by the public join-request route".to_string(),
        ));
    }
    for revoked in [false, true] {
        if let Some(invite) =
            find_raw_invite_link(client, chat_ref, admin_user_id, invite_handle, revoked).await?
        {
            return raw_invite_link(&invite).ok_or_else(|| {
                TelegramError::InvalidPeer(
                    "the selected opaque handle does not identify an exported invite link"
                        .to_string(),
                )
            });
        }
    }
    Err(TelegramError::InvalidPeer(
        "the opaque invite handle was not found for this chat".to_string(),
    ))
}

fn empty_web_page_preview_summary(state: &str) -> WebPagePreviewSummary {
    WebPagePreviewSummary {
        state: state.to_string(),
        webpage_id: None,
        pending_until_unix: None,
        url: None,
        display_url: None,
        page_type: None,
        site_name: None,
        title: None,
        description: None,
        author: None,
        embed_url: None,
        embed_type: None,
        embed_width: None,
        embed_height: None,
        duration_seconds: None,
        has_photo: false,
        has_document: false,
        has_cached_page: false,
        has_large_media: false,
        video_cover_photo: false,
        force_large_media: false,
        force_small_media: false,
        manual: false,
        safe: false,
        related_chats: Vec::new(),
        related_users: Vec::new(),
    }
}

fn web_page_preview_summary(
    response: tl::types::messages::WebPagePreview,
) -> WebPagePreviewSummary {
    let mut summary = empty_web_page_preview_summary("none");
    summary.related_chats = response.chats.into_iter().filter_map(chat_summary).collect();
    summary.related_users = response.users.into_iter().filter_map(contact_summary).collect();
    let tl::enums::MessageMedia::WebPage(media) = response.media else {
        return summary;
    };
    summary.force_large_media = media.force_large_media;
    summary.force_small_media = media.force_small_media;
    summary.manual = media.manual;
    summary.safe = media.safe;
    match media.webpage {
        tl::enums::WebPage::Empty(page) => {
            summary.state = "empty".to_string();
            summary.webpage_id = Some(page.id);
            summary.url = page.url;
        },
        tl::enums::WebPage::Pending(page) => {
            summary.state = "pending".to_string();
            summary.webpage_id = Some(page.id);
            summary.pending_until_unix = Some(page.date);
            summary.url = page.url;
        },
        tl::enums::WebPage::WebPage(page) => {
            summary.state = "loaded".to_string();
            summary.webpage_id = Some(page.id);
            summary.url = Some(page.url);
            summary.display_url = Some(page.display_url);
            summary.page_type = page.r#type;
            summary.site_name = page.site_name;
            summary.title = page.title;
            summary.description = page.description;
            summary.author = page.author;
            summary.embed_url = page.embed_url;
            summary.embed_type = page.embed_type;
            summary.embed_width = page.embed_width;
            summary.embed_height = page.embed_height;
            summary.duration_seconds = page.duration;
            summary.has_photo = page.photo.is_some();
            summary.has_document = page.document.is_some();
            summary.has_cached_page = page.cached_page.is_some();
            summary.has_large_media = page.has_large_media;
            summary.video_cover_photo = page.video_cover_photo;
        },
        tl::enums::WebPage::NotModified(_) => {
            summary.state = "not_modified".to_string();
        },
    }
    summary
}

fn chat_summary(raw: tl::enums::Chat) -> Option<ChatSummary> {
    match ferogram::Chat::from_raw(raw)? {
        ferogram::Chat::Group(group) => Some(ChatSummary {
            chat_id: format!("-{}", group.id()),
            title: group.title().to_string(),
            kind: "group".to_string(),
            username: None,
            participants_count: Some(group.participants_count()),
            creator: group.creator(),
            verified: false,
            restricted: false,
        }),
        ferogram::Chat::Channel(channel) => {
            let kind = match channel.kind() {
                ferogram::ChannelKind::Broadcast => "channel",
                ferogram::ChannelKind::Megagroup => "supergroup",
                ferogram::ChannelKind::Gigagroup => "gigagroup",
            };
            Some(ChatSummary {
                chat_id: format!("-100{}", channel.id()),
                title: channel.title().to_string(),
                kind: kind.to_string(),
                username: channel.username().map(str::to_string),
                participants_count: channel.participants_count(),
                creator: channel.raw.creator,
                verified: channel.verified(),
                restricted: channel.restricted(),
            })
        },
    }
}

async fn chat_auto_delete_summary(
    client: &Client,
    chat_id: &str,
) -> Result<ChatAutoDeleteSummary, TelegramError> {
    let peer_ref = TelegramClient::resolve_peer_ref(client, chat_id).await?;
    let peer = client
        .resolve(peer_ref.as_str())
        .await
        .map_err(|error| TelegramError::Telegram(error.to_string()))?;
    let canonical_chat_id = canonical_peer_ref(&peer);
    let input_peer = client
        .resolve_to_input_peer(&peer)
        .await
        .map_err(|error| TelegramError::Telegram(error.to_string()))?;
    let period_seconds = match input_peer {
        tl::enums::InputPeer::User(user) => {
            let response = client
                .invoke(&tl::functions::users::GetFullUser {
                    id: tl::enums::InputUser::InputUser(tl::types::InputUser {
                        user_id: user.user_id,
                        access_hash: user.access_hash,
                    }),
                })
                .await
                .map_err(|error| TelegramError::Telegram(error.to_string()))?;
            let tl::enums::users::UserFull::UserFull(response) = response;
            let tl::enums::UserFull::UserFull(full) = response.full_user;
            full.ttl_period.unwrap_or_default()
        },
        tl::enums::InputPeer::PeerSelf => {
            let response = client
                .invoke(&tl::functions::users::GetFullUser { id: tl::enums::InputUser::UserSelf })
                .await
                .map_err(|error| TelegramError::Telegram(error.to_string()))?;
            let tl::enums::users::UserFull::UserFull(response) = response;
            let tl::enums::UserFull::UserFull(full) = response.full_user;
            full.ttl_period.unwrap_or_default()
        },
        tl::enums::InputPeer::UserFromMessage(user) => {
            let response = client
                .invoke(&tl::functions::users::GetFullUser {
                    id: tl::enums::InputUser::FromMessage(tl::types::InputUserFromMessage {
                        peer: user.peer,
                        msg_id: user.msg_id,
                        user_id: user.user_id,
                    }),
                })
                .await
                .map_err(|error| TelegramError::Telegram(error.to_string()))?;
            let tl::enums::users::UserFull::UserFull(response) = response;
            let tl::enums::UserFull::UserFull(full) = response.full_user;
            full.ttl_period.unwrap_or_default()
        },
        tl::enums::InputPeer::Chat(_)
        | tl::enums::InputPeer::Channel(_)
        | tl::enums::InputPeer::ChannelFromMessage(_) => {
            let response = client
                .get_chat_full(peer_ref.as_str())
                .await
                .map_err(|error| TelegramError::Telegram(error.to_string()))?;
            let tl::enums::messages::ChatFull::ChatFull(response) = response;
            match response.full_chat {
                tl::enums::ChatFull::ChatFull(full) => full.ttl_period.unwrap_or_default(),
                tl::enums::ChatFull::ChannelFull(full) => full.ttl_period.unwrap_or_default(),
                tl::enums::ChatFull::CommunityFull(_) => {
                    return Err(TelegramError::InvalidPeer(
                        "Telegram communities do not expose a history TTL".to_string(),
                    ));
                },
            }
        },
        tl::enums::InputPeer::Empty => {
            return Err(TelegramError::InvalidPeer(
                "auto-delete requires a concrete Telegram peer".to_string(),
            ));
        },
    };
    Ok(ChatAutoDeleteSummary { chat_id: canonical_chat_id, period_seconds })
}

async fn chat_lifecycle_state(
    client: &Client,
    chat_id: &str,
) -> Result<ChatLifecycleState, TelegramError> {
    let peer_ref = normalize_peer_ref(chat_id)?;
    let peer = client
        .resolve(peer_ref.as_str())
        .await
        .map_err(|error| TelegramError::Telegram(error.to_string()))?;
    let canonical_chat_id = canonical_peer_ref(&peer);
    let input_peer = client
        .resolve_to_input_peer(&peer)
        .await
        .map_err(|error| TelegramError::Telegram(error.to_string()))?;
    let in_dialogs = input_peer_is_in_dialogs(client, input_peer).await?;
    let full = client
        .get_chat_full(peer_ref.as_str())
        .await
        .map_err(|error| TelegramError::Telegram(error.to_string()))?;
    let tl::enums::messages::ChatFull::ChatFull(full) = full;
    let chat = full
        .chats
        .iter()
        .cloned()
        .filter_map(chat_summary)
        .find(|chat| chat.chat_id == canonical_chat_id)
        .ok_or_else(|| {
            TelegramError::PeerNotFound(format!(
                "Telegram returned no chat object for {canonical_chat_id}"
            ))
        })?;
    let (about, photo_handle) = match &full.full_chat {
        tl::enums::ChatFull::ChatFull(details) => (
            nonempty_owned(&details.about),
            details
                .chat_photo
                .as_ref()
                .and_then(|photo| opaque_chat_photo_from_raw(&canonical_chat_id, photo)),
        ),
        tl::enums::ChatFull::ChannelFull(details) => (
            nonempty_owned(&details.about),
            opaque_chat_photo_from_raw(&canonical_chat_id, &details.chat_photo),
        ),
        tl::enums::ChatFull::CommunityFull(details) => (
            nonempty_owned(&details.about),
            opaque_chat_photo_from_raw(&canonical_chat_id, &details.chat_photo),
        ),
    };
    Ok(ChatLifecycleState { details: ChatDetailsSummary { chat, about, photo_handle }, in_dialogs })
}

async fn input_peer_is_in_dialogs(
    client: &Client,
    input_peer: tl::enums::InputPeer,
) -> Result<bool, TelegramError> {
    let response = client
        .invoke(&tl::functions::messages::GetPeerDialogs {
            peers: vec![tl::enums::InputDialogPeer::InputDialogPeer(tl::types::InputDialogPeer {
                peer: input_peer,
            })],
        })
        .await
        .map_err(|error| TelegramError::Telegram(error.to_string()))?;
    let tl::enums::messages::PeerDialogs::PeerDialogs(response) = response;
    Ok(response.dialogs.iter().any(|dialog| matches!(dialog, tl::enums::Dialog::Dialog(_))))
}

fn opaque_chat_photo_from_raw(chat_id: &str, photo: &tl::enums::Photo) -> Option<String> {
    match photo {
        tl::enums::Photo::Photo(photo) => Some(opaque_chat_photo_handle(chat_id, photo.id)),
        tl::enums::Photo::Empty(_) => None,
    }
}

fn nonempty_owned(value: &str) -> Option<String> {
    (!value.is_empty()).then(|| value.to_string())
}

fn forum_topic_summary(topic: tl::enums::ForumTopic) -> ForumTopicSummary {
    match topic {
        tl::enums::ForumTopic::Deleted(topic) => ForumTopicSummary {
            topic_id: topic.id,
            deleted: true,
            title: None,
            date_unix: None,
            creator_ref: None,
            my: false,
            closed: false,
            pinned: false,
            short: false,
            hidden: false,
            title_missing: false,
            icon_color: None,
            icon_emoji_id: None,
            top_message_id: None,
            read_inbox_max_id: None,
            read_outbox_max_id: None,
            unread_count: None,
            unread_mentions_count: None,
            unread_reactions_count: None,
            unread_poll_votes_count: None,
            notification_settings: None,
            draft: None,
        },
        tl::enums::ForumTopic::ForumTopic(topic) => ForumTopicSummary {
            topic_id: topic.id,
            deleted: false,
            title: Some(topic.title),
            date_unix: Some(topic.date),
            creator_ref: Some(canonical_peer_ref(&topic.from_id)),
            my: topic.my,
            closed: topic.closed,
            pinned: topic.pinned,
            short: topic.short,
            hidden: topic.hidden,
            title_missing: topic.title_missing,
            icon_color: Some(topic.icon_color),
            icon_emoji_id: topic.icon_emoji_id,
            top_message_id: Some(topic.top_message),
            read_inbox_max_id: Some(topic.read_inbox_max_id),
            read_outbox_max_id: Some(topic.read_outbox_max_id),
            unread_count: Some(topic.unread_count),
            unread_mentions_count: Some(topic.unread_mentions_count),
            unread_reactions_count: Some(topic.unread_reactions_count),
            unread_poll_votes_count: Some(topic.unread_poll_votes_count),
            notification_settings: Some(notification_settings_summary(topic.notify_settings)),
            draft: topic.draft.and_then(draft_summary),
        },
    }
}

fn draft_summary(draft: tl::enums::DraftMessage) -> Option<DraftSummary> {
    let mut serialized = Vec::new();
    tl::Serializable::serialize(&draft, &mut serialized);
    let state_sha256 = format!("{:x}", Sha256::digest(serialized));
    match draft {
        tl::enums::DraftMessage::Empty(_) => None,
        tl::enums::DraftMessage::DraftMessage(draft) => Some(DraftSummary {
            message: draft.message,
            date_unix: draft.date,
            no_webpage: draft.no_webpage,
            invert_media: draft.invert_media,
            has_reply_to: draft.reply_to.is_some(),
            has_entities: draft.entities.is_some(),
            has_media: draft.media.is_some(),
            effect_id: draft.effect,
            has_suggested_post: draft.suggested_post.is_some(),
            has_rich_message: draft.rich_message.is_some(),
            state_sha256,
        }),
    }
}

fn notification_settings_summary(
    settings: tl::enums::PeerNotifySettings,
) -> NotificationSettingsSummary {
    let tl::enums::PeerNotifySettings::PeerNotifySettings(settings) = settings;
    NotificationSettingsSummary {
        show_previews: settings.show_previews,
        silent: settings.silent,
        mute_until_unix: settings.mute_until,
        ios_sound: settings.ios_sound.map(notification_sound_summary),
        android_sound: settings.android_sound.map(notification_sound_summary),
        other_sound: settings.other_sound.map(notification_sound_summary),
        stories_muted: settings.stories_muted,
        stories_hide_sender: settings.stories_hide_sender,
        stories_ios_sound: settings.stories_ios_sound.map(notification_sound_summary),
        stories_android_sound: settings.stories_android_sound.map(notification_sound_summary),
        stories_other_sound: settings.stories_other_sound.map(notification_sound_summary),
    }
}

fn notification_sound_summary(sound: tl::enums::NotificationSound) -> NotificationSoundSummary {
    match sound {
        tl::enums::NotificationSound::Default => NotificationSoundSummary {
            kind: "default".to_string(),
            title: None,
            data: None,
            ringtone_id: None,
        },
        tl::enums::NotificationSound::None => NotificationSoundSummary {
            kind: "none".to_string(),
            title: None,
            data: None,
            ringtone_id: None,
        },
        tl::enums::NotificationSound::Local(sound) => NotificationSoundSummary {
            kind: "local".to_string(),
            title: Some(sound.title),
            data: Some(sound.data),
            ringtone_id: None,
        },
        tl::enums::NotificationSound::Ringtone(sound) => NotificationSoundSummary {
            kind: "ringtone".to_string(),
            title: None,
            data: None,
            ringtone_id: Some(sound.id),
        },
    }
}

fn notification_sound_to_tl(
    sound: &NotificationSoundSummary,
) -> Result<tl::enums::NotificationSound, TelegramError> {
    match sound.kind.as_str() {
        "default" => Ok(tl::enums::NotificationSound::Default),
        "none" => Ok(tl::enums::NotificationSound::None),
        "local" => Ok(tl::enums::NotificationSound::Local(tl::types::NotificationSoundLocal {
            title: sound.title.clone().ok_or_else(|| {
                TelegramError::InvalidPeer("local notification sound requires title".to_string())
            })?,
            data: sound.data.clone().ok_or_else(|| {
                TelegramError::InvalidPeer("local notification sound requires data".to_string())
            })?,
        })),
        "ringtone" => {
            Ok(tl::enums::NotificationSound::Ringtone(tl::types::NotificationSoundRingtone {
                id: sound.ringtone_id.ok_or_else(|| {
                    TelegramError::InvalidPeer(
                        "ringtone notification sound requires ringtone_id".to_string(),
                    )
                })?,
            }))
        },
        other => {
            Err(TelegramError::InvalidPeer(format!("unsupported notification sound kind: {other}")))
        },
    }
}

fn privacy_key_to_tl(key: PrivacyKey) -> tl::enums::InputPrivacyKey {
    match key {
        PrivacyKey::StatusTimestamp => tl::enums::InputPrivacyKey::StatusTimestamp,
        PrivacyKey::ChatInvite => tl::enums::InputPrivacyKey::ChatInvite,
        PrivacyKey::PhoneCall => tl::enums::InputPrivacyKey::PhoneCall,
        PrivacyKey::PhoneP2p => tl::enums::InputPrivacyKey::PhoneP2P,
        PrivacyKey::Forwards => tl::enums::InputPrivacyKey::Forwards,
        PrivacyKey::ProfilePhoto => tl::enums::InputPrivacyKey::ProfilePhoto,
        PrivacyKey::PhoneNumber => tl::enums::InputPrivacyKey::PhoneNumber,
        PrivacyKey::AddedByPhone => tl::enums::InputPrivacyKey::AddedByPhone,
        PrivacyKey::VoiceMessages => tl::enums::InputPrivacyKey::VoiceMessages,
        PrivacyKey::About => tl::enums::InputPrivacyKey::About,
        PrivacyKey::Birthday => tl::enums::InputPrivacyKey::Birthday,
        PrivacyKey::StarGiftsAutoSave => tl::enums::InputPrivacyKey::StarGiftsAutoSave,
        PrivacyKey::NoPaidMessages => tl::enums::InputPrivacyKey::NoPaidMessages,
        PrivacyKey::SavedMusic => tl::enums::InputPrivacyKey::SavedMusic,
    }
}

fn privacy_rule_summary(rule: tl::enums::PrivacyRule) -> PrivacyRuleSummary {
    match rule {
        tl::enums::PrivacyRule::PrivacyValueAllowContacts => {
            privacy_rule("allow_contacts", Vec::new(), Vec::new())
        },
        tl::enums::PrivacyRule::PrivacyValueAllowAll => {
            privacy_rule("allow_all", Vec::new(), Vec::new())
        },
        tl::enums::PrivacyRule::PrivacyValueAllowUsers(rule) => {
            privacy_rule("allow_users", rule.users, Vec::new())
        },
        tl::enums::PrivacyRule::PrivacyValueDisallowContacts => {
            privacy_rule("disallow_contacts", Vec::new(), Vec::new())
        },
        tl::enums::PrivacyRule::PrivacyValueDisallowAll => {
            privacy_rule("disallow_all", Vec::new(), Vec::new())
        },
        tl::enums::PrivacyRule::PrivacyValueDisallowUsers(rule) => {
            privacy_rule("disallow_users", rule.users, Vec::new())
        },
        tl::enums::PrivacyRule::PrivacyValueAllowChatParticipants(rule) => {
            privacy_rule("allow_chat_participants", Vec::new(), rule.chats)
        },
        tl::enums::PrivacyRule::PrivacyValueDisallowChatParticipants(rule) => {
            privacy_rule("disallow_chat_participants", Vec::new(), rule.chats)
        },
        tl::enums::PrivacyRule::PrivacyValueAllowCloseFriends => {
            privacy_rule("allow_close_friends", Vec::new(), Vec::new())
        },
        tl::enums::PrivacyRule::PrivacyValueAllowPremium => {
            privacy_rule("allow_premium", Vec::new(), Vec::new())
        },
        tl::enums::PrivacyRule::PrivacyValueAllowBots => {
            privacy_rule("allow_bots", Vec::new(), Vec::new())
        },
        tl::enums::PrivacyRule::PrivacyValueDisallowBots => {
            privacy_rule("disallow_bots", Vec::new(), Vec::new())
        },
    }
}

async fn privacy_rule_to_tl(
    client: &Client,
    rule: &PrivacyRuleSummary,
) -> Result<tl::enums::InputPrivacyRule, TelegramError> {
    let input = match rule.kind.as_str() {
        "allow_contacts" => tl::enums::InputPrivacyRule::InputPrivacyValueAllowContacts,
        "allow_all" => tl::enums::InputPrivacyRule::InputPrivacyValueAllowAll,
        "allow_users" => tl::enums::InputPrivacyRule::InputPrivacyValueAllowUsers(
            tl::types::InputPrivacyValueAllowUsers {
                users: privacy_input_users(client, &rule.user_ids).await?,
            },
        ),
        "disallow_contacts" => tl::enums::InputPrivacyRule::InputPrivacyValueDisallowContacts,
        "disallow_all" => tl::enums::InputPrivacyRule::InputPrivacyValueDisallowAll,
        "disallow_users" => tl::enums::InputPrivacyRule::InputPrivacyValueDisallowUsers(
            tl::types::InputPrivacyValueDisallowUsers {
                users: privacy_input_users(client, &rule.user_ids).await?,
            },
        ),
        "allow_chat_participants" => {
            tl::enums::InputPrivacyRule::InputPrivacyValueAllowChatParticipants(
                tl::types::InputPrivacyValueAllowChatParticipants { chats: rule.chat_ids.clone() },
            )
        },
        "disallow_chat_participants" => {
            tl::enums::InputPrivacyRule::InputPrivacyValueDisallowChatParticipants(
                tl::types::InputPrivacyValueDisallowChatParticipants {
                    chats: rule.chat_ids.clone(),
                },
            )
        },
        "allow_close_friends" => tl::enums::InputPrivacyRule::InputPrivacyValueAllowCloseFriends,
        "allow_premium" => tl::enums::InputPrivacyRule::InputPrivacyValueAllowPremium,
        "allow_bots" => tl::enums::InputPrivacyRule::InputPrivacyValueAllowBots,
        "disallow_bots" => tl::enums::InputPrivacyRule::InputPrivacyValueDisallowBots,
        other => {
            return Err(TelegramError::InvalidPeer(format!(
                "unsupported privacy rule kind: {other}"
            )));
        },
    };
    Ok(input)
}

async fn privacy_input_users(
    client: &Client,
    user_ids: &[i64],
) -> Result<Vec<tl::enums::InputUser>, TelegramError> {
    let mut users = Vec::with_capacity(user_ids.len());
    for &user_id in user_ids {
        let peer = tl::enums::Peer::User(tl::types::PeerUser { user_id });
        let input_peer = client
            .resolve_to_input_peer(&peer)
            .await
            .map_err(|error| TelegramError::Telegram(error.to_string()))?;
        let input_user = match input_peer {
            tl::enums::InputPeer::User(user) => {
                tl::enums::InputUser::InputUser(tl::types::InputUser {
                    user_id: user.user_id,
                    access_hash: user.access_hash,
                })
            },
            tl::enums::InputPeer::PeerSelf => tl::enums::InputUser::UserSelf,
            tl::enums::InputPeer::UserFromMessage(user) => {
                tl::enums::InputUser::FromMessage(tl::types::InputUserFromMessage {
                    peer: user.peer,
                    msg_id: user.msg_id,
                    user_id: user.user_id,
                })
            },
            _ => {
                return Err(TelegramError::InvalidPeer(format!(
                    "privacy user id {user_id} did not resolve to a Telegram user"
                )));
            },
        };
        users.push(input_user);
    }
    Ok(users)
}

async fn story_privacy_to_tl(
    client: &Client,
    privacy: &StoryPrivacySpec,
) -> Result<Vec<tl::enums::InputPrivacyRule>, TelegramError> {
    let mut rules = Vec::new();
    if !privacy.disallowed_user_ids.is_empty() {
        rules.push(tl::enums::InputPrivacyRule::InputPrivacyValueDisallowUsers(
            tl::types::InputPrivacyValueDisallowUsers {
                users: privacy_input_users(client, &privacy.disallowed_user_ids).await?,
            },
        ));
    }
    match privacy.kind.as_str() {
        "everyone" if privacy.selected_user_ids.is_empty() => {
            rules.push(tl::enums::InputPrivacyRule::InputPrivacyValueAllowAll);
        },
        "contacts" if privacy.selected_user_ids.is_empty() => {
            rules.push(tl::enums::InputPrivacyRule::InputPrivacyValueAllowContacts);
            rules.push(tl::enums::InputPrivacyRule::InputPrivacyValueDisallowAll);
        },
        "close_friends"
            if privacy.selected_user_ids.is_empty() && privacy.disallowed_user_ids.is_empty() =>
        {
            rules.push(tl::enums::InputPrivacyRule::InputPrivacyValueAllowCloseFriends);
            rules.push(tl::enums::InputPrivacyRule::InputPrivacyValueDisallowAll);
        },
        "selected_contacts"
            if !privacy.selected_user_ids.is_empty() && privacy.disallowed_user_ids.is_empty() =>
        {
            rules.push(tl::enums::InputPrivacyRule::InputPrivacyValueAllowUsers(
                tl::types::InputPrivacyValueAllowUsers {
                    users: privacy_input_users(client, &privacy.selected_user_ids).await?,
                },
            ));
            rules.push(tl::enums::InputPrivacyRule::InputPrivacyValueDisallowAll);
        },
        _ => {
            return Err(TelegramError::InvalidPeer(
                "invalid story privacy specification".to_string(),
            ));
        },
    }
    Ok(rules)
}

fn privacy_rule(kind: &str, user_ids: Vec<i64>, chat_ids: Vec<i64>) -> PrivacyRuleSummary {
    PrivacyRuleSummary { kind: kind.to_string(), user_ids, chat_ids }
}

fn user_status(status: Option<&tl::enums::UserStatus>) -> (Option<String>, Option<i32>) {
    match status {
        Some(tl::enums::UserStatus::Online(status)) => {
            (Some("online".to_string()), Some(status.expires))
        },
        Some(tl::enums::UserStatus::Offline(status)) => {
            (Some("offline".to_string()), Some(status.was_online))
        },
        Some(tl::enums::UserStatus::Recently(_)) => (Some("recently".to_string()), None),
        Some(tl::enums::UserStatus::LastWeek(_)) => (Some("last_week".to_string()), None),
        Some(tl::enums::UserStatus::LastMonth(_)) => (Some("last_month".to_string()), None),
        Some(tl::enums::UserStatus::Empty) | None => (None, None),
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

async fn resolve_channel_input_peer(
    client: &Client,
    chat_id: &str,
    operation: &str,
) -> Result<(String, tl::enums::InputPeer), TelegramError> {
    let peer_ref = TelegramClient::resolve_peer_ref(client, chat_id).await?;
    let peer = client
        .resolve(peer_ref.as_str())
        .await
        .map_err(|error| TelegramError::Telegram(error.to_string()))?;
    let canonical_chat_id = canonical_peer_ref(&peer);
    let input_peer = client
        .resolve_to_input_peer(&peer)
        .await
        .map_err(|error| TelegramError::Telegram(error.to_string()))?;
    if !matches!(input_peer, tl::enums::InputPeer::Channel(_)) {
        return Err(TelegramError::InvalidPeer(format!(
            "{operation} requires a channel or supergroup"
        )));
    }
    Ok((canonical_chat_id, input_peer))
}

async fn resolve_input_user(
    client: &Client,
    user_ref: &str,
    operation: &str,
) -> Result<tl::enums::InputUser, TelegramError> {
    let peer_ref = TelegramClient::resolve_peer_ref(client, user_ref).await?;
    let peer = client
        .resolve(peer_ref.as_str())
        .await
        .map_err(|error| TelegramError::Telegram(error.to_string()))?;
    let input_peer = client
        .resolve_to_input_peer(&peer)
        .await
        .map_err(|error| TelegramError::Telegram(error.to_string()))?;
    match input_peer {
        tl::enums::InputPeer::User(user) => {
            Ok(tl::enums::InputUser::InputUser(tl::types::InputUser {
                user_id: user.user_id,
                access_hash: user.access_hash,
            }))
        },
        tl::enums::InputPeer::PeerSelf => Ok(tl::enums::InputUser::UserSelf),
        tl::enums::InputPeer::UserFromMessage(user) => {
            Ok(tl::enums::InputUser::FromMessage(tl::types::InputUserFromMessage {
                peer: user.peer,
                msg_id: user.msg_id,
                user_id: user.user_id,
            }))
        },
        _ => Err(TelegramError::InvalidPeer(format!(
            "{operation} requires Telegram user references"
        ))),
    }
}

fn admin_log_filter_to_tl(filter: AdminLogEventFilter) -> tl::enums::ChannelAdminLogEventsFilter {
    tl::enums::ChannelAdminLogEventsFilter::ChannelAdminLogEventsFilter(
        tl::types::ChannelAdminLogEventsFilter {
            join: filter.join,
            leave: filter.leave,
            invite: filter.invite,
            ban: filter.ban,
            unban: filter.unban,
            kick: filter.kick,
            unkick: filter.unkick,
            promote: filter.promote,
            demote: filter.demote,
            info: filter.info,
            settings: filter.settings,
            pinned: filter.pinned,
            edit: filter.edit,
            delete: filter.delete,
            group_call: filter.group_call,
            invites: filter.invites,
            send: filter.send,
            forums: filter.forums,
            sub_extend: filter.sub_extend,
            edit_rank: filter.edit_rank,
        },
    )
}

fn channel_participant_user_id(participant: &tl::enums::ChannelParticipant) -> Option<i64> {
    match participant {
        tl::enums::ChannelParticipant::ChannelParticipant(participant) => Some(participant.user_id),
        tl::enums::ChannelParticipant::ParticipantSelf(participant) => Some(participant.user_id),
        tl::enums::ChannelParticipant::Creator(participant) => Some(participant.user_id),
        tl::enums::ChannelParticipant::Admin(participant) => Some(participant.user_id),
        tl::enums::ChannelParticipant::Banned(participant) => peer_user_id(&participant.peer),
        tl::enums::ChannelParticipant::Left(participant) => peer_user_id(&participant.peer),
    }
}

fn forum_topic_id(topic: &tl::enums::ForumTopic) -> i32 {
    match topic {
        tl::enums::ForumTopic::Deleted(topic) => topic.id,
        tl::enums::ForumTopic::ForumTopic(topic) => topic.id,
    }
}

#[allow(clippy::too_many_lines)]
fn admin_log_action_summary(
    action: tl::enums::ChannelAdminLogEventAction,
) -> (AdminLogActionKind, Option<String>, Option<i64>, Option<i32>, Option<i32>) {
    use tl::enums::ChannelAdminLogEventAction as Action;
    match action {
        Action::ChangeTitle(value) => (
            AdminLogActionKind::ChangeTitle,
            Some(format!("\"{}\" → \"{}\"", value.prev_value, value.new_value)),
            None,
            None,
            None,
        ),
        Action::ChangeAbout(_) => (
            AdminLogActionKind::ChangeAbout,
            Some("description changed".to_string()),
            None,
            None,
            None,
        ),
        Action::ChangeUsername(value) => (
            AdminLogActionKind::ChangeUsername,
            Some(format!(
                "@{} → @{}",
                if value.prev_value.is_empty() { "-" } else { &value.prev_value },
                if value.new_value.is_empty() { "-" } else { &value.new_value }
            )),
            None,
            None,
            None,
        ),
        Action::ChangePhoto(_) => {
            (AdminLogActionKind::ChangePhoto, Some("photo changed".to_string()), None, None, None)
        },
        Action::ToggleInvites(value) => (
            AdminLogActionKind::ToggleInvites,
            Some(format!("invites enabled: {}", value.new_value)),
            None,
            None,
            None,
        ),
        Action::ToggleSignatures(value) => (
            AdminLogActionKind::ToggleSignatures,
            Some(format!("signatures enabled: {}", value.new_value)),
            None,
            None,
            None,
        ),
        Action::UpdatePinned(value) => {
            let id = message_id(&value.message);
            (AdminLogActionKind::UpdatePinned, Some(format!("message #{id}")), None, Some(id), None)
        },
        Action::EditMessage(value) => {
            let id = message_id(&value.new_message);
            (AdminLogActionKind::EditMessage, Some(format!("message #{id}")), None, Some(id), None)
        },
        Action::DeleteMessage(value) => {
            let id = message_id(&value.message);
            (
                AdminLogActionKind::DeleteMessage,
                Some(format!("message #{id}")),
                None,
                Some(id),
                None,
            )
        },
        Action::ParticipantJoin => (AdminLogActionKind::ParticipantJoin, None, None, None, None),
        Action::ParticipantLeave => (AdminLogActionKind::ParticipantLeave, None, None, None, None),
        Action::ParticipantInvite(value) => {
            let user_id = channel_participant_user_id(&value.participant);
            (
                AdminLogActionKind::ParticipantInvite,
                user_id.map(|id| format!("invited user {id}")),
                user_id,
                None,
                None,
            )
        },
        Action::ParticipantToggleBan(value) => {
            let user_id = channel_participant_user_id(&value.new_participant);
            (
                AdminLogActionKind::ParticipantToggleBan,
                user_id.map(|id| format!("ban state changed for user {id}")),
                user_id,
                None,
                None,
            )
        },
        Action::ParticipantToggleAdmin(value) => {
            let user_id = channel_participant_user_id(&value.new_participant);
            (
                AdminLogActionKind::ParticipantToggleAdmin,
                user_id.map(|id| format!("administrator rights changed for user {id}")),
                user_id,
                None,
                None,
            )
        },
        Action::ChangeStickerSet(_) => (
            AdminLogActionKind::ChangeStickerSet,
            Some("sticker set changed".to_string()),
            None,
            None,
            None,
        ),
        Action::TogglePreHistoryHidden(value) => (
            AdminLogActionKind::TogglePreHistoryHidden,
            Some(format!("pre-history hidden: {}", value.new_value)),
            None,
            None,
            None,
        ),
        Action::DefaultBannedRights(_) => (
            AdminLogActionKind::DefaultBannedRights,
            Some("default member restrictions changed".to_string()),
            None,
            None,
            None,
        ),
        Action::StopPoll(value) => {
            let id = message_id(&value.message);
            (
                AdminLogActionKind::StopPoll,
                Some(format!("poll in message #{id}")),
                None,
                Some(id),
                None,
            )
        },
        Action::ChangeLinkedChat(value) => (
            AdminLogActionKind::ChangeLinkedChat,
            Some(format!("{} → {}", value.prev_value, value.new_value)),
            None,
            None,
            None,
        ),
        Action::ChangeLocation(_) => (
            AdminLogActionKind::ChangeLocation,
            Some("location changed".to_string()),
            None,
            None,
            None,
        ),
        Action::ToggleSlowMode(value) => (
            AdminLogActionKind::ToggleSlowMode,
            Some(format!("{}s → {}s", value.prev_value, value.new_value)),
            None,
            None,
            None,
        ),
        Action::StartGroupCall(_) => (
            AdminLogActionKind::StartGroupCall,
            Some("group call started".to_string()),
            None,
            None,
            None,
        ),
        Action::DiscardGroupCall(_) => (
            AdminLogActionKind::DiscardGroupCall,
            Some("group call ended".to_string()),
            None,
            None,
            None,
        ),
        Action::ParticipantMute(value) => {
            let tl::enums::GroupCallParticipant::GroupCallParticipant(participant) =
                value.participant;
            let user_id = peer_user_id(&participant.peer);
            (
                AdminLogActionKind::ParticipantMute,
                user_id.map(|id| format!("muted user {id}")),
                user_id,
                None,
                None,
            )
        },
        Action::ParticipantUnmute(value) => {
            let tl::enums::GroupCallParticipant::GroupCallParticipant(participant) =
                value.participant;
            let user_id = peer_user_id(&participant.peer);
            (
                AdminLogActionKind::ParticipantUnmute,
                user_id.map(|id| format!("unmuted user {id}")),
                user_id,
                None,
                None,
            )
        },
        Action::ToggleGroupCallSetting(value) => (
            AdminLogActionKind::ToggleGroupCallSetting,
            Some(format!("new participants join muted: {}", value.join_muted)),
            None,
            None,
            None,
        ),
        Action::ParticipantJoinByInvite(value) => (
            AdminLogActionKind::ParticipantJoinByInvite,
            Some(format!("joined by invite; via chat list: {}", value.via_chatlist)),
            None,
            None,
            None,
        ),
        Action::ExportedInviteDelete(_) => (
            AdminLogActionKind::ExportedInviteDelete,
            Some("invite link deleted".to_string()),
            None,
            None,
            None,
        ),
        Action::ExportedInviteRevoke(_) => (
            AdminLogActionKind::ExportedInviteRevoke,
            Some("invite link revoked".to_string()),
            None,
            None,
            None,
        ),
        Action::ExportedInviteEdit(_) => (
            AdminLogActionKind::ExportedInviteEdit,
            Some("invite link edited".to_string()),
            None,
            None,
            None,
        ),
        Action::ParticipantVolume(value) => {
            let tl::enums::GroupCallParticipant::GroupCallParticipant(participant) =
                value.participant;
            let user_id = peer_user_id(&participant.peer);
            (
                AdminLogActionKind::ParticipantVolume,
                Some(format!("participant volume: {}", participant.volume.unwrap_or_default())),
                user_id,
                None,
                None,
            )
        },
        Action::ChangeHistoryTtl(value) => (
            AdminLogActionKind::ChangeHistoryTtl,
            Some(format!("{}s → {}s", value.prev_value, value.new_value)),
            None,
            None,
            None,
        ),
        Action::ParticipantJoinByRequest(value) => (
            AdminLogActionKind::ParticipantJoinByRequest,
            Some(format!("join request approved by user {}", value.approved_by)),
            None,
            None,
            None,
        ),
        Action::ToggleNoForwards(value) => (
            AdminLogActionKind::ToggleNoForwards,
            Some(format!("content protection enabled: {}", value.new_value)),
            None,
            None,
            None,
        ),
        Action::SendMessage(value) => {
            let id = message_id(&value.message);
            (AdminLogActionKind::SendMessage, Some(format!("message #{id}")), None, Some(id), None)
        },
        Action::ChangeAvailableReactions(_) => (
            AdminLogActionKind::ChangeAvailableReactions,
            Some("available reactions changed".to_string()),
            None,
            None,
            None,
        ),
        Action::ChangeUsernames(value) => (
            AdminLogActionKind::ChangeUsernames,
            Some(format!("{} → {}", value.prev_value.join(", "), value.new_value.join(", "))),
            None,
            None,
            None,
        ),
        Action::ToggleForum(value) => (
            AdminLogActionKind::ToggleForum,
            Some(format!("forum enabled: {}", value.new_value)),
            None,
            None,
            None,
        ),
        Action::CreateTopic(value) => {
            let id = forum_topic_id(&value.topic);
            (AdminLogActionKind::CreateTopic, Some(format!("topic #{id}")), None, None, Some(id))
        },
        Action::EditTopic(value) => {
            let id = forum_topic_id(&value.new_topic);
            (AdminLogActionKind::EditTopic, Some(format!("topic #{id}")), None, None, Some(id))
        },
        Action::DeleteTopic(value) => {
            let id = forum_topic_id(&value.topic);
            (AdminLogActionKind::DeleteTopic, Some(format!("topic #{id}")), None, None, Some(id))
        },
        Action::PinTopic(value) => {
            let id = value.new_topic.as_ref().or(value.prev_topic.as_ref()).map(forum_topic_id);
            (AdminLogActionKind::PinTopic, id.map(|id| format!("topic #{id}")), None, None, id)
        },
        Action::ToggleAntiSpam(value) => (
            AdminLogActionKind::ToggleAntiSpam,
            Some(format!("native anti-spam enabled: {}", value.new_value)),
            None,
            None,
            None,
        ),
        Action::ChangePeerColor(_) => (
            AdminLogActionKind::ChangePeerColor,
            Some("peer color changed".to_string()),
            None,
            None,
            None,
        ),
        Action::ChangeProfilePeerColor(_) => (
            AdminLogActionKind::ChangeProfilePeerColor,
            Some("profile color changed".to_string()),
            None,
            None,
            None,
        ),
        Action::ChangeWallpaper(_) => (
            AdminLogActionKind::ChangeWallpaper,
            Some("wallpaper changed".to_string()),
            None,
            None,
            None,
        ),
        Action::ChangeEmojiStatus(_) => (
            AdminLogActionKind::ChangeEmojiStatus,
            Some("emoji status changed".to_string()),
            None,
            None,
            None,
        ),
        Action::ChangeEmojiStickerSet(_) => (
            AdminLogActionKind::ChangeEmojiStickerSet,
            Some("emoji sticker set changed".to_string()),
            None,
            None,
            None,
        ),
        Action::ToggleSignatureProfiles(value) => (
            AdminLogActionKind::ToggleSignatureProfiles,
            Some(format!("signature profiles enabled: {}", value.new_value)),
            None,
            None,
            None,
        ),
        Action::ParticipantSubExtend(value) => {
            let user_id = channel_participant_user_id(&value.new_participant);
            (
                AdminLogActionKind::ParticipantSubExtend,
                user_id.map(|id| format!("subscription extended for user {id}")),
                user_id,
                None,
                None,
            )
        },
        Action::ToggleAutotranslation(value) => (
            AdminLogActionKind::ToggleAutotranslation,
            Some(format!("automatic translation enabled: {}", value.new_value)),
            None,
            None,
            None,
        ),
        Action::ParticipantEditRank(value) => (
            AdminLogActionKind::ParticipantEditRank,
            Some(format!("\"{}\" → \"{}\"", value.prev_rank, value.new_rank)),
            Some(value.user_id),
            None,
            None,
        ),
    }
}

fn stats_date_range_summary(value: &tl::enums::StatsDateRangeDays) -> StatsDateRangeSummary {
    let tl::enums::StatsDateRangeDays::StatsDateRangeDays(value) = value;
    StatsDateRangeSummary { min_date_unix: value.min_date, max_date_unix: value.max_date }
}

fn stats_value_summary(value: &tl::enums::StatsAbsValueAndPrev) -> StatsValueSummary {
    let tl::enums::StatsAbsValueAndPrev::StatsAbsValueAndPrev(value) = value;
    StatsValueSummary { current: value.current, previous: value.previous }
}

fn stats_percent_summary(value: &tl::enums::StatsPercentValue) -> StatsPercentSummary {
    let tl::enums::StatsPercentValue::StatsPercentValue(value) = value;
    StatsPercentSummary {
        part: value.part,
        total: value.total,
        percentage: (value.total != 0.0).then_some(value.part * 100.0 / value.total),
    }
}

fn stats_graph_summary(name: &str, graph: &tl::enums::StatsGraph) -> StatsGraphSummary {
    match graph {
        tl::enums::StatsGraph::StatsGraph(graph) => {
            let tl::enums::DataJson::DataJson(data) = &graph.json;
            StatsGraphSummary {
                name: name.to_string(),
                state: "loaded".to_string(),
                data: Some(
                    serde_json::from_str(&data.data)
                        .unwrap_or_else(|_| serde_json::Value::String(data.data.clone())),
                ),
                token: graph.zoom_token.clone(),
                error: None,
            }
        },
        tl::enums::StatsGraph::Async(graph) => StatsGraphSummary {
            name: name.to_string(),
            state: "pending".to_string(),
            data: None,
            token: Some(graph.token.clone()),
            error: None,
        },
        tl::enums::StatsGraph::Error(graph) => StatsGraphSummary {
            name: name.to_string(),
            state: "error".to_string(),
            data: None,
            token: None,
            error: Some(graph.error.clone()),
        },
    }
}

fn channel_stats_summary(chat_id: String, stats: ferogram::ChannelStats) -> ChannelStatsSummary {
    match stats {
        ferogram::ChannelStats::Broadcast(stats) => {
            let tl::enums::stats::BroadcastStats::BroadcastStats(stats) = stats;
            let graph_pairs = [
                ("growth", &stats.growth_graph),
                ("followers", &stats.followers_graph),
                ("mute", &stats.mute_graph),
                ("top_hours", &stats.top_hours_graph),
                ("interactions", &stats.interactions_graph),
                ("instant_view_interactions", &stats.iv_interactions_graph),
                ("views_by_source", &stats.views_by_source_graph),
                ("new_followers_by_source", &stats.new_followers_by_source_graph),
                ("languages", &stats.languages_graph),
                ("reactions_by_emotion", &stats.reactions_by_emotion_graph),
                ("story_interactions", &stats.story_interactions_graph),
                ("story_reactions_by_emotion", &stats.story_reactions_by_emotion_graph),
            ];
            let recent_interactions = stats
                .recent_posts_interactions
                .iter()
                .map(|item| match item {
                    tl::enums::PostInteractionCounters::Message(item) => {
                        StatsPostInteractionSummary::Message {
                            message_id: item.msg_id,
                            views: item.views,
                            forwards: item.forwards,
                            reactions: item.reactions,
                        }
                    },
                    tl::enums::PostInteractionCounters::Story(item) => {
                        StatsPostInteractionSummary::Story {
                            story_id: item.story_id,
                            views: item.views,
                            forwards: item.forwards,
                            reactions: item.reactions,
                        }
                    },
                })
                .collect();
            ChannelStatsSummary::Broadcast {
                chat_id,
                period: stats_date_range_summary(&stats.period),
                followers: stats_value_summary(&stats.followers),
                views_per_post: stats_value_summary(&stats.views_per_post),
                shares_per_post: stats_value_summary(&stats.shares_per_post),
                reactions_per_post: stats_value_summary(&stats.reactions_per_post),
                views_per_story: stats_value_summary(&stats.views_per_story),
                shares_per_story: stats_value_summary(&stats.shares_per_story),
                reactions_per_story: stats_value_summary(&stats.reactions_per_story),
                enabled_notifications: stats_percent_summary(&stats.enabled_notifications),
                graphs: graph_pairs
                    .into_iter()
                    .map(|(name, graph)| stats_graph_summary(name, graph))
                    .collect(),
                recent_interactions,
            }
        },
        ferogram::ChannelStats::Megagroup(stats) => {
            let tl::enums::stats::MegagroupStats::MegagroupStats(stats) = stats;
            let graph_pairs = [
                ("growth", &stats.growth_graph),
                ("members", &stats.members_graph),
                ("new_members_by_source", &stats.new_members_by_source_graph),
                ("languages", &stats.languages_graph),
                ("messages", &stats.messages_graph),
                ("actions", &stats.actions_graph),
                ("top_hours", &stats.top_hours_graph),
                ("weekdays", &stats.weekdays_graph),
            ];
            ChannelStatsSummary::Megagroup {
                chat_id,
                period: stats_date_range_summary(&stats.period),
                members: stats_value_summary(&stats.members),
                messages: stats_value_summary(&stats.messages),
                viewers: stats_value_summary(&stats.viewers),
                posters: stats_value_summary(&stats.posters),
                graphs: graph_pairs
                    .into_iter()
                    .map(|(name, graph)| stats_graph_summary(name, graph))
                    .collect(),
                top_posters: stats
                    .top_posters
                    .iter()
                    .map(|item| {
                        let tl::enums::StatsGroupTopPoster::StatsGroupTopPoster(item) = item;
                        StatsTopPosterSummary {
                            user_id: item.user_id,
                            messages: item.messages,
                            average_characters: item.avg_chars,
                        }
                    })
                    .collect(),
                top_admins: stats
                    .top_admins
                    .iter()
                    .map(|item| {
                        let tl::enums::StatsGroupTopAdmin::StatsGroupTopAdmin(item) = item;
                        StatsTopAdminSummary {
                            user_id: item.user_id,
                            deleted_messages: item.deleted,
                            kicked_users: item.kicked,
                            banned_users: item.banned,
                        }
                    })
                    .collect(),
                top_inviters: stats
                    .top_inviters
                    .iter()
                    .map(|item| {
                        let tl::enums::StatsGroupTopInviter::StatsGroupTopInviter(item) = item;
                        StatsTopInviterSummary {
                            user_id: item.user_id,
                            invitations: item.invitations,
                        }
                    })
                    .collect(),
            }
        },
    }
}

fn boost_summary(boost: tl::enums::Boost) -> BoostSummary {
    let tl::enums::Boost::Boost(boost) = boost;
    BoostSummary {
        boost_id: boost.id,
        user_id: boost.user_id,
        applied_at_unix: boost.date,
        expires_at_unix: boost.expires,
        gift: boost.gift,
        giveaway: boost.giveaway,
        unclaimed: boost.unclaimed,
        giveaway_message_id: boost.giveaway_msg_id,
        used_gift_slug: boost.used_gift_slug,
        multiplier: boost.multiplier,
        stars: boost.stars,
    }
}

fn my_boosts_summary(response: tl::enums::premium::MyBoosts) -> MyBoostsSummary {
    let tl::enums::premium::MyBoosts::MyBoosts(response) = response;
    let slots = response
        .my_boosts
        .into_iter()
        .map(|boost| {
            let tl::enums::MyBoost::MyBoost(boost) = boost;
            BoostSlotSummary {
                slot: boost.slot,
                chat_id: boost.peer.as_ref().map(canonical_peer_ref),
                applied_at_unix: boost.date,
                expires_at_unix: boost.expires,
                cooldown_until_unix: boost.cooldown_until_date,
            }
        })
        .collect::<Vec<_>>();
    MyBoostsSummary { returned_count: slots.len(), slots }
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

fn dialog_state_from_raw(dialog: &tl::enums::Dialog) -> Option<DialogStateSnapshot> {
    match dialog {
        tl::enums::Dialog::Dialog(dialog) => Some(DialogStateSnapshot {
            pinned: dialog.pinned,
            unread_mark: dialog.unread_mark,
            top_message_id: dialog.top_message,
            read_inbox_max_id: dialog.read_inbox_max_id,
            unread_count: dialog.unread_count,
            unread_reactions_count: dialog.unread_reactions_count,
            archived: dialog.folder_id == Some(1),
            folder_id: dialog.folder_id,
        }),
        tl::enums::Dialog::Folder(_) | tl::enums::Dialog::Community(_) => None,
    }
}

fn summarize_message(message: &ferogram::update::IncomingMessage) -> MessageSummary {
    MessageSummary {
        message_id: message.id(),
        chat_ref: message.peer_id().map(canonical_peer_ref),
        sender_ref: message.sender_id().map(canonical_peer_ref),
        outgoing: message.outgoing(),
        pinned: message.pinned(),
        silent: message.silent(),
        noforwards: message.noforwards(),
        text: message.text().map(ToOwned::to_owned),
        date: message.date(),
        edit_date: message.edit_date(),
        reply_to_message_id: message.reply_to_message_id(),
        views: message.view_count(),
        forwards: message.forward_count(),
        reply_count: message.reply_count(),
        media_type: message.media().map(media_type_name),
        forward_origin: summarize_forward_origin(message.forward_header()),
    }
}

fn raw_message_has_buttons(message: &ferogram::update::IncomingMessage) -> bool {
    let tl::enums::Message::Message(message) = &message.raw else {
        return false;
    };
    message.reply_markup.as_ref().is_some_and(|markup| match markup {
        tl::enums::ReplyMarkup::ReplyInlineMarkup(markup) => markup.rows.iter().any(|row| {
            let tl::enums::KeyboardButtonRow::KeyboardButtonRow(row) = row;
            !row.buttons.is_empty()
        }),
        tl::enums::ReplyMarkup::ReplyKeyboardMarkup(markup) => markup.rows.iter().any(|row| {
            let tl::enums::KeyboardButtonRow::KeyboardButtonRow(row) = row;
            !row.buttons.is_empty()
        }),
        tl::enums::ReplyMarkup::ReplyKeyboardHide(_)
        | tl::enums::ReplyMarkup::ReplyKeyboardForceReply(_) => false,
    })
}

fn message_button_inventory_from_raw(
    chat_id: &str,
    message: &tl::enums::Message,
) -> Option<MessageButtonInventory> {
    let (message_id, message_date_unix, reply_markup) = match message {
        tl::enums::Message::Message(message) => {
            (message.id, message.date, message.reply_markup.as_ref())
        },
        tl::enums::Message::Service(message) => (message.id, message.date, None),
        tl::enums::Message::Empty(_) => return None,
    };
    let (markup_type, rows) = match reply_markup {
        None => (MessageButtonMarkupType::None, None),
        Some(tl::enums::ReplyMarkup::ReplyInlineMarkup(markup)) => {
            (MessageButtonMarkupType::InlineKeyboard, Some(markup.rows.as_slice()))
        },
        Some(tl::enums::ReplyMarkup::ReplyKeyboardMarkup(markup)) => {
            (MessageButtonMarkupType::ReplyKeyboard, Some(markup.rows.as_slice()))
        },
        Some(tl::enums::ReplyMarkup::ReplyKeyboardForceReply(_)) => {
            (MessageButtonMarkupType::ForceReply, None)
        },
        Some(tl::enums::ReplyMarkup::ReplyKeyboardHide(_)) => {
            (MessageButtonMarkupType::HideKeyboard, None)
        },
    };
    let mut buttons = Vec::new();
    if let Some(rows) = rows {
        for (row_index, row) in rows.iter().enumerate() {
            let tl::enums::KeyboardButtonRow::KeyboardButtonRow(row) = row;
            for (column, button) in row.buttons.iter().enumerate() {
                let index = buttons.len();
                buttons.push(message_button_summary(button, row_index, column, index));
            }
        }
    }
    Some(MessageButtonInventory {
        chat_id: chat_id.to_string(),
        message_id,
        message_date_unix,
        markup_type,
        buttons,
    })
}

fn message_button_summary(
    button: &tl::enums::KeyboardButton,
    row: usize,
    column: usize,
    index: usize,
) -> MessageButtonSummary {
    let mut summary = match button {
        tl::enums::KeyboardButton::KeyboardButton(button) => message_button_base(
            row,
            column,
            index,
            MessageButtonKind::Plain,
            &button.text,
            button.style.as_ref(),
        ),
        tl::enums::KeyboardButton::Url(button) => {
            let mut summary = message_button_base(
                row,
                column,
                index,
                MessageButtonKind::Url,
                &button.text,
                button.style.as_ref(),
            );
            summary.url = Some(button.url.clone());
            summary
        },
        tl::enums::KeyboardButton::Callback(button) => {
            let mut summary = message_button_base(
                row,
                column,
                index,
                MessageButtonKind::Callback,
                &button.text,
                button.style.as_ref(),
            );
            summary.callback_data_base64 = Some(BASE64_STANDARD.encode(&button.data));
            summary.callback_data_sha256 = Some(format!("{:x}", Sha256::digest(&button.data)));
            summary.callback_data_length = Some(button.data.len());
            summary.requires_password = button.requires_password;
            summary
        },
        tl::enums::KeyboardButton::RequestPhone(button) => message_button_base(
            row,
            column,
            index,
            MessageButtonKind::RequestPhone,
            &button.text,
            button.style.as_ref(),
        ),
        tl::enums::KeyboardButton::RequestGeoLocation(button) => message_button_base(
            row,
            column,
            index,
            MessageButtonKind::RequestGeoLocation,
            &button.text,
            button.style.as_ref(),
        ),
        tl::enums::KeyboardButton::SwitchInline(button) => {
            let mut summary = message_button_base(
                row,
                column,
                index,
                MessageButtonKind::SwitchInline,
                &button.text,
                button.style.as_ref(),
            );
            summary.switch_query = Some(button.query.clone());
            summary.same_peer = Some(button.same_peer);
            summary.allowed_peer_types = button.peer_types.as_ref().map(|peer_types| {
                peer_types.iter().map(inline_query_peer_type_name).map(str::to_string).collect()
            });
            summary
        },
        tl::enums::KeyboardButton::Game(button) => message_button_base(
            row,
            column,
            index,
            MessageButtonKind::Game,
            &button.text,
            button.style.as_ref(),
        ),
        tl::enums::KeyboardButton::Buy(button) => message_button_base(
            row,
            column,
            index,
            MessageButtonKind::Buy,
            &button.text,
            button.style.as_ref(),
        ),
        tl::enums::KeyboardButton::UrlAuth(button) => {
            let mut summary = message_button_base(
                row,
                column,
                index,
                MessageButtonKind::UrlAuth,
                &button.text,
                button.style.as_ref(),
            );
            summary.url = Some(button.url.clone());
            summary.button_id = Some(button.button_id);
            summary.forward_text = button.fwd_text.clone();
            summary
        },
        tl::enums::KeyboardButton::InputKeyboardButtonUrlAuth(button) => {
            let mut summary = message_button_base(
                row,
                column,
                index,
                MessageButtonKind::InputUrlAuth,
                &button.text,
                button.style.as_ref(),
            );
            summary.url = Some(button.url.clone());
            summary.forward_text = button.fwd_text.clone();
            summary.request_write_access = Some(button.request_write_access);
            summary.bot_user_id = input_user_id(&button.bot);
            summary
        },
        tl::enums::KeyboardButton::RequestPoll(button) => {
            let mut summary = message_button_base(
                row,
                column,
                index,
                MessageButtonKind::RequestPoll,
                &button.text,
                button.style.as_ref(),
            );
            summary.quiz = button.quiz;
            summary
        },
        tl::enums::KeyboardButton::InputKeyboardButtonUserProfile(button) => {
            let mut summary = message_button_base(
                row,
                column,
                index,
                MessageButtonKind::InputUserProfile,
                &button.text,
                button.style.as_ref(),
            );
            summary.user_id = input_user_id(&button.user_id);
            summary
        },
        tl::enums::KeyboardButton::UserProfile(button) => {
            let mut summary = message_button_base(
                row,
                column,
                index,
                MessageButtonKind::UserProfile,
                &button.text,
                button.style.as_ref(),
            );
            summary.user_id = Some(button.user_id.to_string());
            summary
        },
        tl::enums::KeyboardButton::WebView(button) => {
            let mut summary = message_button_base(
                row,
                column,
                index,
                MessageButtonKind::WebView,
                &button.text,
                button.style.as_ref(),
            );
            summary.url = Some(button.url.clone());
            summary
        },
        tl::enums::KeyboardButton::SimpleWebView(button) => {
            let mut summary = message_button_base(
                row,
                column,
                index,
                MessageButtonKind::SimpleWebView,
                &button.text,
                button.style.as_ref(),
            );
            summary.url = Some(button.url.clone());
            summary
        },
        tl::enums::KeyboardButton::RequestPeer(button) => {
            let mut summary = message_button_base(
                row,
                column,
                index,
                MessageButtonKind::RequestPeer,
                &button.text,
                button.style.as_ref(),
            );
            summary.button_id = Some(button.button_id);
            summary.request_peer_kind = Some(request_peer_type_name(&button.peer_type).to_string());
            summary.max_quantity = Some(button.max_quantity);
            summary
        },
        tl::enums::KeyboardButton::InputKeyboardButtonRequestPeer(button) => {
            let mut summary = message_button_base(
                row,
                column,
                index,
                MessageButtonKind::InputRequestPeer,
                &button.text,
                button.style.as_ref(),
            );
            summary.button_id = Some(button.button_id);
            summary.request_peer_kind = Some(request_peer_type_name(&button.peer_type).to_string());
            summary.max_quantity = Some(button.max_quantity);
            summary.requested_name = Some(button.name_requested);
            summary.requested_username = Some(button.username_requested);
            summary.requested_photo = Some(button.photo_requested);
            summary
        },
        tl::enums::KeyboardButton::Copy(button) => {
            let mut summary = message_button_base(
                row,
                column,
                index,
                MessageButtonKind::Copy,
                &button.text,
                button.style.as_ref(),
            );
            summary.copy_text = Some(button.copy_text.clone());
            summary
        },
    };
    summary.label = summary.label.trim().to_string();
    summary
}

fn message_button_base(
    row: usize,
    column: usize,
    index: usize,
    kind: MessageButtonKind,
    label: &str,
    style: Option<&tl::enums::KeyboardButtonStyle>,
) -> MessageButtonSummary {
    MessageButtonSummary {
        row,
        column,
        index,
        kind,
        label: label.to_string(),
        style: style.map(|style| {
            let tl::enums::KeyboardButtonStyle::KeyboardButtonStyle(style) = style;
            MessageButtonStyleSummary {
                background_primary: style.bg_primary,
                background_danger: style.bg_danger,
                background_success: style.bg_success,
                icon_custom_emoji_id: style.icon.map(|icon| icon.to_string()),
            }
        }),
        callback_data_base64: None,
        callback_data_sha256: None,
        callback_data_length: None,
        url: None,
        switch_query: None,
        same_peer: None,
        user_id: None,
        bot_user_id: None,
        button_id: None,
        copy_text: None,
        requires_password: false,
        quiz: None,
        forward_text: None,
        request_write_access: None,
        allowed_peer_types: None,
        request_peer_kind: None,
        max_quantity: None,
        requested_name: None,
        requested_username: None,
        requested_photo: None,
    }
}

fn input_user_id(user: &tl::enums::InputUser) -> Option<String> {
    match user {
        tl::enums::InputUser::InputUser(user) => Some(user.user_id.to_string()),
        tl::enums::InputUser::FromMessage(user) => Some(user.user_id.to_string()),
        tl::enums::InputUser::Empty | tl::enums::InputUser::UserSelf => None,
    }
}

fn inline_query_peer_type_name(peer_type: &tl::enums::InlineQueryPeerType) -> &'static str {
    match peer_type {
        tl::enums::InlineQueryPeerType::SameBotPm => "same_bot_pm",
        tl::enums::InlineQueryPeerType::Pm => "pm",
        tl::enums::InlineQueryPeerType::Chat => "chat",
        tl::enums::InlineQueryPeerType::Megagroup => "megagroup",
        tl::enums::InlineQueryPeerType::Broadcast => "broadcast",
        tl::enums::InlineQueryPeerType::BotPm => "bot_pm",
    }
}

fn request_peer_type_name(peer_type: &tl::enums::RequestPeerType) -> &'static str {
    match peer_type {
        tl::enums::RequestPeerType::User(_) => "user",
        tl::enums::RequestPeerType::Chat(_) => "chat",
        tl::enums::RequestPeerType::Broadcast(_) => "broadcast",
        tl::enums::RequestPeerType::CreateBot(_) => "create_bot",
    }
}

fn callback_answer_request(
    peer: tl::enums::InputPeer,
    message_id: i32,
    data: Vec<u8>,
) -> tl::functions::messages::GetBotCallbackAnswer {
    tl::functions::messages::GetBotCallbackAnswer {
        game: false,
        peer,
        msg_id: message_id,
        data: Some(data),
        password: None,
    }
}

fn inline_bot_results_request(
    bot: tl::enums::InputUser,
    peer: tl::enums::InputPeer,
    query: String,
    offset: String,
) -> tl::functions::messages::GetInlineBotResults {
    tl::functions::messages::GetInlineBotResults { bot, peer, geo_point: None, query, offset }
}

fn empty_emoji_status_summary() -> EmojiStatusSummary {
    EmojiStatusSummary {
        kind: EmojiStatusKind::Empty,
        document_id: None,
        collectible_id: None,
        until_unix: None,
        title: None,
        slug: None,
        pattern_document_id: None,
        center_color: None,
        edge_color: None,
        pattern_color: None,
        text_color: None,
    }
}

fn emoji_status_summary(status: tl::enums::EmojiStatus) -> EmojiStatusSummary {
    match status {
        tl::enums::EmojiStatus::Empty => empty_emoji_status_summary(),
        tl::enums::EmojiStatus::EmojiStatus(status) => EmojiStatusSummary {
            kind: EmojiStatusKind::Default,
            document_id: Some(status.document_id.to_string()),
            collectible_id: None,
            until_unix: status.until,
            title: None,
            slug: None,
            pattern_document_id: None,
            center_color: None,
            edge_color: None,
            pattern_color: None,
            text_color: None,
        },
        tl::enums::EmojiStatus::Collectible(status) => EmojiStatusSummary {
            kind: EmojiStatusKind::Collectible,
            document_id: Some(status.document_id.to_string()),
            collectible_id: Some(status.collectible_id.to_string()),
            until_unix: status.until,
            title: Some(status.title),
            slug: Some(status.slug),
            pattern_document_id: Some(status.pattern_document_id.to_string()),
            center_color: Some(status.center_color),
            edge_color: Some(status.edge_color),
            pattern_color: Some(status.pattern_color),
            text_color: Some(status.text_color),
        },
        tl::enums::EmojiStatus::InputEmojiStatusCollectible(status) => EmojiStatusSummary {
            kind: EmojiStatusKind::Collectible,
            document_id: None,
            collectible_id: Some(status.collectible_id.to_string()),
            until_unix: status.until,
            title: None,
            slug: None,
            pattern_document_id: None,
            center_color: None,
            edge_color: None,
            pattern_color: None,
            text_color: None,
        },
    }
}

fn positive_emoji_id(value: &str, field: &str) -> Result<i64, TelegramError> {
    value.parse::<i64>().ok().filter(|value| *value > 0).ok_or_else(|| {
        TelegramError::InvalidPeer(format!(
            "{field} must be a canonical positive Telegram i64 decimal id"
        ))
    })
}

fn peer_color_summary(color: tl::enums::PeerColor) -> PeerColorSummary {
    match color {
        tl::enums::PeerColor::PeerColor(color) => PeerColorSummary::Simple {
            color: color.color,
            background_emoji_id: color.background_emoji_id.map(|value| value.to_string()),
        },
        tl::enums::PeerColor::Collectible(color) => PeerColorSummary::Collectible {
            collectible_id: color.collectible_id.to_string(),
            gift_emoji_id: color.gift_emoji_id.to_string(),
            background_emoji_id: color.background_emoji_id.to_string(),
            accent_color: color.accent_color,
            colors: color.colors,
            dark_accent_color: color.dark_accent_color,
            dark_colors: color.dark_colors,
        },
        tl::enums::PeerColor::InputPeerColorCollectible(color) => {
            PeerColorSummary::InputCollectible { collectible_id: color.collectible_id.to_string() }
        },
    }
}

fn account_color_update_request(
    for_profile: bool,
    color: Option<i32>,
    background_emoji_id: Option<&str>,
) -> Result<tl::functions::account::UpdateColor, TelegramError> {
    let background_emoji_id = background_emoji_id
        .map(|value| positive_emoji_id(value, "background_emoji_id"))
        .transpose()?;
    let color = if color.is_none() && background_emoji_id.is_none() {
        None
    } else {
        Some(tl::enums::PeerColor::PeerColor(tl::types::PeerColor { color, background_emoji_id }))
    };
    Ok(tl::functions::account::UpdateColor { for_profile, color })
}

fn input_channel_from_peer(
    peer: tl::enums::InputPeer,
    operation: &str,
) -> Result<tl::enums::InputChannel, TelegramError> {
    match peer {
        tl::enums::InputPeer::Channel(channel) => {
            Ok(tl::enums::InputChannel::InputChannel(tl::types::InputChannel {
                channel_id: channel.channel_id,
                access_hash: channel.access_hash,
            }))
        },
        tl::enums::InputPeer::ChannelFromMessage(channel) => {
            Ok(tl::enums::InputChannel::FromMessage(tl::types::InputChannelFromMessage {
                peer: channel.peer,
                msg_id: channel.msg_id,
                channel_id: channel.channel_id,
            }))
        },
        _ => Err(TelegramError::InvalidPeer(format!(
            "{operation} requires a Telegram channel or supergroup"
        ))),
    }
}

fn input_channel_for_personal_channel(
    peer: tl::enums::InputPeer,
) -> Result<tl::enums::InputChannel, TelegramError> {
    input_channel_from_peer(peer, "personal channel")
}

fn emoji_status_input(
    status: &AccountEmojiStatusSpec,
) -> Result<tl::enums::EmojiStatus, TelegramError> {
    match status {
        AccountEmojiStatusSpec::Empty => Ok(tl::enums::EmojiStatus::Empty),
        AccountEmojiStatusSpec::Default { document_id, until_unix } => {
            Ok(tl::enums::EmojiStatus::EmojiStatus(tl::types::EmojiStatus {
                document_id: positive_emoji_id(document_id, "document_id")?,
                until: *until_unix,
            }))
        },
        AccountEmojiStatusSpec::Collectible { collectible_id, until_unix } => {
            Ok(tl::enums::EmojiStatus::InputEmojiStatusCollectible(
                tl::types::InputEmojiStatusCollectible {
                    collectible_id: positive_emoji_id(collectible_id, "collectible_id")?,
                    until: *until_unix,
                },
            ))
        },
    }
}

fn bot_commands_get_request(language_code: &str) -> tl::functions::bots::GetBotCommands {
    tl::functions::bots::GetBotCommands {
        scope: tl::enums::BotCommandScope::Default,
        lang_code: language_code.to_string(),
    }
}

fn bot_commands_set_request(
    language_code: &str,
    commands: &[BotCommandSummary],
) -> tl::functions::bots::SetBotCommands {
    tl::functions::bots::SetBotCommands {
        scope: tl::enums::BotCommandScope::Default,
        lang_code: language_code.to_string(),
        commands: commands
            .iter()
            .map(|command| {
                tl::enums::BotCommand::BotCommand(tl::types::BotCommand {
                    ephemeral: command.ephemeral,
                    command: command.command.clone(),
                    description: command.description.clone(),
                })
            })
            .collect(),
    }
}

fn bot_commands_reset_request(language_code: &str) -> tl::functions::bots::ResetBotCommands {
    tl::functions::bots::ResetBotCommands {
        scope: tl::enums::BotCommandScope::Default,
        lang_code: language_code.to_string(),
    }
}

#[allow(clippy::too_many_arguments)]
fn inline_bot_result_send_request(
    peer: tl::enums::InputPeer,
    query_id: i64,
    result_id: String,
    reply_to_message_id: Option<i32>,
    silent: bool,
    hide_via: bool,
    clear_draft: bool,
    random_id: i64,
) -> tl::functions::messages::SendInlineBotResult {
    tl::functions::messages::SendInlineBotResult {
        silent,
        background: false,
        clear_draft,
        hide_via,
        peer,
        reply_to: input_reply_to(reply_to_message_id, None),
        random_id,
        query_id,
        id: result_id,
        schedule_date: None,
        send_as: None,
        quick_reply_shortcut: None,
        allow_paid_stars: None,
    }
}

fn inline_bot_result_summary(
    result: tl::enums::BotInlineResult,
) -> Result<InlineBotResultSummary, TelegramError> {
    Ok(match result {
        tl::enums::BotInlineResult::BotInlineResult(result) => {
            let (
                send_message_kind,
                send_message_text,
                send_message_url,
                send_message_geo,
                has_reply_markup,
            ) = inline_bot_send_message_summary(&result.send_message)?;
            InlineBotResultSummary {
                id: result.id,
                result_type: result.r#type,
                source_kind: InlineBotResultSourceKind::Result,
                title: result.title,
                description: result.description,
                url: result.url,
                media_kind: result.content.as_ref().map(|_| "web_document".to_string()),
                has_thumbnail: result.thumb.is_some(),
                has_content: result.content.is_some(),
                send_message_kind: send_message_kind.to_string(),
                send_message_text,
                send_message_url,
                send_message_geo,
                send_message_has_reply_markup: has_reply_markup,
            }
        },
        tl::enums::BotInlineResult::BotInlineMediaResult(result) => {
            let media_kind = match (result.photo.is_some(), result.document.is_some()) {
                (true, false) => Some("photo".to_string()),
                (false, true) => Some("document".to_string()),
                (true, true) => Some("photo_and_document".to_string()),
                (false, false) => None,
            };
            let (
                send_message_kind,
                send_message_text,
                send_message_url,
                send_message_geo,
                has_reply_markup,
            ) = inline_bot_send_message_summary(&result.send_message)?;
            InlineBotResultSummary {
                id: result.id,
                result_type: result.r#type,
                source_kind: InlineBotResultSourceKind::MediaResult,
                title: result.title,
                description: result.description,
                url: None,
                has_thumbnail: false,
                has_content: media_kind.is_some(),
                media_kind,
                send_message_kind: send_message_kind.to_string(),
                send_message_text,
                send_message_url,
                send_message_geo,
                send_message_has_reply_markup: has_reply_markup,
            }
        },
    })
}

type InlineBotSendMessageSummary =
    (&'static str, Option<String>, Option<String>, Option<InlineBotGeoSummary>, bool);

fn inline_bot_send_message_summary(
    message: &tl::enums::BotInlineMessage,
) -> Result<InlineBotSendMessageSummary, TelegramError> {
    Ok(match message {
        tl::enums::BotInlineMessage::MediaAuto(message) => (
            "media_auto",
            (!message.message.is_empty()).then(|| message.message.clone()),
            None,
            None,
            message.reply_markup.is_some(),
        ),
        tl::enums::BotInlineMessage::Text(message) => (
            "text",
            (!message.message.is_empty()).then(|| message.message.clone()),
            None,
            None,
            message.reply_markup.is_some(),
        ),
        tl::enums::BotInlineMessage::MediaGeo(message) => (
            "geo",
            None,
            None,
            Some(InlineBotGeoSummary {
                point: geo_point_from_tl(&message.geo)?,
                heading_degrees: message.heading,
                live_period_seconds: message.period,
                proximity_notification_radius_m: message.proximity_notification_radius,
            }),
            message.reply_markup.is_some(),
        ),
        tl::enums::BotInlineMessage::MediaVenue(message) => (
            "venue",
            Some(format!("{} — {}", message.title, message.address)),
            None,
            None,
            message.reply_markup.is_some(),
        ),
        tl::enums::BotInlineMessage::MediaContact(message) => (
            "contact",
            Some(format!(
                "{} {} ({})",
                message.first_name, message.last_name, message.phone_number
            )),
            None,
            None,
            message.reply_markup.is_some(),
        ),
        tl::enums::BotInlineMessage::MediaInvoice(message) => (
            "invoice",
            Some(format!(
                "{} — {} {} minor units; {}",
                message.title, message.total_amount, message.currency, message.description
            )),
            None,
            None,
            message.reply_markup.is_some(),
        ),
        tl::enums::BotInlineMessage::MediaWebPage(message) => (
            "web_page",
            (!message.message.is_empty()).then(|| message.message.clone()),
            Some(message.url.clone()),
            None,
            message.reply_markup.is_some(),
        ),
        tl::enums::BotInlineMessage::RichMessage(message) => {
            ("rich_message", None, None, None, message.reply_markup.is_some())
        },
    })
}

fn summarize_raw_message(message: &tl::enums::Message) -> Option<MessageSummary> {
    match message {
        tl::enums::Message::Message(message) => Some(MessageSummary {
            message_id: message.id,
            chat_ref: Some(canonical_peer_ref(&message.peer_id)),
            sender_ref: message.from_id.as_ref().map(canonical_peer_ref),
            outgoing: message.out,
            pinned: message.pinned,
            silent: message.silent,
            noforwards: message.noforwards,
            text: (!message.message.is_empty()).then(|| message.message.clone()),
            date: message.date,
            edit_date: message.edit_date,
            reply_to_message_id: message.reply_to.as_ref().and_then(|reply| match reply {
                tl::enums::MessageReplyHeader::MessageReplyHeader(header) => header.reply_to_msg_id,
                _ => None,
            }),
            views: message.views,
            forwards: message.forwards,
            reply_count: message.replies.as_ref().map(|replies| match replies {
                tl::enums::MessageReplies::MessageReplies(replies) => replies.replies,
            }),
            media_type: message.media.as_ref().map(media_type_name),
            forward_origin: summarize_forward_origin(message.fwd_from.as_ref()),
        }),
        tl::enums::Message::Service(message) => Some(MessageSummary {
            message_id: message.id,
            chat_ref: Some(canonical_peer_ref(&message.peer_id)),
            sender_ref: message.from_id.as_ref().map(canonical_peer_ref),
            outgoing: message.out,
            pinned: false,
            silent: message.silent,
            noforwards: false,
            text: None,
            date: message.date,
            edit_date: None,
            reply_to_message_id: message.reply_to.as_ref().and_then(|reply| match reply {
                tl::enums::MessageReplyHeader::MessageReplyHeader(header) => header.reply_to_msg_id,
                _ => None,
            }),
            views: None,
            forwards: None,
            reply_count: None,
            media_type: None,
            forward_origin: None,
        }),
        tl::enums::Message::Empty(_) => None,
    }
}

fn quick_reply_stored_message_summary(
    message: &tl::enums::Message,
) -> Option<QuickReplyStoredMessageSummary> {
    let summary = summarize_raw_message(message)?;
    let (is_service, service_action_kind, quick_reply_shortcut_id) = match message {
        tl::enums::Message::Message(message) => (false, None, message.quick_reply_shortcut_id),
        tl::enums::Message::Service(message) => {
            (true, Some(tl_variant_kind(&message.action)), None)
        },
        tl::enums::Message::Empty(_) => return None,
    };
    Some(QuickReplyStoredMessageSummary {
        message: summary,
        is_service,
        service_action_kind,
        quick_reply_shortcut_id,
    })
}

fn tl_variant_kind<T: std::fmt::Debug>(value: &T) -> String {
    let debug = format!("{value:?}");
    let variant = debug.split(['(', '{']).next().unwrap_or(debug.as_str());
    let mut kind = String::with_capacity(variant.len().saturating_add(4));
    for (index, character) in variant.chars().enumerate() {
        if character.is_ascii_uppercase() && index > 0 {
            kind.push('_');
        }
        kind.push(character.to_ascii_lowercase());
    }
    kind
}

fn updates_state_summary(state: tl::enums::updates::State) -> UpdatesStateSummary {
    let tl::enums::updates::State::State(state) = state;
    UpdatesStateSummary {
        pts: state.pts,
        qts: state.qts,
        date: state.date,
        seq: state.seq,
        unread_count: state.unread_count,
    }
}

fn updates_cursor_summary(
    state: tl::enums::updates::State,
    include_unread_count: bool,
) -> UpdatesCursorSummary {
    let state = updates_state_summary(state);
    UpdatesCursorSummary {
        pts: state.pts,
        qts: state.qts,
        date: state.date,
        seq: state.seq,
        unread_count: include_unread_count.then_some(state.unread_count),
    }
}

fn split_update_summaries(
    updates: Vec<tl::enums::Update>,
) -> (Vec<MessageSummary>, Vec<DeletedMessagesSummary>, Vec<UpdateKindSummary>) {
    let mut edited_messages = Vec::new();
    let mut deleted_messages = Vec::new();
    let mut other_updates = Vec::new();
    for update in updates {
        match update {
            tl::enums::Update::EditMessage(update) => {
                if let Some(message) = summarize_raw_message(&update.message) {
                    edited_messages.push(message);
                }
            },
            tl::enums::Update::EditChannelMessage(update) => {
                if let Some(message) = summarize_raw_message(&update.message) {
                    edited_messages.push(message);
                }
            },
            tl::enums::Update::DeleteMessages(update) => {
                deleted_messages.push(DeletedMessagesSummary::Global {
                    message_ids: update.messages,
                    pts: update.pts,
                    pts_count: update.pts_count,
                });
            },
            tl::enums::Update::DeleteChannelMessages(update) => {
                deleted_messages.push(DeletedMessagesSummary::Channel {
                    chat_id: format!("-100{}", update.channel_id),
                    message_ids: update.messages,
                    pts: update.pts,
                    pts_count: update.pts_count,
                });
            },
            update => other_updates.push(UpdateKindSummary { kind: tl_variant_kind(&update) }),
        }
    }
    (edited_messages, deleted_messages, other_updates)
}

fn updates_difference_summary(
    request: UpdatesDifferenceRequestSummary,
    response: tl::enums::updates::Difference,
) -> Result<UpdatesDifferenceSummary, TelegramError> {
    Ok(match response {
        tl::enums::updates::Difference::Empty(response) => UpdatesDifferenceSummary {
            next_cursor: Some(UpdatesCursorSummary {
                pts: request.pts,
                qts: request.qts,
                date: response.date,
                seq: response.seq,
                unread_count: None,
            }),
            request,
            kind: UpdatesDifferenceKind::Empty,
            is_final: true,
            continuation_required: false,
            requires_resync: false,
            resync_pts: None,
            new_messages: Vec::new(),
            edited_messages: Vec::new(),
            encrypted_message_count: 0,
            deleted_messages: Vec::new(),
            other_updates: Vec::new(),
            related_chats: Vec::new(),
            related_users: Vec::new(),
        },
        tl::enums::updates::Difference::Difference(response) => {
            let (edited_messages, deleted_messages, other_updates) =
                split_update_summaries(response.other_updates);
            UpdatesDifferenceSummary {
                request,
                kind: UpdatesDifferenceKind::Complete,
                is_final: true,
                continuation_required: false,
                requires_resync: false,
                next_cursor: Some(updates_cursor_summary(response.state, true)),
                resync_pts: None,
                new_messages: response
                    .new_messages
                    .iter()
                    .filter_map(summarize_raw_message)
                    .collect(),
                edited_messages,
                encrypted_message_count: response.new_encrypted_messages.len(),
                deleted_messages,
                other_updates,
                related_chats: response.chats.into_iter().filter_map(chat_summary).collect(),
                related_users: response.users.into_iter().filter_map(contact_summary).collect(),
            }
        },
        tl::enums::updates::Difference::Slice(response) => {
            let (edited_messages, deleted_messages, other_updates) =
                split_update_summaries(response.other_updates);
            UpdatesDifferenceSummary {
                request,
                kind: UpdatesDifferenceKind::Slice,
                is_final: false,
                continuation_required: true,
                requires_resync: false,
                next_cursor: Some(updates_cursor_summary(response.intermediate_state, true)),
                resync_pts: None,
                new_messages: response
                    .new_messages
                    .iter()
                    .filter_map(summarize_raw_message)
                    .collect(),
                edited_messages,
                encrypted_message_count: response.new_encrypted_messages.len(),
                deleted_messages,
                other_updates,
                related_chats: response.chats.into_iter().filter_map(chat_summary).collect(),
                related_users: response.users.into_iter().filter_map(contact_summary).collect(),
            }
        },
        tl::enums::updates::Difference::TooLong(response) => UpdatesDifferenceSummary {
            request,
            kind: UpdatesDifferenceKind::TooLong,
            is_final: false,
            continuation_required: true,
            requires_resync: true,
            next_cursor: None,
            resync_pts: Some(response.pts),
            new_messages: Vec::new(),
            edited_messages: Vec::new(),
            encrypted_message_count: 0,
            deleted_messages: Vec::new(),
            other_updates: Vec::new(),
            related_chats: Vec::new(),
            related_users: Vec::new(),
        },
    })
}

fn channel_updates_difference_summary(
    request: ChannelUpdatesRequestSummary,
    response: tl::enums::updates::ChannelDifference,
) -> Result<ChannelUpdatesDifferenceSummary, TelegramError> {
    Ok(match response {
        tl::enums::updates::ChannelDifference::Empty(response) => ChannelUpdatesDifferenceSummary {
            request,
            kind: ChannelUpdatesDifferenceKind::Empty,
            is_final: response.r#final,
            continuation_required: !response.r#final,
            requires_resync: false,
            next_pts: Some(response.pts),
            resync_pts: None,
            timeout_seconds: response.timeout,
            new_messages: Vec::new(),
            edited_messages: Vec::new(),
            snapshot_messages: Vec::new(),
            deleted_messages: Vec::new(),
            other_updates: Vec::new(),
            related_chats: Vec::new(),
            related_users: Vec::new(),
        },
        tl::enums::updates::ChannelDifference::ChannelDifference(response) => {
            let (edited_messages, deleted_messages, other_updates) =
                split_update_summaries(response.other_updates);
            ChannelUpdatesDifferenceSummary {
                request,
                kind: ChannelUpdatesDifferenceKind::Difference,
                is_final: response.r#final,
                continuation_required: !response.r#final,
                requires_resync: false,
                next_pts: Some(response.pts),
                resync_pts: None,
                timeout_seconds: response.timeout,
                new_messages: response
                    .new_messages
                    .iter()
                    .filter_map(summarize_raw_message)
                    .collect(),
                edited_messages,
                snapshot_messages: Vec::new(),
                deleted_messages,
                other_updates,
                related_chats: response.chats.into_iter().filter_map(chat_summary).collect(),
                related_users: response.users.into_iter().filter_map(contact_summary).collect(),
            }
        },
        tl::enums::updates::ChannelDifference::TooLong(response) => {
            let (dialog_chat_id, resync_pts) = match response.dialog {
                tl::enums::Dialog::Dialog(dialog) => (
                    canonical_peer_ref(&dialog.peer),
                    dialog.pts.ok_or_else(|| {
                        TelegramError::Telegram(
                            "Telegram returned channelDifferenceTooLong without dialog.pts"
                                .to_string(),
                        )
                    })?,
                ),
                tl::enums::Dialog::Folder(_) | tl::enums::Dialog::Community(_) => {
                    return Err(TelegramError::Telegram(
                        "Telegram returned channelDifferenceTooLong with a non-channel dialog"
                            .to_string(),
                    ));
                },
            };
            if dialog_chat_id != request.chat_id {
                return Err(TelegramError::Telegram(format!(
                    "Telegram returned channelDifferenceTooLong for {dialog_chat_id}, expected {}",
                    request.chat_id
                )));
            }
            ChannelUpdatesDifferenceSummary {
                request,
                kind: ChannelUpdatesDifferenceKind::TooLong,
                is_final: response.r#final,
                continuation_required: true,
                requires_resync: true,
                next_pts: None,
                resync_pts: Some(resync_pts),
                timeout_seconds: response.timeout,
                new_messages: Vec::new(),
                edited_messages: Vec::new(),
                snapshot_messages: response
                    .messages
                    .iter()
                    .filter_map(summarize_raw_message)
                    .collect(),
                deleted_messages: Vec::new(),
                other_updates: Vec::new(),
                related_chats: response.chats.into_iter().filter_map(chat_summary).collect(),
                related_users: response.users.into_iter().filter_map(contact_summary).collect(),
            }
        },
    })
}

fn quick_reply_catalog_page(response: tl::enums::messages::QuickReplies) -> QuickReplyCatalogPage {
    match response {
        tl::enums::messages::QuickReplies::NotModified => QuickReplyCatalogPage {
            not_modified: true,
            shortcuts: Vec::new(),
            top_messages: Vec::new(),
            related_chats: Vec::new(),
            related_users: Vec::new(),
        },
        tl::enums::messages::QuickReplies::QuickReplies(response) => QuickReplyCatalogPage {
            not_modified: false,
            shortcuts: response
                .quick_replies
                .into_iter()
                .map(|shortcut| {
                    let tl::enums::QuickReply::QuickReply(shortcut) = shortcut;
                    QuickReplyShortcutSummary {
                        shortcut_id: shortcut.shortcut_id,
                        shortcut: shortcut.shortcut,
                        top_message_id: shortcut.top_message,
                        count: shortcut.count,
                    }
                })
                .collect(),
            top_messages: response
                .messages
                .iter()
                .filter_map(quick_reply_stored_message_summary)
                .collect(),
            related_chats: response.chats.into_iter().filter_map(chat_summary).collect(),
            related_users: response.users.into_iter().filter_map(contact_summary).collect(),
        },
    }
}

fn quick_reply_messages_page(
    shortcut_id: i32,
    response: tl::enums::messages::Messages,
) -> QuickReplyMessagesPage {
    let (not_modified, messages, total_count, chats, users) = match response {
        tl::enums::messages::Messages::Messages(response) => {
            let count = i32::try_from(response.messages.len()).ok();
            (false, response.messages, count, response.chats, response.users)
        },
        tl::enums::messages::Messages::Slice(response) => {
            (false, response.messages, Some(response.count), response.chats, response.users)
        },
        tl::enums::messages::Messages::ChannelMessages(response) => {
            (false, response.messages, Some(response.count), response.chats, response.users)
        },
        tl::enums::messages::Messages::NotModified(_) => {
            (true, Vec::new(), None, Vec::new(), Vec::new())
        },
    };
    QuickReplyMessagesPage {
        shortcut_id,
        not_modified,
        total_count,
        messages: messages.iter().filter_map(quick_reply_stored_message_summary).collect(),
        related_chats: chats.into_iter().filter_map(chat_summary).collect(),
        related_users: users.into_iter().filter_map(contact_summary).collect(),
    }
}

fn serialized_message_entities_sha256(entities: &[tl::enums::MessageEntity]) -> String {
    let mut serialized = Vec::new();
    tl::Serializable::serialize(&entities.to_vec(), &mut serialized);
    format!("{:x}", Sha256::digest(serialized))
}

fn rich_message_snapshot(
    chat_id: String,
    message_id: i32,
    rich_message: &tl::enums::RichMessage,
) -> RichMessageSnapshot {
    let mut serialized = Vec::new();
    tl::Serializable::serialize(rich_message, &mut serialized);
    match rich_message {
        tl::enums::RichMessage::RichMessage(value) => RichMessageSnapshot {
            chat_id,
            message_id,
            rtl: value.rtl,
            part: value.part,
            block_count: value.blocks.len(),
            photo_count: value.photos.len(),
            document_count: value.documents.len(),
            state_sha256: format!("{:x}", Sha256::digest(serialized)),
        },
    }
}

fn business_chat_link_slug(link: &str) -> Result<String, TelegramError> {
    let path = link.split(['?', '#']).next().unwrap_or(link);
    path.split('/').rev().find(|segment| !segment.is_empty()).map(str::to_string).ok_or_else(|| {
        TelegramError::Telegram(
            "Telegram returned a Business chat link without a final path slug".to_string(),
        )
    })
}

fn business_chat_link_summary(
    link: tl::enums::BusinessChatLink,
) -> Result<BusinessChatLinkSummary, TelegramError> {
    let tl::enums::BusinessChatLink::BusinessChatLink(link) = link;
    let slug = business_chat_link_slug(&link.link)?;
    let entities = link.entities.unwrap_or_default();
    Ok(BusinessChatLinkSummary {
        slug,
        link: link.link,
        message: link.message,
        entity_count: entities.len(),
        entities_sha256: serialized_message_entities_sha256(&entities),
        title: link.title,
        views: link.views,
    })
}

fn business_chat_links_page(
    response: tl::types::account::BusinessChatLinks,
) -> Result<BusinessChatLinksPage, TelegramError> {
    let links = response
        .links
        .into_iter()
        .map(business_chat_link_summary)
        .collect::<Result<Vec<_>, _>>()?;
    Ok(BusinessChatLinksPage {
        count: links.len(),
        links,
        related_chats: response.chats.into_iter().filter_map(chat_summary).collect(),
        related_users: response.users.into_iter().filter_map(contact_summary).collect(),
    })
}

fn resolved_business_chat_link_summary(
    response: tl::types::account::ResolvedBusinessChatLinks,
) -> ResolvedBusinessChatLinkSummary {
    let entities = response.entities.unwrap_or_default();
    ResolvedBusinessChatLinkSummary {
        peer_ref: canonical_peer_ref(&response.peer),
        message: response.message,
        entity_count: entities.len(),
        entities_sha256: serialized_message_entities_sha256(&entities),
        related_chats: response.chats.into_iter().filter_map(chat_summary).collect(),
        related_users: response.users.into_iter().filter_map(contact_summary).collect(),
    }
}

fn business_chat_link_content_input(
    content: &BusinessChatLinkContent,
) -> Result<tl::enums::InputBusinessChatLink, TelegramError> {
    reject_rich_media_caption(content.parse_mode, "Business chat link")?;
    let parsed = caption_input(&content.source_message, content.parse_mode);
    let entities = parsed.entities.unwrap_or_default();
    if business_chat_link_content_from_source(
        &content.source_message,
        content.parse_mode,
        content.title.clone(),
    ) != *content
    {
        return Err(TelegramError::InvalidPeer(
            "immutable Business chat-link content no longer matches its parsed entity binding"
                .to_string(),
        ));
    }
    Ok(tl::enums::InputBusinessChatLink::InputBusinessChatLink(tl::types::InputBusinessChatLink {
        message: parsed.text,
        entities: (!entities.is_empty()).then_some(entities),
        title: content.title.clone(),
    }))
}

pub(crate) fn business_chat_link_content_from_source(
    source_message: &str,
    parse_mode: CaptionParseMode,
    title: Option<String>,
) -> BusinessChatLinkContent {
    let parsed = caption_input(source_message, parse_mode);
    let entities = parsed.entities.unwrap_or_default();
    BusinessChatLinkContent {
        source_message: source_message.to_string(),
        parse_mode,
        rendered_message: parsed.text,
        entity_count: entities.len(),
        entities_sha256: serialized_message_entities_sha256(&entities),
        title,
    }
}

fn business_work_hours_summary(value: tl::enums::BusinessWorkHours) -> BusinessWorkHoursSummary {
    let tl::enums::BusinessWorkHours::BusinessWorkHours(value) = value;
    BusinessWorkHoursSummary {
        config: BusinessWorkHoursConfig {
            timezone_id: value.timezone_id,
            weekly_open: value
                .weekly_open
                .into_iter()
                .map(|interval| {
                    let tl::enums::BusinessWeeklyOpen::BusinessWeeklyOpen(interval) = interval;
                    BusinessWeeklyOpenSummary {
                        start_minute: interval.start_minute,
                        end_minute: interval.end_minute,
                    }
                })
                .collect(),
        },
        open_now: value.open_now,
    }
}

fn business_location_summary(
    value: tl::enums::BusinessLocation,
) -> Result<BusinessLocationSummary, TelegramError> {
    let tl::enums::BusinessLocation::BusinessLocation(value) = value;
    let point = match value.geo_point {
        Some(point) => geo_point_from_tl(&point)?,
        None => None,
    };
    Ok(BusinessLocationSummary { address: value.address, point })
}

fn business_recipients_summary(value: tl::enums::BusinessRecipients) -> BusinessRecipientsSummary {
    let tl::enums::BusinessRecipients::BusinessRecipients(value) = value;
    let mut user_ids = value.users.unwrap_or_default();
    user_ids.sort_unstable();
    user_ids.dedup();
    BusinessRecipientsSummary {
        existing_chats: value.existing_chats,
        new_chats: value.new_chats,
        contacts: value.contacts,
        non_contacts: value.non_contacts,
        exclude_selected: value.exclude_selected,
        user_ids,
    }
}

fn business_greeting_summary(
    value: tl::enums::BusinessGreetingMessage,
) -> BusinessGreetingMessageSummary {
    let tl::enums::BusinessGreetingMessage::BusinessGreetingMessage(value) = value;
    BusinessGreetingMessageSummary {
        shortcut_id: value.shortcut_id,
        recipients: business_recipients_summary(value.recipients),
        no_activity_days: value.no_activity_days,
    }
}

fn business_away_schedule_summary(
    value: tl::enums::BusinessAwayMessageSchedule,
) -> BusinessAwayScheduleSummary {
    match value {
        tl::enums::BusinessAwayMessageSchedule::Always => BusinessAwayScheduleSummary::Always,
        tl::enums::BusinessAwayMessageSchedule::OutsideWorkHours => {
            BusinessAwayScheduleSummary::OutsideHours
        },
        tl::enums::BusinessAwayMessageSchedule::Custom(value) => {
            BusinessAwayScheduleSummary::Custom {
                start_date_unix: value.start_date,
                end_date_unix: value.end_date,
            }
        },
    }
}

fn business_away_summary(value: tl::enums::BusinessAwayMessage) -> BusinessAwayMessageSummary {
    let tl::enums::BusinessAwayMessage::BusinessAwayMessage(value) = value;
    BusinessAwayMessageSummary {
        offline_only: value.offline_only,
        shortcut_id: value.shortcut_id,
        schedule: business_away_schedule_summary(value.schedule),
        recipients: business_recipients_summary(value.recipients),
    }
}

fn business_intro_summary(
    value: tl::enums::BusinessIntro,
) -> Result<BusinessIntroSummary, TelegramError> {
    let tl::enums::BusinessIntro::BusinessIntro(value) = value;
    let sticker_handle = match value.sticker {
        Some(tl::enums::Document::Document(document)) => Some(opaque_sticker_handle(document.id)),
        Some(tl::enums::Document::Empty(_)) => {
            return Err(TelegramError::Telegram(
                "Telegram returned an empty document for the configured Business intro sticker"
                    .to_string(),
            ));
        },
        None => None,
    };
    Ok(BusinessIntroSummary { title: value.title, description: value.description, sticker_handle })
}

fn business_account_settings_summary(
    full: &tl::types::UserFull,
) -> Result<BusinessAccountSettingsSummary, TelegramError> {
    Ok(BusinessAccountSettingsSummary {
        work_hours: full.business_work_hours.clone().map(business_work_hours_summary),
        location: full.business_location.clone().map(business_location_summary).transpose()?,
        greeting: full.business_greeting_message.clone().map(business_greeting_summary),
        away: full.business_away_message.clone().map(business_away_summary),
        intro: full.business_intro.clone().map(business_intro_summary).transpose()?,
    })
}

async fn input_business_recipients(
    client: &Client,
    recipients: &BusinessRecipientsSummary,
    context: &str,
) -> Result<tl::enums::InputBusinessRecipients, TelegramError> {
    let mut users = Vec::with_capacity(recipients.user_ids.len());
    for user_id in &recipients.user_ids {
        users.push(resolve_input_user(client, &user_id.to_string(), context).await?);
    }
    Ok(tl::enums::InputBusinessRecipients::InputBusinessRecipients(
        tl::types::InputBusinessRecipients {
            existing_chats: recipients.existing_chats,
            new_chats: recipients.new_chats,
            contacts: recipients.contacts,
            non_contacts: recipients.non_contacts,
            exclude_selected: recipients.exclude_selected,
            users: (!users.is_empty()).then_some(users),
        },
    ))
}

fn business_away_schedule_input(
    schedule: &BusinessAwayScheduleSummary,
) -> tl::enums::BusinessAwayMessageSchedule {
    match schedule {
        BusinessAwayScheduleSummary::Always => tl::enums::BusinessAwayMessageSchedule::Always,
        BusinessAwayScheduleSummary::OutsideHours => {
            tl::enums::BusinessAwayMessageSchedule::OutsideWorkHours
        },
        BusinessAwayScheduleSummary::Custom { start_date_unix, end_date_unix } => {
            tl::enums::BusinessAwayMessageSchedule::Custom(
                tl::types::BusinessAwayMessageScheduleCustom {
                    start_date: *start_date_unix,
                    end_date: *end_date_unix,
                },
            )
        },
    }
}

async fn resolve_active_group_call(
    client: &Client,
    chat_id: &str,
) -> Result<(String, tl::enums::InputGroupCall), TelegramError> {
    let peer_ref = TelegramClient::resolve_peer_ref(client, chat_id).await?;
    let peer = client
        .resolve(peer_ref.as_str())
        .await
        .map_err(|error| TelegramError::Telegram(error.to_string()))?;
    let canonical_chat_id = canonical_peer_ref(&peer);
    let input_peer = client
        .resolve_to_input_peer(&peer)
        .await
        .map_err(|error| TelegramError::Telegram(error.to_string()))?;
    if !matches!(
        input_peer,
        tl::enums::InputPeer::Chat(_)
            | tl::enums::InputPeer::Channel(_)
            | tl::enums::InputPeer::ChannelFromMessage(_)
    ) {
        return Err(TelegramError::InvalidPeer(
            "group calls are only available for groups, supergroups, and channels".to_string(),
        ));
    }
    let full = client
        .get_chat_full(peer_ref.as_str())
        .await
        .map_err(|error| TelegramError::Telegram(error.to_string()))?;
    let tl::enums::messages::ChatFull::ChatFull(full) = full;
    let call = match full.full_chat {
        tl::enums::ChatFull::ChatFull(full) => full.call,
        tl::enums::ChatFull::ChannelFull(full) => full.call,
        tl::enums::ChatFull::CommunityFull(_) => None,
    }
    .ok_or_else(|| {
        TelegramError::InvalidPeer(format!("{canonical_chat_id} has no active group call"))
    })?;
    Ok((canonical_chat_id, call))
}

async fn resolve_stars_peer(
    client: &Client,
    peer_ref: &str,
) -> Result<(String, tl::enums::InputPeer), TelegramError> {
    let trimmed = peer_ref.trim();
    if ["me", "@me", "self", "@self"].iter().any(|alias| trimmed.eq_ignore_ascii_case(alias)) {
        let me =
            client.get_me().await.map_err(|error| TelegramError::Telegram(error.to_string()))?;
        return Ok((me.id.to_string(), tl::enums::InputPeer::PeerSelf));
    }
    let normalized = normalize_peer_ref(trimmed)?;
    let peer = client
        .resolve(normalized.as_str())
        .await
        .map_err(|error| TelegramError::Telegram(error.to_string()))?;
    let canonical_peer_ref = canonical_peer_ref(&peer);
    let input_peer = client
        .resolve_to_input_peer(&peer)
        .await
        .map_err(|error| TelegramError::Telegram(error.to_string()))?;
    Ok((canonical_peer_ref, input_peer))
}

async fn resolve_rich_message_peer(
    client: &Client,
    peer_ref: &str,
) -> Result<(String, tl::enums::InputPeer), TelegramError> {
    let trimmed = peer_ref.trim();
    let me = client.get_me().await.map_err(|error| TelegramError::Telegram(error.to_string()))?;
    let is_self_alias =
        ["me", "@me", "self", "@self"].iter().any(|alias| trimmed.eq_ignore_ascii_case(alias));
    let is_self_id = trimmed.parse::<i64>().ok() == Some(me.id);
    if is_self_alias || is_self_id {
        return Ok((me.id.to_string(), tl::enums::InputPeer::PeerSelf));
    }
    let normalized = normalize_peer_ref(trimmed)?;
    let peer = client
        .resolve(normalized.as_str())
        .await
        .map_err(|error| TelegramError::Telegram(error.to_string()))?;
    let canonical_peer_ref = canonical_peer_ref(&peer);
    let input_peer = client
        .resolve_to_input_peer(&peer)
        .await
        .map_err(|error| TelegramError::Telegram(error.to_string()))?;
    Ok((canonical_peer_ref, input_peer))
}

async fn input_saved_star_gift(
    client: &Client,
    reference: &SavedStarGiftRef,
) -> Result<tl::enums::InputSavedStarGift, TelegramError> {
    match reference {
        SavedStarGiftRef::UserMessage { message_id } => {
            if *message_id <= 0 {
                return Err(TelegramError::InvalidPeer(
                    "saved Star Gift message_id must be positive".to_string(),
                ));
            }
            Ok(tl::enums::InputSavedStarGift::User(tl::types::InputSavedStarGiftUser {
                msg_id: *message_id,
            }))
        },
        SavedStarGiftRef::ChatSaved { chat_id, saved_id } => {
            let saved_id = saved_id.parse::<i64>().map_err(|_| {
                TelegramError::InvalidPeer(
                    "saved Star Gift saved_id must be a signed 64-bit decimal integer".to_string(),
                )
            })?;
            if saved_id <= 0 {
                return Err(TelegramError::InvalidPeer(
                    "saved Star Gift saved_id must be positive".to_string(),
                ));
            }
            let (_, peer) = resolve_stars_peer(client, chat_id).await?;
            Ok(tl::enums::InputSavedStarGift::Chat(tl::types::InputSavedStarGiftChat {
                peer,
                saved_id,
            }))
        },
    }
}

fn group_call_participant_video_summary(
    video: tl::enums::GroupCallParticipantVideo,
) -> GroupCallParticipantVideoSummary {
    let tl::enums::GroupCallParticipantVideo::GroupCallParticipantVideo(video) = video;
    GroupCallParticipantVideoSummary {
        paused: video.paused,
        endpoint: video.endpoint,
        source_groups: video
            .source_groups
            .into_iter()
            .map(|group| {
                let tl::enums::GroupCallParticipantVideoSourceGroup::GroupCallParticipantVideoSourceGroup(
                    group,
                ) = group;
                GroupCallParticipantVideoSourceGroupSummary {
                    semantics: group.semantics,
                    sources: group.sources,
                }
            })
            .collect(),
        audio_source: video.audio_source,
    }
}

fn group_call_participant_summary(
    participant: tl::enums::GroupCallParticipant,
) -> GroupCallParticipantSummary {
    let tl::enums::GroupCallParticipant::GroupCallParticipant(participant) = participant;
    GroupCallParticipantSummary {
        peer_ref: canonical_peer_ref(&participant.peer),
        date_unix: participant.date,
        active_date_unix: participant.active_date,
        source: participant.source,
        volume: participant.volume,
        muted: participant.muted,
        left: participant.left,
        can_self_unmute: participant.can_self_unmute,
        just_joined: participant.just_joined,
        versioned: participant.versioned,
        min: participant.min,
        self_participant: participant.is_self,
        muted_by_you: participant.muted_by_you,
        volume_by_admin: participant.volume_by_admin,
        video_joined: participant.video_joined,
        about: participant.about,
        raise_hand_rating: participant.raise_hand_rating.map(|rating| rating.to_string()),
        video: participant.video.map(group_call_participant_video_summary),
        presentation: participant.presentation.map(group_call_participant_video_summary),
        paid_stars_total: participant.paid_stars_total.map(|stars| stars.to_string()),
    }
}

fn group_call_info_summary(call: tl::enums::GroupCall) -> GroupCallInfoSummary {
    match call {
        tl::enums::GroupCall::GroupCall(call) => GroupCallInfoSummary::Active {
            call_id: call.id.to_string(),
            participants_count: call.participants_count,
            title: call.title,
            stream_dc_id: call.stream_dc_id,
            record_start_date_unix: call.record_start_date,
            schedule_date_unix: call.schedule_date,
            unmuted_video_count: call.unmuted_video_count,
            unmuted_video_limit: call.unmuted_video_limit,
            version: call.version,
            join_muted: call.join_muted,
            can_change_join_muted: call.can_change_join_muted,
            join_date_asc: call.join_date_asc,
            schedule_start_subscribed: call.schedule_start_subscribed,
            can_start_video: call.can_start_video,
            record_video_active: call.record_video_active,
            rtmp_stream: call.rtmp_stream,
            listeners_hidden: call.listeners_hidden,
            conference: call.conference,
            creator: call.creator,
            messages_enabled: call.messages_enabled,
            can_change_messages_enabled: call.can_change_messages_enabled,
            min: call.min,
            has_invite_link: call.invite_link.is_some(),
            send_paid_messages_stars: call.send_paid_messages_stars.map(|stars| stars.to_string()),
            default_send_as: call.default_send_as.as_ref().map(canonical_peer_ref),
        },
        tl::enums::GroupCall::Discarded(call) => GroupCallInfoSummary::Discarded {
            call_id: call.id.to_string(),
            duration_seconds: call.duration,
        },
    }
}

fn group_call_page(chat_id: String, response: tl::enums::phone::GroupCall) -> GroupCallPage {
    let tl::enums::phone::GroupCall::GroupCall(response) = response;
    GroupCallPage {
        chat_id,
        call: group_call_info_summary(response.call),
        participants: response
            .participants
            .into_iter()
            .map(group_call_participant_summary)
            .collect(),
        participants_next_offset: nonempty_owned(&response.participants_next_offset),
        related_chats: response.chats.into_iter().filter_map(chat_summary).collect(),
        related_users: response.users.into_iter().filter_map(contact_summary).collect(),
    }
}

fn group_call_participants_page(
    chat_id: String,
    response: tl::enums::phone::GroupParticipants,
) -> GroupCallParticipantsPage {
    let tl::enums::phone::GroupParticipants::GroupParticipants(response) = response;
    GroupCallParticipantsPage {
        chat_id,
        count: response.count,
        participants: response
            .participants
            .into_iter()
            .map(group_call_participant_summary)
            .collect(),
        next_offset: nonempty_owned(&response.next_offset),
        version: response.version,
        related_chats: response.chats.into_iter().filter_map(chat_summary).collect(),
        related_users: response.users.into_iter().filter_map(contact_summary).collect(),
    }
}

fn stars_amount_summary(amount: tl::enums::StarsAmount) -> StarsAmountSummary {
    match amount {
        tl::enums::StarsAmount::StarsAmount(amount) => StarsAmountSummary {
            kind: StarsAmountKind::Stars,
            amount: amount.amount.to_string(),
            nanos: Some(amount.nanos),
        },
        tl::enums::StarsAmount::StarsTonAmount(amount) => StarsAmountSummary {
            kind: StarsAmountKind::Ton,
            amount: amount.amount.to_string(),
            nanos: None,
        },
    }
}

fn star_gift_rarity_summary(rarity: tl::enums::StarGiftAttributeRarity) -> StarGiftRaritySummary {
    match rarity {
        tl::enums::StarGiftAttributeRarity::StarGiftAttributeRarity(rarity) => {
            StarGiftRaritySummary::Exact { permille: rarity.permille }
        },
        tl::enums::StarGiftAttributeRarity::Uncommon => StarGiftRaritySummary::Uncommon,
        tl::enums::StarGiftAttributeRarity::Rare => StarGiftRaritySummary::Rare,
        tl::enums::StarGiftAttributeRarity::Epic => StarGiftRaritySummary::Epic,
        tl::enums::StarGiftAttributeRarity::Legendary => StarGiftRaritySummary::Legendary,
    }
}

fn star_gift_attribute_summary(
    attribute: tl::enums::StarGiftAttribute,
) -> StarGiftAttributeSummary {
    match attribute {
        tl::enums::StarGiftAttribute::Model(attribute) => StarGiftAttributeSummary::Model {
            name: attribute.name,
            document_id: raw_document_id(&attribute.document).to_string(),
            crafted: attribute.crafted,
            rarity: star_gift_rarity_summary(attribute.rarity),
        },
        tl::enums::StarGiftAttribute::Pattern(attribute) => StarGiftAttributeSummary::Pattern {
            name: attribute.name,
            document_id: raw_document_id(&attribute.document).to_string(),
            rarity: star_gift_rarity_summary(attribute.rarity),
        },
        tl::enums::StarGiftAttribute::Backdrop(attribute) => StarGiftAttributeSummary::Backdrop {
            name: attribute.name,
            backdrop_id: attribute.backdrop_id,
            center_color: attribute.center_color,
            edge_color: attribute.edge_color,
            pattern_color: attribute.pattern_color,
            text_color: attribute.text_color,
            rarity: star_gift_rarity_summary(attribute.rarity),
        },
        tl::enums::StarGiftAttribute::OriginalDetails(attribute) => {
            let message = attribute
                .message
                .as_ref()
                .map(|message| text_with_entities_text(message).to_string());
            let message_has_entities =
                attribute.message.as_ref().is_some_and(text_with_entities_has_entities);
            StarGiftAttributeSummary::OriginalDetails {
                sender_peer_ref: attribute.sender_id.as_ref().map(canonical_peer_ref),
                recipient_peer_ref: canonical_peer_ref(&attribute.recipient_id),
                date_unix: attribute.date,
                message,
                message_has_entities,
            }
        },
    }
}

fn star_gift_summary(gift: tl::enums::StarGift) -> StarGiftSummary {
    match gift {
        tl::enums::StarGift::StarGift(gift) => StarGiftSummary::Regular {
            gift_id: gift.id.to_string(),
            sticker_document_id: raw_document_id(&gift.sticker).to_string(),
            stars: gift.stars.to_string(),
            convert_stars: gift.convert_stars.to_string(),
            limited: gift.limited,
            sold_out: gift.sold_out,
            birthday: gift.birthday,
            require_premium: gift.require_premium,
            limited_per_user: gift.limited_per_user,
            peer_color_available: gift.peer_color_available,
            auction: gift.auction,
            availability_remains: gift.availability_remains,
            availability_total: gift.availability_total,
            availability_resale: gift.availability_resale.map(|amount| amount.to_string()),
            first_sale_date_unix: gift.first_sale_date,
            last_sale_date_unix: gift.last_sale_date,
            upgrade_stars: gift.upgrade_stars.map(|amount| amount.to_string()),
            resell_min_stars: gift.resell_min_stars.map(|amount| amount.to_string()),
            title: gift.title,
            released_by_peer_ref: gift.released_by.as_ref().map(canonical_peer_ref),
            per_user_total: gift.per_user_total,
            per_user_remains: gift.per_user_remains,
            locked_until_date_unix: gift.locked_until_date,
            auction_slug: gift.auction_slug,
            gifts_per_round: gift.gifts_per_round,
            auction_start_date_unix: gift.auction_start_date,
            upgrade_variants: gift.upgrade_variants,
            has_background: gift.background.is_some(),
        },
        tl::enums::StarGift::Unique(gift) => StarGiftSummary::Unique {
            unique_gift_id: gift.id.to_string(),
            gift_id: gift.gift_id.to_string(),
            title: gift.title,
            slug: gift.slug,
            number: gift.num,
            require_premium: gift.require_premium,
            resale_ton_only: gift.resale_ton_only,
            theme_available: gift.theme_available,
            burned: gift.burned,
            crafted: gift.crafted,
            owner_peer_ref: gift.owner_id.as_ref().map(canonical_peer_ref),
            owner_name: gift.owner_name,
            has_owner_address: gift.owner_address.is_some(),
            attributes: gift.attributes.into_iter().map(star_gift_attribute_summary).collect(),
            availability_issued: gift.availability_issued,
            availability_total: gift.availability_total,
            has_gift_address: gift.gift_address.is_some(),
            resell_amounts: gift
                .resell_amount
                .unwrap_or_default()
                .into_iter()
                .map(stars_amount_summary)
                .collect(),
            released_by_peer_ref: gift.released_by.as_ref().map(canonical_peer_ref),
            value_amount: gift.value_amount.map(|amount| amount.to_string()),
            value_currency: gift.value_currency,
            value_usd_amount: gift.value_usd_amount.map(|amount| amount.to_string()),
            theme_peer_ref: gift.theme_peer.as_ref().map(canonical_peer_ref),
            has_peer_color: gift.peer_color.is_some(),
            host_peer_ref: gift.host_id.as_ref().map(canonical_peer_ref),
            offer_min_stars: gift.offer_min_stars.map(|amount| amount.to_string()),
            craft_chance_permille: gift.craft_chance_permille,
        },
    }
}

fn star_gift_is_limited_regular(gift: &StarGiftSummary) -> bool {
    matches!(gift, StarGiftSummary::Regular { limited: true, .. })
}

fn available_star_gifts_page(response: tl::enums::payments::StarGifts) -> AvailableStarGiftsPage {
    match response {
        tl::enums::payments::StarGifts::NotModified => AvailableStarGiftsPage {
            not_modified: true,
            hash: None,
            gifts: Vec::new(),
            related_chats: Vec::new(),
            related_users: Vec::new(),
        },
        tl::enums::payments::StarGifts::StarGifts(response) => AvailableStarGiftsPage {
            not_modified: false,
            hash: Some(response.hash),
            gifts: response.gifts.into_iter().map(star_gift_summary).collect(),
            related_chats: response.chats.into_iter().filter_map(chat_summary).collect(),
            related_users: response.users.into_iter().filter_map(contact_summary).collect(),
        },
    }
}

fn saved_star_gift_summary(gift: tl::enums::SavedStarGift) -> SavedStarGiftSummary {
    let tl::enums::SavedStarGift::SavedStarGift(gift) = gift;
    let message = gift.message.as_ref().map(|message| text_with_entities_text(message).to_string());
    let message_has_entities = gift.message.as_ref().is_some_and(text_with_entities_has_entities);
    let prepaid_upgrade_hash_sha256 = gift
        .prepaid_upgrade_hash
        .as_ref()
        .map(|hash| format!("{:x}", Sha256::digest(hash.as_bytes())));
    let prepaid_upgrade_hash_length = gift.prepaid_upgrade_hash.as_ref().map(String::len);
    SavedStarGiftSummary {
        gift: star_gift_summary(gift.gift),
        name_hidden: gift.name_hidden,
        unsaved: gift.unsaved,
        refunded: gift.refunded,
        can_upgrade: gift.can_upgrade,
        pinned_to_top: gift.pinned_to_top,
        upgrade_separate: gift.upgrade_separate,
        from_peer_ref: gift.from_id.as_ref().map(canonical_peer_ref),
        date_unix: gift.date,
        message,
        message_has_entities,
        message_id: gift.msg_id,
        saved_id: gift.saved_id.map(|id| id.to_string()),
        convert_stars: gift.convert_stars.map(|amount| amount.to_string()),
        upgrade_stars: gift.upgrade_stars.map(|amount| amount.to_string()),
        can_export_at_unix: gift.can_export_at,
        transfer_stars: gift.transfer_stars.map(|amount| amount.to_string()),
        can_transfer_at_unix: gift.can_transfer_at,
        can_resell_at_unix: gift.can_resell_at,
        collection_ids: gift.collection_id.unwrap_or_default(),
        has_prepaid_upgrade_hash: gift.prepaid_upgrade_hash.is_some(),
        prepaid_upgrade_hash_sha256,
        prepaid_upgrade_hash_length,
        drop_original_details_stars: gift
            .drop_original_details_stars
            .map(|amount| amount.to_string()),
        gift_number: gift.gift_num,
        can_craft_at_unix: gift.can_craft_at,
    }
}

fn saved_star_gifts_page(
    peer_ref: String,
    response: tl::types::payments::SavedStarGifts,
) -> SavedStarGiftsPage {
    SavedStarGiftsPage {
        peer_ref,
        count: response.count,
        chat_notifications_enabled: response.chat_notifications_enabled,
        gifts: response.gifts.into_iter().map(saved_star_gift_summary).collect(),
        next_offset: response.next_offset.filter(|offset| !offset.is_empty()),
        related_chats: response.chats.into_iter().filter_map(chat_summary).collect(),
        related_users: response.users.into_iter().filter_map(contact_summary).collect(),
    }
}

async fn fetch_saved_star_gifts(
    client: &Client,
    peer: tl::enums::InputPeer,
    request: &SavedStarGiftsRequest,
    exclude_unlimited: bool,
    exclude_unique: bool,
    offset: String,
    limit: i32,
) -> Result<tl::types::payments::SavedStarGifts, TelegramError> {
    let response = client
        .invoke(&tl::functions::payments::GetSavedStarGifts {
            exclude_unsaved: request.exclude_unsaved,
            exclude_saved: request.exclude_saved,
            exclude_unlimited,
            exclude_unique,
            sort_by_value: request.sort_by_value,
            exclude_upgradable: request.exclude_upgradable,
            exclude_unupgradable: request.exclude_unupgradable,
            peer_color_available: request.peer_color_available,
            exclude_hosted: request.exclude_hosted,
            peer,
            collection_id: request.collection_id,
            offset,
            limit,
        })
        .await
        .map_err(|error| TelegramError::Telegram(error.to_string()))?;
    let tl::enums::payments::SavedStarGifts::SavedStarGifts(response) = response;
    Ok(response)
}

fn deduplicate_star_gift_related_peers(page: &mut SavedStarGiftsPage) {
    let mut chat_ids = HashSet::new();
    page.related_chats.retain(|chat| chat_ids.insert(chat.chat_id.clone()));
    let mut user_ids = HashSet::new();
    page.related_users.retain(|user| user_ids.insert(user.user_id));
}

fn stars_topup_options_page(response: Vec<tl::enums::StarsTopupOption>) -> StarsTopupOptionsPage {
    StarsTopupOptionsPage {
        options: response
            .into_iter()
            .map(|option| {
                let tl::enums::StarsTopupOption::StarsTopupOption(option) = option;
                StarsTopupOptionSummary {
                    stars: option.stars.to_string(),
                    store_product: option.store_product,
                    currency: option.currency,
                    amount: option.amount.to_string(),
                    extended: option.extended,
                }
            })
            .collect(),
    }
}

fn stars_subscriptions_page(
    peer_ref: String,
    response: tl::enums::payments::StarsStatus,
) -> StarsSubscriptionsPage {
    let tl::enums::payments::StarsStatus::StarsStatus(response) = response;
    StarsSubscriptionsPage {
        peer_ref,
        balance: stars_amount_summary(response.balance),
        subscriptions: response
            .subscriptions
            .unwrap_or_default()
            .into_iter()
            .map(stars_subscription_summary)
            .collect(),
        next_offset: response.subscriptions_next_offset.filter(|offset| !offset.is_empty()),
        subscriptions_missing_balance: response
            .subscriptions_missing_balance
            .map(|amount| amount.to_string()),
        related_chats: response.chats.into_iter().filter_map(chat_summary).collect(),
        related_users: response.users.into_iter().filter_map(contact_summary).collect(),
    }
}

fn stars_transaction_peer_summary(
    peer: tl::enums::StarsTransactionPeer,
) -> StarsTransactionPeerSummary {
    match peer {
        tl::enums::StarsTransactionPeer::Unsupported => StarsTransactionPeerSummary::Unsupported,
        tl::enums::StarsTransactionPeer::AppStore => StarsTransactionPeerSummary::AppStore,
        tl::enums::StarsTransactionPeer::PlayMarket => StarsTransactionPeerSummary::PlayMarket,
        tl::enums::StarsTransactionPeer::PremiumBot => StarsTransactionPeerSummary::PremiumBot,
        tl::enums::StarsTransactionPeer::Fragment => StarsTransactionPeerSummary::Fragment,
        tl::enums::StarsTransactionPeer::Ads => StarsTransactionPeerSummary::Ads,
        tl::enums::StarsTransactionPeer::Api => StarsTransactionPeerSummary::Api,
        tl::enums::StarsTransactionPeer::StarsTransactionPeer(peer) => {
            StarsTransactionPeerSummary::Peer { peer_ref: canonical_peer_ref(&peer.peer) }
        },
    }
}

fn stars_transaction_gift_summary(gift: tl::enums::StarGift) -> StarsTransactionGiftSummary {
    match gift {
        tl::enums::StarGift::StarGift(gift) => {
            StarsTransactionGiftSummary::Regular { gift_id: gift.id.to_string(), title: gift.title }
        },
        tl::enums::StarGift::Unique(gift) => StarsTransactionGiftSummary::Unique {
            unique_gift_id: gift.id.to_string(),
            gift_id: gift.gift_id.to_string(),
            title: gift.title,
            slug: gift.slug,
            number: gift.num,
        },
    }
}

fn stars_subscription_summary(
    subscription: tl::enums::StarsSubscription,
) -> StarsSubscriptionSummary {
    let tl::enums::StarsSubscription::StarsSubscription(subscription) = subscription;
    let tl::enums::StarsSubscriptionPricing::StarsSubscriptionPricing(pricing) =
        subscription.pricing;
    StarsSubscriptionSummary {
        subscription_id: subscription.id,
        peer_ref: canonical_peer_ref(&subscription.peer),
        until_date_unix: subscription.until_date,
        pricing: StarsSubscriptionPricingSummary {
            period_seconds: pricing.period,
            amount_stars: pricing.amount.to_string(),
        },
        canceled: subscription.canceled,
        can_refulfill: subscription.can_refulfill,
        missing_balance: subscription.missing_balance,
        bot_canceled: subscription.bot_canceled,
        has_chat_invite_hash: subscription.chat_invite_hash.is_some(),
        title: subscription.title,
        has_photo: subscription.photo.is_some(),
        invoice_slug: subscription.invoice_slug,
    }
}

fn stars_transaction_summary(transaction: tl::enums::StarsTransaction) -> StarsTransactionSummary {
    let tl::enums::StarsTransaction::StarsTransaction(transaction) = transaction;
    let bot_payload_sha256 =
        transaction.bot_payload.as_ref().map(|payload| format!("{:x}", Sha256::digest(payload)));
    let bot_payload_length = transaction.bot_payload.as_ref().map(Vec::len);
    StarsTransactionSummary {
        transaction_id: transaction.id,
        amount: stars_amount_summary(transaction.amount),
        date_unix: transaction.date,
        peer: stars_transaction_peer_summary(transaction.peer),
        refund: transaction.refund,
        pending: transaction.pending,
        failed: transaction.failed,
        gift: transaction.gift,
        reaction: transaction.reaction,
        star_gift_upgrade: transaction.stargift_upgrade,
        business_transfer: transaction.business_transfer,
        star_gift_resale: transaction.stargift_resale,
        posts_search: transaction.posts_search,
        star_gift_prepaid_upgrade: transaction.stargift_prepaid_upgrade,
        star_gift_drop_original_details: transaction.stargift_drop_original_details,
        phone_group_message: transaction.phonegroup_message,
        star_gift_auction_bid: transaction.stargift_auction_bid,
        offer: transaction.offer,
        title: transaction.title,
        description: transaction.description,
        has_photo: transaction.photo.is_some(),
        transaction_date_unix: transaction.transaction_date,
        transaction_url: transaction.transaction_url,
        bot_payload_sha256,
        bot_payload_length,
        message_id: transaction.msg_id,
        extended_media_types: transaction
            .extended_media
            .map(|media| media.iter().map(media_type_name).collect()),
        subscription_period_seconds: transaction.subscription_period,
        giveaway_post_id: transaction.giveaway_post_id,
        star_gift: transaction.stargift.map(stars_transaction_gift_summary),
        floodskip_number: transaction.floodskip_number,
        star_ref_commission_permille: transaction.starref_commission_permille,
        star_ref_peer_ref: transaction.starref_peer.as_ref().map(canonical_peer_ref),
        star_ref_amount: transaction.starref_amount.map(stars_amount_summary),
        paid_messages: transaction.paid_messages,
        premium_gift_months: transaction.premium_gift_months,
        ads_proceeds_from_date_unix: transaction.ads_proceeds_from_date,
        ads_proceeds_to_date_unix: transaction.ads_proceeds_to_date,
    }
}

fn stars_status_page(
    peer_ref: String,
    response: tl::enums::payments::StarsStatus,
) -> StarsStatusPage {
    let tl::enums::payments::StarsStatus::StarsStatus(response) = response;
    StarsStatusPage {
        peer_ref,
        balance: stars_amount_summary(response.balance),
        subscriptions: response
            .subscriptions
            .unwrap_or_default()
            .into_iter()
            .map(stars_subscription_summary)
            .collect(),
        subscriptions_next_offset: response
            .subscriptions_next_offset
            .filter(|offset| !offset.is_empty()),
        subscriptions_missing_balance: response
            .subscriptions_missing_balance
            .map(|amount| amount.to_string()),
        history: response
            .history
            .unwrap_or_default()
            .into_iter()
            .map(stars_transaction_summary)
            .collect(),
        next_offset: response.next_offset.filter(|offset| !offset.is_empty()),
        related_chats: response.chats.into_iter().filter_map(chat_summary).collect(),
        related_users: response.users.into_iter().filter_map(contact_summary).collect(),
    }
}

fn message_page_from_tl(page: tl::enums::messages::Messages, limit: usize) -> MessagePage {
    let (raw_messages, total_count) = match page {
        tl::enums::messages::Messages::Messages(page) => (page.messages, None),
        tl::enums::messages::Messages::Slice(page) => {
            (page.messages, usize::try_from(page.count).ok())
        },
        tl::enums::messages::Messages::ChannelMessages(page) => {
            (page.messages, usize::try_from(page.count).ok())
        },
        tl::enums::messages::Messages::NotModified(_) => (Vec::new(), Some(0)),
    };
    let raw_count = raw_messages.len();
    let messages = raw_messages.iter().filter_map(summarize_raw_message).collect::<Vec<_>>();
    let has_more = total_count.is_some_and(|count| count > raw_count) || raw_count >= limit;
    let next_offset_id =
        has_more.then(|| messages.last().map(|message| message.message_id)).flatten();
    let completeness =
        if next_offset_id.is_some() { Completeness::Page } else { Completeness::Complete };
    MessagePage { messages, next_offset_id, completeness }
}

fn saved_history_page_from_tl(
    page: tl::enums::messages::Messages,
    peer_ref: String,
    limit: usize,
) -> SavedHistoryPage {
    let (raw_messages, total_count) = match page {
        tl::enums::messages::Messages::Messages(page) => {
            let count = i32::try_from(page.messages.len()).ok();
            (page.messages, count)
        },
        tl::enums::messages::Messages::Slice(page) => (page.messages, Some(page.count)),
        tl::enums::messages::Messages::ChannelMessages(page) => (page.messages, Some(page.count)),
        tl::enums::messages::Messages::NotModified(_) => (Vec::new(), Some(0)),
    };
    let raw_count = raw_messages.len();
    let messages = raw_messages.iter().filter_map(summarize_raw_message).collect::<Vec<_>>();
    let has_more = total_count
        .and_then(|count| usize::try_from(count).ok())
        .is_some_and(|count| count > raw_count)
        || raw_count >= limit;
    let next_offset_id =
        has_more.then(|| messages.last().map(|message| message.message_id)).flatten();
    SavedHistoryPage {
        peer_ref,
        messages,
        total_count,
        next_offset_id,
        completeness: if next_offset_id.is_some() {
            Completeness::Page
        } else {
            Completeness::Complete
        },
    }
}

async fn resolve_input_peer(
    client: &Client,
    peer_ref: &str,
) -> Result<tl::enums::InputPeer, TelegramError> {
    let resolved_ref = TelegramClient::resolve_peer_ref(client, peer_ref).await?;
    let peer = client
        .resolve(resolved_ref.as_str())
        .await
        .map_err(|error| TelegramError::Telegram(error.to_string()))?;
    client
        .resolve_to_input_peer(&peer)
        .await
        .map_err(|error| TelegramError::Telegram(error.to_string()))
}

fn summarize_forward_origin(
    header: Option<&tl::enums::MessageFwdHeader>,
) -> Option<MessageForwardOrigin> {
    let tl::enums::MessageFwdHeader::MessageFwdHeader(header) = header?;
    Some(MessageForwardOrigin {
        from_ref: header.from_id.as_ref().map(canonical_peer_ref),
        source_message_id: header.channel_post,
        saved_from_ref: header.saved_from_peer.as_ref().map(canonical_peer_ref),
        saved_from_message_id: header.saved_from_msg_id,
        date: header.date,
    })
}

fn is_concrete_message(message: &ferogram::update::IncomingMessage) -> bool {
    is_concrete_raw_message(&message.raw)
}

fn is_concrete_raw_message(message: &tl::enums::Message) -> bool {
    !matches!(message, tl::enums::Message::Empty(_))
}

/// The current account's reaction as carried by a message's own `reactions`
/// summary — the one source that exists in broadcast channels too.
///
/// `chosen_order` on a `reactionCount` is set for the reaction this account
/// chose; `my` on a `messagePeerReaction` marks the same fact in the
/// recent-reactions list. A message the account has not reacted to (including
/// one that carries no reactions at all) yields `None`.
fn my_reaction_from_reactions(
    reactions: Option<&tl::enums::MessageReactions>,
) -> Result<Option<ReactionSpec>, TelegramError> {
    let Some(tl::enums::MessageReactions::MessageReactions(reactions)) = reactions else {
        return Ok(None);
    };
    let chosen = reactions.results.iter().find_map(|count| match count {
        tl::enums::ReactionCount::ReactionCount(count) if count.chosen_order.is_some() => {
            Some(&count.reaction)
        },
        _ => None,
    });
    if let Some(reaction) = chosen {
        return reaction_spec_from_reaction(reaction, false);
    }
    let mine = reactions.recent_reactions.as_ref().and_then(|recent| {
        recent.iter().find_map(|reaction| match reaction {
            tl::enums::MessagePeerReaction::MessagePeerReaction(reaction) if reaction.my => {
                Some(reaction)
            },
            _ => None,
        })
    });
    match mine {
        Some(reaction) => reaction_spec_from_reaction(&reaction.reaction, reaction.big),
        None => Ok(None),
    }
}

/// The reaction summary the message itself carries — the one source that
/// exists in broadcast channels too.
fn message_reactions_of(
    messages: &[ferogram::update::IncomingMessage],
) -> Option<&tl::enums::MessageReactions> {
    messages
        .iter()
        .find_map(|message| match &message.raw {
            tl::enums::Message::Message(message) => Some(message.reactions.as_ref()),
            tl::enums::Message::Service(message) => Some(message.reactions.as_ref()),
            tl::enums::Message::Empty(_) => None,
        })
        .flatten()
}

/// Why Telegram refused the per-viewer reaction list at this place, when it
/// did: broadcast channels answer `403 BROADCAST_FORBIDDEN` and no retry
/// changes that. Any other failure stays an honest error.
fn viewer_list_forbidden_reason(error: &ferogram::InvocationError) -> Option<String> {
    match error {
        ferogram::InvocationError::Rpc(rpc) if rpc.code == 403 => Some(rpc.name.clone()),
        _ => None,
    }
}

/// Map one wire reaction onto its kind and payload, shared by the per-viewer
/// list and the message summary.
fn reaction_kind_parts(
    reaction: &tl::enums::Reaction,
) -> (&'static str, Option<String>, Option<i64>) {
    match reaction {
        tl::enums::Reaction::Empty => ("empty", None, None),
        tl::enums::Reaction::Emoji(emoji) => ("emoji", Some(emoji.emoticon.clone()), None),
        tl::enums::Reaction::CustomEmoji(emoji) => ("custom_emoji", None, Some(emoji.document_id)),
        tl::enums::Reaction::Paid => ("paid", None, None),
    }
}

/// One per-viewer row of Telegram's reaction list.
fn reaction_summary_from_peer_reaction(
    reaction: &tl::enums::MessagePeerReaction,
) -> MessageReactionSummary {
    let tl::enums::MessagePeerReaction::MessagePeerReaction(reaction) = reaction;
    let (kind, emoji, custom_emoji_id) = reaction_kind_parts(&reaction.reaction);
    MessageReactionSummary {
        peer_ref: canonical_peer_ref(&reaction.peer_id),
        date_unix: reaction.date,
        big: reaction.big,
        unread: reaction.unread,
        my: reaction.my,
        kind: kind.to_string(),
        emoji,
        custom_emoji_id,
    }
}

/// The per-viewer list Telegram returned, mapped onto the tool page.
fn reactions_list_page(page: tl::types::messages::MessageReactionsList) -> MessageReactionPage {
    MessageReactionPage {
        total_count: page.count,
        reactions: page.reactions.iter().map(reaction_summary_from_peer_reaction).collect(),
        next_offset: page.next_offset,
        counts: Vec::new(),
        viewer_list_forbidden: None,
    }
}

/// The reaction read as this tool must answer it: the per-viewer list when
/// Telegram allows it, the message's own summary when Telegram refuses that list
/// (broadcast channels answer `403 BROADCAST_FORBIDDEN`), the error otherwise.
/// `fetch_summary` runs only on the forbidden refusal, so the common path pays
/// no extra request.
async fn reaction_read_outcome(
    listed: Result<tl::types::messages::MessageReactionsList, ferogram::InvocationError>,
    fetch_summary: impl AsyncFnOnce() -> Result<
        Vec<ferogram::update::IncomingMessage>,
        ferogram::InvocationError,
    >,
) -> Result<MessageReactionPage, TelegramError> {
    match listed {
        Ok(page) => Ok(reactions_list_page(page)),
        Err(error) => match viewer_list_forbidden_reason(&error) {
            Some(reason) => {
                let messages = fetch_summary().await.map_err(|summary_error| {
                    TelegramError::Telegram(format!(
                        "the per-viewer reaction list is forbidden ({reason}), and reading the \
                         message summary failed too: {summary_error}"
                    ))
                })?;
                Ok(reaction_page_from_summary(message_reactions_of(&messages), Some(reason)))
            },
            None => Err(TelegramError::Telegram(error.to_string())),
        },
    }
}

/// The message's own reaction summary as a tool page, used where Telegram
/// refuses the per-viewer list: `counts` carries per-reaction totals (with
/// `chosen` marking this account's own reaction) and `reactions` holds only
/// the recent reactors the summary exposes.
fn reaction_page_from_summary(
    reactions: Option<&tl::enums::MessageReactions>,
    viewer_list_forbidden: Option<String>,
) -> MessageReactionPage {
    let Some(tl::enums::MessageReactions::MessageReactions(reactions)) = reactions else {
        return MessageReactionPage {
            total_count: 0,
            reactions: Vec::new(),
            next_offset: None,
            counts: Vec::new(),
            viewer_list_forbidden,
        };
    };
    let counts = reactions
        .results
        .iter()
        .map(|count| {
            let tl::enums::ReactionCount::ReactionCount(count) = count;
            let (kind, emoji, custom_emoji_id) = reaction_kind_parts(&count.reaction);
            MessageReactionCount {
                kind: kind.to_string(),
                emoji,
                custom_emoji_id,
                count: count.count,
                chosen: count.chosen_order.is_some(),
            }
        })
        .collect::<Vec<_>>();
    let total_count = counts.iter().fold(0i32, |total, count| total.saturating_add(count.count));
    let reactions = reactions
        .recent_reactions
        .as_deref()
        .unwrap_or_default()
        .iter()
        .map(reaction_summary_from_peer_reaction)
        .collect();
    MessageReactionPage { total_count, reactions, next_offset: None, counts, viewer_list_forbidden }
}

/// Map one wire reaction onto the mutation-side reaction spec, keeping the
/// error semantics the reaction approval flow relies on.
fn reaction_spec_from_reaction(
    reaction: &tl::enums::Reaction,
    big: bool,
) -> Result<Option<ReactionSpec>, TelegramError> {
    match reaction {
        tl::enums::Reaction::Emoji(emoji) => Ok(Some(ReactionSpec::Emoji {
            emoji: emoji.emoticon.clone(),
            big,
            add_to_recent: false,
        })),
        tl::enums::Reaction::CustomEmoji(emoji) => Ok(Some(ReactionSpec::CustomEmoji {
            document_id: emoji.document_id,
            big,
            add_to_recent: false,
        })),
        tl::enums::Reaction::Paid => Err(TelegramError::InvalidPeer(
            "paid reactions are not represented by the ordinary reaction approval flow".to_string(),
        )),
        tl::enums::Reaction::Empty => Err(TelegramError::Telegram(
            "Telegram returned unsupported current reaction kind empty".to_string(),
        )),
    }
}

fn sticker_set_summary_from_tl(set: tl::types::StickerSet) -> StickerSetSummary {
    StickerSetSummary {
        sticker_set_id: set.id,
        title: set.title,
        short_name: set.short_name,
        installed_date_unix: set.installed_date,
        archived: set.archived,
        official: set.official,
        masks: set.masks,
        emojis: set.emojis,
        text_color: set.text_color,
        channel_emoji_status: set.channel_emoji_status,
        creator: set.creator,
        thumb_dc_id: set.thumb_dc_id,
        thumb_version: set.thumb_version,
        thumb_document_id: set.thumb_document_id,
        sticker_count: set.count,
        content_hash: set.hash,
    }
}

fn covered_sticker_set(covered: &tl::enums::StickerSetCovered) -> tl::types::StickerSet {
    let set = match covered {
        tl::enums::StickerSetCovered::StickerSetCovered(covered) => &covered.set,
        tl::enums::StickerSetCovered::StickerSetMultiCovered(covered) => &covered.set,
        tl::enums::StickerSetCovered::StickerSetFullCovered(covered) => &covered.set,
        tl::enums::StickerSetCovered::StickerSetNoCovered(covered) => &covered.set,
    };
    let tl::enums::StickerSet::StickerSet(set) = set;
    set.clone()
}

fn raw_document_id(document: &tl::enums::Document) -> i64 {
    match document {
        tl::enums::Document::Empty(document) => document.id,
        tl::enums::Document::Document(document) => document.id,
    }
}

async fn installed_sticker_set_names(
    client: &Client,
) -> Result<HashMap<i64, String>, TelegramError> {
    let sets = client
        .get_all_stickers(0)
        .await
        .map_err(|error| TelegramError::Telegram(error.to_string()))?
        .unwrap_or_default();
    Ok(sets.into_iter().map(|set| (set.id, set.short_name)).collect())
}

fn input_sticker_set_short_name(
    set: &tl::enums::InputStickerSet,
    installed_names: &HashMap<i64, String>,
) -> Option<String> {
    match set {
        tl::enums::InputStickerSet::ShortName(set) => Some(set.short_name.clone()),
        tl::enums::InputStickerSet::Id(set) => installed_names.get(&set.id).cloned(),
        _ => None,
    }
}

fn sticker_document_summary(
    document: &tl::enums::Document,
    fallback_set_short_name: Option<&str>,
    installed_names: &HashMap<i64, String>,
    recent_date_unix: Option<i32>,
) -> Option<StickerDocumentSummary> {
    let tl::enums::Document::Document(document) = document else {
        return None;
    };
    let mut emoji = None;
    let mut set_short_name = fallback_set_short_name.map(str::to_string);
    let mut animated = document.mime_type == "application/x-tgsticker";
    let mut video = document.mime_type == "video/webm";
    let mut custom_emoji = false;
    let mut is_sticker = false;
    for attribute in &document.attributes {
        match attribute {
            tl::enums::DocumentAttribute::Animated => animated = true,
            tl::enums::DocumentAttribute::Video(_) => video = true,
            tl::enums::DocumentAttribute::Sticker(sticker) => {
                is_sticker = true;
                emoji = Some(sticker.alt.clone());
                if set_short_name.is_none() {
                    set_short_name =
                        input_sticker_set_short_name(&sticker.stickerset, installed_names);
                }
            },
            tl::enums::DocumentAttribute::CustomEmoji(sticker) => {
                is_sticker = true;
                custom_emoji = true;
                emoji = Some(sticker.alt.clone());
                if set_short_name.is_none() {
                    set_short_name =
                        input_sticker_set_short_name(&sticker.stickerset, installed_names);
                }
            },
            _ => {},
        }
    }
    if !is_sticker {
        return None;
    }
    Some(StickerDocumentSummary {
        sticker_handle: opaque_sticker_handle(document.id),
        set_short_name,
        emoji,
        mime_type: document.mime_type.clone(),
        size_bytes: u64::try_from(document.size).ok()?,
        date_unix: document.date,
        recent_date_unix,
        animated,
        video,
        custom_emoji,
    })
}

fn custom_emoji_document_summary(
    document: &tl::enums::Document,
) -> Option<CustomEmojiDocumentSummary> {
    let document_id = raw_document_id(document);
    let sticker = sticker_document_summary(document, None, &HashMap::new(), None)?;
    sticker.custom_emoji.then_some(CustomEmojiDocumentSummary { document_id, sticker })
}

fn sticker_set_details_page(page: tl::types::messages::StickerSet) -> StickerSetDetailsPage {
    let tl::enums::StickerSet::StickerSet(set) = page.set;
    let short_name = set.short_name.clone();
    let installed_names = HashMap::from([(set.id, short_name.clone())]);
    let sticker_set = sticker_set_summary_from_tl(set);
    let stickers = page
        .documents
        .iter()
        .filter_map(|document| {
            sticker_document_summary(document, Some(&short_name), &installed_names, None)
        })
        .collect::<Vec<_>>();
    let handles = page
        .documents
        .iter()
        .map(|document| {
            let document_id = raw_document_id(document);
            (document_id, opaque_sticker_handle(document_id))
        })
        .collect::<HashMap<_, _>>();
    let packs = page
        .packs
        .into_iter()
        .map(|pack| {
            let tl::enums::StickerPack::StickerPack(pack) = pack;
            StickerEmojiPackSummary {
                emoji: pack.emoticon,
                sticker_handles: pack
                    .documents
                    .iter()
                    .filter_map(|document_id| handles.get(document_id).cloned())
                    .collect(),
            }
        })
        .collect();
    let keywords =
        page.keywords
            .into_iter()
            .filter_map(|keyword| {
                let tl::enums::StickerKeyword::StickerKeyword(keyword) = keyword;
                handles.get(&keyword.document_id).cloned().map(|sticker_handle| {
                    StickerKeywordSummary { sticker_handle, keywords: keyword.keyword }
                })
            })
            .collect();
    StickerSetDetailsPage {
        not_modified: false,
        sticker_set: Some(sticker_set),
        packs,
        keywords,
        stickers,
    }
}

async fn resolve_raw_sticker_document(
    client: &Client,
    set_short_name: &str,
    sticker_handle: &str,
) -> Result<tl::types::Document, TelegramError> {
    let page = client
        .invoke(&tl::functions::messages::GetStickerSet {
            stickerset: tl::enums::InputStickerSet::ShortName(
                tl::types::InputStickerSetShortName { short_name: set_short_name.to_string() },
            ),
            hash: 0,
        })
        .await
        .map_err(|error| TelegramError::Telegram(error.to_string()))?;
    let tl::enums::messages::StickerSet::StickerSet(page) = page else {
        return Err(TelegramError::Telegram(
            "Telegram unexpectedly returned stickerSetNotModified for hash 0".to_string(),
        ));
    };
    let tl::enums::StickerSet::StickerSet(set) = page.set;
    if set.short_name != set_short_name {
        return Err(TelegramError::InvalidPeer(
            "Telegram returned a different sticker set than requested".to_string(),
        ));
    }
    page.documents
        .into_iter()
        .find_map(|document| match document {
            tl::enums::Document::Document(document)
                if opaque_sticker_handle(document.id) == sticker_handle
                    && document.attributes.iter().any(|attribute| {
                        matches!(
                            attribute,
                            tl::enums::DocumentAttribute::Sticker(_)
                                | tl::enums::DocumentAttribute::CustomEmoji(_)
                        )
                    }) =>
            {
                Some(document)
            },
            _ => None,
        })
        .ok_or_else(|| {
            TelegramError::InvalidPeer(
                "the opaque sticker handle was not found in the selected sticker set".to_string(),
            )
        })
}

fn tl_json_object_field<'a>(
    value: &'a tl::enums::Jsonvalue,
    key: &str,
) -> Option<&'a tl::enums::Jsonvalue> {
    let tl::enums::Jsonvalue::JsonObject(object) = value else {
        return None;
    };
    object.value.iter().find_map(|entry| {
        let tl::enums::JsonobjectValue::JsonObjectValue(entry) = entry;
        (entry.key == key).then_some(&entry.value)
    })
}

fn tl_json_string_array(value: &tl::enums::Jsonvalue) -> Option<Vec<String>> {
    let tl::enums::Jsonvalue::JsonArray(array) = value else {
        return None;
    };
    array
        .value
        .iter()
        .map(|value| match value {
            tl::enums::Jsonvalue::JsonString(value) => Some(value.value.clone()),
            _ => None,
        })
        .collect()
}

fn input_geo_point(point: &GeoPointSpec) -> tl::enums::InputGeoPoint {
    tl::enums::InputGeoPoint::InputGeoPoint(tl::types::InputGeoPoint {
        lat: f64::from(point.latitude_e7) / 10_000_000.0,
        long: f64::from(point.longitude_e7) / 10_000_000.0,
        accuracy_radius: point.accuracy_radius_m,
    })
}

fn input_reply_to(
    reply_to_message_id: Option<i32>,
    topic_id: Option<i32>,
) -> Option<tl::enums::InputReplyTo> {
    let reply_to_msg_id = reply_to_message_id.or(topic_id)?;
    Some(tl::enums::InputReplyTo::Message(tl::types::InputReplyToMessage {
        reply_to_msg_id,
        top_msg_id: topic_id,
        reply_to_peer_id: None,
        quote_text: None,
        quote_entities: None,
        quote_offset: None,
        monoforum_peer_id: None,
        poll_option: None,
        todo_item_id: None,
    }))
}

fn caption_input(caption: &str, parse_mode: CaptionParseMode) -> InputMessage {
    match parse_mode {
        CaptionParseMode::Plain => InputMessage::text(caption),
        CaptionParseMode::Markdown => tel_anchors_as_phone_entities(InputMessage::markdown(caption)),
        CaptionParseMode::Html => tel_anchors_as_phone_entities(InputMessage::html(caption)),
        CaptionParseMode::RichMarkdown => {
            let normalized_caption = normalize_rich_markdown(caption);
            InputMessage::text("")
                .rich_text(ferogram::parsers::parse_rich_markdown(&normalized_caption))
        },
    }
}

/// Edited messages carry the same entity payload as send paths; rich text is
/// rejected because `messages.editMessage` has no rich-message field here.
fn edit_text_input(text: &str, parse_mode: CaptionParseMode) -> Result<InputMessage, TelegramError> {
    match parse_mode {
        CaptionParseMode::Plain => Ok(InputMessage::text(text)),
        CaptionParseMode::Markdown => {
            Ok(tel_anchors_as_phone_entities(InputMessage::markdown(text)))
        },
        CaptionParseMode::Html => Ok(tel_anchors_as_phone_entities(InputMessage::html(text))),
        CaptionParseMode::RichMarkdown => Err(TelegramError::Telegram(
            "rich_markdown parse mode is not supported for message edits".to_string(),
        )),
    }
}

/// Telegram silently drops a `text_url` entity whose URL is a `tel:` link, but
/// a phone number is clickable through `messageEntityPhone`. Convert the HTML
/// `<a href="tel:…">` anchor (and its Markdown twin) at the boundary so the
/// visible text stays untouched and the entity survives on the server.
fn tel_anchors_as_phone_entities(message: InputMessage) -> InputMessage {
    let Some(entities) = message.entities else {
        return message;
    };
    let entities = entities
        .into_iter()
        .map(|entity| match entity {
            tl::enums::MessageEntity::TextUrl(url) if canonical_tel_url(&url.url).is_some() => {
                tl::enums::MessageEntity::Phone(tl::types::MessageEntityPhone {
                    offset: url.offset,
                    length: url.length,
                })
            },
            other => other,
        })
        .collect();
    InputMessage { entities: Some(entities), ..message }
}

/// Canonical `tel:+7…` form of a `tel:` URL, or `None` for any other URL.
pub(crate) fn canonical_tel_url(url: &str) -> Option<String> {
    if !url.get(..4).is_some_and(|prefix| prefix.eq_ignore_ascii_case("tel:")) {
        return None;
    }
    Some(format!("tel:{}", canonical_phone_number(&url[4..])))
}

fn canonical_phone_number(raw: &str) -> String {
    let digits: String = raw.chars().filter(char::is_ascii_digit).collect();
    if digits.len() == 11 && digits.starts_with('8') {
        format!("+7{}", &digits[1..])
    } else if digits.len() == 11 && digits.starts_with('7') {
        format!("+{digits}")
    } else if let Some(rest) = digits.strip_prefix("00") {
        format!("+{rest}")
    } else {
        format!("+{digits}")
    }
}

/// Renders a message exactly like Telegram HTML, with phone entities exposed as
/// `tel:` anchors so readback and delete-recovery snapshots keep the link.
fn html_with_phone_links(text: &str, entities: &[tl::enums::MessageEntity]) -> String {
    let mapped: Vec<tl::enums::MessageEntity> = entities
        .iter()
        .map(|entity| match entity {
            tl::enums::MessageEntity::Phone(phone) => {
                let visible = utf16_slice(text, phone.offset, phone.length).unwrap_or_default();
                tl::enums::MessageEntity::TextUrl(tl::types::MessageEntityTextUrl {
                    offset: phone.offset,
                    length: phone.length,
                    url: format!("tel:{}", canonical_phone_number(visible)),
                })
            },
            other => other.clone(),
        })
        .collect();
    ferogram::parsers::generate_html(text, &mapped)
}

/// UTF-16 slice of `text`, the coordinate system Telegram entities use.
fn utf16_slice(text: &str, offset: i32, length: i32) -> Option<&str> {
    let offset = usize::try_from(offset).ok()?;
    let length = usize::try_from(length).ok()?;
    let end = offset.checked_add(length)?;
    let mut utf16 = 0usize;
    let mut start = None;
    for (index, character) in text.char_indices() {
        if start.is_none() && utf16 == offset {
            start = Some(index);
        }
        if utf16 >= end {
            return start.map(|start| &text[start..index]);
        }
        utf16 = utf16.checked_add(character.len_utf16())?;
    }
    if utf16 == end { start.map(|start| &text[start..]) } else { None }
}

/// Turns the result of `messages.uploadMedia` for an album photo into the file reference
/// `messages.sendMultiMedia` accepts.
///
/// An album item must reference already-uploaded media (`inputMediaPhoto`): passing the
/// freshly uploaded `InputMedia::UploadedPhoto` straight into `sendMultiMedia` makes Telegram
/// answer 400 `MEDIA_INVALID` (live probe, task #5125). `messages.uploadMedia` is the step that
/// finalizes the upload and returns the `MessageMedia` whose id/access_hash/file_reference are
/// safe to reference.
fn finalized_album_photo_media(
    media: tl::enums::MessageMedia,
) -> Result<tl::enums::InputMedia, TelegramError> {
    let tl::enums::MessageMedia::Photo(photo_media) = media else {
        return Err(TelegramError::SendRejected(
            "Telegram returned non-photo media for an uploaded album photo".to_string(),
        ));
    };
    let Some(tl::enums::Photo::Photo(photo)) = photo_media.photo else {
        return Err(TelegramError::SendRejected(
            "Telegram did not return an uploaded album photo".to_string(),
        ));
    };
    Ok(tl::enums::InputMedia::Photo(tl::types::InputMediaPhoto {
        spoiler: false,
        live_photo: false,
        id: tl::enums::InputPhoto::InputPhoto(tl::types::InputPhoto {
            id: photo.id,
            access_hash: photo.access_hash,
            file_reference: photo.file_reference,
        }),
        ttl_seconds: None,
        video: None,
    }))
}

/// Turns the result of `messages.uploadMedia` for an album video into the file reference
/// `messages.sendMultiMedia` accepts.
///
/// Same reason as [`finalized_album_photo_media`]: a fresh `InputMedia::UploadedDocument` inside
/// `sendMultiMedia` is rejected with 400 `MEDIA_INVALID`, so the upload is finalized first and the
/// album item carries `inputMediaDocument`.
fn finalized_album_document_media(
    media: tl::enums::MessageMedia,
) -> Result<tl::enums::InputMedia, TelegramError> {
    let tl::enums::MessageMedia::Document(doc_media) = media else {
        return Err(TelegramError::SendRejected(
            "Telegram returned non-document media for an uploaded album video".to_string(),
        ));
    };
    let Some(tl::enums::Document::Document(document)) = doc_media.document else {
        return Err(TelegramError::SendRejected(
            "Telegram did not return an uploaded album document".to_string(),
        ));
    };
    Ok(tl::enums::InputMedia::Document(tl::types::InputMediaDocument {
        spoiler: false,
        id: tl::enums::InputDocument::InputDocument(tl::types::InputDocument {
            id: document.id,
            access_hash: document.access_hash,
            file_reference: document.file_reference,
        }),
        video_cover: None,
        video_timestamp: None,
        ttl_seconds: None,
        query: None,
    }))
}

pub(crate) fn normalize_rich_markdown(markdown: &str) -> String {
    markdown
        .split_inclusive('\n')
        .map(|segment| {
            let line = segment.strip_suffix('\n').unwrap_or(segment);
            let line_without_cr = line.strip_suffix('\r').unwrap_or(line);
            if rich_markdown_line_is_literal_hash(line_without_cr) {
                format!("\\{segment}")
            } else {
                segment.to_string()
            }
        })
        .collect()
}

fn rich_markdown_line_is_literal_hash(line: &str) -> bool {
    if !line.starts_with('#') {
        return false;
    }
    let hash_count = line.chars().take_while(|character| *character == '#').count();
    if !(1..=6).contains(&hash_count) {
        return false;
    }
    line.chars().nth(hash_count).is_some_and(|character| !character.is_whitespace())
}

fn reject_rich_media_caption(
    parse_mode: CaptionParseMode,
    operation: &str,
) -> Result<(), TelegramError> {
    if matches!(parse_mode, CaptionParseMode::RichMarkdown) {
        return Err(TelegramError::InvalidPeer(format!(
            "{operation} does not support native RichMessage captions; use a text message"
        )));
    }
    Ok(())
}

fn rich_media_to_input(media: &RichMediaSpec) -> tl::enums::InputMedia {
    match media {
        RichMediaSpec::Location {
            point,
            live_period_seconds,
            heading_degrees,
            proximity_notification_radius_m,
        } => match live_period_seconds {
            Some(period) => tl::enums::InputMedia::GeoLive(tl::types::InputMediaGeoLive {
                stopped: false,
                geo_point: input_geo_point(point),
                heading: *heading_degrees,
                period: Some(*period),
                proximity_notification_radius: *proximity_notification_radius_m,
            }),
            None => tl::enums::InputMedia::GeoPoint(tl::types::InputMediaGeoPoint {
                geo_point: input_geo_point(point),
            }),
        },
        RichMediaSpec::Venue { point, title, address, provider, venue_id, venue_type } => {
            tl::enums::InputMedia::Venue(tl::types::InputMediaVenue {
                geo_point: input_geo_point(point),
                title: title.clone(),
                address: address.clone(),
                provider: provider.clone(),
                venue_id: venue_id.clone(),
                venue_type: venue_type.clone(),
            })
        },
        RichMediaSpec::Contact { phone_number, first_name, last_name, vcard } => {
            tl::enums::InputMedia::Contact(tl::types::InputMediaContact {
                phone_number: phone_number.clone(),
                first_name: first_name.clone(),
                last_name: last_name.clone(),
                vcard: vcard.clone(),
            })
        },
        RichMediaSpec::Dice { emoji } => {
            tl::enums::InputMedia::Dice(tl::types::InputMediaDice { emoticon: emoji.clone() })
        },
    }
}

fn geo_point_from_tl(geo: &tl::enums::GeoPoint) -> Result<Option<GeoPointSpec>, TelegramError> {
    let tl::enums::GeoPoint::GeoPoint(geo) = geo else {
        return Ok(None);
    };
    #[allow(clippy::cast_possible_truncation)]
    fn coordinate_to_e7(value: f64, label: &str) -> Result<i32, TelegramError> {
        if !value.is_finite() {
            return Err(TelegramError::Telegram(format!(
                "Telegram returned a non-finite {label} coordinate"
            )));
        }
        let rounded = (value * 10_000_000.0).round();
        if rounded < f64::from(i32::MIN) || rounded > f64::from(i32::MAX) {
            return Err(TelegramError::Telegram(format!(
                "Telegram returned an out-of-range {label} coordinate"
            )));
        }
        Ok(rounded as i32)
    }
    Ok(Some(GeoPointSpec {
        latitude_e7: coordinate_to_e7(geo.lat, "latitude")?,
        longitude_e7: coordinate_to_e7(geo.long, "longitude")?,
        accuracy_radius_m: geo.accuracy_radius,
    }))
}

fn rich_media_summary_from_tl(
    media: &tl::enums::MessageMedia,
) -> Result<Option<SentRichMediaSummary>, TelegramError> {
    let (media, dice_value) = match media {
        tl::enums::MessageMedia::Geo(media) => {
            let Some(point) = geo_point_from_tl(&media.geo)? else {
                return Ok(None);
            };
            (
                RichMediaSpec::Location {
                    point,
                    live_period_seconds: None,
                    heading_degrees: None,
                    proximity_notification_radius_m: None,
                },
                None,
            )
        },
        tl::enums::MessageMedia::GeoLive(media) => {
            let Some(point) = geo_point_from_tl(&media.geo)? else {
                return Ok(None);
            };
            (
                RichMediaSpec::Location {
                    point,
                    live_period_seconds: Some(media.period),
                    heading_degrees: media.heading,
                    proximity_notification_radius_m: media.proximity_notification_radius,
                },
                None,
            )
        },
        tl::enums::MessageMedia::Venue(media) => {
            let Some(point) = geo_point_from_tl(&media.geo)? else {
                return Ok(None);
            };
            (
                RichMediaSpec::Venue {
                    point,
                    title: media.title.clone(),
                    address: media.address.clone(),
                    provider: media.provider.clone(),
                    venue_id: media.venue_id.clone(),
                    venue_type: media.venue_type.clone(),
                },
                None,
            )
        },
        tl::enums::MessageMedia::Contact(media) => (
            RichMediaSpec::Contact {
                phone_number: media.phone_number.clone(),
                first_name: media.first_name.clone(),
                last_name: media.last_name.clone(),
                vcard: media.vcard.clone(),
            },
            None,
        ),
        tl::enums::MessageMedia::Dice(media) => {
            (RichMediaSpec::Dice { emoji: media.emoticon.clone() }, Some(media.value))
        },
        _ => return Ok(None),
    };
    Ok(Some(SentRichMediaSummary { media, dice_value }))
}

fn document_media_summary_from_tl(
    media: &tl::enums::MessageMedia,
) -> Result<Option<SentDocumentMediaSummary>, TelegramError> {
    let tl::enums::MessageMedia::Document(media_document) = media else {
        return Ok(None);
    };
    let Some(tl::enums::Document::Document(document)) = media_document.document.as_ref() else {
        return Ok(None);
    };
    let mut duration_seconds = None;
    let mut width_px = None;
    let mut height_px = None;
    for attribute in &document.attributes {
        match attribute {
            tl::enums::DocumentAttribute::Audio(attribute) => {
                duration_seconds = Some(attribute.duration);
            },
            tl::enums::DocumentAttribute::Video(attribute) => {
                if !attribute.duration.is_finite()
                    || attribute.duration < 0.0
                    || attribute.duration > f64::from(i32::MAX)
                {
                    return Err(TelegramError::Telegram(
                        "Telegram returned an invalid document video duration".to_string(),
                    ));
                }
                #[allow(clippy::cast_possible_truncation)]
                let rounded_duration = attribute.duration.round() as i32;
                duration_seconds = Some(rounded_duration);
                width_px = Some(attribute.w);
                height_px = Some(attribute.h);
            },
            _ => {},
        }
    }
    Ok(Some(SentDocumentMediaSummary {
        media_type: media_type_name(media),
        mime_type: document.mime_type.clone(),
        duration_seconds,
        width_px,
        height_px,
    }))
}

fn media_type_name(media: &tl::enums::MessageMedia) -> String {
    match media {
        tl::enums::MessageMedia::Photo(_) => "photo",
        tl::enums::MessageMedia::Document(media) => {
            if media.document.as_ref().is_some_and(|document| match document {
                tl::enums::Document::Document(document) => {
                    document.attributes.iter().any(|attribute| {
                        matches!(
                            attribute,
                            tl::enums::DocumentAttribute::Sticker(_)
                                | tl::enums::DocumentAttribute::CustomEmoji(_)
                        )
                    })
                },
                tl::enums::Document::Empty(_) => false,
            }) {
                "sticker"
            } else if media.document.as_ref().is_some_and(|document| match document {
                tl::enums::Document::Document(document) => document
                    .attributes
                    .iter()
                    .any(|attribute| matches!(attribute, tl::enums::DocumentAttribute::Animated)),
                tl::enums::Document::Empty(_) => false,
            }) {
                "gif"
            } else if media.voice {
                "voice"
            } else if media.round {
                "video_note"
            } else if media.video {
                "video"
            } else {
                "document"
            }
        },
        tl::enums::MessageMedia::Geo(_) | tl::enums::MessageMedia::GeoLive(_) => "geo",
        tl::enums::MessageMedia::Venue(_) => "venue",
        tl::enums::MessageMedia::Contact(_) => "contact",
        tl::enums::MessageMedia::Poll(_) => "poll",
        tl::enums::MessageMedia::Dice(_) => "dice",
        tl::enums::MessageMedia::WebPage(_) => "web_page",
        _ => "other",
    }
    .to_string()
}

fn stealth_mode_dates(mode: &tl::enums::StoriesStealthMode) -> (Option<i32>, Option<i32>) {
    let tl::enums::StoriesStealthMode::StoriesStealthMode(mode) = mode;
    (mode.active_until_date, mode.cooldown_until_date)
}

fn story_peer_summary(peer: tl::types::PeerStories) -> StoryPeerSummary {
    let peer_ref = canonical_peer_ref(&peer.peer);
    let stories = peer.stories.into_iter().map(story_summary).collect::<Vec<_>>();
    StoryPeerSummary { peer_ref, max_read_id: peer.max_read_id, stories }
}

fn story_collection_page(
    peer_ref: String,
    response: tl::enums::stories::Stories,
    page_limit: Option<usize>,
) -> StoryCollectionPage {
    let tl::enums::stories::Stories::Stories(response) = response;
    let stories = response.stories.into_iter().map(story_summary).collect::<Vec<_>>();
    let has_more = page_limit.is_some_and(|limit| {
        stories.len() == limit && i32::try_from(stories.len()).unwrap_or(i32::MAX) < response.count
    });
    StoryCollectionPage {
        peer_ref,
        count: response.count,
        next_offset_id: has_more.then(|| stories.last().map(|story| story.story_id)).flatten(),
        completeness: if has_more { Completeness::Page } else { Completeness::Complete },
        stories,
        pinned_to_top_ids: response.pinned_to_top.unwrap_or_default(),
    }
}

fn story_summary(item: tl::enums::StoryItem) -> StorySummary {
    match item {
        tl::enums::StoryItem::Deleted(item) => StorySummary {
            state: "deleted".to_string(),
            story_id: item.id,
            date_unix: 0,
            expire_date_unix: 0,
            from_ref: None,
            caption: None,
            pinned: false,
            public: false,
            close_friends: false,
            noforwards: false,
            edited: false,
            contacts: false,
            selected_contacts: false,
            outgoing: false,
            live: false,
            media_kind: None,
            views_count: None,
            forwards_count: None,
            reactions_count: None,
            sent_reaction: None,
        },
        tl::enums::StoryItem::Skipped(item) => StorySummary {
            state: "skipped".to_string(),
            story_id: item.id,
            date_unix: item.date,
            expire_date_unix: item.expire_date,
            from_ref: None,
            caption: None,
            pinned: false,
            public: false,
            close_friends: item.close_friends,
            noforwards: false,
            edited: false,
            contacts: false,
            selected_contacts: false,
            outgoing: false,
            live: item.live,
            media_kind: None,
            views_count: None,
            forwards_count: None,
            reactions_count: None,
            sent_reaction: None,
        },
        tl::enums::StoryItem::StoryItem(item) => {
            let (views_count, forwards_count, reactions_count) =
                item.views.as_ref().map(story_views_counts).unwrap_or((None, None, None));
            StorySummary {
                state: "active".to_string(),
                story_id: item.id,
                date_unix: item.date,
                expire_date_unix: item.expire_date,
                from_ref: item.from_id.as_ref().map(canonical_peer_ref),
                caption: item.caption,
                pinned: item.pinned,
                public: item.public,
                close_friends: item.close_friends,
                noforwards: item.noforwards,
                edited: item.edited,
                contacts: item.contacts,
                selected_contacts: item.selected_contacts,
                outgoing: item.out,
                live: false,
                media_kind: Some(media_type_name(&item.media)),
                views_count,
                forwards_count,
                reactions_count,
                sent_reaction: story_reaction_spec(item.sent_reaction),
            }
        },
    }
}

fn story_reaction_spec(reaction: Option<tl::enums::Reaction>) -> Option<ReactionSpec> {
    match reaction {
        Some(tl::enums::Reaction::Emoji(reaction)) => {
            Some(ReactionSpec::Emoji { emoji: reaction.emoticon, big: false, add_to_recent: false })
        },
        Some(tl::enums::Reaction::CustomEmoji(reaction)) => Some(ReactionSpec::CustomEmoji {
            document_id: reaction.document_id,
            big: false,
            add_to_recent: false,
        }),
        Some(tl::enums::Reaction::Empty | tl::enums::Reaction::Paid) | None => None,
    }
}

fn story_views_counts(views: &tl::enums::StoryViews) -> (Option<i32>, Option<i32>, Option<i32>) {
    let tl::enums::StoryViews::StoryViews(views) = views;
    (Some(views.views_count), views.forwards_count, views.reactions_count)
}

fn story_viewer_summary(
    view: tl::enums::StoryView,
    users: &[tl::enums::User],
) -> StoryViewerSummary {
    match view {
        tl::enums::StoryView::StoryView(view) => {
            let user_id = view.user_id;
            let (reaction_kind, reaction_emoji, reaction_custom_emoji_id) =
                story_reaction_summary(view.reaction);
            StoryViewerSummary {
                kind: "user".to_string(),
                user_id: Some(user_id),
                source_peer_ref: None,
                user: contact_summary_by_user_id(users, user_id),
                date_unix: Some(view.date),
                blocked: view.blocked,
                blocked_my_stories_from: view.blocked_my_stories_from,
                reaction_kind,
                reaction_emoji,
                reaction_custom_emoji_id,
            }
        },
        tl::enums::StoryView::PublicForward(view) => StoryViewerSummary {
            kind: "public_forward".to_string(),
            user_id: None,
            source_peer_ref: None,
            user: None,
            date_unix: None,
            blocked: view.blocked,
            blocked_my_stories_from: view.blocked_my_stories_from,
            reaction_kind: None,
            reaction_emoji: None,
            reaction_custom_emoji_id: None,
        },
        tl::enums::StoryView::PublicRepost(view) => StoryViewerSummary {
            kind: "public_repost".to_string(),
            user_id: None,
            source_peer_ref: Some(canonical_peer_ref(&view.peer_id)),
            user: None,
            date_unix: None,
            blocked: view.blocked,
            blocked_my_stories_from: view.blocked_my_stories_from,
            reaction_kind: None,
            reaction_emoji: None,
            reaction_custom_emoji_id: None,
        },
    }
}

fn story_reaction_summary(
    reaction: Option<tl::enums::Reaction>,
) -> (Option<String>, Option<String>, Option<i64>) {
    match reaction {
        Some(tl::enums::Reaction::Empty) | None => (None, None, None),
        Some(tl::enums::Reaction::Emoji(reaction)) => {
            (Some("emoji".to_string()), Some(reaction.emoticon), None)
        },
        Some(tl::enums::Reaction::CustomEmoji(reaction)) => {
            (Some("custom_emoji".to_string()), None, Some(reaction.document_id))
        },
        Some(tl::enums::Reaction::Paid) => (Some("paid".to_string()), None, None),
    }
}

fn reaction_identity_from_tl(reaction: tl::enums::Reaction) -> ReactionIdentity {
    match reaction {
        tl::enums::Reaction::Empty => ReactionIdentity::Empty,
        tl::enums::Reaction::Emoji(reaction) => {
            ReactionIdentity::Emoji { emoji: reaction.emoticon }
        },
        tl::enums::Reaction::CustomEmoji(reaction) => {
            ReactionIdentity::CustomEmoji { document_id: reaction.document_id }
        },
        tl::enums::Reaction::Paid => ReactionIdentity::Paid,
    }
}

fn chat_reactions_policy_from_tl(reactions: tl::enums::ChatReactions) -> ChatReactionsPolicy {
    match reactions {
        tl::enums::ChatReactions::None => ChatReactionsPolicy::None,
        tl::enums::ChatReactions::All(reactions) => {
            ChatReactionsPolicy::All { allow_custom: reactions.allow_custom }
        },
        tl::enums::ChatReactions::Some(reactions) => ChatReactionsPolicy::Some {
            reactions: reactions.reactions.into_iter().map(reaction_identity_from_tl).collect(),
        },
    }
}

fn chat_reactions_policy_to_tl(
    reactions: &ChatReactionsPolicy,
) -> Result<tl::enums::ChatReactions, TelegramError> {
    Ok(match reactions {
        ChatReactionsPolicy::None => tl::enums::ChatReactions::None,
        ChatReactionsPolicy::All { allow_custom } => {
            tl::enums::ChatReactions::All(tl::types::ChatReactionsAll {
                allow_custom: *allow_custom,
            })
        },
        ChatReactionsPolicy::Some { reactions } => {
            let reactions = reactions
                .iter()
                .map(|reaction| match reaction {
                    ReactionIdentity::Emoji { emoji } => {
                        if emoji.trim().is_empty() {
                            return Err(TelegramError::InvalidPeer(
                                "chat reaction emoji must not be empty".to_string(),
                            ));
                        }
                        Ok(tl::enums::Reaction::Emoji(tl::types::ReactionEmoji {
                            emoticon: emoji.clone(),
                        }))
                    },
                    ReactionIdentity::CustomEmoji { document_id } if *document_id > 0 => {
                        Ok(tl::enums::Reaction::CustomEmoji(tl::types::ReactionCustomEmoji {
                            document_id: *document_id,
                        }))
                    },
                    ReactionIdentity::CustomEmoji { .. } => Err(TelegramError::InvalidPeer(
                        "chat custom-emoji reaction document_id must be positive".to_string(),
                    )),
                    ReactionIdentity::Empty | ReactionIdentity::Paid => {
                        Err(TelegramError::InvalidPeer(
                            "chat reaction policy accepts only emoji and custom_emoji entries"
                                .to_string(),
                        ))
                    },
                })
                .collect::<Result<Vec<_>, _>>()?;
            tl::enums::ChatReactions::Some(tl::types::ChatReactionsSome { reactions })
        },
    })
}

fn typing_action_to_tl(action: TypingAction) -> tl::enums::SendMessageAction {
    match action {
        TypingAction::Typing => tl::enums::SendMessageAction::SendMessageTypingAction,
        TypingAction::UploadPhoto => tl::enums::SendMessageAction::SendMessageUploadPhotoAction(
            tl::types::SendMessageUploadPhotoAction { progress: 0 },
        ),
        TypingAction::UploadDocument => {
            tl::enums::SendMessageAction::SendMessageUploadDocumentAction(
                tl::types::SendMessageUploadDocumentAction { progress: 0 },
            )
        },
        TypingAction::Cancel => tl::enums::SendMessageAction::SendMessageCancelAction,
    }
}

fn saved_tag_reaction_to_tl(
    reaction: &ReactionIdentity,
) -> Result<tl::enums::Reaction, TelegramError> {
    match reaction {
        ReactionIdentity::Emoji { emoji } => {
            Ok(tl::enums::Reaction::Emoji(tl::types::ReactionEmoji { emoticon: emoji.clone() }))
        },
        ReactionIdentity::CustomEmoji { document_id } => {
            Ok(tl::enums::Reaction::CustomEmoji(tl::types::ReactionCustomEmoji {
                document_id: *document_id,
            }))
        },
        ReactionIdentity::Empty | ReactionIdentity::Paid => Err(TelegramError::InvalidPeer(
            "Saved Messages tags require an emoji or custom-emoji reaction".to_string(),
        )),
    }
}

fn saved_reaction_tags_page(tags: tl::enums::messages::SavedReactionTags) -> SavedReactionTagsPage {
    match tags {
        tl::enums::messages::SavedReactionTags::NotModified => {
            SavedReactionTagsPage { tags: Vec::new(), hash: None, not_modified: true }
        },
        tl::enums::messages::SavedReactionTags::SavedReactionTags(tags) => SavedReactionTagsPage {
            tags: tags
                .tags
                .into_iter()
                .map(|tag| {
                    let tl::enums::SavedReactionTag::SavedReactionTag(tag) = tag;
                    SavedReactionTagSummary {
                        reaction: reaction_identity_from_tl(tag.reaction),
                        title: tag.title,
                        count: tag.count,
                    }
                })
                .collect(),
            hash: Some(tags.hash.to_string()),
            not_modified: false,
        },
    }
}

fn saved_dialogs_page(page: tl::enums::messages::SavedDialogs, limit: usize) -> SavedDialogsPage {
    let (raw_dialogs, users, chats, total_count) = match page {
        tl::enums::messages::SavedDialogs::SavedDialogs(page) => {
            let count = i32::try_from(page.dialogs.len()).ok();
            (page.dialogs, page.users, page.chats, count)
        },
        tl::enums::messages::SavedDialogs::Slice(page) => {
            (page.dialogs, page.users, page.chats, Some(page.count))
        },
        tl::enums::messages::SavedDialogs::NotModified(page) => {
            return SavedDialogsPage {
                dialogs: Vec::new(),
                total_count: Some(page.count),
                next_offset_id: None,
                completeness: Completeness::Complete,
            };
        },
    };
    let mut titles = users
        .into_iter()
        .filter_map(contact_summary)
        .map(|user| (user.user_id.to_string(), user.display_name))
        .collect::<HashMap<_, _>>();
    for chat in chats.into_iter().filter_map(chat_summary) {
        titles.insert(chat.chat_id, chat.title);
    }
    let raw_count = raw_dialogs.len();
    let dialogs = raw_dialogs
        .into_iter()
        .map(|dialog| {
            let (peer, pinned, top_message_id) = match dialog {
                tl::enums::SavedDialog::SavedDialog(dialog) => {
                    (dialog.peer, dialog.pinned, dialog.top_message)
                },
                tl::enums::SavedDialog::MonoForumDialog(dialog) => {
                    (dialog.peer, false, dialog.top_message)
                },
            };
            let peer_ref = canonical_peer_ref(&peer);
            SavedDialogSummary {
                title: titles.get(&peer_ref).cloned(),
                peer_ref,
                pinned,
                top_message_id,
            }
        })
        .collect::<Vec<_>>();
    let has_more = total_count
        .and_then(|count| usize::try_from(count).ok())
        .is_some_and(|count| count > raw_count)
        || raw_count >= limit;
    let next_offset_id =
        has_more.then(|| dialogs.last().map(|dialog| dialog.top_message_id)).flatten();
    SavedDialogsPage {
        dialogs,
        total_count,
        next_offset_id,
        completeness: if next_offset_id.is_some() {
            Completeness::Page
        } else {
            Completeness::Complete
        },
    }
}

fn default_reaction_to_tl(reaction: &DefaultReactionSpec) -> tl::enums::Reaction {
    match reaction {
        DefaultReactionSpec::Emoji { emoji } => {
            tl::enums::Reaction::Emoji(tl::types::ReactionEmoji { emoticon: emoji.clone() })
        },
        DefaultReactionSpec::CustomEmoji { document_id } => {
            tl::enums::Reaction::CustomEmoji(tl::types::ReactionCustomEmoji {
                document_id: *document_id,
            })
        },
    }
}

fn reaction_catalog_page(reactions: tl::enums::messages::Reactions) -> ReactionCatalogPage {
    match reactions {
        tl::enums::messages::Reactions::NotModified => {
            ReactionCatalogPage { not_modified: true, hash: None, reactions: Vec::new() }
        },
        tl::enums::messages::Reactions::Reactions(reactions) => ReactionCatalogPage {
            not_modified: false,
            hash: Some(reactions.hash),
            reactions: reactions.reactions.into_iter().map(reaction_identity_from_tl).collect(),
        },
    }
}

fn plain_text_with_entities(text: String) -> tl::enums::TextWithEntities {
    tl::enums::TextWithEntities::TextWithEntities(tl::types::TextWithEntities {
        text,
        entities: Vec::new(),
    })
}

fn text_with_entities_text(text: &tl::enums::TextWithEntities) -> &str {
    let tl::enums::TextWithEntities::TextWithEntities(text) = text;
    &text.text
}

fn text_with_entities_has_entities(text: &tl::enums::TextWithEntities) -> bool {
    let tl::enums::TextWithEntities::TextWithEntities(text) = text;
    !text.entities.is_empty()
}

async fn get_raw_dialog_filters(
    client: &Client,
) -> Result<(bool, Vec<tl::enums::DialogFilter>), TelegramError> {
    let response = client
        .invoke(&tl::functions::messages::GetDialogFilters {})
        .await
        .map_err(|error| TelegramError::Telegram(error.to_string()))?;
    let tl::enums::messages::DialogFilters::DialogFilters(response) = response;
    Ok((response.tags_enabled, response.filters))
}

fn dialog_filter_id(filter: &tl::enums::DialogFilter) -> i32 {
    match filter {
        tl::enums::DialogFilter::Default => 0,
        tl::enums::DialogFilter::DialogFilter(filter) => filter.id,
        tl::enums::DialogFilter::Chatlist(filter) => filter.id,
    }
}

fn input_peer_ref(
    peer: &tl::enums::InputPeer,
    account_user_id: i64,
) -> Result<String, TelegramError> {
    match peer {
        tl::enums::InputPeer::PeerSelf => Ok(account_user_id.to_string()),
        tl::enums::InputPeer::Chat(peer) => Ok(format!("-{}", peer.chat_id)),
        tl::enums::InputPeer::User(peer) => Ok(peer.user_id.to_string()),
        tl::enums::InputPeer::Channel(peer) => Ok(format!("-100{}", peer.channel_id)),
        tl::enums::InputPeer::UserFromMessage(peer) => Ok(peer.user_id.to_string()),
        tl::enums::InputPeer::ChannelFromMessage(peer) => Ok(format!("-100{}", peer.channel_id)),
        tl::enums::InputPeer::Empty => Err(TelegramError::Telegram(
            "Telegram returned an empty peer inside a dialog folder".into(),
        )),
    }
}

fn input_peer_refs(
    peers: &[tl::enums::InputPeer],
    account_user_id: i64,
) -> Result<Vec<String>, TelegramError> {
    peers.iter().map(|peer| input_peer_ref(peer, account_user_id)).collect()
}

fn dialog_folder_summary(
    filter: &tl::enums::DialogFilter,
    account_user_id: i64,
) -> Result<DialogFolderSummary, TelegramError> {
    match filter {
        tl::enums::DialogFilter::Default => Ok(DialogFolderSummary {
            folder_id: 0,
            kind: DialogFolderKind::Default,
            title: "All chats".to_string(),
            title_has_entities: false,
            title_noanimate: false,
            emoticon: None,
            color: None,
            contacts: true,
            non_contacts: true,
            groups: true,
            broadcasts: true,
            bots: true,
            exclude_muted: false,
            exclude_read: false,
            exclude_archived: false,
            has_my_invites: false,
            pinned_chat_ids: Vec::new(),
            included_chat_ids: Vec::new(),
            excluded_chat_ids: Vec::new(),
        }),
        tl::enums::DialogFilter::DialogFilter(filter) => Ok(DialogFolderSummary {
            folder_id: filter.id,
            kind: DialogFolderKind::Custom,
            title: text_with_entities_text(&filter.title).to_string(),
            title_has_entities: text_with_entities_has_entities(&filter.title),
            title_noanimate: filter.title_noanimate,
            emoticon: filter.emoticon.clone(),
            color: filter.color,
            contacts: filter.contacts,
            non_contacts: filter.non_contacts,
            groups: filter.groups,
            broadcasts: filter.broadcasts,
            bots: filter.bots,
            exclude_muted: filter.exclude_muted,
            exclude_read: filter.exclude_read,
            exclude_archived: filter.exclude_archived,
            has_my_invites: false,
            pinned_chat_ids: input_peer_refs(&filter.pinned_peers, account_user_id)?,
            included_chat_ids: input_peer_refs(&filter.include_peers, account_user_id)?,
            excluded_chat_ids: input_peer_refs(&filter.exclude_peers, account_user_id)?,
        }),
        tl::enums::DialogFilter::Chatlist(filter) => Ok(DialogFolderSummary {
            folder_id: filter.id,
            kind: DialogFolderKind::Chatlist,
            title: text_with_entities_text(&filter.title).to_string(),
            title_has_entities: text_with_entities_has_entities(&filter.title),
            title_noanimate: filter.title_noanimate,
            emoticon: filter.emoticon.clone(),
            color: filter.color,
            contacts: false,
            non_contacts: false,
            groups: false,
            broadcasts: false,
            bots: false,
            exclude_muted: false,
            exclude_read: false,
            exclude_archived: false,
            has_my_invites: filter.has_my_invites,
            pinned_chat_ids: input_peer_refs(&filter.pinned_peers, account_user_id)?,
            included_chat_ids: input_peer_refs(&filter.include_peers, account_user_id)?,
            excluded_chat_ids: Vec::new(),
        }),
    }
}

fn disallowed_gifts_settings_summary(
    settings: tl::enums::DisallowedGiftsSettings,
) -> DisallowedGiftsSettingsSummary {
    let tl::enums::DisallowedGiftsSettings::DisallowedGiftsSettings(settings) = settings;
    DisallowedGiftsSettingsSummary {
        disallow_unlimited_stargifts: settings.disallow_unlimited_stargifts,
        disallow_limited_stargifts: settings.disallow_limited_stargifts,
        disallow_unique_stargifts: settings.disallow_unique_stargifts,
        disallow_premium_gifts: settings.disallow_premium_gifts,
        disallow_stargifts_from_channels: settings.disallow_stargifts_from_channels,
    }
}

fn disallowed_gifts_settings_input(
    settings: &DisallowedGiftsSettingsSummary,
) -> tl::enums::DisallowedGiftsSettings {
    tl::types::DisallowedGiftsSettings {
        disallow_unlimited_stargifts: settings.disallow_unlimited_stargifts,
        disallow_limited_stargifts: settings.disallow_limited_stargifts,
        disallow_unique_stargifts: settings.disallow_unique_stargifts,
        disallow_premium_gifts: settings.disallow_premium_gifts,
        disallow_stargifts_from_channels: settings.disallow_stargifts_from_channels,
    }
    .into()
}

fn global_privacy_settings_summary(
    settings: tl::enums::GlobalPrivacySettings,
) -> GlobalPrivacySettingsSummary {
    let tl::enums::GlobalPrivacySettings::GlobalPrivacySettings(settings) = settings;
    GlobalPrivacySettingsSummary {
        archive_and_mute_new_noncontact_peers: settings.archive_and_mute_new_noncontact_peers,
        keep_archived_unmuted: settings.keep_archived_unmuted,
        keep_archived_folders: settings.keep_archived_folders,
        hide_read_marks: settings.hide_read_marks,
        new_noncontact_peers_require_premium: settings.new_noncontact_peers_require_premium,
        display_gifts_button: settings.display_gifts_button,
        noncontact_peers_paid_stars: settings
            .noncontact_peers_paid_stars
            .map(|value| value.to_string()),
        disallowed_gifts: settings.disallowed_gifts.map(disallowed_gifts_settings_summary),
    }
}

fn global_privacy_settings_input(
    settings: &GlobalPrivacySettingsSummary,
) -> Result<tl::enums::GlobalPrivacySettings, TelegramError> {
    let noncontact_peers_paid_stars = settings
        .noncontact_peers_paid_stars
        .as_deref()
        .map(|value| positive_telegram_i64(value, "noncontact_peers_paid_stars"))
        .transpose()?;
    Ok(tl::types::GlobalPrivacySettings {
        archive_and_mute_new_noncontact_peers: settings.archive_and_mute_new_noncontact_peers,
        keep_archived_unmuted: settings.keep_archived_unmuted,
        keep_archived_folders: settings.keep_archived_folders,
        hide_read_marks: settings.hide_read_marks,
        new_noncontact_peers_require_premium: settings.new_noncontact_peers_require_premium,
        display_gifts_button: settings.display_gifts_button,
        noncontact_peers_paid_stars,
        disallowed_gifts: settings.disallowed_gifts.as_ref().map(disallowed_gifts_settings_input),
    }
    .into())
}

fn positive_telegram_i64(value: &str, field: &str) -> Result<i64, TelegramError> {
    if value.is_empty()
        || !value.bytes().all(|byte| byte.is_ascii_digit())
        || (value.len() > 1 && value.starts_with('0'))
    {
        return Err(TelegramError::InvalidPeer(format!(
            "{field} must be canonical positive decimal text"
        )));
    }
    value.parse::<i64>().ok().filter(|value| *value > 0).ok_or_else(|| {
        TelegramError::InvalidPeer(format!(
            "{field} must fit Telegram's positive signed 64-bit range"
        ))
    })
}

async fn resolve_input_peer_refs(
    client: &Client,
    peer_refs: &[String],
) -> Result<Vec<tl::enums::InputPeer>, TelegramError> {
    let mut peers = Vec::with_capacity(peer_refs.len());
    for peer_ref in peer_refs {
        let peer = client
            .resolve(peer_ref.as_str())
            .await
            .map_err(|error| TelegramError::Telegram(error.to_string()))?;
        peers.push(
            client
                .resolve_to_input_peer(&peer)
                .await
                .map_err(|error| TelegramError::Telegram(error.to_string()))?,
        );
    }
    Ok(peers)
}

async fn build_dialog_filter(
    client: &Client,
    folder: &DialogFolderSummary,
    preserved_title: Option<tl::enums::TextWithEntities>,
) -> Result<tl::enums::DialogFilter, TelegramError> {
    if folder.kind == DialogFolderKind::Default || folder.folder_id <= 0 {
        return Err(TelegramError::InvalidPeer(
            "the default dialog folder cannot be created or edited".into(),
        ));
    }
    let title = preserved_title.unwrap_or_else(|| plain_text_with_entities(folder.title.clone()));
    let pinned_peers = resolve_input_peer_refs(client, &folder.pinned_chat_ids).await?;
    let include_peers = resolve_input_peer_refs(client, &folder.included_chat_ids).await?;
    match folder.kind {
        DialogFolderKind::Custom => {
            Ok(tl::enums::DialogFilter::DialogFilter(tl::types::DialogFilter {
                contacts: folder.contacts,
                non_contacts: folder.non_contacts,
                groups: folder.groups,
                broadcasts: folder.broadcasts,
                bots: folder.bots,
                exclude_muted: folder.exclude_muted,
                exclude_read: folder.exclude_read,
                exclude_archived: folder.exclude_archived,
                title_noanimate: folder.title_noanimate,
                id: folder.folder_id,
                title,
                emoticon: folder.emoticon.clone(),
                color: folder.color,
                pinned_peers,
                include_peers,
                exclude_peers: resolve_input_peer_refs(client, &folder.excluded_chat_ids).await?,
            }))
        },
        DialogFolderKind::Chatlist => {
            if folder.contacts
                || folder.non_contacts
                || folder.groups
                || folder.broadcasts
                || folder.bots
                || folder.exclude_muted
                || folder.exclude_read
                || folder.exclude_archived
                || !folder.excluded_chat_ids.is_empty()
            {
                return Err(TelegramError::InvalidPeer(
                    "chatlist folders cannot contain category flags or excluded peers".into(),
                ));
            }
            Ok(tl::enums::DialogFilter::Chatlist(tl::types::DialogFilterChatlist {
                has_my_invites: folder.has_my_invites,
                title_noanimate: folder.title_noanimate,
                id: folder.folder_id,
                title,
                emoticon: folder.emoticon.clone(),
                color: folder.color,
                pinned_peers,
                include_peers,
            }))
        },
        DialogFolderKind::Default => unreachable!("validated above"),
    }
}

fn remove_input_peer_ref(
    peers: &mut Vec<tl::enums::InputPeer>,
    target_ref: &str,
    account_user_id: i64,
) -> Result<(), TelegramError> {
    let mut retained = Vec::with_capacity(peers.len());
    for peer in peers.drain(..) {
        if input_peer_ref(&peer, account_user_id)? != target_ref {
            retained.push(peer);
        }
    }
    *peers = retained;
    Ok(())
}

fn contains_input_peer_ref(
    peers: &[tl::enums::InputPeer],
    target_ref: &str,
    account_user_id: i64,
) -> Result<bool, TelegramError> {
    for peer in peers {
        if input_peer_ref(peer, account_user_id)? == target_ref {
            return Ok(true);
        }
    }
    Ok(false)
}

fn mutate_dialog_filter_membership(
    filter: tl::enums::DialogFilter,
    input_peer: tl::enums::InputPeer,
    target_ref: &str,
    account_user_id: i64,
    included: bool,
) -> Result<tl::enums::DialogFilter, TelegramError> {
    match filter {
        tl::enums::DialogFilter::DialogFilter(mut filter) => {
            remove_input_peer_ref(&mut filter.pinned_peers, target_ref, account_user_id)?;
            remove_input_peer_ref(&mut filter.include_peers, target_ref, account_user_id)?;
            remove_input_peer_ref(&mut filter.exclude_peers, target_ref, account_user_id)?;
            if included {
                filter.include_peers.push(input_peer);
            } else {
                filter.exclude_peers.push(input_peer);
            }
            Ok(tl::enums::DialogFilter::DialogFilter(filter))
        },
        tl::enums::DialogFilter::Chatlist(mut filter) => {
            let was_pinned =
                contains_input_peer_ref(&filter.pinned_peers, target_ref, account_user_id)?;
            remove_input_peer_ref(&mut filter.pinned_peers, target_ref, account_user_id)?;
            remove_input_peer_ref(&mut filter.include_peers, target_ref, account_user_id)?;
            if included {
                if was_pinned {
                    filter.pinned_peers.push(input_peer.clone());
                }
                filter.include_peers.push(input_peer);
            }
            Ok(tl::enums::DialogFilter::Chatlist(filter))
        },
        tl::enums::DialogFilter::Default => Err(TelegramError::InvalidPeer(
            "the default dialog folder has no explicit membership".into(),
        )),
    }
}

fn hex_encode(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn decode_hex(value: &str) -> Result<Vec<u8>, TelegramError> {
    if !value.len().is_multiple_of(2) {
        return Err(TelegramError::Telegram(
            "opaque option hex binding has odd length".to_string(),
        ));
    }
    let mut chars = value.chars();
    let mut decoded = Vec::with_capacity(value.len() / 2);
    while let Some(high) = chars.next() {
        let Some(low) = chars.next() else {
            return Err(TelegramError::Telegram(
                "opaque option hex binding is incomplete".to_string(),
            ));
        };
        let high = high.to_digit(16).ok_or_else(|| {
            TelegramError::Telegram("opaque option binding is not hexadecimal".to_string())
        })?;
        let low = low.to_digit(16).ok_or_else(|| {
            TelegramError::Telegram("opaque option binding is not hexadecimal".to_string())
        })?;
        let byte = high
            .checked_mul(16)
            .and_then(|value| value.checked_add(low))
            .and_then(|value| u8::try_from(value).ok())
            .ok_or_else(|| TelegramError::Telegram("opaque option binding is invalid".into()))?;
        decoded.push(byte);
    }
    Ok(decoded)
}

fn poll_media_from_message(
    message: &ferogram::update::IncomingMessage,
) -> Option<&tl::types::MessageMediaPoll> {
    let tl::enums::Message::Message(message) = &message.raw else {
        return None;
    };
    match message.media.as_ref()? {
        tl::enums::MessageMedia::Poll(poll) => Some(poll),
        _ => None,
    }
}

fn poll_update_from_updates<'a>(
    updates: &'a tl::enums::Updates,
    poll_id: i64,
    message_id: i32,
) -> Option<&'a tl::types::UpdateMessagePoll> {
    let matches = |update: &'a tl::enums::Update| match update {
        tl::enums::Update::MessagePoll(update)
            if update.poll_id == poll_id
                && update.msg_id.is_none_or(|candidate| candidate == message_id) =>
        {
            Some(update)
        },
        _ => None,
    };
    match updates {
        tl::enums::Updates::UpdateShort(update) => matches(&update.update),
        tl::enums::Updates::Combined(updates) => updates.updates.iter().find_map(matches),
        tl::enums::Updates::Updates(updates) => updates.updates.iter().find_map(matches),
        tl::enums::Updates::TooLong
        | tl::enums::Updates::UpdateShortMessage(_)
        | tl::enums::Updates::UpdateShortChatMessage(_)
        | tl::enums::Updates::UpdateShortSentMessage(_) => None,
    }
}

fn poll_summary_from_parts(
    chat_id: String,
    message_id: i32,
    poll: &tl::enums::Poll,
    results: &tl::enums::PollResults,
    attached_media: Option<&tl::enums::MessageMedia>,
) -> Result<PollSummary, TelegramError> {
    let tl::enums::Poll::Poll(poll) = poll;
    let tl::enums::PollResults::PollResults(results) = results;
    let result_by_option = results
        .results
        .as_ref()
        .into_iter()
        .flatten()
        .map(|result| {
            let tl::enums::PollAnswerVoters::PollAnswerVoters(result) = result;
            (hex_encode(&result.option), result)
        })
        .collect::<HashMap<_, _>>();
    let mut known_options = std::collections::HashSet::new();
    let answers = poll
        .answers
        .iter()
        .enumerate()
        .map(|(index, answer)| {
            let tl::enums::PollAnswer::PollAnswer(answer) = answer else {
                return Err(TelegramError::Telegram(
                    "Telegram returned an input-only poll answer without opaque option bytes"
                        .to_string(),
                ));
            };
            let option_hex = hex_encode(&answer.option);
            known_options.insert(option_hex.clone());
            let result = result_by_option.get(&option_hex).copied();
            Ok(PollAnswerSummary {
                option: PollOptionBinding {
                    index,
                    text: text_with_entities_text(&answer.text).to_string(),
                    option_hex,
                },
                media_type: answer.media.as_ref().map(media_type_name),
                added_by_ref: answer.added_by.as_ref().map(canonical_peer_ref),
                date_unix: answer.date,
                chosen: result.is_some_and(|result| result.chosen),
                correct: result.is_some_and(|result| result.correct),
                voters: result.and_then(|result| result.voters),
                recent_voter_refs: result
                    .and_then(|result| result.recent_voters.as_ref())
                    .into_iter()
                    .flatten()
                    .map(canonical_peer_ref)
                    .collect(),
            })
        })
        .collect::<Result<Vec<_>, TelegramError>>()?;
    let unmatched_result_option_hex = result_by_option
        .keys()
        .filter(|option| !known_options.contains(*option))
        .cloned()
        .collect();
    Ok(PollSummary {
        chat_id,
        message_id,
        poll_id: poll.id,
        closed: poll.closed,
        public_voters: poll.public_voters,
        multiple_choice: poll.multiple_choice,
        quiz: poll.quiz,
        open_answers: poll.open_answers,
        revoting_disabled: poll.revoting_disabled,
        shuffle_answers: poll.shuffle_answers,
        hide_results_until_close: poll.hide_results_until_close,
        creator: poll.creator,
        subscribers_only: poll.subscribers_only,
        question: text_with_entities_text(&poll.question).to_string(),
        answers,
        unmatched_result_option_hex,
        close_period_seconds: poll.close_period,
        close_date_unix: poll.close_date,
        countries_iso2: poll.countries_iso2.clone().unwrap_or_default(),
        poll_hash: poll.hash,
        results_minimal: results.min,
        has_unread_votes: results.has_unread_votes,
        can_view_stats: results.can_view_stats,
        total_voters: results.total_voters,
        recent_voter_refs: results
            .recent_voters
            .as_ref()
            .into_iter()
            .flatten()
            .map(canonical_peer_ref)
            .collect(),
        solution: results.solution.clone(),
        solution_has_media: results.solution_media.is_some(),
        attached_media_type: attached_media.map(media_type_name),
    })
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

fn sent_story_id(
    updates: &tl::enums::Updates,
    random_id: i64,
    expected_peer: &tl::enums::Peer,
) -> Option<i32> {
    let from_update = |update: &tl::enums::Update| match update {
        tl::enums::Update::StoryId(value) if value.random_id == random_id => Some(value.id),
        tl::enums::Update::Story(value) if value.peer == *expected_peer => {
            Some(story_item_id(&value.story))
        },
        _ => None,
    };
    match updates {
        tl::enums::Updates::UpdateShort(value) => from_update(&value.update),
        tl::enums::Updates::Combined(value) => value.updates.iter().find_map(from_update),
        tl::enums::Updates::Updates(value) => value.updates.iter().find_map(from_update),
        tl::enums::Updates::TooLong
        | tl::enums::Updates::UpdateShortSentMessage(_)
        | tl::enums::Updates::UpdateShortMessage(_)
        | tl::enums::Updates::UpdateShortChatMessage(_) => None,
    }
}

fn story_item_id(story: &tl::enums::StoryItem) -> i32 {
    match story {
        tl::enums::StoryItem::Deleted(story) => story.id,
        tl::enums::StoryItem::Skipped(story) => story.id,
        tl::enums::StoryItem::StoryItem(story) => story.id,
    }
}

fn sent_message_ids(updates: &tl::enums::Updates, random_ids: &[i64]) -> Option<Vec<i32>> {
    if random_ids.len() == 1 {
        return random_ids
            .first()
            .and_then(|random_id| sent_message_id(updates, *random_id))
            .map(|message_id| vec![message_id]);
    }
    let update_list = match updates {
        tl::enums::Updates::Combined(value) => &value.updates,
        tl::enums::Updates::Updates(value) => &value.updates,
        _ => return None,
    };
    random_ids
        .iter()
        .map(|random_id| {
            update_list.iter().find_map(|update| match update {
                tl::enums::Update::MessageId(value) if value.random_id == *random_id => {
                    Some(value.id)
                },
                _ => None,
            })
        })
        .collect()
}

fn sent_message_id_from_update(update: &tl::enums::Update, random_id: i64) -> Option<i32> {
    match update {
        tl::enums::Update::MessageId(value) if value.random_id == random_id => Some(value.id),
        tl::enums::Update::NewMessage(value) => Some(message_id(&value.message)),
        tl::enums::Update::NewChannelMessage(value) => Some(message_id(&value.message)),
        tl::enums::Update::NewScheduledMessage(value) => Some(message_id(&value.message)),
        _ => None,
    }
}

fn scheduled_transition_receipt(
    updates: &tl::enums::Updates,
    expected_peer: &tl::enums::Peer,
    expected_message_ids: &[i32],
) -> Option<Option<Vec<i32>>> {
    let from_update = |update: &tl::enums::Update| match update {
        tl::enums::Update::DeleteScheduledMessages(value)
            if value.peer == *expected_peer
                && value.messages.len() == expected_message_ids.len()
                && value.messages.iter().all(|id| expected_message_ids.contains(id))
                && expected_message_ids.iter().all(|id| value.messages.contains(id)) =>
        {
            Some(value.sent_messages.clone())
        },
        _ => None,
    };
    match updates {
        tl::enums::Updates::UpdateShort(value) => from_update(&value.update),
        tl::enums::Updates::Combined(value) => value.updates.iter().find_map(from_update),
        tl::enums::Updates::Updates(value) => value.updates.iter().find_map(from_update),
        tl::enums::Updates::TooLong
        | tl::enums::Updates::UpdateShortMessage(_)
        | tl::enums::Updates::UpdateShortChatMessage(_)
        | tl::enums::Updates::UpdateShortSentMessage(_) => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn edit_text_input_turns_tel_anchors_into_phone_entities() {
        let html = "<b>Запись</b> по телефону: \
                    <a href=\"tel:+79193773549\">8 919 377-35-49</a>";
        let message = edit_text_input(html, CaptionParseMode::Html).expect("html edit input");
        assert_eq!(message.text, "Запись по телефону: 8 919 377-35-49");
        let phone_at = message.text[..message.text.find("8 919 377-35-49").expect("phone")]
            .encode_utf16()
            .count() as i32;
        let entities = message.entities.expect("entities");
        assert!(
            entities.iter().any(|entity| matches!(entity, tl::enums::MessageEntity::Bold(_)))
        );
        assert!(entities.iter().any(|entity| matches!(
            entity,
            tl::enums::MessageEntity::Phone(phone)
                if phone.offset == phone_at && phone.length == 15
        )));
        assert!(!entities.iter().any(|entity| matches!(
            entity,
            tl::enums::MessageEntity::TextUrl(url) if url.url.starts_with("tel:")
        )));
    }

    #[test]
    fn phone_entities_render_as_canonical_tel_anchors() {
        let text = "Звоните: 8 919 377-35-49";
        let offset = text[..text.find("8 919").expect("phone")].encode_utf16().count() as i32;
        let entities = vec![tl::enums::MessageEntity::Phone(tl::types::MessageEntityPhone {
            offset,
            length: 15,
        })];
        let html = html_with_phone_links(text, &entities);
        assert!(
            html.contains("<a href=\"tel:+79193773549\">8 919 377-35-49</a>"),
            "{html}"
        );
        assert_eq!(canonical_tel_url("tel:8 (919) 377-35-49").as_deref(), Some("tel:+79193773549"));
        assert_eq!(canonical_tel_url("https://example.com"), None);
    }

    #[test]
    fn rich_markdown_hash_tags_are_not_promoted_to_headings() {
        let source = "# Заголовок\n\n#нейродинамика #осанка\n### Подзаголовок";
        let normalized = normalize_rich_markdown(source);
        assert_eq!(normalized, "# Заголовок\n\n\\#нейродинамика #осанка\n### Подзаголовок");
        let blocks = ferogram::parsers::parse_rich_markdown(&normalized);
        assert!(matches!(blocks.first(), Some(tl::enums::PageBlock::Heading1(_))));
        assert!(matches!(blocks.get(1), Some(tl::enums::PageBlock::Paragraph(_))));
        assert!(matches!(blocks.get(2), Some(tl::enums::PageBlock::Heading3(_))));
    }

    fn raw_test_message(
        id: i32,
        peer_id: tl::enums::Peer,
        text: &str,
        edit_date: Option<i32>,
    ) -> tl::enums::Message {
        tl::enums::Message::Message(tl::types::Message {
            out: false,
            mentioned: false,
            media_unread: false,
            silent: false,
            post: false,
            from_scheduled: false,
            legacy: false,
            edit_hide: false,
            pinned: false,
            noforwards: false,
            invert_media: false,
            offline: false,
            video_processing_pending: false,
            paid_suggested_post_stars: false,
            paid_suggested_post_ton: false,
            id,
            from_id: Some(tl::enums::Peer::User(tl::types::PeerUser { user_id: 7 })),
            from_boosts_applied: None,
            from_rank: None,
            peer_id,
            saved_peer_id: None,
            fwd_from: None,
            via_bot_id: None,
            via_business_bot_id: None,
            guestchat_via_from: None,
            reply_to: None,
            date: 1_700_004_000,
            message: text.to_string(),
            media: None,
            reply_markup: None,
            entities: None,
            views: None,
            forwards: None,
            replies: None,
            edit_date,
            post_author: None,
            grouped_id: None,
            reactions: None,
            restriction_reason: None,
            ttl_period: None,
            quick_reply_shortcut_id: None,
            effect: None,
            factcheck: None,
            report_delivery_until_date: None,
            paid_message_stars: None,
            suggested_post: None,
            schedule_repeat_period: None,
            summary_from_language: None,
            rich_message: None,
        })
    }

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
    fn typing_actions_map_to_exact_telegram_constructors() {
        assert!(matches!(
            typing_action_to_tl(TypingAction::Typing),
            tl::enums::SendMessageAction::SendMessageTypingAction
        ));
        assert!(matches!(
            typing_action_to_tl(TypingAction::UploadPhoto),
            tl::enums::SendMessageAction::SendMessageUploadPhotoAction(action)
                if action.progress == 0
        ));
        assert!(matches!(
            typing_action_to_tl(TypingAction::UploadDocument),
            tl::enums::SendMessageAction::SendMessageUploadDocumentAction(action)
                if action.progress == 0
        ));
        assert!(matches!(
            typing_action_to_tl(TypingAction::Cancel),
            tl::enums::SendMessageAction::SendMessageCancelAction
        ));
    }

    #[test]
    fn update_cursors_use_exact_current_tl_contracts_and_typed_difference_mapping() {
        use tl::Identifiable;

        assert_eq!(tl::functions::updates::GetState::CONSTRUCTOR_ID, 0xedd4882a);
        assert_eq!(tl::functions::updates::GetDifference::CONSTRUCTOR_ID, 0x19c2f763);
        assert_eq!(tl::functions::updates::GetChannelDifference::CONSTRUCTOR_ID, 0x03173d78);
        assert_eq!(tl::types::updates::State::CONSTRUCTOR_ID, 0xa56c2a3e);
        assert_eq!(tl::types::updates::DifferenceEmpty::CONSTRUCTOR_ID, 0x5d75a138);
        assert_eq!(tl::types::updates::Difference::CONSTRUCTOR_ID, 0x00f49ca0);
        assert_eq!(tl::types::updates::DifferenceSlice::CONSTRUCTOR_ID, 0xa8fb1981);
        assert_eq!(tl::types::updates::DifferenceTooLong::CONSTRUCTOR_ID, 0x4afe8f6d);
        assert_eq!(tl::types::updates::ChannelDifferenceEmpty::CONSTRUCTOR_ID, 0x3e11affb);
        assert_eq!(tl::types::updates::ChannelDifference::CONSTRUCTOR_ID, 0x2064674e);
        assert_eq!(tl::types::updates::ChannelDifferenceTooLong::CONSTRUCTOR_ID, 0xa4bcc6fe);
        assert_eq!(
            tl::enums::ChannelMessagesFilter::Empty,
            tl::enums::ChannelMessagesFilter::Empty
        );

        let state = tl::enums::updates::State::State(tl::types::updates::State {
            pts: 120,
            qts: 7,
            date: 1_700_004_000,
            seq: 10,
            unread_count: 2,
        });
        assert_eq!(
            updates_state_summary(state.clone()),
            UpdatesStateSummary { pts: 120, qts: 7, date: 1_700_004_000, seq: 10, unread_count: 2 }
        );

        let request = UpdatesDifferenceRequestSummary {
            pts: 119,
            qts: 7,
            date: 1_700_003_999,
            pts_limit: Some(100),
            pts_total_limit: Some(1_000),
            qts_limit: Some(50),
        };
        let exact_request = tl::functions::updates::GetDifference {
            pts: request.pts,
            pts_limit: request.pts_limit,
            pts_total_limit: request.pts_total_limit,
            date: request.date,
            qts: request.qts,
            qts_limit: request.qts_limit,
        };
        assert_eq!(exact_request.pts_limit, Some(100));
        assert_eq!(exact_request.pts_total_limit, Some(1_000));
        assert_eq!(exact_request.qts_limit, Some(50));

        let mapped = updates_difference_summary(
            request.clone(),
            tl::enums::updates::Difference::Difference(tl::types::updates::Difference {
                new_messages: Vec::new(),
                new_encrypted_messages: Vec::new(),
                other_updates: vec![
                    tl::enums::Update::EditMessage(tl::types::UpdateEditMessage {
                        message: raw_test_message(
                            43,
                            tl::enums::Peer::User(tl::types::PeerUser { user_id: 101 }),
                            "edited direct",
                            Some(1_700_004_001),
                        ),
                        pts: 120,
                        pts_count: 1,
                    }),
                    tl::enums::Update::EditChannelMessage(tl::types::UpdateEditChannelMessage {
                        message: raw_test_message(
                            50,
                            tl::enums::Peer::Channel(tl::types::PeerChannel { channel_id: 505 }),
                            "edited channel",
                            Some(1_700_004_002),
                        ),
                        pts: 50,
                        pts_count: 1,
                    }),
                    tl::enums::Update::DeleteMessages(tl::types::UpdateDeleteMessages {
                        messages: vec![41, 42],
                        pts: 120,
                        pts_count: 2,
                    }),
                    tl::enums::Update::DeleteChannelMessages(
                        tl::types::UpdateDeleteChannelMessages {
                            channel_id: 505,
                            messages: vec![49],
                            pts: 50,
                            pts_count: 1,
                        },
                    ),
                    tl::enums::Update::PtsChanged,
                ],
                chats: Vec::new(),
                users: Vec::new(),
                state: state.clone(),
            }),
        )
        .expect("typed global difference");
        assert_eq!(mapped.kind, UpdatesDifferenceKind::Complete);
        assert!(mapped.is_final);
        assert_eq!(mapped.next_cursor.as_ref().and_then(|cursor| cursor.unread_count), Some(2));
        assert_eq!(mapped.edited_messages.len(), 2);
        assert_eq!(mapped.edited_messages[0].message_id, 43);
        assert_eq!(mapped.edited_messages[0].text.as_deref(), Some("edited direct"));
        assert_eq!(mapped.edited_messages[1].chat_ref.as_deref(), Some("-100505"));
        assert_eq!(mapped.edited_messages[1].edit_date, Some(1_700_004_002));
        assert_eq!(
            mapped.deleted_messages,
            vec![
                DeletedMessagesSummary::Global {
                    message_ids: vec![41, 42],
                    pts: 120,
                    pts_count: 2,
                },
                DeletedMessagesSummary::Channel {
                    chat_id: "-100505".to_string(),
                    message_ids: vec![49],
                    pts: 50,
                    pts_count: 1,
                },
            ]
        );
        assert_eq!(mapped.other_updates, vec![UpdateKindSummary { kind: "pts_changed".into() }]);

        let empty = updates_difference_summary(
            request.clone(),
            tl::enums::updates::Difference::Empty(tl::types::updates::DifferenceEmpty {
                date: 1_700_004_001,
                seq: 11,
            }),
        )
        .expect("typed empty difference");
        assert_eq!(empty.kind, UpdatesDifferenceKind::Empty);
        assert_eq!(empty.next_cursor.as_ref().map(|cursor| cursor.pts), Some(119));
        assert_eq!(empty.next_cursor.as_ref().and_then(|cursor| cursor.unread_count), None);

        let too_long = updates_difference_summary(
            request,
            tl::enums::updates::Difference::TooLong(tl::types::updates::DifferenceTooLong {
                pts: 120,
            }),
        )
        .expect("typed too-long difference");
        assert_eq!(too_long.kind, UpdatesDifferenceKind::TooLong);
        assert!(too_long.requires_resync);
        assert_eq!(too_long.resync_pts, Some(120));

        let channel_request = ChannelUpdatesRequestSummary {
            chat_id: "-100505".to_string(),
            pts: 49,
            limit: 100,
            force: true,
        };
        let exact_channel_request = tl::functions::updates::GetChannelDifference {
            force: channel_request.force,
            channel: tl::enums::InputChannel::InputChannel(tl::types::InputChannel {
                channel_id: 505,
                access_hash: 5_050,
            }),
            filter: tl::enums::ChannelMessagesFilter::Empty,
            pts: channel_request.pts,
            limit: channel_request.limit,
        };
        assert!(exact_channel_request.force);
        assert_eq!(exact_channel_request.limit, 100);
        assert!(matches!(exact_channel_request.filter, tl::enums::ChannelMessagesFilter::Empty));

        let channel = channel_updates_difference_summary(
            channel_request,
            tl::enums::updates::ChannelDifference::ChannelDifference(
                tl::types::updates::ChannelDifference {
                    r#final: false,
                    pts: 50,
                    timeout: Some(5),
                    new_messages: Vec::new(),
                    other_updates: vec![tl::enums::Update::DeleteChannelMessages(
                        tl::types::UpdateDeleteChannelMessages {
                            channel_id: 505,
                            messages: vec![49],
                            pts: 50,
                            pts_count: 1,
                        },
                    )],
                    chats: Vec::new(),
                    users: Vec::new(),
                },
            ),
        )
        .expect("typed channel difference");
        assert_eq!(channel.kind, ChannelUpdatesDifferenceKind::Difference);
        assert!(!channel.is_final);
        assert!(channel.continuation_required);
        assert_eq!(channel.next_pts, Some(50));
        assert_eq!(channel.timeout_seconds, Some(5));
        assert_eq!(channel.deleted_messages.len(), 1);
    }

    #[test]
    fn chat_moderation_uses_exact_current_tl_contracts_and_reaction_policy_mapping() {
        use tl::Identifiable;

        assert_eq!(
            tl::functions::messages::EditChatDefaultBannedRights::CONSTRUCTOR_ID,
            0xa5866b41
        );
        assert_eq!(tl::functions::messages::SetChatAvailableReactions::CONSTRUCTOR_ID, 0x864b2581);
        assert_eq!(tl::functions::channels::ToggleForum::CONSTRUCTOR_ID, 0x3ff75734);
        assert_eq!(tl::functions::channels::ToggleAntiSpam::CONSTRUCTOR_ID, 0x68f3e4eb);
        assert_eq!(tl::types::ChatReactionsNone::CONSTRUCTOR_ID, 0xeafc32bc);
        assert_eq!(tl::types::ChatReactionsAll::CONSTRUCTOR_ID, 0x52928bca);
        assert_eq!(tl::types::ChatReactionsSome::CONSTRUCTOR_ID, 0x661d4037);

        let all = ChatReactionsPolicy::All { allow_custom: true };
        let exact_all = chat_reactions_policy_to_tl(&all).expect("all reactions policy");
        assert!(matches!(
            exact_all,
            tl::enums::ChatReactions::All(ref policy) if policy.allow_custom
        ));
        assert_eq!(chat_reactions_policy_from_tl(exact_all), all);

        let some = ChatReactionsPolicy::Some {
            reactions: vec![
                ReactionIdentity::Emoji { emoji: "👍".to_string() },
                ReactionIdentity::CustomEmoji { document_id: 7_001 },
            ],
        };
        let exact_some = chat_reactions_policy_to_tl(&some).expect("selected reactions policy");
        assert!(matches!(
            exact_some,
            tl::enums::ChatReactions::Some(ref policy)
                if matches!(
                    policy.reactions.as_slice(),
                    [
                        tl::enums::Reaction::Emoji(emoji),
                        tl::enums::Reaction::CustomEmoji(custom),
                    ] if emoji.emoticon == "👍" && custom.document_id == 7_001
                )
        ));
        assert_eq!(chat_reactions_policy_from_tl(exact_some), some);
        assert!(matches!(
            chat_reactions_policy_from_tl(tl::enums::ChatReactions::None),
            ChatReactionsPolicy::None
        ));

        for invalid in [
            ChatReactionsPolicy::Some { reactions: vec![ReactionIdentity::Empty] },
            ChatReactionsPolicy::Some { reactions: vec![ReactionIdentity::Paid] },
            ChatReactionsPolicy::Some {
                reactions: vec![ReactionIdentity::CustomEmoji { document_id: 0 }],
            },
        ] {
            assert!(matches!(
                chat_reactions_policy_to_tl(&invalid),
                Err(TelegramError::InvalidPeer(_))
            ));
        }
    }

    #[test]
    fn recent_stickers_and_admin_invites_use_exact_current_tl_contracts() {
        use tl::Identifiable;

        assert_eq!(tl::functions::messages::GetRecentStickers::CONSTRUCTOR_ID, 0x9da9403b);
        assert_eq!(tl::functions::messages::SaveRecentSticker::CONSTRUCTOR_ID, 0x392718f8);
        assert_eq!(tl::functions::messages::GetExportedChatInvites::CONSTRUCTOR_ID, 0xa2b5a3f6);
        assert_eq!(tl::types::messages::ExportedChatInvites::CONSTRUCTOR_ID, 0xbdc62dcc);

        let document = tl::enums::InputDocument::InputDocument(tl::types::InputDocument {
            id: 9_101,
            access_hash: 91_010,
            file_reference: vec![1, 2, 3],
        });
        let recent = tl::functions::messages::SaveRecentSticker {
            attached: true,
            id: document.clone(),
            unsave: true,
        };
        assert!(recent.attached);
        assert!(recent.unsave);
        assert_eq!(recent.id, document);

        let invites = tl::functions::messages::GetExportedChatInvites {
            revoked: true,
            peer: tl::enums::InputPeer::Channel(tl::types::InputPeerChannel {
                channel_id: 505,
                access_hash: 5_050,
            }),
            admin_id: tl::enums::InputUser::InputUser(tl::types::InputUser {
                user_id: 303,
                access_hash: 3_030,
            }),
            offset_date: Some(1_700_000_000),
            offset_link: Some("https://t.me/+opaque-provider-value".to_string()),
            limit: 100,
        };
        assert!(invites.revoked);
        assert_eq!(invites.limit, 100);
        assert_eq!(invites.offset_date, Some(1_700_000_000));
        assert!(matches!(
            invites.admin_id,
            tl::enums::InputUser::InputUser(admin)
                if admin.user_id == 303 && admin.access_hash == 3_030
        ));
    }

    #[test]
    fn callback_buttons_preserve_exact_tl_fields_and_use_get_bot_callback_answer() {
        let data = b"approve:contract:11".to_vec();
        let summary = message_button_summary(
            &tl::enums::KeyboardButton::Callback(tl::types::KeyboardButtonCallback {
                requires_password: false,
                style: Some(tl::enums::KeyboardButtonStyle::KeyboardButtonStyle(
                    tl::types::KeyboardButtonStyle {
                        bg_primary: true,
                        bg_danger: false,
                        bg_success: false,
                        icon: Some(9_001),
                    },
                )),
                text: "Approve".to_string(),
                data: data.clone(),
            }),
            2,
            3,
            7,
        );
        assert_eq!(summary.kind, MessageButtonKind::Callback);
        assert_eq!((summary.row, summary.column, summary.index), (2, 3, 7));
        assert_eq!(summary.label, "Approve");
        assert_eq!(summary.callback_data_base64.as_deref(), Some("YXBwcm92ZTpjb250cmFjdDoxMQ=="));
        assert_eq!(summary.callback_data_length, Some(data.len()));
        assert_eq!(
            summary.callback_data_sha256.as_deref(),
            Some(format!("{:x}", Sha256::digest(&data)).as_str())
        );
        let style = summary.style.expect("button style");
        assert!(style.background_primary);
        assert_eq!(style.icon_custom_emoji_id.as_deref(), Some("9001"));

        let switch = message_button_summary(
            &tl::enums::KeyboardButton::SwitchInline(tl::types::KeyboardButtonSwitchInline {
                same_peer: true,
                style: None,
                text: "Share".to_string(),
                query: "contract 11".to_string(),
                peer_types: Some(vec![
                    tl::enums::InlineQueryPeerType::Pm,
                    tl::enums::InlineQueryPeerType::Megagroup,
                ]),
            }),
            0,
            1,
            1,
        );
        assert_eq!(switch.kind, MessageButtonKind::SwitchInline);
        assert_eq!(switch.switch_query.as_deref(), Some("contract 11"));
        assert_eq!(switch.same_peer, Some(true));
        assert_eq!(
            switch.allowed_peer_types,
            Some(vec!["pm".to_string(), "megagroup".to_string()])
        );

        let request = callback_answer_request(
            tl::enums::InputPeer::User(tl::types::InputPeerUser { user_id: 101, access_hash: 202 }),
            11,
            data.clone(),
        );
        assert!(!request.game);
        assert_eq!(request.msg_id, 11);
        assert_eq!(request.data, Some(data));
        assert!(request.password.is_none());
        assert!(matches!(
            request.peer,
            tl::enums::InputPeer::User(peer)
                if peer.user_id == 101 && peer.access_hash == 202
        ));
    }

    #[test]
    fn inline_bot_requests_use_exact_tl_constructors_and_preserve_every_send_flag() {
        use tl::Identifiable;

        let bot = tl::enums::InputUser::InputUser(tl::types::InputUser {
            user_id: 909,
            access_hash: 9_090,
        });
        let peer = tl::enums::InputPeer::User(tl::types::InputPeerUser {
            user_id: 101,
            access_hash: 1_010,
        });
        let query_request = inline_bot_results_request(
            bot.clone(),
            peer.clone(),
            String::new(),
            "next:1".to_string(),
        );
        assert_eq!(tl::functions::messages::GetInlineBotResults::CONSTRUCTOR_ID, 0x514e999d);
        assert_eq!(query_request.bot, bot);
        assert_eq!(query_request.peer, peer);
        assert!(query_request.geo_point.is_none());
        assert_eq!(query_request.query, "");
        assert_eq!(query_request.offset, "next:1");

        let send_request = inline_bot_result_send_request(
            peer.clone(),
            9_090_000_000_000_001,
            "article:0".to_string(),
            Some(11),
            true,
            true,
            true,
            7_777,
        );
        assert_eq!(tl::functions::messages::SendInlineBotResult::CONSTRUCTOR_ID, 0xc0cf7646);
        assert!(send_request.silent);
        assert!(!send_request.background);
        assert!(send_request.clear_draft);
        assert!(send_request.hide_via);
        assert_eq!(send_request.peer, peer);
        assert_eq!(send_request.random_id, 7_777);
        assert_eq!(send_request.query_id, 9_090_000_000_000_001);
        assert_eq!(send_request.id, "article:0");
        assert!(send_request.schedule_date.is_none());
        assert!(send_request.send_as.is_none());
        assert!(send_request.quick_reply_shortcut.is_none());
        assert!(send_request.allow_paid_stars.is_none());
        assert!(matches!(
            send_request.reply_to,
            Some(tl::enums::InputReplyTo::Message(reply))
                if reply.reply_to_msg_id == 11
                    && reply.top_msg_id.is_none()
                    && reply.reply_to_peer_id.is_none()
                    && reply.quote_text.is_none()
                    && reply.quote_entities.is_none()
                    && reply.quote_offset.is_none()
                    && reply.monoforum_peer_id.is_none()
                    && reply.todo_item_id.is_none()
                    && reply.poll_option.is_none()
        ));
    }

    #[test]
    fn inline_geo_preview_preserves_coordinates_and_live_location_controls() {
        let message = tl::enums::BotInlineMessage::MediaGeo(tl::types::BotInlineMessageMediaGeo {
            geo: tl::enums::GeoPoint::GeoPoint(tl::types::GeoPoint {
                long: 37.6173,
                lat: 55.7558,
                access_hash: 42,
                accuracy_radius: Some(12),
            }),
            heading: Some(90),
            period: Some(600),
            proximity_notification_radius: Some(50),
            reply_markup: None,
        });
        let (kind, text, url, geo, has_reply_markup) =
            inline_bot_send_message_summary(&message).expect("valid geo preview");
        assert_eq!(kind, "geo");
        assert!(text.is_none());
        assert!(url.is_none());
        assert!(!has_reply_markup);
        let geo = geo.expect("geo details");
        assert_eq!(geo.point.as_ref().map(|point| point.latitude_e7), Some(557_558_000));
        assert_eq!(geo.point.as_ref().map(|point| point.longitude_e7), Some(376_173_000));
        assert_eq!(geo.point.as_ref().and_then(|point| point.accuracy_radius_m), Some(12));
        assert_eq!(geo.heading_degrees, Some(90));
        assert_eq!(geo.live_period_seconds, Some(600));
        assert_eq!(geo.proximity_notification_radius_m, Some(50));
    }

    #[test]
    fn bot_command_requests_use_exact_default_scope_tl_contracts() {
        use tl::Identifiable;

        let commands = vec![BotCommandSummary {
            command: "status".to_string(),
            description: "Show release status".to_string(),
            ephemeral: true,
        }];
        let get = bot_commands_get_request("en");
        assert_eq!(tl::functions::bots::GetBotCommands::CONSTRUCTOR_ID, 0xe34c0dd6);
        assert!(matches!(get.scope, tl::enums::BotCommandScope::Default));
        assert_eq!(get.lang_code, "en");

        let set = bot_commands_set_request("en", &commands);
        assert_eq!(tl::functions::bots::SetBotCommands::CONSTRUCTOR_ID, 0x0517165a);
        assert!(matches!(set.scope, tl::enums::BotCommandScope::Default));
        assert_eq!(set.lang_code, "en");
        assert!(matches!(
            set.commands.as_slice(),
            [tl::enums::BotCommand::BotCommand(command)]
                if command.command == "status"
                    && command.description == "Show release status"
                    && command.ephemeral
        ));

        let reset = bot_commands_reset_request("");
        assert_eq!(tl::functions::bots::ResetBotCommands::CONSTRUCTOR_ID, 0x3d8de0f9);
        assert!(matches!(reset.scope, tl::enums::BotCommandScope::Default));
        assert_eq!(reset.lang_code, "");
    }

    #[test]
    fn emoji_status_requests_and_summaries_preserve_exact_tl_contracts() {
        use tl::Identifiable;

        assert_eq!(tl::functions::account::GetDefaultEmojiStatuses::CONSTRUCTOR_ID, 0xd6753386);
        assert_eq!(tl::functions::account::GetRecentEmojiStatuses::CONSTRUCTOR_ID, 0x0f578105);
        assert_eq!(tl::functions::account::ClearRecentEmojiStatuses::CONSTRUCTOR_ID, 0x18201aae);
        assert_eq!(
            tl::functions::account::GetChannelDefaultEmojiStatuses::CONSTRUCTOR_ID,
            0x7727a7d5
        );
        assert_eq!(tl::functions::account::GetCollectibleEmojiStatuses::CONSTRUCTOR_ID, 0x2e7b4543);
        assert_eq!(tl::functions::account::UpdateEmojiStatus::CONSTRUCTOR_ID, 0xfbd3de6b);

        let default = emoji_status_input(&AccountEmojiStatusSpec::Default {
            document_id: "7001".to_string(),
            until_unix: Some(2_000_000_000),
        })
        .expect("default input");
        assert!(matches!(
            default,
            tl::enums::EmojiStatus::EmojiStatus(status)
                if status.document_id == 7_001 && status.until == Some(2_000_000_000)
        ));

        let collectible = emoji_status_input(&AccountEmojiStatusSpec::Collectible {
            collectible_id: "8001".to_string(),
            until_unix: None,
        })
        .expect("collectible input");
        assert!(matches!(
            collectible,
            tl::enums::EmojiStatus::InputEmojiStatusCollectible(status)
                if status.collectible_id == 8_001 && status.until.is_none()
        ));

        let summary = emoji_status_summary(tl::enums::EmojiStatus::Collectible(
            tl::types::EmojiStatusCollectible {
                collectible_id: 8_001,
                document_id: 7_101,
                title: "Aurora".to_string(),
                slug: "aurora".to_string(),
                pattern_document_id: 17_101,
                center_color: 0x123456,
                edge_color: 0x234567,
                pattern_color: 0x345678,
                text_color: 0x456789,
                until: Some(2_000_000_001),
            },
        ));
        assert_eq!(summary.kind, EmojiStatusKind::Collectible);
        assert_eq!(summary.collectible_id.as_deref(), Some("8001"));
        assert_eq!(summary.document_id.as_deref(), Some("7101"));
        assert_eq!(summary.pattern_document_id.as_deref(), Some("17101"));
        assert_eq!(summary.title.as_deref(), Some("Aurora"));
        assert_eq!(summary.slug.as_deref(), Some("aurora"));
        assert_eq!(summary.center_color, Some(0x123456));
        assert_eq!(summary.edge_color, Some(0x234567));
        assert_eq!(summary.pattern_color, Some(0x345678));
        assert_eq!(summary.text_color, Some(0x456789));
        assert_eq!(summary.until_unix, Some(2_000_000_001));
    }

    #[test]
    fn account_color_and_personal_channel_use_exact_tl_contracts() {
        use tl::Identifiable;

        assert_eq!(tl::functions::account::UpdateColor::CONSTRUCTOR_ID, 0x684d214e);
        assert_eq!(tl::functions::account::UpdatePersonalChannel::CONSTRUCTOR_ID, 0xd94305e0);
        let color = account_color_update_request(true, Some(12), Some("9001"))
            .expect("account color request");
        assert!(color.for_profile);
        assert!(matches!(
            color.color,
            Some(tl::enums::PeerColor::PeerColor(color))
                if color.color == Some(12) && color.background_emoji_id == Some(9_001)
        ));
        let reset = account_color_update_request(false, None, None).expect("color reset");
        assert!(!reset.for_profile);
        assert!(reset.color.is_none());

        let channel = input_channel_for_personal_channel(tl::enums::InputPeer::Channel(
            tl::types::InputPeerChannel { channel_id: 505, access_hash: 5_050 },
        ))
        .expect("direct input channel");
        assert!(matches!(
            channel,
            tl::enums::InputChannel::InputChannel(channel)
                if channel.channel_id == 505 && channel.access_hash == 5_050
        ));
        let from_message =
            input_channel_for_personal_channel(tl::enums::InputPeer::ChannelFromMessage(Box::new(
                tl::types::InputPeerChannelFromMessage {
                    peer: tl::enums::InputPeer::User(tl::types::InputPeerUser {
                        user_id: 101,
                        access_hash: 1_010,
                    }),
                    msg_id: 11,
                    channel_id: 505,
                },
            )))
            .expect("message-scoped input channel");
        assert!(matches!(
            from_message,
            tl::enums::InputChannel::FromMessage(channel)
                if channel.msg_id == 11 && channel.channel_id == 505
        ));
        assert!(matches!(
            input_channel_for_personal_channel(tl::enums::InputPeer::Chat(
                tl::types::InputPeerChat { chat_id: 202 }
            )),
            Err(TelegramError::InvalidPeer(_))
        ));

        let collectible = peer_color_summary(tl::enums::PeerColor::Collectible(
            tl::types::PeerColorCollectible {
                collectible_id: 9_100,
                gift_emoji_id: 9_101,
                background_emoji_id: 9_102,
                accent_color: 0x112233,
                colors: vec![0x112233, 0x445566],
                dark_accent_color: Some(0x010203),
                dark_colors: Some(vec![0x010203, 0x040506]),
            },
        ));
        assert!(matches!(
            collectible,
            PeerColorSummary::Collectible {
                collectible_id,
                gift_emoji_id,
                background_emoji_id,
                accent_color: 0x112233,
                colors,
                dark_accent_color: Some(0x010203),
                dark_colors: Some(dark_colors),
            } if collectible_id == "9100"
                && gift_emoji_id == "9101"
                && background_emoji_id == "9102"
                && colors == vec![0x112233, 0x445566]
                && dark_colors == vec![0x010203, 0x040506]
        ));
    }

    #[test]
    fn quick_replies_use_exact_tl_contracts_and_preserve_message_kinds() {
        use tl::Identifiable;

        assert_eq!(tl::functions::messages::GetQuickReplies::CONSTRUCTOR_ID, 0xd483f2a8);
        assert_eq!(tl::functions::messages::GetQuickReplyMessages::CONSTRUCTOR_ID, 0x94a495c3);
        let get_catalog = tl::functions::messages::GetQuickReplies { hash: 12_001 };
        assert_eq!(get_catalog.hash, 12_001);
        let get_messages = tl::functions::messages::GetQuickReplyMessages {
            shortcut_id: 7,
            id: Some(vec![900, 901]),
            hash: 12_002,
        };
        assert_eq!(get_messages.shortcut_id, 7);
        assert_eq!(get_messages.id.as_deref(), Some([900, 901].as_slice()));
        assert_eq!(get_messages.hash, 12_002);

        let regular = tl::enums::Message::Message(tl::types::Message {
            out: true,
            mentioned: false,
            media_unread: false,
            silent: false,
            post: false,
            from_scheduled: false,
            legacy: false,
            edit_hide: false,
            pinned: false,
            noforwards: false,
            invert_media: false,
            offline: false,
            video_processing_pending: false,
            paid_suggested_post_stars: false,
            paid_suggested_post_ton: false,
            id: 901,
            from_id: Some(tl::enums::Peer::User(tl::types::PeerUser { user_id: 7 })),
            from_boosts_applied: None,
            from_rank: None,
            peer_id: tl::enums::Peer::User(tl::types::PeerUser { user_id: 7 }),
            saved_peer_id: None,
            fwd_from: None,
            via_bot_id: None,
            via_business_bot_id: None,
            guestchat_via_from: None,
            reply_to: None,
            date: 1_700_002_901,
            message: "Hello — how can I help?".to_string(),
            media: None,
            reply_markup: None,
            entities: None,
            views: None,
            forwards: None,
            replies: None,
            edit_date: Some(1_700_002_902),
            post_author: None,
            grouped_id: None,
            reactions: None,
            restriction_reason: None,
            ttl_period: None,
            quick_reply_shortcut_id: Some(7),
            effect: None,
            factcheck: None,
            report_delivery_until_date: None,
            paid_message_stars: None,
            suggested_post: None,
            schedule_repeat_period: None,
            summary_from_language: None,
            rich_message: None,
        });
        let service = tl::enums::Message::Service(tl::types::MessageService {
            out: true,
            mentioned: false,
            media_unread: false,
            reactions_are_possible: false,
            silent: true,
            post: false,
            legacy: false,
            id: 900,
            from_id: Some(tl::enums::Peer::User(tl::types::PeerUser { user_id: 7 })),
            peer_id: tl::enums::Peer::User(tl::types::PeerUser { user_id: 7 }),
            saved_peer_id: None,
            reply_to: None,
            date: 1_700_002_900,
            action: tl::enums::MessageAction::HistoryClear,
            reactions: None,
            ttl_period: None,
        });
        let catalog = quick_reply_catalog_page(tl::enums::messages::QuickReplies::QuickReplies(
            tl::types::messages::QuickReplies {
                quick_replies: vec![tl::enums::QuickReply::QuickReply(tl::types::QuickReply {
                    shortcut_id: 7,
                    shortcut: "greeting".to_string(),
                    top_message: 901,
                    count: 2,
                })],
                messages: vec![regular.clone()],
                chats: Vec::new(),
                users: Vec::new(),
            },
        ));
        assert!(!catalog.not_modified);
        assert!(matches!(
            catalog.shortcuts.as_slice(),
            [shortcut]
                if shortcut.shortcut_id == 7
                    && shortcut.shortcut == "greeting"
                    && shortcut.top_message_id == 901
                    && shortcut.count == 2
        ));
        assert!(matches!(
            catalog.top_messages.as_slice(),
            [message]
                if !message.is_service
                    && message.quick_reply_shortcut_id == Some(7)
                    && message.message.text.as_deref() == Some("Hello — how can I help?")
                    && message.message.edit_date == Some(1_700_002_902)
        ));

        let messages = quick_reply_messages_page(
            7,
            tl::enums::messages::Messages::Messages(tl::types::messages::Messages {
                messages: vec![regular, service],
                topics: Vec::new(),
                chats: Vec::new(),
                users: Vec::new(),
            }),
        );
        assert_eq!(messages.total_count, Some(2));
        assert_eq!(messages.messages.len(), 2);
        assert_eq!(messages.messages[0].quick_reply_shortcut_id, Some(7));
        assert!(messages.messages[1].is_service);
        assert_eq!(messages.messages[1].service_action_kind.as_deref(), Some("history_clear"));

        let not_modified = quick_reply_catalog_page(tl::enums::messages::QuickReplies::NotModified);
        assert!(not_modified.not_modified);
        assert!(not_modified.shortcuts.is_empty());
    }

    #[test]
    fn group_calls_use_exact_tl_contracts_and_preserve_participant_media() {
        use tl::Identifiable;

        assert_eq!(tl::types::GroupCallDiscarded::CONSTRUCTOR_ID, 0x7780bcb4);
        assert_eq!(tl::types::GroupCall::CONSTRUCTOR_ID, 0xefb2b617);
        assert_eq!(tl::types::InputGroupCall::CONSTRUCTOR_ID, 0xd8aa840f);
        assert_eq!(tl::types::GroupCallParticipant::CONSTRUCTOR_ID, 0x2a3dc7ac);
        assert_eq!(tl::types::GroupCallParticipantVideo::CONSTRUCTOR_ID, 0x67753ac8);
        assert_eq!(tl::types::GroupCallParticipantVideoSourceGroup::CONSTRUCTOR_ID, 0xdcb118b7);
        assert_eq!(tl::types::phone::GroupCall::CONSTRUCTOR_ID, 0x9e727aad);
        assert_eq!(tl::types::phone::GroupParticipants::CONSTRUCTOR_ID, 0xf47751b6);
        assert_eq!(tl::functions::phone::GetGroupCall::CONSTRUCTOR_ID, 0x041845db);
        assert_eq!(tl::functions::phone::GetGroupParticipants::CONSTRUCTOR_ID, 0xc558d8ab);

        let get = tl::functions::phone::GetGroupCall {
            call: tl::enums::InputGroupCall::InputGroupCall(tl::types::InputGroupCall {
                id: 1_001,
                access_hash: 90_001,
            }),
            limit: 25,
        };
        assert_eq!(get.limit, 25);
        assert!(matches!(
            get.call,
            tl::enums::InputGroupCall::InputGroupCall(call)
                if call.id == 1_001 && call.access_hash == 90_001
        ));

        let get_participants = tl::functions::phone::GetGroupParticipants {
            call: tl::enums::InputGroupCall::InputGroupCall(tl::types::InputGroupCall {
                id: 1_001,
                access_hash: 90_001,
            }),
            ids: vec![tl::enums::InputPeer::User(tl::types::InputPeerUser {
                user_id: 101,
                access_hash: 1_010,
            })],
            sources: vec![111],
            offset: "next".to_string(),
            limit: 100,
        };
        assert_eq!(get_participants.sources, vec![111]);
        assert_eq!(get_participants.offset, "next");
        assert_eq!(get_participants.limit, 100);
        assert!(matches!(
            get_participants.ids.as_slice(),
            [tl::enums::InputPeer::User(peer)]
                if peer.user_id == 101 && peer.access_hash == 1_010
        ));

        let participant = tl::enums::GroupCallParticipant::GroupCallParticipant(
            tl::types::GroupCallParticipant {
                muted: true,
                left: false,
                can_self_unmute: true,
                just_joined: false,
                versioned: true,
                min: false,
                muted_by_you: false,
                volume_by_admin: true,
                is_self: false,
                video_joined: true,
                peer: tl::enums::Peer::User(tl::types::PeerUser { user_id: 101 }),
                date: 1_700_003_001,
                active_date: Some(1_700_003_002),
                source: 111,
                volume: Some(10_000),
                about: Some("Release owner".to_string()),
                raise_hand_rating: Some(42),
                video: Some(tl::enums::GroupCallParticipantVideo::GroupCallParticipantVideo(
                    tl::types::GroupCallParticipantVideo {
                        paused: false,
                        endpoint: "video:101".to_string(),
                        source_groups: vec![
                            tl::enums::GroupCallParticipantVideoSourceGroup::GroupCallParticipantVideoSourceGroup(
                                tl::types::GroupCallParticipantVideoSourceGroup {
                                    semantics: "SIM".to_string(),
                                    sources: vec![111, 112],
                                },
                            ),
                        ],
                        audio_source: Some(113),
                    },
                )),
                presentation: None,
                paid_stars_total: Some(5),
            },
        );
        let page = group_call_page(
            "-100505".to_string(),
            tl::enums::phone::GroupCall::GroupCall(tl::types::phone::GroupCall {
                call: tl::enums::GroupCall::GroupCall(tl::types::GroupCall {
                    join_muted: true,
                    can_change_join_muted: true,
                    join_date_asc: false,
                    schedule_start_subscribed: false,
                    can_start_video: true,
                    record_video_active: true,
                    rtmp_stream: false,
                    listeners_hidden: false,
                    conference: false,
                    creator: true,
                    messages_enabled: true,
                    can_change_messages_enabled: true,
                    min: false,
                    id: 1_001,
                    access_hash: 90_001,
                    participants_count: 3,
                    title: Some("Release stand-up".to_string()),
                    stream_dc_id: Some(2),
                    record_start_date: Some(1_700_003_000),
                    schedule_date: None,
                    unmuted_video_count: Some(1),
                    unmuted_video_limit: 30,
                    version: 7,
                    invite_link: Some("credential-like-link".to_string()),
                    send_paid_messages_stars: Some(25),
                    default_send_as: Some(tl::enums::Peer::Channel(tl::types::PeerChannel {
                        channel_id: 505,
                    })),
                }),
                participants: vec![participant],
                participants_next_offset: "next".to_string(),
                chats: Vec::new(),
                users: Vec::new(),
            }),
        );
        assert_eq!(page.chat_id, "-100505");
        assert_eq!(page.participants_next_offset.as_deref(), Some("next"));
        assert!(matches!(
            page.call,
            GroupCallInfoSummary::Active {
                ref call_id,
                participants_count: 3,
                record_video_active: true,
                has_invite_link: true,
                ref send_paid_messages_stars,
                ref default_send_as,
                ..
            } if call_id == "1001"
                && send_paid_messages_stars.as_deref() == Some("25")
                && default_send_as.as_deref() == Some("-100505")
        ));
        assert!(matches!(
            page.participants.as_slice(),
            [participant]
                if participant.peer_ref == "101"
                    && participant.muted
                    && participant.can_self_unmute
                    && participant.volume_by_admin
                    && participant.video_joined
                    && participant.raise_hand_rating.as_deref() == Some("42")
                    && participant.paid_stars_total.as_deref() == Some("5")
                    && participant.video.as_ref().is_some_and(|video| {
                        video.endpoint == "video:101"
                            && video.audio_source == Some(113)
                            && matches!(
                                video.source_groups.as_slice(),
                                [group] if group.semantics == "SIM" && group.sources == vec![111, 112]
                            )
                    })
        ));

        let discarded = group_call_info_summary(tl::enums::GroupCall::Discarded(
            tl::types::GroupCallDiscarded { id: 1_002, access_hash: 90_002, duration: 360 },
        ));
        assert!(matches!(
            discarded,
            GroupCallInfoSummary::Discarded {
                ref call_id,
                duration_seconds: 360,
            } if call_id == "1002"
        ));
    }

    #[test]
    fn business_chat_links_use_exact_tl_contracts_and_bind_parsed_entities() {
        use tl::Identifiable;

        assert_eq!(tl::functions::account::CreateBusinessChatLink::CONSTRUCTOR_ID, 0x8851e68e);
        assert_eq!(tl::functions::account::EditBusinessChatLink::CONSTRUCTOR_ID, 0x8c3410af);
        assert_eq!(tl::functions::account::DeleteBusinessChatLink::CONSTRUCTOR_ID, 0x60073674);
        assert_eq!(tl::functions::account::GetBusinessChatLinks::CONSTRUCTOR_ID, 0x6f70dde1);
        assert_eq!(tl::functions::account::ResolveBusinessChatLink::CONSTRUCTOR_ID, 0x5492e5ee);
        assert_eq!(tl::types::BusinessChatLink::CONSTRUCTOR_ID, 0xb4ae666f);
        assert_eq!(tl::types::InputBusinessChatLink::CONSTRUCTOR_ID, 0x11679fa7);
        assert_eq!(tl::types::account::BusinessChatLinks::CONSTRUCTOR_ID, 0xec43a2d1);
        assert_eq!(tl::types::account::ResolvedBusinessChatLinks::CONSTRUCTOR_ID, 0x9a23af21);

        let content = business_chat_link_content_from_source(
            "<b>Release contact</b>",
            CaptionParseMode::Html,
            Some("Release".to_string()),
        );
        assert_eq!(content.rendered_message, "Release contact");
        assert_eq!(content.entity_count, 1);
        assert_eq!(content.entities_sha256.len(), 64);
        let input = business_chat_link_content_input(&content).expect("bound Business input");
        let tl::enums::InputBusinessChatLink::InputBusinessChatLink(input_fields) = input.clone();
        assert_eq!(input_fields.message, "Release contact");
        assert_eq!(input_fields.title.as_deref(), Some("Release"));
        assert_eq!(input_fields.entities.as_ref().map(Vec::len), Some(1));

        let create = tl::functions::account::CreateBusinessChatLink { link: input.clone() };
        assert_eq!(create.link, input);
        let edit = tl::functions::account::EditBusinessChatLink {
            slug: "release-contact".to_string(),
            link: create.link.clone(),
        };
        assert_eq!(edit.slug, "release-contact");
        assert_eq!(edit.link, create.link);
        let delete = tl::functions::account::DeleteBusinessChatLink { slug: edit.slug.clone() };
        let resolve = tl::functions::account::ResolveBusinessChatLink { slug: edit.slug.clone() };
        assert_eq!(delete.slug, "release-contact");
        assert_eq!(resolve.slug, "release-contact");

        let raw = tl::enums::BusinessChatLink::BusinessChatLink(tl::types::BusinessChatLink {
            link: "https://t.me/m/release-contact".to_string(),
            message: input_fields.message.clone(),
            entities: input_fields.entities.clone(),
            title: input_fields.title.clone(),
            views: 17,
        });
        let summary = business_chat_link_summary(raw.clone()).expect("Business summary");
        assert_eq!(summary.slug, "release-contact");
        assert_eq!(summary.link, "https://t.me/m/release-contact");
        assert_eq!(summary.message, content.rendered_message);
        assert_eq!(summary.entity_count, content.entity_count);
        assert_eq!(summary.entities_sha256, content.entities_sha256);
        assert_eq!(summary.title.as_deref(), Some("Release"));
        assert_eq!(summary.views, 17);

        let page = business_chat_links_page(tl::types::account::BusinessChatLinks {
            links: vec![raw],
            chats: Vec::new(),
            users: Vec::new(),
        })
        .expect("Business page");
        assert_eq!(page.count, 1);
        assert_eq!(page.links, vec![summary]);

        let resolved =
            resolved_business_chat_link_summary(tl::types::account::ResolvedBusinessChatLinks {
                peer: tl::enums::Peer::User(tl::types::PeerUser { user_id: 7 }),
                message: input_fields.message,
                entities: input_fields.entities,
                chats: Vec::new(),
                users: Vec::new(),
            });
        assert_eq!(resolved.peer_ref, "7");
        assert_eq!(resolved.message, content.rendered_message);
        assert_eq!(resolved.entity_count, content.entity_count);
        assert_eq!(resolved.entities_sha256, content.entities_sha256);

        let mut tampered = content;
        tampered.entities_sha256 = "0".repeat(64);
        assert!(matches!(
            business_chat_link_content_input(&tampered),
            Err(TelegramError::InvalidPeer(_))
        ));
    }

    #[test]
    fn business_settings_use_exact_current_tl_contracts_and_map_readback() {
        use tl::Identifiable;

        assert_eq!(tl::functions::account::UpdateBusinessWorkHours::CONSTRUCTOR_ID, 0x4b00e066);
        assert_eq!(tl::functions::account::UpdateBusinessLocation::CONSTRUCTOR_ID, 0x9e6b131a);
        assert_eq!(
            tl::functions::account::UpdateBusinessGreetingMessage::CONSTRUCTOR_ID,
            0x66cdafc4
        );
        assert_eq!(tl::functions::account::UpdateBusinessAwayMessage::CONSTRUCTOR_ID, 0xa26a7fa5);
        assert_eq!(tl::functions::account::UpdateBusinessIntro::CONSTRUCTOR_ID, 0xa614d034);
        assert_eq!(tl::types::BusinessWeeklyOpen::CONSTRUCTOR_ID, 0x120b1ab9);
        assert_eq!(tl::types::BusinessWorkHours::CONSTRUCTOR_ID, 0x8c92b098);
        assert_eq!(tl::types::BusinessLocation::CONSTRUCTOR_ID, 0xac5c1af7);
        assert_eq!(tl::types::InputBusinessRecipients::CONSTRUCTOR_ID, 0x6f8b32aa);
        assert_eq!(tl::types::InputBusinessGreetingMessage::CONSTRUCTOR_ID, 0x0194cb3b);
        assert_eq!(tl::types::InputBusinessAwayMessage::CONSTRUCTOR_ID, 0x832175e0);
        assert_eq!(tl::types::BusinessAwayMessageScheduleAlways::CONSTRUCTOR_ID, 0xc9b9e2b9);
        assert_eq!(
            tl::types::BusinessAwayMessageScheduleOutsideWorkHours::CONSTRUCTOR_ID,
            0xc3f2f501
        );
        assert_eq!(tl::types::BusinessAwayMessageScheduleCustom::CONSTRUCTOR_ID, 0xcc4d9ecc);
        assert_eq!(tl::types::InputBusinessIntro::CONSTRUCTOR_ID, 0x09c469cd);

        let weekly_open = vec![tl::enums::BusinessWeeklyOpen::BusinessWeeklyOpen(
            tl::types::BusinessWeeklyOpen { start_minute: 540, end_minute: 1_080 },
        )];
        let work_hours =
            tl::enums::BusinessWorkHours::BusinessWorkHours(tl::types::BusinessWorkHours {
                open_now: false,
                timezone_id: "Europe/Moscow".to_string(),
                weekly_open: weekly_open.clone(),
            });
        let work_request = tl::functions::account::UpdateBusinessWorkHours {
            business_work_hours: Some(work_hours.clone()),
        };
        assert_eq!(work_request.business_work_hours.as_ref(), Some(&work_hours));
        let mapped_work_hours = business_work_hours_summary(
            tl::enums::BusinessWorkHours::BusinessWorkHours(tl::types::BusinessWorkHours {
                open_now: true,
                timezone_id: "Europe/Moscow".to_string(),
                weekly_open,
            }),
        );
        assert!(mapped_work_hours.open_now);
        assert_eq!(mapped_work_hours.config.timezone_id, "Europe/Moscow");
        assert_eq!(
            mapped_work_hours.config.weekly_open,
            vec![BusinessWeeklyOpenSummary { start_minute: 540, end_minute: 1_080 }]
        );

        let location = tl::enums::BusinessLocation::BusinessLocation(tl::types::BusinessLocation {
            geo_point: None,
            address: "Fixture office".to_string(),
        });
        let location_summary = business_location_summary(location).expect("Business location");
        assert_eq!(location_summary.address, "Fixture office");
        assert_eq!(location_summary.point, None);
        let location_request = tl::functions::account::UpdateBusinessLocation {
            geo_point: Some(input_geo_point(&GeoPointSpec {
                latitude_e7: 557_558_260,
                longitude_e7: 376_173_000,
                accuracy_radius_m: Some(25),
            })),
            address: Some("Fixture office".to_string()),
        };
        assert_eq!(location_request.address.as_deref(), Some("Fixture office"));
        assert!(matches!(
            location_request.geo_point,
            Some(tl::enums::InputGeoPoint::InputGeoPoint(point))
                if (point.lat - 55.755826).abs() < f64::EPSILON
                    && (point.long - 37.6173).abs() < f64::EPSILON
                    && point.accuracy_radius == Some(25)
        ));

        let recipients = tl::enums::InputBusinessRecipients::InputBusinessRecipients(
            tl::types::InputBusinessRecipients {
                existing_chats: false,
                new_chats: true,
                contacts: false,
                non_contacts: false,
                exclude_selected: true,
                users: Some(vec![tl::enums::InputUser::UserSelf]),
            },
        );
        let greeting = tl::enums::InputBusinessGreetingMessage::InputBusinessGreetingMessage(
            tl::types::InputBusinessGreetingMessage {
                shortcut_id: 7,
                recipients: recipients.clone(),
                no_activity_days: 14,
            },
        );
        let greeting_request = tl::functions::account::UpdateBusinessGreetingMessage {
            message: Some(greeting.clone()),
        };
        assert_eq!(greeting_request.message.as_ref(), Some(&greeting));

        let away = tl::enums::InputBusinessAwayMessage::InputBusinessAwayMessage(
            tl::types::InputBusinessAwayMessage {
                offline_only: true,
                shortcut_id: 8,
                schedule: business_away_schedule_input(&BusinessAwayScheduleSummary::Custom {
                    start_date_unix: 1_800_000_000,
                    end_date_unix: 1_800_003_600,
                }),
                recipients,
            },
        );
        let away_request =
            tl::functions::account::UpdateBusinessAwayMessage { message: Some(away.clone()) };
        assert_eq!(away_request.message.as_ref(), Some(&away));
        assert!(matches!(
            business_away_schedule_input(&BusinessAwayScheduleSummary::Always),
            tl::enums::BusinessAwayMessageSchedule::Always
        ));
        assert!(matches!(
            business_away_schedule_input(&BusinessAwayScheduleSummary::OutsideHours),
            tl::enums::BusinessAwayMessageSchedule::OutsideWorkHours
        ));

        let intro =
            tl::enums::InputBusinessIntro::InputBusinessIntro(tl::types::InputBusinessIntro {
                title: "Hello".to_string(),
                description: "Tell us what you need".to_string(),
                sticker: None,
            });
        let intro_request =
            tl::functions::account::UpdateBusinessIntro { intro: Some(intro.clone()) };
        assert_eq!(intro_request.intro.as_ref(), Some(&intro));

        let mapped_recipients = business_recipients_summary(
            tl::enums::BusinessRecipients::BusinessRecipients(tl::types::BusinessRecipients {
                existing_chats: true,
                new_chats: false,
                contacts: false,
                non_contacts: false,
                exclude_selected: true,
                users: Some(vec![9, 7, 9]),
            }),
        );
        assert_eq!(mapped_recipients.user_ids, vec![7, 9]);
        assert!(mapped_recipients.existing_chats);
        assert!(mapped_recipients.exclude_selected);

        let mapped_greeting =
            business_greeting_summary(tl::enums::BusinessGreetingMessage::BusinessGreetingMessage(
                tl::types::BusinessGreetingMessage {
                    shortcut_id: 7,
                    recipients: tl::enums::BusinessRecipients::BusinessRecipients(
                        tl::types::BusinessRecipients {
                            existing_chats: false,
                            new_chats: true,
                            contacts: false,
                            non_contacts: false,
                            exclude_selected: false,
                            users: None,
                        },
                    ),
                    no_activity_days: 7,
                },
            ));
        assert_eq!(mapped_greeting.shortcut_id, 7);
        assert_eq!(mapped_greeting.no_activity_days, 7);
        assert!(mapped_greeting.recipients.new_chats);

        let mapped_away = business_away_summary(
            tl::enums::BusinessAwayMessage::BusinessAwayMessage(tl::types::BusinessAwayMessage {
                offline_only: true,
                shortcut_id: 8,
                schedule: tl::enums::BusinessAwayMessageSchedule::OutsideWorkHours,
                recipients: tl::enums::BusinessRecipients::BusinessRecipients(
                    tl::types::BusinessRecipients {
                        existing_chats: false,
                        new_chats: false,
                        contacts: true,
                        non_contacts: false,
                        exclude_selected: false,
                        users: None,
                    },
                ),
            }),
        );
        assert!(mapped_away.offline_only);
        assert_eq!(mapped_away.schedule, BusinessAwayScheduleSummary::OutsideHours);
        assert!(mapped_away.recipients.contacts);

        let mapped_intro = business_intro_summary(tl::enums::BusinessIntro::BusinessIntro(
            tl::types::BusinessIntro {
                title: "Hello".to_string(),
                description: "Tell us what you need".to_string(),
                sticker: None,
            },
        ))
        .expect("Business intro");
        assert_eq!(mapped_intro.title, "Hello");
        assert_eq!(mapped_intro.description, "Tell us what you need");
        assert_eq!(mapped_intro.sticker_handle, None);
    }

    #[test]
    fn stars_wallet_reads_use_exact_tl_contracts_and_preserve_current_fields() {
        use tl::Identifiable;

        assert_eq!(tl::functions::payments::GetStarsStatus::CONSTRUCTOR_ID, 0x4ea9b3bf);
        assert_eq!(tl::functions::payments::GetStarsTransactions::CONSTRUCTOR_ID, 0x69da4557);
        assert_eq!(tl::types::payments::StarsStatus::CONSTRUCTOR_ID, 0x6c9ce8ed);
        assert_eq!(tl::types::StarsAmount::CONSTRUCTOR_ID, 0xbbb6b4a3);
        assert_eq!(tl::types::StarsTransaction::CONSTRUCTOR_ID, 0x13659eb0);
        assert_eq!(tl::types::StarsSubscription::CONSTRUCTOR_ID, 0x2e6eab1a);
        assert_eq!(tl::functions::payments::GetStarGifts::CONSTRUCTOR_ID, 0xc4563590);
        assert_eq!(tl::functions::payments::GetSavedStarGifts::CONSTRUCTOR_ID, 0xa319e569);
        assert_eq!(tl::functions::payments::GetStarsTopupOptions::CONSTRUCTOR_ID, 0xc00ec7d3);
        assert_eq!(tl::functions::payments::GetStarsSubscriptions::CONSTRUCTOR_ID, 0x032512c5);
        assert_eq!(tl::functions::payments::GetSavedStarGift::CONSTRUCTOR_ID, 0xb455a106);
        assert_eq!(tl::functions::payments::SaveStarGift::CONSTRUCTOR_ID, 0x2a2a697c);
        assert_eq!(tl::functions::payments::ConvertStarGift::CONSTRUCTOR_ID, 0x74bf076b);
        assert_eq!(tl::functions::payments::ChangeStarsSubscription::CONSTRUCTOR_ID, 0xc7770878);
        assert_eq!(tl::types::InputSavedStarGiftUser::CONSTRUCTOR_ID, 0x69279795);
        assert_eq!(tl::types::InputSavedStarGiftChat::CONSTRUCTOR_ID, 0xf101aa7f);
        assert_eq!(tl::types::StarGift::CONSTRUCTOR_ID, 0x313a9547);
        assert_eq!(tl::types::StarGiftUnique::CONSTRUCTOR_ID, 0x85f0a9cd);
        assert_eq!(tl::types::SavedStarGift::CONSTRUCTOR_ID, 0x41df43fc);
        assert_eq!(tl::types::payments::SavedStarGifts::CONSTRUCTOR_ID, 0x95f389b1);
        assert_eq!(tl::types::payments::StarGifts::CONSTRUCTOR_ID, 0x2ed82995);
        assert_eq!(tl::types::StarsTopupOption::CONSTRUCTOR_ID, 0x0bd915c0);

        let status_request = tl::functions::payments::GetStarsStatus {
            ton: false,
            peer: tl::enums::InputPeer::PeerSelf,
        };
        assert!(!status_request.ton);
        assert!(matches!(status_request.peer, tl::enums::InputPeer::PeerSelf));

        let transactions_request = tl::functions::payments::GetStarsTransactions {
            inbound: true,
            outbound: false,
            ascending: true,
            ton: false,
            subscription_id: Some("sub-fixture".to_string()),
            peer: tl::enums::InputPeer::Channel(tl::types::InputPeerChannel {
                channel_id: 505,
                access_hash: 5_050,
            }),
            offset: "next".to_string(),
            limit: 50,
        };
        assert!(transactions_request.inbound);
        assert!(!transactions_request.outbound);
        assert!(transactions_request.ascending);
        assert!(!transactions_request.ton);
        assert_eq!(transactions_request.subscription_id.as_deref(), Some("sub-fixture"));
        assert_eq!(transactions_request.offset, "next");
        assert_eq!(transactions_request.limit, 50);

        let gifts_request = tl::functions::payments::GetStarGifts { hash: 0 };
        assert_eq!(gifts_request.hash, 0);
        let saved_gifts_request = tl::functions::payments::GetSavedStarGifts {
            exclude_unsaved: true,
            exclude_saved: false,
            exclude_unlimited: true,
            exclude_unique: false,
            sort_by_value: true,
            exclude_upgradable: true,
            exclude_unupgradable: false,
            peer_color_available: true,
            exclude_hosted: true,
            peer: tl::enums::InputPeer::PeerSelf,
            collection_id: Some(7),
            offset: "gift-next".to_string(),
            limit: 20,
        };
        assert!(saved_gifts_request.exclude_unsaved);
        assert!(saved_gifts_request.exclude_unlimited);
        assert!(saved_gifts_request.sort_by_value);
        assert!(saved_gifts_request.exclude_upgradable);
        assert!(saved_gifts_request.peer_color_available);
        assert!(saved_gifts_request.exclude_hosted);
        assert_eq!(saved_gifts_request.collection_id, Some(7));
        assert_eq!(saved_gifts_request.offset, "gift-next");
        assert_eq!(saved_gifts_request.limit, 20);
        let subscriptions_request = tl::functions::payments::GetStarsSubscriptions {
            missing_balance: true,
            peer: tl::enums::InputPeer::PeerSelf,
            offset: "subscription-next".to_string(),
        };
        assert!(subscriptions_request.missing_balance);
        assert_eq!(subscriptions_request.offset, "subscription-next");

        let user_saved_gift =
            tl::enums::InputSavedStarGift::User(tl::types::InputSavedStarGiftUser { msg_id: 31 });
        let chat_saved_gift =
            tl::enums::InputSavedStarGift::Chat(tl::types::InputSavedStarGiftChat {
                peer: tl::enums::InputPeer::Channel(tl::types::InputPeerChannel {
                    channel_id: 505,
                    access_hash: 5_050,
                }),
                saved_id: 80_001,
            });
        let exact_gift_request = tl::functions::payments::GetSavedStarGift {
            stargift: vec![user_saved_gift.clone(), chat_saved_gift.clone()],
        };
        assert_eq!(exact_gift_request.stargift.len(), 2);
        let save_request =
            tl::functions::payments::SaveStarGift { unsave: true, stargift: user_saved_gift };
        assert!(save_request.unsave);
        assert!(matches!(
            save_request.stargift,
            tl::enums::InputSavedStarGift::User(tl::types::InputSavedStarGiftUser { msg_id: 31 })
        ));
        let convert_request =
            tl::functions::payments::ConvertStarGift { stargift: chat_saved_gift };
        assert!(matches!(
            convert_request.stargift,
            tl::enums::InputSavedStarGift::Chat(tl::types::InputSavedStarGiftChat {
                saved_id: 80_001,
                ..
            })
        ));
        let change_subscription_request = tl::functions::payments::ChangeStarsSubscription {
            peer: tl::enums::InputPeer::PeerSelf,
            subscription_id: "sub-fixture".to_string(),
            canceled: Some(false),
        };
        assert_eq!(change_subscription_request.subscription_id, "sub-fixture");
        assert_eq!(change_subscription_request.canceled, Some(false));

        let unique_gift = tl::enums::StarGift::Unique(tl::types::StarGiftUnique {
            require_premium: false,
            resale_ton_only: false,
            theme_available: true,
            burned: false,
            crafted: false,
            id: 9_001,
            gift_id: 8_001,
            title: "Aurora".to_string(),
            slug: "aurora-42".to_string(),
            num: 42,
            owner_id: Some(tl::enums::Peer::User(tl::types::PeerUser { user_id: 101 })),
            owner_name: Some("Alex".to_string()),
            owner_address: None,
            attributes: Vec::new(),
            availability_issued: 10,
            availability_total: 100,
            gift_address: None,
            resell_amount: None,
            released_by: None,
            value_amount: None,
            value_currency: None,
            value_usd_amount: None,
            theme_peer: None,
            peer_color: None,
            host_id: None,
            offer_min_stars: Some(75),
            craft_chance_permille: None,
        });
        let payload = b"fixture-payload".to_vec();
        let transaction =
            tl::enums::StarsTransaction::StarsTransaction(tl::types::StarsTransaction {
                refund: true,
                pending: true,
                failed: false,
                gift: true,
                reaction: true,
                stargift_upgrade: true,
                business_transfer: true,
                stargift_resale: true,
                posts_search: true,
                stargift_prepaid_upgrade: true,
                stargift_drop_original_details: true,
                phonegroup_message: true,
                stargift_auction_bid: true,
                offer: true,
                id: "tx-fixture".to_string(),
                amount: tl::enums::StarsAmount::StarsAmount(tl::types::StarsAmount {
                    amount: -25,
                    nanos: 500_000_000,
                }),
                date: 1_700_004_001,
                peer: tl::enums::StarsTransactionPeer::StarsTransactionPeer(
                    tl::types::StarsTransactionPeer {
                        peer: tl::enums::Peer::User(tl::types::PeerUser { user_id: 101 }),
                    },
                ),
                title: Some("Fixture transaction".to_string()),
                description: Some("Every current field".to_string()),
                photo: None,
                transaction_date: Some(1_700_004_002),
                transaction_url: Some("https://example.invalid/untrusted".to_string()),
                bot_payload: Some(payload.clone()),
                msg_id: Some(11),
                extended_media: None,
                subscription_period: Some(2_592_000),
                giveaway_post_id: Some(77),
                stargift: Some(unique_gift.clone()),
                floodskip_number: Some(3),
                starref_commission_permille: Some(125),
                starref_peer: Some(tl::enums::Peer::Channel(tl::types::PeerChannel {
                    channel_id: 505,
                })),
                starref_amount: Some(tl::enums::StarsAmount::StarsAmount(tl::types::StarsAmount {
                    amount: 1,
                    nanos: 250_000_000,
                })),
                paid_messages: Some(2),
                premium_gift_months: Some(3),
                ads_proceeds_from_date: Some(1_700_000_000),
                ads_proceeds_to_date: Some(1_700_000_100),
            });
        let subscription =
            tl::enums::StarsSubscription::StarsSubscription(tl::types::StarsSubscription {
                canceled: true,
                can_refulfill: true,
                missing_balance: true,
                bot_canceled: true,
                id: "sub-fixture".to_string(),
                peer: tl::enums::Peer::User(tl::types::PeerUser { user_id: 303 }),
                until_date: 1_800_000_000,
                pricing: tl::enums::StarsSubscriptionPricing::StarsSubscriptionPricing(
                    tl::types::StarsSubscriptionPricing { period: 2_592_000, amount: 25 },
                ),
                chat_invite_hash: Some("credential-like-invite".to_string()),
                title: Some("Fixture Pro".to_string()),
                photo: None,
                invoice_slug: Some("fixture-pro".to_string()),
            });
        let page = stars_status_page(
            "7".to_string(),
            tl::enums::payments::StarsStatus::StarsStatus(tl::types::payments::StarsStatus {
                balance: tl::enums::StarsAmount::StarsAmount(tl::types::StarsAmount {
                    amount: 125,
                    nanos: 500_000_000,
                }),
                subscriptions: Some(vec![subscription.clone()]),
                subscriptions_next_offset: Some("subscriptions-next".to_string()),
                subscriptions_missing_balance: Some(25),
                history: Some(vec![transaction]),
                next_offset: Some("history-next".to_string()),
                chats: Vec::new(),
                users: Vec::new(),
            }),
        );
        assert_eq!(page.peer_ref, "7");
        assert_eq!(page.balance.kind, StarsAmountKind::Stars);
        assert_eq!(page.balance.amount, "125");
        assert_eq!(page.balance.nanos, Some(500_000_000));
        assert_eq!(page.subscriptions_missing_balance.as_deref(), Some("25"));
        assert!(matches!(
            page.subscriptions.as_slice(),
            [subscription]
                if subscription.subscription_id == "sub-fixture"
                    && subscription.peer_ref == "303"
                    && subscription.pricing.amount_stars == "25"
                    && subscription.canceled
                    && subscription.can_refulfill
                    && subscription.missing_balance
                    && subscription.bot_canceled
                    && subscription.has_chat_invite_hash
                    && !subscription.has_photo
        ));
        assert!(matches!(
            page.history.as_slice(),
            [transaction]
                if transaction.transaction_id == "tx-fixture"
                    && transaction.amount.amount == "-25"
                    && transaction.amount.nanos == Some(500_000_000)
                    && matches!(
                        transaction.peer,
                        StarsTransactionPeerSummary::Peer { ref peer_ref }
                            if peer_ref == "101"
                    )
                    && transaction.refund
                    && transaction.pending
                    && transaction.gift
                    && transaction.reaction
                    && transaction.star_gift_upgrade
                    && transaction.business_transfer
                    && transaction.star_gift_resale
                    && transaction.posts_search
                    && transaction.star_gift_prepaid_upgrade
                    && transaction.star_gift_drop_original_details
                    && transaction.phone_group_message
                    && transaction.star_gift_auction_bid
                    && transaction.offer
                    && transaction.bot_payload_length == Some(payload.len())
                    && transaction.bot_payload_sha256.as_deref()
                        == Some(format!("{:x}", Sha256::digest(&payload)).as_str())
                    && transaction.star_ref_peer_ref.as_deref() == Some("-100505")
                    && transaction.star_ref_amount.as_ref().is_some_and(|amount| {
                        amount.amount == "1" && amount.nanos == Some(250_000_000)
                    })
                    && matches!(
                        transaction.star_gift,
                        Some(StarsTransactionGiftSummary::Unique {
                            ref unique_gift_id,
                            ref gift_id,
                            ref title,
                            ref slug,
                            number: 42,
                        }) if unique_gift_id == "9001"
                            && gift_id == "8001"
                            && title == "Aurora"
                            && slug == "aurora-42"
                    )
        ));

        let regular_gift = tl::enums::StarGift::StarGift(tl::types::StarGift {
            limited: true,
            sold_out: false,
            birthday: true,
            require_premium: true,
            limited_per_user: true,
            peer_color_available: true,
            auction: true,
            id: 9_002,
            sticker: tl::enums::Document::Empty(tl::types::DocumentEmpty { id: 19_002 }),
            stars: 50,
            availability_remains: Some(12),
            availability_total: Some(100),
            availability_resale: Some(7),
            convert_stars: 42,
            first_sale_date: Some(1_700_005_001),
            last_sale_date: Some(1_700_005_002),
            upgrade_stars: Some(20),
            resell_min_stars: Some(60),
            title: Some("Launch Star".to_string()),
            released_by: Some(tl::enums::Peer::Channel(tl::types::PeerChannel { channel_id: 505 })),
            per_user_total: Some(2),
            per_user_remains: Some(1),
            locked_until_date: Some(1_800_005_001),
            auction_slug: Some("launch-auction".to_string()),
            gifts_per_round: Some(5),
            auction_start_date: Some(1_700_005_003),
            upgrade_variants: Some(3),
            background: None,
        });
        let available = available_star_gifts_page(tl::enums::payments::StarGifts::StarGifts(
            tl::types::payments::StarGifts {
                hash: 9_900,
                gifts: vec![regular_gift.clone(), unique_gift.clone()],
                chats: Vec::new(),
                users: Vec::new(),
            },
        ));
        assert!(!available.not_modified);
        assert_eq!(available.hash, Some(9_900));
        assert!(matches!(
            available.gifts.as_slice(),
            [
                StarGiftSummary::Regular {
                    gift_id,
                    sticker_document_id,
                    stars,
                    convert_stars,
                    limited: true,
                    auction: true,
                    availability_resale: Some(availability_resale),
                    released_by_peer_ref: Some(released_by),
                    ..
                },
                StarGiftSummary::Unique {
                    unique_gift_id,
                    slug,
                    offer_min_stars: Some(offer_min_stars),
                    ..
                },
            ] if gift_id == "9002"
                && sticker_document_id == "19002"
                && stars == "50"
                && convert_stars == "42"
                && availability_resale == "7"
                && released_by == "-100505"
                && unique_gift_id == "9001"
                && slug == "aurora-42"
                && offer_min_stars == "75"
        ));
        assert!(
            available_star_gifts_page(tl::enums::payments::StarGifts::NotModified).not_modified
        );

        let prepaid_hash = "credential-like-prepaid-upgrade".to_string();
        let saved = saved_star_gifts_page(
            "7".to_string(),
            tl::types::payments::SavedStarGifts {
                count: 1,
                chat_notifications_enabled: Some(true),
                gifts: vec![tl::enums::SavedStarGift::SavedStarGift(tl::types::SavedStarGift {
                    name_hidden: true,
                    unsaved: false,
                    refunded: true,
                    can_upgrade: true,
                    pinned_to_top: true,
                    upgrade_separate: true,
                    from_id: Some(tl::enums::Peer::User(tl::types::PeerUser { user_id: 101 })),
                    date: 1_700_005_010,
                    gift: regular_gift,
                    message: Some(plain_text_with_entities("For launch".to_string())),
                    msg_id: Some(31),
                    saved_id: Some(70_001),
                    convert_stars: Some(42),
                    upgrade_stars: Some(20),
                    can_export_at: Some(1_800_005_010),
                    transfer_stars: Some(5),
                    can_transfer_at: Some(1_700_105_010),
                    can_resell_at: Some(1_700_205_010),
                    collection_id: Some(vec![7]),
                    prepaid_upgrade_hash: Some(prepaid_hash.clone()),
                    drop_original_details_stars: Some(3),
                    gift_num: Some(17),
                    can_craft_at: Some(1_700_305_010),
                })],
                next_offset: Some("gift-next".to_string()),
                chats: Vec::new(),
                users: Vec::new(),
            },
        );
        assert_eq!(saved.peer_ref, "7");
        assert_eq!(saved.count, 1);
        assert_eq!(saved.next_offset.as_deref(), Some("gift-next"));
        assert!(matches!(
            saved.gifts.as_slice(),
            [gift]
                if gift.name_hidden
                    && gift.refunded
                    && gift.can_upgrade
                    && gift.pinned_to_top
                    && gift.upgrade_separate
                    && gift.from_peer_ref.as_deref() == Some("101")
                    && gift.message.as_deref() == Some("For launch")
                    && gift.saved_id.as_deref() == Some("70001")
                    && gift.has_prepaid_upgrade_hash
                    && gift.prepaid_upgrade_hash_length == Some(prepaid_hash.len())
                    && gift.prepaid_upgrade_hash_sha256.as_deref()
                        == Some(format!("{:x}", Sha256::digest(prepaid_hash.as_bytes())).as_str())
        ));

        let topup = stars_topup_options_page(vec![tl::enums::StarsTopupOption::StarsTopupOption(
            tl::types::StarsTopupOption {
                extended: true,
                stars: 500,
                store_product: Some("stars.500".to_string()),
                currency: "USD".to_string(),
                amount: 799,
            },
        )]);
        assert!(matches!(
            topup.options.as_slice(),
            [option]
                if option.stars == "500"
                    && option.store_product.as_deref() == Some("stars.500")
                    && option.currency == "USD"
                    && option.amount == "799"
                    && option.extended
        ));

        let subscriptions = stars_subscriptions_page(
            "7".to_string(),
            tl::enums::payments::StarsStatus::StarsStatus(tl::types::payments::StarsStatus {
                balance: tl::enums::StarsAmount::StarsAmount(tl::types::StarsAmount {
                    amount: 100,
                    nanos: 0,
                }),
                subscriptions: Some(vec![subscription]),
                subscriptions_next_offset: Some("subscription-next".to_string()),
                subscriptions_missing_balance: Some(25),
                history: None,
                next_offset: None,
                chats: Vec::new(),
                users: Vec::new(),
            }),
        );
        assert_eq!(subscriptions.next_offset.as_deref(), Some("subscription-next"));
        assert_eq!(subscriptions.subscriptions_missing_balance.as_deref(), Some("25"));
        assert_eq!(subscriptions.subscriptions[0].subscription_id, "sub-fixture");
    }

    #[test]
    fn folder_suggestions_tags_and_global_privacy_use_exact_tl_contracts() {
        use tl::Identifiable;

        assert_eq!(tl::functions::messages::GetSuggestedDialogFilters::CONSTRUCTOR_ID, 0xa29cd42c);
        assert_eq!(tl::functions::messages::ToggleDialogFilterTags::CONSTRUCTOR_ID, 0xfd2dda49);
        assert_eq!(tl::functions::account::GetGlobalPrivacySettings::CONSTRUCTOR_ID, 0xeb2b4cf6);
        assert_eq!(tl::functions::account::SetGlobalPrivacySettings::CONSTRUCTOR_ID, 0x1edaaac2);

        let toggle = tl::functions::messages::ToggleDialogFilterTags { enabled: true };
        assert!(toggle.enabled);

        let suggested = tl::types::DialogFilterSuggested {
            filter: tl::enums::DialogFilter::DialogFilter(tl::types::DialogFilter {
                contacts: true,
                non_contacts: true,
                groups: true,
                broadcasts: true,
                bots: true,
                exclude_muted: false,
                exclude_read: true,
                exclude_archived: false,
                title_noanimate: false,
                id: 7,
                title: plain_text_with_entities("Unread".to_string()),
                emoticon: Some("📥".to_string()),
                color: Some(1),
                pinned_peers: Vec::new(),
                include_peers: Vec::new(),
                exclude_peers: Vec::new(),
            }),
            description: "Chats with unread messages".to_string(),
        };
        let folder = dialog_folder_summary(&suggested.filter, 42).expect("suggested folder");
        assert_eq!(folder.folder_id, 7);
        assert_eq!(folder.title, "Unread");
        assert_eq!(folder.emoticon.as_deref(), Some("📥"));
        assert!(folder.exclude_read);
        assert_eq!(suggested.description, "Chats with unread messages");

        let source = tl::enums::GlobalPrivacySettings::GlobalPrivacySettings(
            tl::types::GlobalPrivacySettings {
                archive_and_mute_new_noncontact_peers: true,
                keep_archived_unmuted: false,
                keep_archived_folders: true,
                hide_read_marks: true,
                new_noncontact_peers_require_premium: false,
                display_gifts_button: true,
                noncontact_peers_paid_stars: Some(125),
                disallowed_gifts: Some(
                    tl::types::DisallowedGiftsSettings {
                        disallow_unlimited_stargifts: true,
                        disallow_limited_stargifts: false,
                        disallow_unique_stargifts: true,
                        disallow_premium_gifts: false,
                        disallow_stargifts_from_channels: true,
                    }
                    .into(),
                ),
            },
        );
        let summary = global_privacy_settings_summary(source);
        assert_eq!(summary.noncontact_peers_paid_stars.as_deref(), Some("125"));
        assert!(summary.display_gifts_button);
        assert!(summary.disallowed_gifts.as_ref().is_some_and(|gifts| {
            gifts.disallow_unique_stargifts && gifts.disallow_stargifts_from_channels
        }));

        let input = global_privacy_settings_input(&summary).expect("global privacy input");
        assert!(matches!(
            input,
            tl::enums::GlobalPrivacySettings::GlobalPrivacySettings(settings)
                if settings.archive_and_mute_new_noncontact_peers
                    && !settings.keep_archived_unmuted
                    && settings.keep_archived_folders
                    && settings.hide_read_marks
                    && !settings.new_noncontact_peers_require_premium
                    && settings.display_gifts_button
                    && settings.noncontact_peers_paid_stars == Some(125)
                    && matches!(
                        settings.disallowed_gifts.as_ref(),
                        Some(tl::enums::DisallowedGiftsSettings::DisallowedGiftsSettings(gifts))
                            if gifts.disallow_unlimited_stargifts
                                && !gifts.disallow_limited_stargifts
                                && gifts.disallow_unique_stargifts
                                && !gifts.disallow_premium_gifts
                                && gifts.disallow_stargifts_from_channels
                    )
        ));

        let mut invalid = summary;
        invalid.noncontact_peers_paid_stars = Some("0125".to_string());
        assert!(matches!(
            global_privacy_settings_input(&invalid),
            Err(TelegramError::InvalidPeer(_))
        ));
    }

    #[test]
    fn reset_authorizations_uses_exact_all_other_sessions_tl_contract() {
        use tl::Identifiable;

        assert_eq!(tl::functions::auth::ResetAuthorizations::CONSTRUCTOR_ID, 0x9fab0d1a);
        let request = tl::functions::auth::ResetAuthorizations {};
        assert_eq!(request, tl::functions::auth::ResetAuthorizations {});
    }

    #[test]
    fn set_history_ttl_uses_exact_chat_auto_delete_tl_contract() {
        use tl::Identifiable;

        assert_eq!(tl::functions::messages::SetHistoryTtl::CONSTRUCTOR_ID, 0xb80e5fe4);
        let request = tl::functions::messages::SetHistoryTtl {
            peer: tl::enums::InputPeer::Channel(tl::types::InputPeerChannel {
                channel_id: 505,
                access_hash: 5_050,
            }),
            period: 86_400,
        };
        assert_eq!(request.period, 86_400);
        assert!(matches!(
            request.peer,
            tl::enums::InputPeer::Channel(peer)
                if peer.channel_id == 505 && peer.access_hash == 5_050
        ));
    }

    #[test]
    fn clear_all_drafts_uses_exact_tl_contract() {
        use tl::Identifiable;

        assert_eq!(tl::functions::messages::ClearAllDrafts::CONSTRUCTOR_ID, 0x7e58ee9c);
        let request = tl::functions::messages::ClearAllDrafts {};
        assert_eq!(request, tl::functions::messages::ClearAllDrafts {});
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

    #[test]
    fn participant_filters_bind_exact_telegram_constructors() {
        assert!(matches!(
            channel_participants_filter(ParticipantFilter::Recent, None),
            tl::enums::ChannelParticipantsFilter::ChannelParticipantsRecent
        ));
        assert!(matches!(
            channel_participants_filter(ParticipantFilter::Recent, Some("alex")),
            tl::enums::ChannelParticipantsFilter::ChannelParticipantsSearch(filter)
                if filter.q == "alex"
        ));
        assert!(matches!(
            channel_participants_filter(ParticipantFilter::Admins, Some("ignored locally")),
            tl::enums::ChannelParticipantsFilter::ChannelParticipantsAdmins
        ));
        assert!(matches!(
            channel_participants_filter(ParticipantFilter::Kicked, Some("kai")),
            tl::enums::ChannelParticipantsFilter::ChannelParticipantsKicked(filter)
                if filter.q == "kai"
        ));
        assert!(matches!(
            channel_participants_filter(ParticipantFilter::Banned, Some("rin")),
            tl::enums::ChannelParticipantsFilter::ChannelParticipantsBanned(filter)
                if filter.q == "rin"
        ));
        assert!(matches!(
            channel_participants_filter(ParticipantFilter::Bots, None),
            tl::enums::ChannelParticipantsFilter::ChannelParticipantsBots
        ));
        assert!(matches!(
            channel_participants_filter(ParticipantFilter::Contacts, Some("mira")),
            tl::enums::ChannelParticipantsFilter::ChannelParticipantsContacts(filter)
                if filter.q == "mira"
        ));
    }

    fn emoji_reaction(emoji: &str) -> tl::enums::Reaction {
        tl::enums::Reaction::Emoji(tl::types::ReactionEmoji { emoticon: emoji.to_string() })
    }

    fn message_reactions(
        results: Vec<tl::enums::ReactionCount>,
        recent: Option<Vec<tl::enums::MessagePeerReaction>>,
    ) -> tl::enums::MessageReactions {
        tl::enums::MessageReactions::MessageReactions(tl::types::MessageReactions {
            min: false,
            can_see_list: false,
            reactions_as_tags: false,
            results,
            recent_reactions: recent,
            top_reactors: None,
        })
    }

    fn reaction_count_with(
        reaction: tl::enums::Reaction,
        chosen_order: Option<i32>,
        count: i32,
    ) -> tl::enums::ReactionCount {
        tl::enums::ReactionCount::ReactionCount(tl::types::ReactionCount {
            chosen_order,
            reaction,
            count,
        })
    }

    fn reaction_count(
        reaction: tl::enums::Reaction,
        chosen_order: Option<i32>,
    ) -> tl::enums::ReactionCount {
        reaction_count_with(reaction, chosen_order, 1)
    }

    fn peer_reaction(reaction: tl::enums::Reaction, my: bool, big: bool) -> tl::enums::MessagePeerReaction {
        tl::enums::MessagePeerReaction::MessagePeerReaction(tl::types::MessagePeerReaction {
            big,
            unread: false,
            my,
            peer_id: tl::enums::Peer::User(tl::types::PeerUser { user_id: 42 }),
            date: 1_792_000_000,
            reaction,
        })
    }

    #[test]
    fn chosen_order_marks_the_current_account_reaction() {
        // Broadcast channels forbid the per-user reaction list, so the message
        // summary is the only source left; `chosen_order` is what it sets for
        // the reaction this account chose.
        let reactions = message_reactions(
            vec![
                reaction_count(emoji_reaction("❤"), None),
                reaction_count(emoji_reaction("👍"), Some(7)),
            ],
            None,
        );
        assert_eq!(
            my_reaction_from_reactions(Some(&reactions)).expect("reaction read"),
            Some(ReactionSpec::Emoji { emoji: "👍".to_string(), big: false, add_to_recent: false })
        );
    }

    #[test]
    fn recent_reactions_my_entry_marks_the_current_account_reaction() {
        let reactions = message_reactions(
            vec![reaction_count(emoji_reaction("🎉"), None)],
            Some(vec![
                peer_reaction(emoji_reaction("❤"), false, false),
                peer_reaction(tl::enums::Reaction::CustomEmoji(tl::types::ReactionCustomEmoji { document_id: 777 }), true, true),
            ]),
        );
        assert_eq!(
            my_reaction_from_reactions(Some(&reactions)).expect("reaction read"),
            Some(ReactionSpec::CustomEmoji { document_id: 777, big: true, add_to_recent: false })
        );
    }

    #[test]
    fn a_message_without_the_account_reaction_reads_as_none() {
        let others_only = message_reactions(
            vec![reaction_count(emoji_reaction("🔥"), None)],
            Some(vec![peer_reaction(emoji_reaction("🔥"), false, false)]),
        );
        assert_eq!(my_reaction_from_reactions(Some(&others_only)).expect("reaction read"), None);
        let empty = message_reactions(Vec::new(), None);
        assert_eq!(my_reaction_from_reactions(Some(&empty)).expect("reaction read"), None);
        assert_eq!(my_reaction_from_reactions(None).expect("reaction read"), None);
    }

    #[test]
    fn a_paid_reaction_stays_out_of_the_ordinary_approval_flow() {
        let reactions = message_reactions(vec![reaction_count(tl::enums::Reaction::Paid, Some(1))], None);
        assert!(matches!(
            my_reaction_from_reactions(Some(&reactions)),
            Err(TelegramError::InvalidPeer(_))
        ));
    }

    #[test]
    fn a_forbidden_per_viewer_list_falls_back_to_the_message_summary() {
        // 403 BROADCAST_FORBIDDEN is what a broadcast channel answers to the
        // per-viewer list; the message summary still carries the totals and
        // this account's own choice.
        let summary = message_reactions(
            vec![
                reaction_count_with(emoji_reaction("❤"), None, 1),
                reaction_count_with(emoji_reaction("👍"), Some(7), 5),
            ],
            Some(vec![peer_reaction(emoji_reaction("👍"), true, false)]),
        );
        let page =
            reaction_page_from_summary(Some(&summary), Some("BROADCAST_FORBIDDEN".to_string()));
        assert_eq!(page.viewer_list_forbidden.as_deref(), Some("BROADCAST_FORBIDDEN"));
        assert_eq!(page.total_count, 6);
        assert_eq!(page.next_offset, None);
        assert_eq!(
            page.counts,
            vec![
                MessageReactionCount {
                    kind: "emoji".to_string(),
                    emoji: Some("❤".to_string()),
                    custom_emoji_id: None,
                    count: 1,
                    chosen: false,
                },
                MessageReactionCount {
                    kind: "emoji".to_string(),
                    emoji: Some("👍".to_string()),
                    custom_emoji_id: None,
                    count: 5,
                    chosen: true,
                },
            ]
        );
        assert_eq!(page.reactions.len(), 1);
        assert!(page.reactions[0].my);
        assert_eq!(page.reactions[0].emoji.as_deref(), Some("👍"));
    }

    #[test]
    fn only_a_permanent_read_refusal_switches_to_the_summary() {
        let forbidden = ferogram::InvocationError::Rpc(ferogram::RpcError {
            code: 403,
            name: "BROADCAST_FORBIDDEN".to_string(),
            value: None,
        });
        assert_eq!(
            viewer_list_forbidden_reason(&forbidden).as_deref(),
            Some("BROADCAST_FORBIDDEN")
        );
        let bad_request = ferogram::InvocationError::Rpc(ferogram::RpcError {
            code: 400,
            name: "MSG_ID_INVALID".to_string(),
            value: None,
        });
        assert_eq!(viewer_list_forbidden_reason(&bad_request), None);
        let flood = ferogram::InvocationError::Rpc(ferogram::RpcError {
            code: 420,
            name: "FLOOD_WAIT".to_string(),
            value: Some(30),
        });
        assert_eq!(viewer_list_forbidden_reason(&flood), None);
        assert_eq!(viewer_list_forbidden_reason(&ferogram::InvocationError::Dropped), None);
    }

    #[test]
    fn a_message_without_a_reaction_summary_reads_as_an_empty_forbidden_page() {
        let page = reaction_page_from_summary(None, Some("BROADCAST_FORBIDDEN".to_string()));
        assert_eq!(page.total_count, 0);
        assert!(page.counts.is_empty());
        assert!(page.reactions.is_empty());
        assert_eq!(page.next_offset, None);
        assert_eq!(page.viewer_list_forbidden.as_deref(), Some("BROADCAST_FORBIDDEN"));
    }

    fn rpc_error(code: i32, name: &str) -> ferogram::InvocationError {
        ferogram::InvocationError::Rpc(ferogram::RpcError {
            code,
            name: name.to_string(),
            value: None,
        })
    }

    #[tokio::test]
    async fn a_forbidden_list_answers_from_the_message_summary_instead_of_failing() {
        let page =
            reaction_read_outcome(Err(rpc_error(403, "BROADCAST_FORBIDDEN")), async || Ok(Vec::new()))
                .await
                .expect("a forbidden per-viewer list must fall back to the message summary");
        assert_eq!(page.viewer_list_forbidden.as_deref(), Some("BROADCAST_FORBIDDEN"));
        assert_eq!(page.total_count, 0);
        assert!(page.counts.is_empty());
    }

    #[tokio::test]
    async fn a_forbidden_list_keeps_both_reasons_when_the_summary_read_fails_too() {
        let outcome = reaction_read_outcome(
            Err(rpc_error(403, "BROADCAST_FORBIDDEN")),
            async || Err(rpc_error(420, "FLOOD_WAIT")),
        )
        .await;
        let error = outcome.expect_err("a failing summary read stays an error").to_string();
        assert!(error.contains("BROADCAST_FORBIDDEN"), "{error}");
        assert!(error.contains("FLOOD_WAIT"), "{error}");
    }

    #[tokio::test]
    async fn any_other_refusal_stays_an_error_without_reading_the_summary() {
        let outcome = reaction_read_outcome(
            Err(rpc_error(400, "MSG_ID_INVALID")),
            async || panic!("the summary must not be read for a non-forbidden refusal"),
        )
        .await;
        let error = outcome.expect_err("other refusals stay errors").to_string();
        assert!(error.contains("MSG_ID_INVALID"), "{error}");
    }

    type AlbumMessageMedia = tl::enums::MessageMedia;

    fn album_photo_media(id: i64, hash: i64, reference: Vec<u8>) -> AlbumMessageMedia {
        AlbumMessageMedia::Photo(tl::types::MessageMediaPhoto {
            spoiler: false,
            live_photo: false,
            photo: Some(tl::enums::Photo::Photo(tl::types::Photo {
                has_stickers: false,
                id,
                access_hash: hash,
                file_reference: reference,
                date: 1_700_000_000,
                sizes: Vec::new(),
                video_sizes: None,
                dc_id: 2,
            })),
            ttl_seconds: None,
            video: None,
        })
    }

    fn album_document_media(id: i64, hash: i64, reference: Vec<u8>) -> AlbumMessageMedia {
        AlbumMessageMedia::Document(tl::types::MessageMediaDocument {
            nopremium: false,
            spoiler: false,
            video: false,
            round: false,
            voice: false,
            document: Some(tl::enums::Document::Document(tl::types::Document {
                id,
                access_hash: hash,
                file_reference: reference,
                date: 1_700_000_000,
                mime_type: "video/mp4".to_string(),
                size: 0,
                thumbs: None,
                video_thumbs: None,
                dc_id: 2,
                attributes: Vec::new(),
            })),
            alt_documents: None,
            video_cover: None,
            video_timestamp: None,
            ttl_seconds: None,
        })
    }

    fn album_media_rejected(outcome: Result<tl::enums::InputMedia, TelegramError>) -> bool {
        matches!(outcome, Err(TelegramError::SendRejected(_)))
    }

    #[test]
    fn album_photo_media_is_a_finalized_reference() {
        let media = finalized_album_photo_media(album_photo_media(10, 20, vec![1, 2, 3]))
            .expect("a messageMediaPhoto with a concrete photo must finalize");
        assert!(
            !matches!(&media, tl::enums::InputMedia::UploadedPhoto(_)),
            "album items must not carry the raw upload"
        );
        let tl::enums::InputMedia::Photo(tl::types::InputMediaPhoto {
            id: tl::enums::InputPhoto::InputPhoto(photo),
            ..
        }) = media
        else {
            panic!("a photo upload must finalize into an inputPhoto reference");
        };
        assert_eq!((photo.id, photo.access_hash), (10, 20));
        assert_eq!(photo.file_reference, vec![1, 2, 3]);
    }

    #[test]
    fn album_document_media_is_a_finalized_reference() {
        let media = finalized_album_document_media(album_document_media(30, 40, vec![4, 5, 6]))
            .expect("a messageMediaDocument with a concrete document must finalize");
        assert!(
            !matches!(&media, tl::enums::InputMedia::UploadedDocument(_)),
            "album items must not carry the raw upload"
        );
        let tl::enums::InputMedia::Document(tl::types::InputMediaDocument {
            id: tl::enums::InputDocument::InputDocument(document),
            ..
        }) = media
        else {
            panic!("a video upload must finalize into an inputDocument reference");
        };
        assert_eq!((document.id, document.access_hash), (30, 40));
        assert_eq!(document.file_reference, vec![4, 5, 6]);
    }

    #[test]
    fn album_media_rejects_missing_payloads() {
        let photo_without_payload = AlbumMessageMedia::Photo(tl::types::MessageMediaPhoto {
            spoiler: false,
            live_photo: false,
            photo: None,
            ttl_seconds: None,
            video: None,
        });
        let empty_photo = AlbumMessageMedia::Photo(tl::types::MessageMediaPhoto {
            spoiler: false,
            live_photo: false,
            photo: Some(tl::enums::Photo::Empty(tl::types::PhotoEmpty { id: 11 })),
            ttl_seconds: None,
            video: None,
        });
        let document_without_payload =
            AlbumMessageMedia::Document(tl::types::MessageMediaDocument {
                nopremium: false,
                spoiler: false,
                video: false,
                round: false,
                voice: false,
                document: None,
                alt_documents: None,
                video_cover: None,
                video_timestamp: None,
                ttl_seconds: None,
            });
        let photo_reference = album_photo_media(10, 20, vec![1, 2, 3]);
        let document_reference = album_document_media(30, 40, vec![4, 5, 6]);
        assert!(album_media_rejected(finalized_album_photo_media(photo_without_payload)));
        assert!(album_media_rejected(finalized_album_photo_media(empty_photo)));
        assert!(album_media_rejected(finalized_album_document_media(document_without_payload)));
        assert!(album_media_rejected(finalized_album_document_media(photo_reference)));
        assert!(album_media_rejected(finalized_album_photo_media(document_reference)));
    }
}
