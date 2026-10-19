#![allow(
    clippy::arithmetic_side_effects,
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::indexing_slicing
)]

// This module is a deterministic in-memory test/demo backend. Its fixture indexes,
// counters, and synthetic timestamps are bounded by constructors in this file and
// cannot reach Telegram or mutate an external account. Production implementations
// remain subject to the workspace-wide checked-arithmetic and indexing policy.

use crate::backend::TelegramBackend;
use crate::domain::{
    AccountEmojiStatusSpec, AccountProfileSummary, AdminLogActionKind, AdminLogEventFilter,
    AdminLogEventSummary, AdminLogPage, AlbumMediaKind, AlbumUploadSpec, AttachmentDescriptor,
    AudioTranscriptionSummary, AvailableStarGiftsPage, BirthdaySummary, BoostPage,
    BoostSlotSummary, BoostStatusSummary, BoostSummary, BotCallbackAnswerSummary,
    BotCommandSummary, BusinessAccountSettingsSummary, BusinessAwayMessageSummary,
    BusinessChatLinkContent, BusinessChatLinkSummary, BusinessChatLinksPage,
    BusinessGreetingMessageSummary, BusinessIntroSpec, BusinessIntroSummary,
    BusinessLocationSummary, BusinessRecipientsSummary, BusinessWeeklyOpenSummary,
    BusinessWorkHoursConfig, BusinessWorkHoursSummary, CallbackButtonSnapshot, CaptionParseMode,
    ChannelSettingsSummary, ChannelStatsSummary, ChannelUpdatesDifferenceKind,
    ChannelUpdatesDifferenceSummary, ChannelUpdatesRequest, ChannelUpdatesRequestSummary,
    ChatAdminRightsSummary, ChatAutoDeleteSummary, ChatBannedRightsSummary, ChatDetailsSummary,
    ChatLifecycleState, ChatMemberRole, ChatMemberSummary, ChatModerationSettingsSummary,
    ChatReactionsPolicy, ChatSummary, Completeness, ContactSummary, CustomEmojiDocumentSummary,
    DefaultReactionSpec, DefaultReactionSummary, DeletedMessagesSummary, DialogDraftSummary,
    DialogFolderKind, DialogFolderList, DialogFolderSummary, DialogStateSnapshot, DiceEmojiCatalog,
    DisallowedGiftsSettingsSummary, DiscussionMessageSummary, DraftSummary, EmojiStatusCatalogKind,
    EmojiStatusCatalogPage, EmojiStatusKind, EmojiStatusSummary, FactCheckPage, FactCheckSummary,
    FileSendMode, ForumTopicSummary, GeoPointSpec, GlobalPrivacySettingsSummary,
    GroupCallInfoSummary, GroupCallPage, GroupCallParticipantSummary,
    GroupCallParticipantVideoSourceGroupSummary, GroupCallParticipantVideoSummary,
    GroupCallParticipantsPage, InlineBotResultSourceKind, InlineBotResultSummary, InlineBotResults,
    InlineBotSwitchPmSummary, InlineBotSwitchWebViewSummary, InviteJoinerPage, InviteJoinerSummary,
    InviteLinkKind, InviteLinkSummary, MessageButtonInventory, MessageButtonKind,
    MessageButtonMarkupType, MessageButtonSummary, MessageLinkSummary, MessagePage,
    MessageReactionPage, MessageReactionReadReceipt, MessageReactionSummary, MessageReadReceipt,
    MessageTranslationBatch, MessageTranslationSummary, MyBoostsSummary, NotificationSettingsPatch,
    NotificationSettingsSummary, NotificationSoundSummary, OutboxReadDateSummary,
    ParticipantFilter, ParticipantPage, ParticipantSummary, PeerColorSummary,
    PersonalChannelSummary, PollAnswerSummary, PollCreateSpec, PollOptionBinding, PollSummary,
    PollVoterPage, PollVoterSummary, PrivacyKey, PrivacyRuleSummary, ProfilePhotoPage,
    ProfilePhotoSummary, QuickReplyCatalogPage, QuickReplyMessagesPage, QuickReplyShortcutSummary,
    QuickReplyStoredMessageSummary, ReactionCatalogPage, ReactionIdentity, ReactionSpec,
    ResolvedBusinessChatLinkSummary, RichMediaSpec, RichMessageSnapshot, SavedDialogSummary,
    SavedDialogsPage, SavedHistoryPage, SavedMessagesSearchRequest, SavedReactionTagSummary,
    SavedReactionTagsPage, SavedStarGiftRef, SavedStarGiftSummary, SavedStarGiftsPage,
    SavedStarGiftsRequest, SearchMessagesRequest, SentDocumentMediaSummary, SentRichMediaSummary,
    SessionSummary, StarGiftAttributeSummary, StarGiftRaritySummary, StarGiftSummary,
    StarsAmountKind, StarsAmountSummary, StarsStatusPage, StarsSubscriptionPricingSummary,
    StarsSubscriptionSummary, StarsSubscriptionsPage, StarsTopupOptionSummary,
    StarsTopupOptionsPage, StarsTransactionGiftSummary, StarsTransactionPeerSummary,
    StarsTransactionSummary, StatsDateRangeSummary, StatsGraphSummary, StatsPercentSummary,
    StatsPostInteractionSummary, StatsTopAdminSummary, StatsTopInviterSummary,
    StatsTopPosterSummary, StatsValueSummary, StickerCatalogPage, StickerDocumentSummary,
    StickerEmojiPackSummary, StickerKeywordSummary, StickerSetDetailsPage, StickerSetSearchPage,
    StickerSetSummary, StoryCollectionPage, StoryLinkSummary, StoryListPage, StoryPeerPage,
    StoryPeerSummary, StoryPrivacySpec, StoryReportOption, StoryReportResult,
    StoryStealthModeSummary, StorySummary, StoryViewerSummary, StoryViewersPage,
    SuggestedDialogFolderList, SuggestedDialogFolderSummary, TypingAction, UnreadMessagesRequest,
    UpdateKindSummary, UpdatesCursorSummary, UpdatesDifferenceKind, UpdatesDifferenceRequest,
    UpdatesDifferenceRequestSummary, UpdatesDifferenceSummary, UpdatesStateSummary, UserDetails,
    WebPagePreviewSummary,
};
use crate::fs_security::restrict_private_file;
use crate::invite::opaque_invite_handle;
use crate::profile_photo::{opaque_chat_photo_handle, opaque_profile_photo_handle};
use crate::sticker::opaque_sticker_handle;
use crate::telegram::{
    DialogSummary, MessageForwardOrigin, MessageSummary, TelegramError, UserInfo,
};
use async_trait::async_trait;
use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64_STANDARD};
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet, VecDeque};
use std::path::PathBuf;
use tokio::io::AsyncWriteExt;
use tokio::sync::Mutex;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FakeSentMessage {
    pub chat_id: String,
    pub text: String,
    pub message_id: i32,
    pub file_name: Option<String>,
    pub reply_to_message_id: Option<i32>,
    pub file_mode: Option<FileSendMode>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FakeInlineBotResultSend {
    pub chat_id: String,
    pub query_id: String,
    pub result_id: String,
    pub message_id: i32,
    pub reply_to_message_id: Option<i32>,
    pub silent: bool,
    pub hide_via: bool,
    pub clear_draft: bool,
    pub random_id: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FakeInviteOutcome {
    Success,
    Rejected,
    RetryableNoEffect,
    UncertainBeforeApply,
    UncertainAfterApply,
}

#[derive(Debug, Clone)]
struct FakeInviteLink {
    _raw_link: String,
    summary: InviteLinkSummary,
}

#[derive(Debug, Clone)]
struct FakeInviteImporter {
    invite_handle: Option<String>,
    summary: InviteJoinerSummary,
}

#[derive(Debug, Clone)]
struct FakeChannelSettingsState {
    can_set_username: bool,
    can_view_stats: bool,
    signatures_enabled: bool,
    signature_profiles_enabled: bool,
    pre_history_hidden: bool,
    join_request_required: bool,
    join_to_send_required: bool,
    slow_mode_seconds: i32,
    no_forwards: bool,
    forum_enabled: bool,
    history_ttl_seconds: Option<i32>,
    linked_chat_id: Option<String>,
}

#[derive(Debug, Clone)]
struct FakeChatModerationState {
    default_banned_rights: Option<ChatBannedRightsSummary>,
    available_reactions: Option<ChatReactionsPolicy>,
    reactions_limit: Option<i32>,
    anti_spam_enabled: Option<bool>,
    forum_enabled: Option<bool>,
    forum_tabs_enabled: Option<bool>,
}

impl Default for FakeChatModerationState {
    fn default() -> Self {
        Self {
            default_banned_rights: Some(ChatBannedRightsSummary::default()),
            available_reactions: Some(ChatReactionsPolicy::All { allow_custom: false }),
            reactions_limit: None,
            anti_spam_enabled: None,
            forum_enabled: None,
            forum_tabs_enabled: None,
        }
    }
}

impl Default for FakeChannelSettingsState {
    fn default() -> Self {
        Self {
            can_set_username: true,
            can_view_stats: true,
            signatures_enabled: false,
            signature_profiles_enabled: false,
            pre_history_hidden: false,
            join_request_required: false,
            join_to_send_required: false,
            slow_mode_seconds: 0,
            no_forwards: false,
            forum_enabled: false,
            history_ttl_seconds: None,
            linked_chat_id: None,
        }
    }
}

#[derive(Debug)]
struct FakeState {
    account: UserInfo,
    account_is_bot: bool,
    account_profile: AccountProfileSummary,
    profile_photos: Vec<ProfilePhotoSummary>,
    next_profile_photo_id: i64,
    dialog_folders: DialogFolderList,
    suggested_dialog_folders: SuggestedDialogFolderList,
    global_privacy_settings: GlobalPrivacySettingsSummary,
    session_path: PathBuf,
    dialogs: Vec<DialogSummary>,
    known_chats: HashMap<i64, ChatDetailsSummary>,
    channel_settings: HashMap<i64, FakeChannelSettingsState>,
    chat_moderation_settings: HashMap<i64, FakeChatModerationState>,
    stories: HashMap<i64, Vec<StorySummary>>,
    story_archive: HashMap<i64, Vec<StorySummary>>,
    story_pinned_to_top: HashMap<i64, Vec<i32>>,
    story_stealth_mode: StoryStealthModeSummary,
    story_max_read_ids: HashMap<i64, i32>,
    story_viewers: HashMap<(i64, i32), Vec<StoryViewerSummary>>,
    story_by_random_id: HashMap<i64, (i64, i32)>,
    next_story_id: i32,
    next_chat_id: i64,
    next_chat_photo_id: i64,
    messages: HashMap<i64, Vec<MessageSummary>>,
    message_html: HashMap<(i64, i32), String>,
    history_calls: usize,
    history_rows_returned: usize,
    list_dialogs_calls: usize,
    quick_reply_shortcuts: Vec<QuickReplyShortcutSummary>,
    quick_reply_messages: HashMap<i32, Vec<QuickReplyStoredMessageSummary>>,
    group_calls: HashMap<i64, (GroupCallInfoSummary, Vec<GroupCallParticipantSummary>)>,
    business_chat_links: BusinessChatLinksPage,
    next_business_chat_link_id: u64,
    business_chat_link_create_calls: usize,
    business_chat_link_create_uncertain_once: bool,
    business_chat_link_edit_calls: usize,
    business_chat_link_edit_uncertain_once: bool,
    business_chat_link_delete_calls: usize,
    business_chat_link_delete_uncertain_once: bool,
    business_work_hours_calls: usize,
    business_work_hours_uncertain_once: bool,
    business_location_calls: usize,
    business_location_uncertain_once: bool,
    business_greeting_calls: usize,
    business_greeting_uncertain_once: bool,
    business_away_calls: usize,
    business_away_uncertain_once: bool,
    business_intro_calls: usize,
    business_intro_uncertain_once: bool,
    stars_statuses: HashMap<String, StarsStatusPage>,
    stars_transaction_subscription_ids: HashMap<String, String>,
    available_star_gifts: AvailableStarGiftsPage,
    saved_star_gifts: HashMap<String, SavedStarGiftsPage>,
    stars_topup_options: StarsTopupOptionsPage,
    star_gift_save_calls: usize,
    star_gift_save_uncertain_once: bool,
    star_gift_convert_calls: usize,
    star_gift_convert_uncertain_once: bool,
    stars_subscription_change_calls: usize,
    stars_subscription_change_uncertain_once: bool,
    message_buttons: HashMap<(i64, i32), MessageButtonInventory>,
    saved_dialogs: Vec<SavedDialogSummary>,
    saved_histories: HashMap<i64, Vec<MessageSummary>>,
    saved_message_tags: HashMap<i32, Vec<ReactionIdentity>>,
    saved_reaction_tags: Vec<SavedReactionTagSummary>,
    typing_actions: Vec<(String, TypingAction)>,
    pressed_callback_buttons: Vec<CallbackButtonSnapshot>,
    inline_queries: HashMap<i64, Vec<InlineBotResultSummary>>,
    next_inline_query_id: i64,
    sent_inline_results: Vec<FakeInlineBotResultSend>,
    bot_commands: HashMap<String, Vec<BotCommandSummary>>,
    account_emoji_status: EmojiStatusSummary,
    emoji_status_catalogs: HashMap<EmojiStatusCatalogKind, (i64, Vec<EmojiStatusSummary>)>,
    contacts: Vec<ContactSummary>,
    chat_members: HashMap<(i64, i64), ChatMemberSummary>,
    chat_member_invite_scripts: HashMap<(i64, i64), VecDeque<FakeInviteOutcome>>,
    chat_member_invite_calls: Vec<(String, i64)>,
    invite_links: HashMap<i64, Vec<FakeInviteLink>>,
    join_requests: HashMap<i64, Vec<FakeInviteImporter>>,
    invite_joiners: HashMap<i64, Vec<FakeInviteImporter>>,
    blocked_users: HashSet<i64>,
    privacy_rules: HashMap<PrivacyKey, Vec<PrivacyRuleSummary>>,
    notification_settings: HashMap<i64, NotificationSettingsSummary>,
    chat_auto_delete: HashMap<i64, i32>,
    sessions: Vec<SessionSummary>,
    pinned_messages: HashMap<i64, i32>,
    scheduled_messages: HashMap<i64, Vec<MessageSummary>>,
    forum_topics: HashMap<i64, Vec<ForumTopicSummary>>,
    forum_topic_by_random_id: HashMap<i64, i32>,
    polls: HashMap<(i64, i32), PollSummary>,
    poll_voters: HashMap<(i64, i32), Vec<PollVoterSummary>>,
    drafts: HashMap<i64, DraftSummary>,
    dialog_read_inbox_max_ids: HashMap<i64, i32>,
    dialog_unread_reactions: HashMap<i64, i32>,
    read_receipts: HashMap<(i64, i32), Vec<MessageReadReceipt>>,
    attachments: HashMap<(i64, i32), (AttachmentDescriptor, Vec<u8>)>,
    sent: Vec<FakeSentMessage>,
    sent_by_random_id: HashMap<i64, i32>,
    my_reactions: HashMap<(i64, i32), ReactionSpec>,
    default_reaction: Option<ReactionIdentity>,
    sticker_set_installed: bool,
    sticker_set_archived: bool,
    favorite_stickers: HashSet<String>,
    custom_emoji_size_hint: Option<u64>,
    rich_media: HashMap<(i64, i32), SentRichMediaSummary>,
    rich_messages: HashMap<(i64, i32), RichMessageSnapshot>,
    document_media: HashMap<(i64, i32), SentDocumentMediaSummary>,
    message_topics: HashMap<(i64, i32), i32>,
    message_grouped_ids: HashMap<(i64, i32), i64>,
    my_boosts: Vec<BoostSlotSummary>,
    transcriptions: HashMap<(i64, i32), AudioTranscriptionSummary>,
    transcription_ratings: HashMap<(i64, i32, String), bool>,
    fact_checks: HashMap<(i64, i32), FactCheckSummary>,
    next_message_id: i32,
    translate_calls: usize,
    translate_uncertain_once: bool,
    transcription_start_calls: usize,
    transcription_poll_calls: usize,
    transcription_uncertain_once: bool,
    transcription_rate_calls: usize,
    transcription_rate_uncertain_once: bool,
    fact_check_edit_calls: usize,
    fact_check_edit_uncertain_once: bool,
    fact_check_delete_calls: usize,
    fact_check_delete_uncertain_once: bool,
    saved_history_delete_calls: usize,
    saved_history_delete_uncertain_once: bool,
    saved_dialog_pin_calls: usize,
    saved_dialog_pin_uncertain_once: bool,
    saved_reaction_tag_update_calls: usize,
    saved_reaction_tag_update_uncertain_once: bool,
    typing_action_calls: usize,
    typing_action_uncertain_once: bool,
    callback_button_press_calls: usize,
    callback_button_press_uncertain_once: bool,
    inline_query_calls: usize,
    inline_result_send_calls: usize,
    inline_result_send_uncertain_once: bool,
    bot_command_set_calls: usize,
    bot_command_set_uncertain_once: bool,
    emoji_status_set_calls: usize,
    emoji_status_set_uncertain_once: bool,
    emoji_recent_clear_calls: usize,
    emoji_recent_clear_uncertain_once: bool,
    account_color_update_calls: usize,
    account_color_update_uncertain_once: bool,
    personal_channel_update_calls: usize,
    personal_channel_update_uncertain_once: bool,
    edit_calls: usize,
    delete_calls: usize,
    pin_calls: usize,
    forward_calls: usize,
    reaction_calls: usize,
    default_reaction_calls: usize,
    default_reaction_uncertain_once: bool,
    sticker_send_calls: usize,
    sticker_send_uncertain_once: bool,
    recent_stickers: HashSet<String>,
    attached_recent_stickers: HashSet<String>,
    recent_sticker_calls: usize,
    recent_sticker_uncertain_once: bool,
    sticker_favorite_calls: usize,
    sticker_favorite_uncertain_once: bool,
    sticker_install_calls: usize,
    sticker_install_uncertain_once: bool,
    sticker_uninstall_calls: usize,
    rich_media_send_calls: usize,
    rich_media_send_uncertain_once: bool,
    voice_send_calls: usize,
    voice_send_uncertain_once: bool,
    video_note_send_calls: usize,
    video_note_send_uncertain_once: bool,
    album_send_calls: usize,
    album_send_uncertain_once: bool,
    boost_apply_calls: usize,
    boost_apply_uncertain_once: bool,
    draft_calls: usize,
    draft_clear_all_calls: usize,
    draft_clear_all_uncertain_once: bool,
    dialog_unread_calls: usize,
    mark_messages_read_calls: usize,
    mark_reactions_read_calls: usize,
    dialog_archive_calls: usize,
    dialog_pin_calls: usize,
    block_calls: usize,
    session_terminate_calls: usize,
    session_reset_calls: usize,
    session_reset_uncertain_once: bool,
    chat_auto_delete_calls: usize,
    chat_auto_delete_uncertain_once: bool,
    contact_calls: usize,
    privacy_calls: usize,
    notification_calls: usize,
    schedule_create_calls: usize,
    schedule_edit_calls: usize,
    schedule_delete_calls: usize,
    schedule_send_now_calls: usize,
    poll_create_calls: usize,
    poll_vote_calls: usize,
    poll_stop_calls: usize,
    forum_topic_create_calls: usize,
    forum_topic_edit_calls: usize,
    forum_topic_pin_calls: usize,
    forum_topic_delete_calls: usize,
    chat_member_add_calls: usize,
    chat_member_remove_calls: usize,
    chat_member_admin_calls: usize,
    chat_member_restriction_calls: usize,
    invite_export_calls: usize,
    invite_revoke_calls: usize,
    invite_delete_calls: usize,
    join_request_calls: usize,
    profile_update_calls: usize,
    username_update_calls: usize,
    birthday_update_calls: usize,
    presence_update_calls: usize,
    profile_photo_upload_calls: usize,
    profile_photo_delete_calls: usize,
    dialog_folder_upsert_calls: usize,
    dialog_folder_delete_calls: usize,
    dialog_folder_reorder_calls: usize,
    dialog_folder_membership_calls: usize,
    dialog_folder_tags_calls: usize,
    dialog_folder_tags_uncertain_once: bool,
    global_privacy_update_calls: usize,
    global_privacy_update_uncertain_once: bool,
    chat_create_calls: usize,
    chat_join_calls: usize,
    chat_leave_calls: usize,
    chat_delete_calls: usize,
    chat_title_calls: usize,
    chat_about_calls: usize,
    chat_photo_update_calls: usize,
    chat_photo_delete_calls: usize,
    chat_username_calls: usize,
    chat_slow_mode_calls: usize,
    chat_signatures_calls: usize,
    chat_pre_history_calls: usize,
    chat_join_request_calls: usize,
    chat_permissions_calls: usize,
    chat_reactions_policy_calls: usize,
    chat_anti_spam_calls: usize,
    chat_forum_mode_calls: usize,
    chat_permissions_uncertain_once: bool,
    chat_reactions_policy_uncertain_once: bool,
    chat_anti_spam_uncertain_once: bool,
    chat_forum_mode_uncertain_once: bool,
    story_view_calls: usize,
    story_read_calls: usize,
    story_reaction_calls: usize,
    story_pin_calls: usize,
    story_top_pin_calls: usize,
    story_stealth_calls: usize,
    story_send_calls: usize,
    story_edit_calls: usize,
    story_report_calls: usize,
    story_report_uncertain_once: bool,
    story_delete_calls: usize,
}

#[derive(Debug)]
pub struct FakeTelegramBackend {
    state: Mutex<FakeState>,
}

fn default_notification_settings() -> NotificationSettingsSummary {
    NotificationSettingsSummary {
        show_previews: Some(true),
        silent: Some(false),
        mute_until_unix: None,
        ios_sound: Some(NotificationSoundSummary {
            kind: "default".to_string(),
            title: None,
            data: None,
            ringtone_id: None,
        }),
        android_sound: Some(NotificationSoundSummary {
            kind: "default".to_string(),
            title: None,
            data: None,
            ringtone_id: None,
        }),
        other_sound: Some(NotificationSoundSummary {
            kind: "none".to_string(),
            title: None,
            data: None,
            ringtone_id: None,
        }),
        stories_muted: Some(false),
        stories_hide_sender: Some(false),
        stories_ios_sound: None,
        stories_android_sound: None,
        stories_other_sound: None,
    }
}

fn fixture_suggested_dialog_folders() -> SuggestedDialogFolderList {
    SuggestedDialogFolderList {
        suggestions: vec![SuggestedDialogFolderSummary {
            description: "Chats with unread messages".to_string(),
            folder: DialogFolderSummary {
                folder_id: 7,
                kind: DialogFolderKind::Custom,
                title: "Unread".to_string(),
                title_has_entities: false,
                title_noanimate: false,
                emoticon: Some("📥".to_string()),
                color: Some(1),
                contacts: true,
                non_contacts: true,
                groups: true,
                broadcasts: true,
                bots: true,
                exclude_muted: false,
                exclude_read: true,
                exclude_archived: false,
                has_my_invites: false,
                pinned_chat_ids: Vec::new(),
                included_chat_ids: Vec::new(),
                excluded_chat_ids: Vec::new(),
            },
        }],
    }
}

fn fixture_global_privacy_settings() -> GlobalPrivacySettingsSummary {
    GlobalPrivacySettingsSummary {
        archive_and_mute_new_noncontact_peers: false,
        keep_archived_unmuted: true,
        keep_archived_folders: false,
        hide_read_marks: false,
        new_noncontact_peers_require_premium: false,
        display_gifts_button: true,
        noncontact_peers_paid_stars: Some("25".to_string()),
        disallowed_gifts: Some(DisallowedGiftsSettingsSummary {
            disallow_unlimited_stargifts: false,
            disallow_limited_stargifts: true,
            disallow_unique_stargifts: false,
            disallow_premium_gifts: false,
            disallow_stargifts_from_channels: true,
        }),
    }
}

const FIXTURE_QUICK_REPLIES_HASH: i64 = 12_001;
const FIXTURE_QUICK_REPLY_MESSAGES_HASH: i64 = 12_002;
const FIXTURE_UPDATES_PTS: i32 = 120;
const FIXTURE_UPDATES_QTS: i32 = 7;
const FIXTURE_UPDATES_DATE: i32 = 1_700_004_000;
const FIXTURE_UPDATES_SEQ: i32 = 10;
const FIXTURE_CHANNEL_UPDATES_PTS: i32 = 50;

fn fixture_quick_reply_shortcuts() -> Vec<QuickReplyShortcutSummary> {
    vec![QuickReplyShortcutSummary {
        shortcut_id: 7,
        shortcut: "greeting".to_string(),
        top_message_id: 901,
        count: 2,
    }]
}

fn fixture_quick_reply_messages() -> HashMap<i32, Vec<QuickReplyStoredMessageSummary>> {
    HashMap::from([(
        7,
        vec![
            QuickReplyStoredMessageSummary {
                message: MessageSummary {
                    message_id: 901,
                    chat_ref: Some("7".to_string()),
                    sender_ref: Some("7".to_string()),
                    outgoing: true,
                    pinned: false,
                    silent: false,
                    noforwards: false,
                    text: Some("Hello — how can I help?".to_string()),
                    date: 1_700_002_901,
                    edit_date: Some(1_700_002_902),
                    reply_to_message_id: None,
                    views: None,
                    forwards: None,
                    reply_count: None,
                    media_type: None,
                    forward_origin: None,
                },
                is_service: false,
                service_action_kind: None,
                quick_reply_shortcut_id: Some(7),
            },
            QuickReplyStoredMessageSummary {
                message: MessageSummary {
                    message_id: 900,
                    chat_ref: Some("7".to_string()),
                    sender_ref: Some("7".to_string()),
                    outgoing: true,
                    pinned: false,
                    silent: true,
                    noforwards: false,
                    text: None,
                    date: 1_700_002_900,
                    edit_date: None,
                    reply_to_message_id: Some(901),
                    views: None,
                    forwards: None,
                    reply_count: None,
                    media_type: None,
                    forward_origin: None,
                },
                is_service: true,
                service_action_kind: Some("history_clear".to_string()),
                quick_reply_shortcut_id: None,
            },
        ],
    )])
}

fn fixture_group_calls() -> HashMap<i64, (GroupCallInfoSummary, Vec<GroupCallParticipantSummary>)> {
    HashMap::from([(
        505,
        (
            GroupCallInfoSummary::Active {
                call_id: "1001".to_string(),
                participants_count: 3,
                title: Some("Release stand-up".to_string()),
                stream_dc_id: Some(2),
                record_start_date_unix: Some(1_700_003_000),
                schedule_date_unix: None,
                unmuted_video_count: Some(1),
                unmuted_video_limit: 30,
                version: 7,
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
                has_invite_link: true,
                send_paid_messages_stars: Some("25".to_string()),
                default_send_as: Some("-100505".to_string()),
            },
            vec![
                GroupCallParticipantSummary {
                    peer_ref: "101".to_string(),
                    date_unix: 1_700_003_001,
                    active_date_unix: Some(1_700_003_002),
                    source: 111,
                    volume: Some(10_000),
                    muted: true,
                    left: false,
                    can_self_unmute: true,
                    just_joined: false,
                    versioned: true,
                    min: false,
                    self_participant: false,
                    muted_by_you: false,
                    volume_by_admin: true,
                    video_joined: true,
                    about: Some("Release owner".to_string()),
                    raise_hand_rating: Some("42".to_string()),
                    video: Some(GroupCallParticipantVideoSummary {
                        paused: false,
                        endpoint: "video:101".to_string(),
                        source_groups: vec![GroupCallParticipantVideoSourceGroupSummary {
                            semantics: "SIM".to_string(),
                            sources: vec![111, 112],
                        }],
                        audio_source: Some(113),
                    }),
                    presentation: None,
                    paid_stars_total: Some("5".to_string()),
                },
                GroupCallParticipantSummary {
                    peer_ref: "303".to_string(),
                    date_unix: 1_700_003_003,
                    active_date_unix: None,
                    source: 222,
                    volume: None,
                    muted: false,
                    left: false,
                    can_self_unmute: false,
                    just_joined: true,
                    versioned: false,
                    min: false,
                    self_participant: true,
                    muted_by_you: false,
                    volume_by_admin: false,
                    video_joined: false,
                    about: None,
                    raise_hand_rating: None,
                    video: None,
                    presentation: Some(GroupCallParticipantVideoSummary {
                        paused: true,
                        endpoint: "presentation:303".to_string(),
                        source_groups: Vec::new(),
                        audio_source: None,
                    }),
                    paid_stars_total: None,
                },
            ],
        ),
    )])
}

fn fixture_stars_statuses(
    contacts: &[ContactSummary],
    known_chats: &HashMap<i64, ChatDetailsSummary>,
) -> (HashMap<String, StarsStatusPage>, HashMap<String, String>) {
    let related_chats =
        known_chats.get(&505).map(|details| vec![details.chat.clone()]).unwrap_or_default();
    let history = vec![
        StarsTransactionSummary {
            transaction_id: "stars-in-gift".to_string(),
            amount: StarsAmountSummary {
                kind: StarsAmountKind::Stars,
                amount: "50".to_string(),
                nanos: Some(500_000_000),
            },
            date_unix: 1_700_004_001,
            peer: StarsTransactionPeerSummary::Peer { peer_ref: "101".to_string() },
            refund: false,
            pending: false,
            failed: false,
            gift: true,
            reaction: false,
            star_gift_upgrade: false,
            business_transfer: false,
            star_gift_resale: false,
            posts_search: false,
            star_gift_prepaid_upgrade: false,
            star_gift_drop_original_details: false,
            phone_group_message: false,
            star_gift_auction_bid: false,
            offer: false,
            title: Some("Launch gift".to_string()),
            description: Some("Fixture inbound gift".to_string()),
            has_photo: true,
            transaction_date_unix: None,
            transaction_url: None,
            bot_payload_sha256: None,
            bot_payload_length: None,
            message_id: Some(11),
            extended_media_types: None,
            subscription_period_seconds: None,
            giveaway_post_id: None,
            star_gift: Some(StarsTransactionGiftSummary::Regular {
                gift_id: "9001".to_string(),
                title: Some("Launch Star".to_string()),
            }),
            floodskip_number: None,
            star_ref_commission_permille: None,
            star_ref_peer_ref: None,
            star_ref_amount: None,
            paid_messages: None,
            premium_gift_months: None,
            ads_proceeds_from_date_unix: None,
            ads_proceeds_to_date_unix: None,
        },
        StarsTransactionSummary {
            transaction_id: "stars-out-reaction".to_string(),
            amount: StarsAmountSummary {
                kind: StarsAmountKind::Stars,
                amount: "-10".to_string(),
                nanos: Some(0),
            },
            date_unix: 1_700_004_002,
            peer: StarsTransactionPeerSummary::PremiumBot,
            refund: false,
            pending: false,
            failed: false,
            gift: false,
            reaction: true,
            star_gift_upgrade: false,
            business_transfer: false,
            star_gift_resale: false,
            posts_search: false,
            star_gift_prepaid_upgrade: false,
            star_gift_drop_original_details: false,
            phone_group_message: false,
            star_gift_auction_bid: false,
            offer: false,
            title: Some("Paid reaction".to_string()),
            description: None,
            has_photo: false,
            transaction_date_unix: Some(1_700_004_003),
            transaction_url: Some("https://example.invalid/untrusted-receipt".to_string()),
            bot_payload_sha256: Some(format!("{:x}", Sha256::digest(b"fixture-payload"))),
            bot_payload_length: Some(b"fixture-payload".len()),
            message_id: Some(50),
            extended_media_types: Some(vec!["photo".to_string()]),
            subscription_period_seconds: None,
            giveaway_post_id: None,
            star_gift: None,
            floodskip_number: None,
            star_ref_commission_permille: Some(125),
            star_ref_peer_ref: Some("-100505".to_string()),
            star_ref_amount: Some(StarsAmountSummary {
                kind: StarsAmountKind::Stars,
                amount: "1".to_string(),
                nanos: Some(250_000_000),
            }),
            paid_messages: Some(1),
            premium_gift_months: None,
            ads_proceeds_from_date_unix: None,
            ads_proceeds_to_date_unix: None,
        },
        StarsTransactionSummary {
            transaction_id: "stars-subscription".to_string(),
            amount: StarsAmountSummary {
                kind: StarsAmountKind::Stars,
                amount: "-25".to_string(),
                nanos: Some(0),
            },
            date_unix: 1_700_004_004,
            peer: StarsTransactionPeerSummary::Peer { peer_ref: "303".to_string() },
            refund: false,
            pending: true,
            failed: false,
            gift: false,
            reaction: false,
            star_gift_upgrade: false,
            business_transfer: false,
            star_gift_resale: false,
            posts_search: false,
            star_gift_prepaid_upgrade: false,
            star_gift_drop_original_details: false,
            phone_group_message: false,
            star_gift_auction_bid: false,
            offer: false,
            title: Some("Fixture subscription".to_string()),
            description: None,
            has_photo: false,
            transaction_date_unix: None,
            transaction_url: None,
            bot_payload_sha256: None,
            bot_payload_length: None,
            message_id: None,
            extended_media_types: None,
            subscription_period_seconds: Some(2_592_000),
            giveaway_post_id: None,
            star_gift: None,
            floodskip_number: None,
            star_ref_commission_permille: None,
            star_ref_peer_ref: None,
            star_ref_amount: None,
            paid_messages: None,
            premium_gift_months: None,
            ads_proceeds_from_date_unix: None,
            ads_proceeds_to_date_unix: None,
        },
    ];
    let self_status = StarsStatusPage {
        peer_ref: "7".to_string(),
        balance: StarsAmountSummary {
            kind: StarsAmountKind::Stars,
            amount: "125".to_string(),
            nanos: Some(500_000_000),
        },
        subscriptions: vec![
            StarsSubscriptionSummary {
                subscription_id: "sub-fixture".to_string(),
                peer_ref: "303".to_string(),
                until_date_unix: 1_800_000_000,
                pricing: StarsSubscriptionPricingSummary {
                    period_seconds: 2_592_000,
                    amount_stars: "25".to_string(),
                },
                canceled: false,
                can_refulfill: true,
                missing_balance: true,
                bot_canceled: false,
                has_chat_invite_hash: true,
                title: Some("Fixture Pro".to_string()),
                has_photo: true,
                invoice_slug: Some("fixture-pro".to_string()),
            },
            StarsSubscriptionSummary {
                subscription_id: "sub-archive".to_string(),
                peer_ref: "101".to_string(),
                until_date_unix: 1_790_000_000,
                pricing: StarsSubscriptionPricingSummary {
                    period_seconds: 604_800,
                    amount_stars: "5".to_string(),
                },
                canceled: true,
                can_refulfill: false,
                missing_balance: false,
                bot_canceled: true,
                has_chat_invite_hash: false,
                title: Some("Fixture Archive".to_string()),
                has_photo: false,
                invoice_slug: None,
            },
        ],
        subscriptions_next_offset: Some("subscriptions:1".to_string()),
        subscriptions_missing_balance: Some("25".to_string()),
        history,
        next_offset: Some("3".to_string()),
        related_chats: related_chats.clone(),
        related_users: contacts.to_vec(),
    };
    let channel_status = StarsStatusPage {
        peer_ref: "-100505".to_string(),
        balance: StarsAmountSummary {
            kind: StarsAmountKind::Stars,
            amount: "500".to_string(),
            nanos: Some(0),
        },
        subscriptions: Vec::new(),
        subscriptions_next_offset: None,
        subscriptions_missing_balance: None,
        history: vec![self_status.history[0].clone()],
        next_offset: None,
        related_chats,
        related_users: contacts.to_vec(),
    };
    (
        HashMap::from([
            (self_status.peer_ref.clone(), self_status),
            (channel_status.peer_ref.clone(), channel_status),
        ]),
        HashMap::from([("stars-subscription".to_string(), "sub-fixture".to_string())]),
    )
}

fn fixture_regular_star_gift(
    gift_id: i64,
    stars: i64,
    limited: bool,
    peer_color_available: bool,
    title: &str,
) -> StarGiftSummary {
    StarGiftSummary::Regular {
        gift_id: gift_id.to_string(),
        sticker_document_id: (gift_id + 10_000).to_string(),
        stars: stars.to_string(),
        convert_stars: (stars.saturating_sub(8)).to_string(),
        limited,
        sold_out: false,
        birthday: gift_id == 9_002,
        require_premium: false,
        limited_per_user: limited,
        peer_color_available,
        auction: false,
        availability_remains: limited.then_some(12),
        availability_total: limited.then_some(100),
        availability_resale: limited.then(|| "7".to_string()),
        first_sale_date_unix: limited.then_some(1_700_005_000),
        last_sale_date_unix: None,
        upgrade_stars: Some("20".to_string()),
        resell_min_stars: limited.then(|| "60".to_string()),
        title: Some(title.to_string()),
        released_by_peer_ref: Some("-100505".to_string()),
        per_user_total: limited.then_some(2),
        per_user_remains: limited.then_some(1),
        locked_until_date_unix: None,
        auction_slug: None,
        gifts_per_round: None,
        auction_start_date_unix: None,
        upgrade_variants: Some(3),
        has_background: peer_color_available,
    }
}

fn fixture_unique_star_gift() -> StarGiftSummary {
    StarGiftSummary::Unique {
        unique_gift_id: "9901".to_string(),
        gift_id: "9001".to_string(),
        title: "Aurora".to_string(),
        slug: "aurora-42".to_string(),
        number: 42,
        require_premium: false,
        resale_ton_only: false,
        theme_available: true,
        burned: false,
        crafted: true,
        owner_peer_ref: Some("7".to_string()),
        owner_name: Some("Fixture Owner".to_string()),
        has_owner_address: true,
        attributes: vec![
            StarGiftAttributeSummary::Model {
                name: "Comet".to_string(),
                document_id: "19901".to_string(),
                crafted: true,
                rarity: StarGiftRaritySummary::Rare,
            },
            StarGiftAttributeSummary::Backdrop {
                name: "Night".to_string(),
                backdrop_id: 7,
                center_color: 0x112233,
                edge_color: 0x223344,
                pattern_color: 0x334455,
                text_color: 0x445566,
                rarity: StarGiftRaritySummary::Exact { permille: 25 },
            },
            StarGiftAttributeSummary::OriginalDetails {
                sender_peer_ref: Some("101".to_string()),
                recipient_peer_ref: "7".to_string(),
                date_unix: 1_700_005_002,
                message: Some("For the release".to_string()),
                message_has_entities: true,
            },
        ],
        availability_issued: 10,
        availability_total: 100,
        has_gift_address: true,
        resell_amounts: vec![StarsAmountSummary {
            kind: StarsAmountKind::Stars,
            amount: "80".to_string(),
            nanos: Some(0),
        }],
        released_by_peer_ref: Some("-100505".to_string()),
        value_amount: Some("1499".to_string()),
        value_currency: Some("USD".to_string()),
        value_usd_amount: Some("1499".to_string()),
        theme_peer_ref: Some("-100505".to_string()),
        has_peer_color: true,
        host_peer_ref: Some("-100505".to_string()),
        offer_min_stars: Some("75".to_string()),
        craft_chance_permille: Some(125),
    }
}

fn fake_star_gift_value(gift: &StarGiftSummary) -> i128 {
    match gift {
        StarGiftSummary::Regular { stars, .. } => stars.parse::<i128>().unwrap_or_default(),
        StarGiftSummary::Unique { value_amount, offer_min_stars, .. } => value_amount
            .as_deref()
            .or(offer_min_stars.as_deref())
            .and_then(|value| value.parse::<i128>().ok())
            .unwrap_or_default(),
    }
}

fn fixture_star_gift_catalogs(
    contacts: &[ContactSummary],
    known_chats: &HashMap<i64, ChatDetailsSummary>,
) -> (AvailableStarGiftsPage, HashMap<String, SavedStarGiftsPage>, StarsTopupOptionsPage) {
    let limited = fixture_regular_star_gift(9_001, 50, true, true, "Launch Star");
    let unlimited = fixture_regular_star_gift(9_002, 25, false, false, "Birthday Star");
    let unique = fixture_unique_star_gift();
    let related_chats =
        known_chats.get(&505).map(|details| vec![details.chat.clone()]).unwrap_or_default();
    let related_users = contacts.to_vec();
    let available = AvailableStarGiftsPage {
        not_modified: false,
        hash: Some(9_900),
        gifts: vec![limited.clone(), unlimited.clone(), unique.clone()],
        related_chats: related_chats.clone(),
        related_users: related_users.clone(),
    };
    let saved = SavedStarGiftsPage {
        peer_ref: "7".to_string(),
        count: 3,
        chat_notifications_enabled: Some(true),
        gifts: vec![
            SavedStarGiftSummary {
                gift: limited.clone(),
                name_hidden: false,
                unsaved: false,
                refunded: false,
                can_upgrade: true,
                pinned_to_top: true,
                upgrade_separate: true,
                from_peer_ref: Some("101".to_string()),
                date_unix: 1_700_005_001,
                message: Some("Launch gift".to_string()),
                message_has_entities: true,
                message_id: Some(31),
                saved_id: Some("70001".to_string()),
                convert_stars: Some("42".to_string()),
                upgrade_stars: Some("20".to_string()),
                can_export_at_unix: Some(1_800_005_001),
                transfer_stars: Some("5".to_string()),
                can_transfer_at_unix: Some(1_700_105_001),
                can_resell_at_unix: Some(1_700_205_001),
                collection_ids: vec![7],
                has_prepaid_upgrade_hash: true,
                prepaid_upgrade_hash_sha256: Some(format!(
                    "{:x}",
                    Sha256::digest(b"fixture-prepaid-upgrade")
                )),
                prepaid_upgrade_hash_length: Some(b"fixture-prepaid-upgrade".len()),
                drop_original_details_stars: Some("3".to_string()),
                gift_number: Some(17),
                can_craft_at_unix: Some(1_700_305_001),
            },
            SavedStarGiftSummary {
                gift: unique,
                name_hidden: true,
                unsaved: true,
                refunded: false,
                can_upgrade: false,
                pinned_to_top: false,
                upgrade_separate: false,
                from_peer_ref: Some("303".to_string()),
                date_unix: 1_700_005_002,
                message: None,
                message_has_entities: false,
                message_id: Some(32),
                saved_id: Some("70002".to_string()),
                convert_stars: None,
                upgrade_stars: None,
                can_export_at_unix: None,
                transfer_stars: Some("9".to_string()),
                can_transfer_at_unix: None,
                can_resell_at_unix: Some(1_700_205_002),
                collection_ids: vec![7, 8],
                has_prepaid_upgrade_hash: false,
                prepaid_upgrade_hash_sha256: None,
                prepaid_upgrade_hash_length: None,
                drop_original_details_stars: None,
                gift_number: Some(42),
                can_craft_at_unix: None,
            },
            SavedStarGiftSummary {
                gift: unlimited.clone(),
                name_hidden: false,
                unsaved: false,
                refunded: true,
                can_upgrade: false,
                pinned_to_top: false,
                upgrade_separate: false,
                from_peer_ref: None,
                date_unix: 1_700_005_003,
                message: Some("Birthday gift".to_string()),
                message_has_entities: false,
                message_id: Some(33),
                saved_id: Some("70003".to_string()),
                convert_stars: Some("17".to_string()),
                upgrade_stars: Some("20".to_string()),
                can_export_at_unix: None,
                transfer_stars: None,
                can_transfer_at_unix: None,
                can_resell_at_unix: None,
                collection_ids: Vec::new(),
                has_prepaid_upgrade_hash: false,
                prepaid_upgrade_hash_sha256: None,
                prepaid_upgrade_hash_length: None,
                drop_original_details_stars: None,
                gift_number: None,
                can_craft_at_unix: None,
            },
        ],
        next_offset: None,
        related_chats: related_chats.clone(),
        related_users: related_users.clone(),
    };
    let channel_saved = SavedStarGiftsPage {
        peer_ref: "-100505".to_string(),
        count: 1,
        chat_notifications_enabled: Some(false),
        gifts: vec![SavedStarGiftSummary {
            gift: unlimited,
            name_hidden: false,
            unsaved: false,
            refunded: false,
            can_upgrade: false,
            pinned_to_top: false,
            upgrade_separate: false,
            from_peer_ref: Some("7".to_string()),
            date_unix: 1_700_005_004,
            message: None,
            message_has_entities: false,
            message_id: None,
            saved_id: Some("80001".to_string()),
            convert_stars: Some("17".to_string()),
            upgrade_stars: Some("20".to_string()),
            can_export_at_unix: None,
            transfer_stars: None,
            can_transfer_at_unix: None,
            can_resell_at_unix: None,
            collection_ids: Vec::new(),
            has_prepaid_upgrade_hash: false,
            prepaid_upgrade_hash_sha256: None,
            prepaid_upgrade_hash_length: None,
            drop_original_details_stars: None,
            gift_number: None,
            can_craft_at_unix: None,
        }],
        next_offset: None,
        related_chats,
        related_users,
    };
    let topup = StarsTopupOptionsPage {
        options: vec![
            StarsTopupOptionSummary {
                stars: "100".to_string(),
                store_product: Some("stars.100".to_string()),
                currency: "USD".to_string(),
                amount: "199".to_string(),
                extended: false,
            },
            StarsTopupOptionSummary {
                stars: "500".to_string(),
                store_product: None,
                currency: "USD".to_string(),
                amount: "799".to_string(),
                extended: true,
            },
        ],
    };
    (
        available,
        HashMap::from([
            (saved.peer_ref.clone(), saved),
            (channel_saved.peer_ref.clone(), channel_saved),
        ]),
        topup,
    )
}

fn fixture_sticker_set_summary() -> StickerSetSummary {
    StickerSetSummary {
        sticker_set_id: 9001,
        title: "Fixture reactions".to_string(),
        short_name: "fixture_reactions".to_string(),
        installed_date_unix: Some(1_700_000_000),
        archived: false,
        official: false,
        masks: false,
        emojis: false,
        text_color: false,
        channel_emoji_status: false,
        creator: true,
        thumb_dc_id: Some(2),
        thumb_version: Some(1),
        thumb_document_id: Some(9002),
        sticker_count: 2,
        content_hash: 4242,
    }
}

fn fixture_stickers() -> Vec<StickerDocumentSummary> {
    vec![
        StickerDocumentSummary {
            sticker_handle: opaque_sticker_handle(9_101),
            set_short_name: Some("fixture_reactions".to_string()),
            emoji: Some("👍".to_string()),
            mime_type: "image/webp".to_string(),
            size_bytes: 1_024,
            date_unix: 1_700_000_100,
            recent_date_unix: None,
            animated: false,
            video: false,
            custom_emoji: false,
        },
        StickerDocumentSummary {
            sticker_handle: opaque_sticker_handle(9_102),
            set_short_name: Some("fixture_reactions".to_string()),
            emoji: Some("🔥".to_string()),
            mime_type: "application/x-tgsticker".to_string(),
            size_bytes: 2_048,
            date_unix: 1_700_000_101,
            recent_date_unix: None,
            animated: true,
            video: false,
            custom_emoji: false,
        },
    ]
}

const FIXTURE_CUSTOM_EMOJI_7001_BYTES: &[u8] = b"fixture custom emoji 7001 tgs\n";
const FIXTURE_CUSTOM_EMOJI_8001_BYTES: &[u8] = b"fixture custom emoji 8001 webm\n";

fn fixture_custom_emoji(document_id: i64) -> Option<CustomEmojiDocumentSummary> {
    let (emoji, mime_type, bytes, animated, video) = match document_id {
        7_001 => ("✨", "application/x-tgsticker", FIXTURE_CUSTOM_EMOJI_7001_BYTES, true, false),
        8_001 => ("🚀", "video/webm", FIXTURE_CUSTOM_EMOJI_8001_BYTES, false, true),
        _ => return None,
    };
    Some(CustomEmojiDocumentSummary {
        document_id,
        sticker: StickerDocumentSummary {
            sticker_handle: opaque_sticker_handle(document_id),
            set_short_name: Some("fixture_custom_emoji".to_string()),
            emoji: Some(emoji.to_string()),
            mime_type: mime_type.to_string(),
            size_bytes: u64::try_from(bytes.len()).unwrap_or(u64::MAX),
            date_unix: 1_700_000_200 + i32::try_from(document_id % 100).unwrap_or_default(),
            recent_date_unix: None,
            animated,
            video,
            custom_emoji: true,
        },
    })
}

fn fixture_custom_emoji_bytes(document_id: i64) -> Option<&'static [u8]> {
    match document_id {
        7_001 => Some(FIXTURE_CUSTOM_EMOJI_7001_BYTES),
        8_001 => Some(FIXTURE_CUSTOM_EMOJI_8001_BYTES),
        _ => None,
    }
}

fn fixture_sticker_set_details() -> StickerSetDetailsPage {
    StickerSetDetailsPage {
        not_modified: false,
        sticker_set: Some(fixture_sticker_set_summary()),
        packs: vec![
            StickerEmojiPackSummary {
                emoji: "👍".to_string(),
                sticker_handles: vec![opaque_sticker_handle(9_101)],
            },
            StickerEmojiPackSummary {
                emoji: "🔥".to_string(),
                sticker_handles: vec![opaque_sticker_handle(9_102)],
            },
        ],
        keywords: vec![StickerKeywordSummary {
            sticker_handle: opaque_sticker_handle(9_102),
            keywords: vec!["fire".to_string(), "release".to_string()],
        }],
        stickers: fixture_stickers(),
    }
}

fn finalize_fixture_draft(mut draft: DraftSummary) -> DraftSummary {
    draft.state_sha256.clear();
    let bytes = serde_json::to_vec(&draft).expect("fixture draft must serialize");
    draft.state_sha256 = format!("{:x}", Sha256::digest(bytes));
    draft
}

fn fixture_forum_topic() -> ForumTopicSummary {
    ForumTopicSummary {
        topic_id: 44,
        deleted: false,
        title: Some("Release coordination".to_string()),
        date_unix: Some(1_700_000_000),
        creator_ref: Some("7".to_string()),
        my: true,
        closed: false,
        pinned: true,
        short: false,
        hidden: false,
        title_missing: false,
        icon_color: Some(0x006f_bad0),
        icon_emoji_id: None,
        top_message_id: Some(21),
        read_inbox_max_id: Some(21),
        read_outbox_max_id: Some(21),
        unread_count: Some(0),
        unread_mentions_count: Some(0),
        unread_reactions_count: Some(0),
        unread_poll_votes_count: Some(0),
        notification_settings: Some(default_notification_settings()),
        draft: Some(finalize_fixture_draft(DraftSummary {
            message: "Ship after the final E2E pass".to_string(),
            date_unix: 1_700_000_022,
            no_webpage: false,
            invert_media: false,
            has_reply_to: false,
            has_entities: false,
            has_media: false,
            effect_id: None,
            has_suggested_post: false,
            has_rich_message: false,
            state_sha256: String::new(),
        })),
    }
}

fn fixture_story(story_id: i32, caption: &str, views_count: i32) -> StorySummary {
    StorySummary {
        state: "active".to_string(),
        story_id,
        date_unix: 1_700_000_200 + story_id,
        expire_date_unix: 1_700_086_600 + story_id,
        from_ref: Some("7".to_string()),
        caption: Some(caption.to_string()),
        pinned: story_id == 41,
        public: true,
        close_friends: false,
        noforwards: false,
        edited: false,
        contacts: false,
        selected_contacts: false,
        outgoing: true,
        live: false,
        media_kind: Some("photo".to_string()),
        views_count: Some(views_count),
        forwards_count: Some(2),
        reactions_count: Some(1),
        sent_reaction: None,
    }
}

fn fixture_account_contact() -> ContactSummary {
    ContactSummary {
        user_id: 7,
        first_name: "Fixture".to_string(),
        last_name: None,
        display_name: "Fixture".to_string(),
        username: Some("fixture_account".to_string()),
        phone: None,
        is_contact: false,
        mutual_contact: false,
        bot: false,
        verified: false,
        premium: false,
        scam: false,
        restricted: false,
    }
}

fn fixture_admin_rights() -> ChatAdminRightsSummary {
    ChatAdminRightsSummary {
        change_info: true,
        post_messages: true,
        edit_messages: true,
        delete_messages: true,
        ban_users: true,
        invite_users: true,
        pin_messages: true,
        add_admins: true,
        anonymous: false,
        manage_call: true,
        other: true,
        manage_topics: true,
        post_stories: true,
        edit_stories: true,
        delete_stories: true,
        manage_direct_messages: true,
        manage_ranks: true,
        manage_linked_peers: true,
    }
}

fn fixture_invite_link(
    raw_link: &str,
    revoked: bool,
    title: &str,
    admin_user_id: i64,
) -> FakeInviteLink {
    FakeInviteLink {
        _raw_link: raw_link.to_string(),
        summary: InviteLinkSummary {
            invite_handle: opaque_invite_handle(raw_link),
            kind: InviteLinkKind::Exported,
            revoked,
            permanent: false,
            request_needed: true,
            admin_user_id: Some(admin_user_id),
            date_unix: Some(1_700_000_050),
            start_date_unix: None,
            expire_date_unix: Some(1_900_000_000),
            usage_limit: None,
            usage_count: Some(if revoked { 2 } else { 1 }),
            requested_count: Some(0),
            subscription_expired_count: None,
            title: Some(title.to_string()),
            subscription_pricing: None,
        },
    }
}

fn fixture_invite_importer(
    user: ContactSummary,
    invite_handle: Option<String>,
    requested: bool,
    date_unix: i32,
    about: Option<&str>,
    approved_by_user_id: Option<i64>,
) -> FakeInviteImporter {
    FakeInviteImporter {
        invite_handle,
        summary: InviteJoinerSummary {
            user_id: user.user_id,
            user: Some(user),
            requested,
            via_chatlist: false,
            date_unix,
            about: about.map(str::to_string),
            approved_by_user_id,
            source_invite_handle: None,
        },
    }
}

fn fixture_chat_member(
    chat_id: &str,
    user: ContactSummary,
    role: ChatMemberRole,
    admin_rights: Option<ChatAdminRightsSummary>,
    rank: Option<&str>,
) -> ChatMemberSummary {
    ChatMemberSummary {
        chat_id: chat_id.to_string(),
        user,
        role,
        can_edit: true,
        is_self: false,
        left: false,
        joined_date_unix: Some(1_700_000_100),
        inviter_user_id: Some(7),
        promoted_by_user_id: admin_rights.as_ref().map(|_| 7),
        kicked_by_user_id: None,
        rank: rank.map(str::to_string),
        subscription_until_date_unix: None,
        via_request: false,
        admin_rights,
        banned_rights: None,
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

fn fixture_poll() -> PollSummary {
    PollSummary {
        chat_id: "-202".to_string(),
        message_id: 20,
        poll_id: 9_001,
        closed: false,
        public_voters: true,
        multiple_choice: false,
        quiz: false,
        open_answers: false,
        revoting_disabled: false,
        shuffle_answers: false,
        hide_results_until_close: false,
        creator: true,
        subscribers_only: false,
        question: "Which release path should we use?".to_string(),
        answers: vec![
            PollAnswerSummary {
                option: PollOptionBinding {
                    index: 0,
                    text: "Blue-green".to_string(),
                    option_hex: "00".to_string(),
                },
                media_type: None,
                added_by_ref: None,
                date_unix: None,
                chosen: false,
                correct: false,
                voters: Some(1),
                recent_voter_refs: vec!["101".to_string()],
            },
            PollAnswerSummary {
                option: PollOptionBinding {
                    index: 1,
                    text: "In-place".to_string(),
                    option_hex: "01".to_string(),
                },
                media_type: None,
                added_by_ref: None,
                date_unix: None,
                chosen: false,
                correct: false,
                voters: Some(1),
                recent_voter_refs: vec!["303".to_string()],
            },
            PollAnswerSummary {
                option: PollOptionBinding {
                    index: 2,
                    text: "Postpone".to_string(),
                    option_hex: "02".to_string(),
                },
                media_type: None,
                added_by_ref: None,
                date_unix: None,
                chosen: false,
                correct: false,
                voters: Some(0),
                recent_voter_refs: Vec::new(),
            },
        ],
        unmatched_result_option_hex: Vec::new(),
        close_period_seconds: None,
        close_date_unix: None,
        countries_iso2: Vec::new(),
        poll_hash: 9_001,
        results_minimal: false,
        has_unread_votes: false,
        can_view_stats: true,
        total_voters: Some(2),
        recent_voter_refs: vec!["101".to_string(), "303".to_string()],
        solution: None,
        solution_has_media: false,
        attached_media_type: None,
    }
}

fn hex_encode(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn fixture_message_button(
    row: usize,
    column: usize,
    index: usize,
    kind: MessageButtonKind,
    label: &str,
) -> MessageButtonSummary {
    MessageButtonSummary {
        row,
        column,
        index,
        kind,
        label: label.to_string(),
        style: None,
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

fn fixture_message_buttons() -> MessageButtonInventory {
    let callback_data = b"approve:contract:11";
    let secure_callback_data = b"secure:contract:11";
    let mut approve = fixture_message_button(0, 0, 0, MessageButtonKind::Callback, "Approve");
    approve.callback_data_base64 = Some(BASE64_STANDARD.encode(callback_data));
    approve.callback_data_sha256 = Some(format!("{:x}", Sha256::digest(callback_data)));
    approve.callback_data_length = Some(callback_data.len());

    let mut terms = fixture_message_button(0, 1, 1, MessageButtonKind::Url, "Open terms");
    terms.url = Some("https://example.com/terms".to_string());

    let mut secure = fixture_message_button(1, 0, 2, MessageButtonKind::Callback, "Secure confirm");
    secure.callback_data_base64 = Some(BASE64_STANDARD.encode(secure_callback_data));
    secure.callback_data_sha256 = Some(format!("{:x}", Sha256::digest(secure_callback_data)));
    secure.callback_data_length = Some(secure_callback_data.len());
    secure.requires_password = true;

    let mut share =
        fixture_message_button(1, 1, 3, MessageButtonKind::SwitchInline, "Share inline");
    share.switch_query = Some("contract 11".to_string());
    share.same_peer = Some(true);
    share.allowed_peer_types = Some(vec!["pm".to_string(), "megagroup".to_string()]);

    let mut profile =
        fixture_message_button(2, 0, 4, MessageButtonKind::UserProfile, "Alex profile");
    profile.user_id = Some("101".to_string());

    let mut copy = fixture_message_button(2, 1, 5, MessageButtonKind::Copy, "Copy code");
    copy.copy_text = Some("CONTRACT-11".to_string());

    MessageButtonInventory {
        chat_id: "101".to_string(),
        message_id: 11,
        message_date_unix: 1_700_000_011,
        markup_type: MessageButtonMarkupType::InlineKeyboard,
        buttons: vec![approve, terms, secure, share, profile, copy],
    }
}

fn empty_emoji_status() -> EmojiStatusSummary {
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

fn fixture_default_emoji_status(document_id: i64) -> EmojiStatusSummary {
    EmojiStatusSummary {
        kind: EmojiStatusKind::Default,
        document_id: Some(document_id.to_string()),
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

fn fixture_collectible_emoji_status(collectible_id: i64, document_id: i64) -> EmojiStatusSummary {
    EmojiStatusSummary {
        kind: EmojiStatusKind::Collectible,
        document_id: Some(document_id.to_string()),
        collectible_id: Some(collectible_id.to_string()),
        until_unix: None,
        title: Some(format!("Fixture collectible {collectible_id}")),
        slug: Some(format!("fixture-{collectible_id}")),
        pattern_document_id: Some(document_id.saturating_add(10_000).to_string()),
        center_color: Some(0x123456),
        edge_color: Some(0x234567),
        pattern_color: Some(0x345678),
        text_color: Some(0x456789),
    }
}

impl Default for FakeTelegramBackend {
    fn default() -> Self {
        Self::fixture()
    }
}

impl FakeTelegramBackend {
    pub fn fixture() -> Self {
        let dialogs = vec![
            DialogSummary {
                chat_id: 101,
                canonical_chat_id: "101".to_string(),
                chat_ref: "@alex".to_string(),
                chat_kind: "user".to_string(),
                title: "Alex".to_string(),
                username: Some("alex".to_string()),
                last_message_id: Some(11),
                last_message_text: Some("Please check contract.pdf".to_string()),
                last_message_outgoing: Some(false),
                last_message_date: Some(1_700_000_011),
                pinned: false,
                unread_mark: true,
                unread_count: 2,
                archived: false,
                folder_id: Some(0),
            },
            DialogSummary {
                chat_id: 202,
                canonical_chat_id: "-202".to_string(),
                chat_ref: "-202".to_string(),
                chat_kind: "group".to_string(),
                title: "Project Room".to_string(),
                username: None,
                last_message_id: Some(22),
                last_message_text: Some("Topic follow-up".to_string()),
                last_message_outgoing: Some(false),
                last_message_date: Some(1_700_000_022),
                pinned: true,
                unread_mark: false,
                unread_count: 0,
                archived: false,
                folder_id: Some(0),
            },
            DialogSummary {
                chat_id: 505,
                canonical_chat_id: "-100505".to_string(),
                chat_ref: "-100505".to_string(),
                chat_kind: "channel".to_string(),
                title: "Ops Supergroup".to_string(),
                username: None,
                last_message_id: Some(50),
                last_message_text: Some("Ops announcement".to_string()),
                last_message_outgoing: Some(true),
                last_message_date: Some(1_700_000_050),
                pinned: false,
                unread_mark: false,
                unread_count: 0,
                archived: false,
                folder_id: Some(0),
            },
        ];
        let messages = HashMap::from([
            (
                101,
                vec![
                    MessageSummary {
                        message_id: 11,
                        chat_ref: Some("101".to_string()),
                        sender_ref: Some("101".to_string()),
                        outgoing: false,
                        pinned: false,
                        silent: false,
                        noforwards: false,
                        text: Some("Please check contract.pdf".to_string()),
                        date: 1_700_000_011,
                        edit_date: None,
                        reply_to_message_id: Some(10),
                        views: None,
                        forwards: None,
                        reply_count: None,
                        media_type: Some("document".to_string()),
                        forward_origin: None,
                    },
                    MessageSummary {
                        message_id: 10,
                        chat_ref: Some("101".to_string()),
                        sender_ref: Some("7".to_string()),
                        outgoing: true,
                        pinned: true,
                        silent: false,
                        noforwards: false,
                        text: Some("Here is the latest contract".to_string()),
                        date: 1_700_000_010,
                        edit_date: None,
                        reply_to_message_id: None,
                        views: None,
                        forwards: None,
                        reply_count: Some(1),
                        media_type: None,
                        forward_origin: None,
                    },
                    MessageSummary {
                        message_id: 12,
                        chat_ref: Some("101".to_string()),
                        sender_ref: Some("101".to_string()),
                        outgoing: false,
                        pinned: false,
                        silent: false,
                        noforwards: false,
                        text: None,
                        date: 1_699_999_999,
                        edit_date: None,
                        reply_to_message_id: None,
                        views: None,
                        forwards: None,
                        reply_count: None,
                        media_type: Some("voice".to_string()),
                        forward_origin: None,
                    },
                ],
            ),
            (
                202,
                vec![
                    MessageSummary {
                        message_id: 22,
                        chat_ref: Some("-202".to_string()),
                        sender_ref: Some("101".to_string()),
                        outgoing: false,
                        pinned: false,
                        silent: false,
                        noforwards: false,
                        text: Some("Topic follow-up".to_string()),
                        date: 1_700_000_022,
                        edit_date: None,
                        reply_to_message_id: Some(21),
                        views: None,
                        forwards: None,
                        reply_count: None,
                        media_type: None,
                        forward_origin: None,
                    },
                    MessageSummary {
                        message_id: 21,
                        chat_ref: Some("-202".to_string()),
                        sender_ref: Some("7".to_string()),
                        outgoing: true,
                        pinned: true,
                        silent: false,
                        noforwards: false,
                        text: Some("Release passed".to_string()),
                        date: 1_700_000_021,
                        edit_date: None,
                        reply_to_message_id: None,
                        views: None,
                        forwards: None,
                        reply_count: None,
                        media_type: None,
                        forward_origin: None,
                    },
                    MessageSummary {
                        message_id: 20,
                        chat_ref: Some("-202".to_string()),
                        sender_ref: Some("7".to_string()),
                        outgoing: true,
                        pinned: false,
                        silent: false,
                        noforwards: false,
                        text: None,
                        date: 1_700_000_020,
                        edit_date: None,
                        reply_to_message_id: None,
                        views: None,
                        forwards: None,
                        reply_count: None,
                        media_type: Some("poll".to_string()),
                        forward_origin: None,
                    },
                ],
            ),
            (
                505,
                vec![MessageSummary {
                    message_id: 50,
                    chat_ref: Some("-100505".to_string()),
                    sender_ref: Some("7".to_string()),
                    outgoing: true,
                    pinned: false,
                    silent: false,
                    noforwards: false,
                    text: Some("Ops announcement".to_string()),
                    date: 1_700_000_050,
                    edit_date: None,
                    reply_to_message_id: None,
                    views: Some(12),
                    forwards: Some(1),
                    reply_count: None,
                    media_type: None,
                    forward_origin: None,
                }],
            ),
        ]);
        let message_buttons = HashMap::from([((101, 11), fixture_message_buttons())]);
        let saved_dialogs = vec![
            SavedDialogSummary {
                peer_ref: "101".to_string(),
                title: Some("Alex".to_string()),
                pinned: true,
                top_message_id: 701,
            },
            SavedDialogSummary {
                peer_ref: "-202".to_string(),
                title: Some("Project Room".to_string()),
                pinned: false,
                top_message_id: 702,
            },
        ];
        let saved_histories = HashMap::from([
            (
                101,
                vec![
                    MessageSummary {
                        message_id: 701,
                        chat_ref: Some("7".to_string()),
                        sender_ref: Some("7".to_string()),
                        outgoing: true,
                        pinned: false,
                        silent: false,
                        noforwards: false,
                        text: Some("Saved contract research notes".to_string()),
                        date: 1_700_001_701,
                        edit_date: None,
                        reply_to_message_id: None,
                        views: None,
                        forwards: None,
                        reply_count: None,
                        media_type: None,
                        forward_origin: Some(MessageForwardOrigin {
                            from_ref: Some("101".to_string()),
                            source_message_id: Some(11),
                            saved_from_ref: Some("101".to_string()),
                            saved_from_message_id: Some(11),
                            date: 1_700_000_011,
                        }),
                    },
                    MessageSummary {
                        message_id: 700,
                        chat_ref: Some("7".to_string()),
                        sender_ref: Some("7".to_string()),
                        outgoing: true,
                        pinned: false,
                        silent: false,
                        noforwards: false,
                        text: Some("Saved launch checklist".to_string()),
                        date: 1_700_001_700,
                        edit_date: None,
                        reply_to_message_id: None,
                        views: None,
                        forwards: None,
                        reply_count: None,
                        media_type: None,
                        forward_origin: Some(MessageForwardOrigin {
                            from_ref: Some("101".to_string()),
                            source_message_id: Some(10),
                            saved_from_ref: Some("101".to_string()),
                            saved_from_message_id: Some(10),
                            date: 1_700_000_010,
                        }),
                    },
                ],
            ),
            (
                202,
                vec![MessageSummary {
                    message_id: 702,
                    chat_ref: Some("7".to_string()),
                    sender_ref: Some("7".to_string()),
                    outgoing: true,
                    pinned: false,
                    silent: false,
                    noforwards: false,
                    text: Some("Saved project room release plan".to_string()),
                    date: 1_700_001_702,
                    edit_date: None,
                    reply_to_message_id: None,
                    views: None,
                    forwards: None,
                    reply_count: None,
                    media_type: None,
                    forward_origin: Some(MessageForwardOrigin {
                        from_ref: Some("-202".to_string()),
                        source_message_id: Some(21),
                        saved_from_ref: Some("-202".to_string()),
                        saved_from_message_id: Some(21),
                        date: 1_700_000_021,
                    }),
                }],
            ),
        ]);
        let saved_message_tags = HashMap::from([
            (701, vec![ReactionIdentity::Emoji { emoji: "🧠".to_string() }]),
            (702, vec![ReactionIdentity::CustomEmoji { document_id: 7_001 }]),
        ]);
        let saved_reaction_tags = vec![
            SavedReactionTagSummary {
                reaction: ReactionIdentity::Emoji { emoji: "🧠".to_string() },
                title: Some("Research".to_string()),
                count: 1,
            },
            SavedReactionTagSummary {
                reaction: ReactionIdentity::CustomEmoji { document_id: 7_001 },
                title: Some("Launch".to_string()),
                count: 1,
            },
        ];
        let contacts = vec![
            ContactSummary {
                user_id: 101,
                first_name: "Alex".to_string(),
                last_name: Some("Morgan".to_string()),
                display_name: "Alex Morgan".to_string(),
                username: Some("alex".to_string()),
                phone: Some("+14155550101".to_string()),
                is_contact: true,
                mutual_contact: true,
                bot: false,
                verified: false,
                premium: true,
                scam: false,
                restricted: false,
            },
            ContactSummary {
                user_id: 303,
                first_name: "Mira".to_string(),
                last_name: None,
                display_name: "Mira".to_string(),
                username: Some("mira_ops".to_string()),
                phone: None,
                is_contact: true,
                mutual_contact: false,
                bot: false,
                verified: false,
                premium: false,
                scam: false,
                restricted: false,
            },
            ContactSummary {
                user_id: 404,
                first_name: "Dana".to_string(),
                last_name: Some("Candidate".to_string()),
                display_name: "Dana Candidate".to_string(),
                username: Some("dana_candidate".to_string()),
                phone: None,
                is_contact: false,
                mutual_contact: false,
                bot: false,
                verified: false,
                premium: false,
                scam: false,
                restricted: false,
            },
            ContactSummary {
                user_id: 909,
                first_name: "Fixture Inline Bot".to_string(),
                last_name: None,
                display_name: "Fixture Inline Bot".to_string(),
                username: Some("fixture_inline_bot".to_string()),
                phone: None,
                is_contact: false,
                mutual_contact: false,
                bot: true,
                verified: true,
                premium: false,
                scam: false,
                restricted: false,
            },
            ContactSummary {
                user_id: 707,
                first_name: "Kai".to_string(),
                last_name: Some("Kicked".to_string()),
                display_name: "Kai Kicked".to_string(),
                username: Some("kai_kicked".to_string()),
                phone: None,
                is_contact: false,
                mutual_contact: false,
                bot: false,
                verified: false,
                premium: false,
                scam: false,
                restricted: true,
            },
            ContactSummary {
                user_id: 808,
                first_name: "Rin".to_string(),
                last_name: Some("Restricted".to_string()),
                display_name: "Rin Restricted".to_string(),
                username: Some("rin_restricted".to_string()),
                phone: None,
                is_contact: false,
                mutual_contact: false,
                bot: false,
                verified: false,
                premium: false,
                scam: false,
                restricted: true,
            },
        ];
        let sessions = vec![
            SessionSummary {
                authorization_id: 1,
                current: true,
                official_app: false,
                password_pending: false,
                encrypted_requests_disabled: false,
                call_requests_disabled: false,
                unconfirmed: false,
                device_model: "telegram-personal-mcp fixture".to_string(),
                platform: "test".to_string(),
                system_version: "deterministic".to_string(),
                api_id: 1,
                app_name: "telegram-personal-mcp".to_string(),
                app_version: env!("CARGO_PKG_VERSION").to_string(),
                date_created_unix: 1_700_000_000,
                date_active_unix: 1_700_000_021,
                ip: "127.0.0.1".to_string(),
                country: "Local".to_string(),
                region: "Fixture".to_string(),
            },
            SessionSummary {
                authorization_id: 2,
                current: false,
                official_app: true,
                password_pending: false,
                encrypted_requests_disabled: false,
                call_requests_disabled: false,
                unconfirmed: false,
                device_model: "Fixture phone".to_string(),
                platform: "android".to_string(),
                system_version: "fixture".to_string(),
                api_id: 2,
                app_name: "Telegram".to_string(),
                app_version: "fixture".to_string(),
                date_created_unix: 1_699_000_000,
                date_active_unix: 1_700_000_010,
                ip: "192.0.2.2".to_string(),
                country: "Local".to_string(),
                region: "Secondary fixture".to_string(),
            },
        ];
        let fixture_poll = fixture_poll();
        let account_member = ChatMemberSummary {
            chat_id: "-100505".to_string(),
            user: fixture_account_contact(),
            role: ChatMemberRole::Creator,
            can_edit: false,
            is_self: true,
            left: false,
            joined_date_unix: Some(1_700_000_000),
            inviter_user_id: None,
            promoted_by_user_id: None,
            kicked_by_user_id: None,
            rank: Some("Owner".to_string()),
            subscription_until_date_unix: None,
            via_request: false,
            admin_rights: Some(fixture_admin_rights()),
            banned_rights: None,
        };
        let alex_member =
            fixture_chat_member("-100505", contacts[0].clone(), ChatMemberRole::Member, None, None);
        let mira_admin = fixture_chat_member(
            "-100505",
            contacts[1].clone(),
            ChatMemberRole::Admin,
            Some(ChatAdminRightsSummary {
                delete_messages: true,
                ban_users: true,
                invite_users: true,
                manage_topics: true,
                ..ChatAdminRightsSummary::default()
            }),
            Some("Ops"),
        );
        let bot_member =
            fixture_chat_member("-100505", contacts[3].clone(), ChatMemberRole::Member, None, None);
        let kicked_member = ChatMemberSummary {
            chat_id: "-100505".to_string(),
            user: contacts[4].clone(),
            role: ChatMemberRole::Banned,
            can_edit: false,
            is_self: false,
            left: true,
            joined_date_unix: Some(1_700_000_090),
            inviter_user_id: Some(7),
            promoted_by_user_id: None,
            kicked_by_user_id: Some(7),
            rank: None,
            subscription_until_date_unix: None,
            via_request: false,
            admin_rights: None,
            banned_rights: Some(ChatBannedRightsSummary {
                view_messages: true,
                until_date_unix: 0,
                ..ChatBannedRightsSummary::default()
            }),
        };
        let restricted_member = ChatMemberSummary {
            chat_id: "-100505".to_string(),
            user: contacts[5].clone(),
            role: ChatMemberRole::Restricted,
            can_edit: false,
            is_self: false,
            left: false,
            joined_date_unix: Some(1_700_000_095),
            inviter_user_id: Some(7),
            promoted_by_user_id: None,
            kicked_by_user_id: Some(7),
            rank: None,
            subscription_until_date_unix: None,
            via_request: false,
            admin_rights: None,
            banned_rights: Some(ChatBannedRightsSummary {
                send_messages: true,
                send_docs: true,
                until_date_unix: 0,
                ..ChatBannedRightsSummary::default()
            }),
        };
        let active_invite_handle = opaque_invite_handle("fixture-active-invite-token");
        let join_requests = HashMap::from([(
            505,
            vec![
                fixture_invite_importer(
                    contacts[2].clone(),
                    Some(active_invite_handle.clone()),
                    true,
                    1_700_000_080,
                    Some("I work with the release team"),
                    None,
                ),
                fixture_invite_importer(
                    ContactSummary {
                        user_id: 606,
                        first_name: "Eli".to_string(),
                        last_name: Some("Reviewer".to_string()),
                        display_name: "Eli Reviewer".to_string(),
                        username: Some("eli_reviewer".to_string()),
                        phone: None,
                        is_contact: false,
                        mutual_contact: false,
                        bot: false,
                        verified: false,
                        premium: false,
                        scam: false,
                        restricted: false,
                    },
                    None,
                    true,
                    1_700_000_070,
                    None,
                    None,
                ),
            ],
        )]);
        let invite_joiners = HashMap::from([(
            505,
            vec![fixture_invite_importer(
                contacts[0].clone(),
                Some(active_invite_handle),
                false,
                1_700_000_060,
                None,
                Some(7),
            )],
        )]);
        let known_chats: HashMap<i64, ChatDetailsSummary> = dialogs
            .iter()
            .filter(|dialog| dialog.chat_kind != "user")
            .map(|dialog| {
                (
                    dialog.chat_id,
                    ChatDetailsSummary {
                        chat: ChatSummary {
                            chat_id: dialog.canonical_chat_id.clone(),
                            title: dialog.title.clone(),
                            kind: dialog.chat_kind.clone(),
                            username: dialog.username.clone(),
                            participants_count: Some(3),
                            creator: true,
                            verified: false,
                            restricted: false,
                        },
                        about: (dialog.chat_id == 505)
                            .then(|| "Fixture operations supergroup".to_string()),
                        photo_handle: (dialog.chat_id == 505)
                            .then(|| opaque_chat_photo_handle("-100505", 50_500)),
                    },
                )
            })
            .collect();
        let business_chat_links = BusinessChatLinksPage {
            count: 2,
            links: [
                (
                    "fixture-welcome",
                    "Welcome to the fixture account",
                    Some("Welcome".to_string()),
                    12,
                ),
                ("fixture-support", "Describe the support request", Some("Support".to_string()), 4),
            ]
            .into_iter()
            .map(|(slug, message, title, views)| {
                let content = crate::telegram::business_chat_link_content_from_source(
                    message,
                    CaptionParseMode::Plain,
                    title,
                );
                BusinessChatLinkSummary {
                    slug: slug.to_string(),
                    link: format!("https://t.me/m/{slug}"),
                    message: content.rendered_message,
                    entity_count: content.entity_count,
                    entities_sha256: content.entities_sha256,
                    title: content.title,
                    views,
                }
            })
            .collect(),
            related_chats: known_chats
                .get(&505)
                .map(|details| vec![details.chat.clone()])
                .unwrap_or_default(),
            related_users: contacts.iter().take(2).cloned().collect(),
        };
        let (stars_statuses, stars_transaction_subscription_ids) =
            fixture_stars_statuses(&contacts, &known_chats);
        let (available_star_gifts, saved_star_gifts, stars_topup_options) =
            fixture_star_gift_catalogs(&contacts, &known_chats);
        let stories = HashMap::from([
            (
                505,
                vec![fixture_story(41, "Release rehearsal", 12), fixture_story(42, "Ship it", 23)],
            ),
            (202, vec![fixture_story(51, "Private release note", 4)]),
        ]);
        let story_archive = HashMap::from([(
            505,
            vec![fixture_story(31, "Archived release", 8), fixture_story(30, "Older archive", 3)],
        )]);
        let story_pinned_to_top = HashMap::from([(505, vec![41])]);
        let story_max_read_ids = HashMap::from([(505, 40), (202, 0)]);
        let story_viewers = HashMap::from([(
            (505, 41),
            vec![
                StoryViewerSummary {
                    kind: "user".to_string(),
                    user_id: Some(101),
                    source_peer_ref: None,
                    user: Some(contacts[0].clone()),
                    date_unix: Some(1_700_000_250),
                    blocked: false,
                    blocked_my_stories_from: false,
                    reaction_kind: Some("emoji".to_string()),
                    reaction_emoji: Some("👍".to_string()),
                    reaction_custom_emoji_id: None,
                },
                StoryViewerSummary {
                    kind: "user".to_string(),
                    user_id: Some(303),
                    source_peer_ref: None,
                    user: Some(contacts[1].clone()),
                    date_unix: Some(1_700_000_251),
                    blocked: true,
                    blocked_my_stories_from: false,
                    reaction_kind: None,
                    reaction_emoji: None,
                    reaction_custom_emoji_id: None,
                },
            ],
        )]);
        Self {
            state: Mutex::new(FakeState {
                account: UserInfo {
                    user_id: 7,
                    first_name: "Fixture".to_string(),
                    username: Some("fixture_account".to_string()),
                    premium: false,
                },
                account_is_bot: false,
                account_profile: AccountProfileSummary {
                    user_id: 7,
                    first_name: "Fixture".to_string(),
                    last_name: None,
                    username: Some("fixture_account".to_string()),
                    about: Some("Deterministic Telegram MCP fixture".to_string()),
                    birthday: Some(BirthdaySummary { day: 21, month: 7, year: Some(1990) }),
                    name_color: Some(PeerColorSummary::Simple {
                        color: Some(3),
                        background_emoji_id: None,
                    }),
                    profile_color: None,
                    personal_channel: None,
                    business: BusinessAccountSettingsSummary {
                        work_hours: Some(BusinessWorkHoursSummary {
                            config: BusinessWorkHoursConfig {
                                timezone_id: "Europe/Moscow".to_string(),
                                weekly_open: vec![BusinessWeeklyOpenSummary {
                                    start_minute: 540,
                                    end_minute: 1_080,
                                }],
                            },
                            open_now: false,
                        }),
                        location: Some(BusinessLocationSummary {
                            address: "Fixture office".to_string(),
                            point: Some(GeoPointSpec {
                                latitude_e7: 557_558_000,
                                longitude_e7: 376_173_000,
                                accuracy_radius_m: None,
                            }),
                        }),
                        greeting: Some(BusinessGreetingMessageSummary {
                            shortcut_id: 7,
                            recipients: BusinessRecipientsSummary {
                                existing_chats: false,
                                new_chats: true,
                                contacts: false,
                                non_contacts: false,
                                exclude_selected: false,
                                user_ids: Vec::new(),
                            },
                            no_activity_days: 7,
                        }),
                        away: None,
                        intro: Some(BusinessIntroSummary {
                            title: "Fixture support".to_string(),
                            description: "Tell us what you need".to_string(),
                            sticker_handle: None,
                        }),
                    },
                },
                profile_photos: vec![
                    ProfilePhotoSummary {
                        photo_handle: opaque_profile_photo_handle(9_001),
                        date_unix: 1_700_000_090,
                        has_stickers: false,
                        has_video: false,
                    },
                    ProfilePhotoSummary {
                        photo_handle: opaque_profile_photo_handle(9_000),
                        date_unix: 1_600_000_000,
                        has_stickers: true,
                        has_video: false,
                    },
                ],
                next_profile_photo_id: 9_002,
                dialog_folders: DialogFolderList {
                    tags_enabled: false,
                    folders: vec![
                        DialogFolderSummary {
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
                        },
                        DialogFolderSummary {
                            folder_id: 2,
                            kind: DialogFolderKind::Custom,
                            title: "Release".to_string(),
                            title_has_entities: false,
                            title_noanimate: false,
                            emoticon: Some("🚀".to_string()),
                            color: Some(2),
                            contacts: false,
                            non_contacts: false,
                            groups: true,
                            broadcasts: true,
                            bots: false,
                            exclude_muted: false,
                            exclude_read: false,
                            exclude_archived: true,
                            has_my_invites: false,
                            pinned_chat_ids: vec!["-100505".to_string()],
                            included_chat_ids: vec!["-100505".to_string()],
                            excluded_chat_ids: Vec::new(),
                        },
                    ],
                },
                suggested_dialog_folders: fixture_suggested_dialog_folders(),
                global_privacy_settings: fixture_global_privacy_settings(),
                session_path: PathBuf::from("/tmp/telegram-personal-mcp-fixture.session"),
                dialogs,
                known_chats,
                channel_settings: HashMap::from([(
                    505,
                    FakeChannelSettingsState {
                        slow_mode_seconds: 10,
                        forum_enabled: true,
                        ..FakeChannelSettingsState::default()
                    },
                )]),
                chat_moderation_settings: HashMap::from([
                    (202, FakeChatModerationState::default()),
                    (
                        505,
                        FakeChatModerationState {
                            anti_spam_enabled: Some(false),
                            forum_enabled: Some(true),
                            forum_tabs_enabled: Some(false),
                            ..FakeChatModerationState::default()
                        },
                    ),
                ]),
                stories,
                story_archive,
                story_pinned_to_top,
                story_stealth_mode: StoryStealthModeSummary {
                    active_until_unix: None,
                    cooldown_until_unix: None,
                },
                story_max_read_ids,
                story_viewers,
                story_by_random_id: HashMap::new(),
                next_story_id: 60,
                next_chat_id: 800,
                next_chat_photo_id: 60_000,
                messages,
                message_html: HashMap::new(),
                history_calls: 0,
                history_rows_returned: 0,
                list_dialogs_calls: 0,
                quick_reply_shortcuts: fixture_quick_reply_shortcuts(),
                quick_reply_messages: fixture_quick_reply_messages(),
                group_calls: fixture_group_calls(),
                business_chat_links,
                next_business_chat_link_id: 1,
                business_chat_link_create_calls: 0,
                business_chat_link_create_uncertain_once: false,
                business_chat_link_edit_calls: 0,
                business_chat_link_edit_uncertain_once: false,
                business_chat_link_delete_calls: 0,
                business_chat_link_delete_uncertain_once: false,
                business_work_hours_calls: 0,
                business_work_hours_uncertain_once: false,
                business_location_calls: 0,
                business_location_uncertain_once: false,
                business_greeting_calls: 0,
                business_greeting_uncertain_once: false,
                business_away_calls: 0,
                business_away_uncertain_once: false,
                business_intro_calls: 0,
                business_intro_uncertain_once: false,
                stars_statuses,
                stars_transaction_subscription_ids,
                available_star_gifts,
                saved_star_gifts,
                stars_topup_options,
                star_gift_save_calls: 0,
                star_gift_save_uncertain_once: false,
                star_gift_convert_calls: 0,
                star_gift_convert_uncertain_once: false,
                stars_subscription_change_calls: 0,
                stars_subscription_change_uncertain_once: false,
                message_buttons,
                saved_dialogs,
                saved_histories,
                saved_message_tags,
                saved_reaction_tags,
                typing_actions: Vec::new(),
                pressed_callback_buttons: Vec::new(),
                inline_queries: HashMap::new(),
                next_inline_query_id: 9_090_000_000_000_001,
                sent_inline_results: Vec::new(),
                bot_commands: HashMap::from([(
                    "en".to_string(),
                    vec![BotCommandSummary {
                        command: "start".to_string(),
                        description: "Start the fixture bot".to_string(),
                        ephemeral: false,
                    }],
                )]),
                account_emoji_status: empty_emoji_status(),
                emoji_status_catalogs: HashMap::from([
                    (
                        EmojiStatusCatalogKind::Default,
                        (
                            10_001,
                            vec![
                                fixture_default_emoji_status(7_001),
                                fixture_default_emoji_status(7_002),
                            ],
                        ),
                    ),
                    (
                        EmojiStatusCatalogKind::Recent,
                        (
                            10_002,
                            vec![
                                fixture_default_emoji_status(7_002),
                                fixture_collectible_emoji_status(8_001, 7_101),
                            ],
                        ),
                    ),
                    (
                        EmojiStatusCatalogKind::ChannelDefault,
                        (10_003, vec![fixture_default_emoji_status(7_003)]),
                    ),
                    (
                        EmojiStatusCatalogKind::Collectible,
                        (10_004, vec![fixture_collectible_emoji_status(8_001, 7_101)]),
                    ),
                ]),
                contacts,
                chat_members: HashMap::from([
                    ((505, 7), account_member),
                    ((505, 101), alex_member),
                    ((505, 303), mira_admin),
                    ((505, 707), kicked_member),
                    ((505, 808), restricted_member),
                    ((505, 909), bot_member),
                ]),
                chat_member_invite_scripts: HashMap::new(),
                chat_member_invite_calls: Vec::new(),
                invite_links: HashMap::from([(
                    505,
                    vec![
                        fixture_invite_link("fixture-active-invite-token", false, "Ops access", 7),
                        fixture_invite_link(
                            "fixture-moderator-invite-token",
                            false,
                            "Moderator access",
                            303,
                        ),
                        fixture_invite_link("fixture-revoked-invite-token", true, "Old access", 7),
                    ],
                )]),
                join_requests,
                invite_joiners,
                blocked_users: HashSet::from([303]),
                privacy_rules: HashMap::new(),
                notification_settings: HashMap::new(),
                chat_auto_delete: HashMap::from([(101, 0), (202, 0), (505, 0)]),
                sessions,
                pinned_messages: HashMap::from([(101, 10), (202, 21)]),
                scheduled_messages: HashMap::from([(
                    101,
                    vec![MessageSummary {
                        message_id: 301,
                        chat_ref: Some("101".to_string()),
                        sender_ref: Some("7".to_string()),
                        outgoing: true,
                        pinned: false,
                        silent: false,
                        noforwards: false,
                        text: Some("Scheduled fixture follow-up".to_string()),
                        date: 1_800_000_000,
                        edit_date: None,
                        reply_to_message_id: Some(11),
                        views: None,
                        forwards: None,
                        reply_count: None,
                        media_type: None,
                        forward_origin: None,
                    }],
                )]),
                forum_topics: HashMap::from([(202, vec![fixture_forum_topic()])]),
                forum_topic_by_random_id: HashMap::new(),
                polls: HashMap::from([((202, 20), fixture_poll)]),
                poll_voters: HashMap::from([(
                    (202, 20),
                    vec![
                        PollVoterSummary {
                            peer_ref: "101".to_string(),
                            option_indices: vec![0],
                            unknown_option_hex: Vec::new(),
                            submitted_open_answer: false,
                            date_unix: 1_700_000_022,
                        },
                        PollVoterSummary {
                            peer_ref: "303".to_string(),
                            option_indices: vec![1],
                            unknown_option_hex: Vec::new(),
                            submitted_open_answer: false,
                            date_unix: 1_700_000_023,
                        },
                    ],
                )]),
                drafts: HashMap::from([(
                    101,
                    finalize_fixture_draft(DraftSummary {
                        message: "Review the contract before replying".to_string(),
                        date_unix: 1_700_000_040,
                        no_webpage: false,
                        invert_media: false,
                        has_reply_to: false,
                        has_entities: false,
                        has_media: false,
                        effect_id: None,
                        has_suggested_post: false,
                        has_rich_message: false,
                        state_sha256: String::new(),
                    }),
                )]),
                dialog_read_inbox_max_ids: HashMap::from([(101, 9), (202, 21), (505, 0)]),
                dialog_unread_reactions: HashMap::from([(101, 2), (202, 0), (505, 0)]),
                read_receipts: HashMap::from([(
                    (202, 21),
                    vec![MessageReadReceipt { user_id: 101, read_at_unix: 1_700_000_030 }],
                )]),
                attachments: HashMap::from([(
                    (101, 11),
                    (
                        AttachmentDescriptor {
                            attachment_id: "@alex:11:0".to_string(),
                            chat_id: "@alex".to_string(),
                            message_id: 11,
                            kind: "document".to_string(),
                            file_name: Some("contract.pdf".to_string()),
                            mime_type: Some("application/pdf".to_string()),
                            size_bytes: Some(26),
                            downloadable: true,
                        },
                        b"%PDF-1.4\nfixture contract\n".to_vec(),
                    ),
                )]),
                sent: Vec::new(),
                sent_by_random_id: HashMap::new(),
                my_reactions: HashMap::new(),
                default_reaction: Some(ReactionIdentity::Emoji { emoji: "👍".to_string() }),
                sticker_set_installed: true,
                sticker_set_archived: false,
                favorite_stickers: HashSet::from([opaque_sticker_handle(9_101)]),
                custom_emoji_size_hint: None,
                rich_media: HashMap::new(),
                rich_messages: HashMap::new(),
                document_media: HashMap::new(),
                message_topics: HashMap::new(),
                message_grouped_ids: HashMap::new(),
                my_boosts: vec![
                    BoostSlotSummary {
                        slot: 1,
                        chat_id: Some("-100505".to_string()),
                        applied_at_unix: 1_699_900_000,
                        expires_at_unix: 1_702_500_000,
                        cooldown_until_unix: None,
                    },
                    BoostSlotSummary {
                        slot: 2,
                        chat_id: None,
                        applied_at_unix: 1_699_800_000,
                        expires_at_unix: 1_702_400_000,
                        cooldown_until_unix: Some(1_700_100_000),
                    },
                ],
                transcriptions: HashMap::new(),
                transcription_ratings: HashMap::new(),
                fact_checks: HashMap::from([
                    (
                        (505, 50),
                        FactCheckSummary {
                            message_id: 50,
                            need_check: false,
                            country: Some("US".to_string()),
                            text: Some("Verified fixture release announcement".to_string()),
                            hash: "5001".to_string(),
                        },
                    ),
                    (
                        (202, 21),
                        FactCheckSummary {
                            message_id: 21,
                            need_check: true,
                            country: None,
                            text: None,
                            hash: "2100".to_string(),
                        },
                    ),
                ]),
                next_message_id: 100,
                translate_calls: 0,
                translate_uncertain_once: false,
                transcription_start_calls: 0,
                transcription_poll_calls: 0,
                transcription_uncertain_once: false,
                transcription_rate_calls: 0,
                transcription_rate_uncertain_once: false,
                fact_check_edit_calls: 0,
                fact_check_edit_uncertain_once: false,
                fact_check_delete_calls: 0,
                fact_check_delete_uncertain_once: false,
                saved_history_delete_calls: 0,
                saved_history_delete_uncertain_once: false,
                saved_dialog_pin_calls: 0,
                saved_dialog_pin_uncertain_once: false,
                saved_reaction_tag_update_calls: 0,
                saved_reaction_tag_update_uncertain_once: false,
                typing_action_calls: 0,
                typing_action_uncertain_once: false,
                callback_button_press_calls: 0,
                callback_button_press_uncertain_once: false,
                inline_query_calls: 0,
                inline_result_send_calls: 0,
                inline_result_send_uncertain_once: false,
                bot_command_set_calls: 0,
                bot_command_set_uncertain_once: false,
                emoji_status_set_calls: 0,
                emoji_status_set_uncertain_once: false,
                emoji_recent_clear_calls: 0,
                emoji_recent_clear_uncertain_once: false,
                account_color_update_calls: 0,
                account_color_update_uncertain_once: false,
                personal_channel_update_calls: 0,
                personal_channel_update_uncertain_once: false,
                edit_calls: 0,
                delete_calls: 0,
                pin_calls: 0,
                forward_calls: 0,
                reaction_calls: 0,
                default_reaction_calls: 0,
                default_reaction_uncertain_once: false,
                sticker_send_calls: 0,
                sticker_send_uncertain_once: false,
                recent_stickers: HashSet::from([
                    opaque_sticker_handle(9_101),
                    opaque_sticker_handle(9_102),
                ]),
                attached_recent_stickers: HashSet::from([
                    opaque_sticker_handle(9_101),
                    opaque_sticker_handle(9_102),
                ]),
                recent_sticker_calls: 0,
                recent_sticker_uncertain_once: false,
                sticker_favorite_calls: 0,
                sticker_favorite_uncertain_once: false,
                sticker_install_calls: 0,
                sticker_install_uncertain_once: false,
                sticker_uninstall_calls: 0,
                rich_media_send_calls: 0,
                rich_media_send_uncertain_once: false,
                voice_send_calls: 0,
                voice_send_uncertain_once: false,
                video_note_send_calls: 0,
                video_note_send_uncertain_once: false,
                album_send_calls: 0,
                album_send_uncertain_once: false,
                boost_apply_calls: 0,
                boost_apply_uncertain_once: false,
                draft_calls: 0,
                draft_clear_all_calls: 0,
                draft_clear_all_uncertain_once: false,
                dialog_unread_calls: 0,
                mark_messages_read_calls: 0,
                mark_reactions_read_calls: 0,
                dialog_archive_calls: 0,
                dialog_pin_calls: 0,
                block_calls: 0,
                session_terminate_calls: 0,
                session_reset_calls: 0,
                session_reset_uncertain_once: false,
                chat_auto_delete_calls: 0,
                chat_auto_delete_uncertain_once: false,
                contact_calls: 0,
                privacy_calls: 0,
                notification_calls: 0,
                schedule_create_calls: 0,
                schedule_edit_calls: 0,
                schedule_delete_calls: 0,
                schedule_send_now_calls: 0,
                poll_create_calls: 0,
                poll_vote_calls: 0,
                poll_stop_calls: 0,
                forum_topic_create_calls: 0,
                forum_topic_edit_calls: 0,
                forum_topic_pin_calls: 0,
                forum_topic_delete_calls: 0,
                chat_member_add_calls: 0,
                chat_member_remove_calls: 0,
                chat_member_admin_calls: 0,
                chat_member_restriction_calls: 0,
                invite_export_calls: 0,
                invite_revoke_calls: 0,
                invite_delete_calls: 0,
                join_request_calls: 0,
                profile_update_calls: 0,
                username_update_calls: 0,
                birthday_update_calls: 0,
                presence_update_calls: 0,
                profile_photo_upload_calls: 0,
                profile_photo_delete_calls: 0,
                dialog_folder_upsert_calls: 0,
                dialog_folder_delete_calls: 0,
                dialog_folder_reorder_calls: 0,
                dialog_folder_membership_calls: 0,
                dialog_folder_tags_calls: 0,
                dialog_folder_tags_uncertain_once: false,
                global_privacy_update_calls: 0,
                global_privacy_update_uncertain_once: false,
                chat_create_calls: 0,
                chat_join_calls: 0,
                chat_leave_calls: 0,
                chat_delete_calls: 0,
                chat_title_calls: 0,
                chat_about_calls: 0,
                chat_photo_update_calls: 0,
                chat_photo_delete_calls: 0,
                chat_username_calls: 0,
                chat_slow_mode_calls: 0,
                chat_signatures_calls: 0,
                chat_pre_history_calls: 0,
                chat_join_request_calls: 0,
                chat_permissions_calls: 0,
                chat_reactions_policy_calls: 0,
                chat_anti_spam_calls: 0,
                chat_forum_mode_calls: 0,
                chat_permissions_uncertain_once: false,
                chat_reactions_policy_uncertain_once: false,
                chat_anti_spam_uncertain_once: false,
                chat_forum_mode_uncertain_once: false,
                story_view_calls: 0,
                story_read_calls: 0,
                story_reaction_calls: 0,
                story_pin_calls: 0,
                story_top_pin_calls: 0,
                story_stealth_calls: 0,
                story_send_calls: 0,
                story_edit_calls: 0,
                story_report_calls: 0,
                story_report_uncertain_once: false,
                story_delete_calls: 0,
            }),
        }
    }

    pub async fn sent_messages(&self) -> Vec<FakeSentMessage> {
        self.state.lock().await.sent.clone()
    }

    #[doc(hidden)]
    pub async fn set_fixture_attachment_size_hint(&self, size_bytes: Option<u64>) {
        let mut state = self.state.lock().await;
        if let Some((descriptor, _)) = state.attachments.get_mut(&(101, 11)) {
            descriptor.size_bytes = size_bytes;
        }
    }

    #[doc(hidden)]
    pub async fn set_fixture_custom_emoji_size_hint(&self, size_bytes: Option<u64>) {
        self.state.lock().await.custom_emoji_size_hint = size_bytes;
    }

    #[doc(hidden)]
    pub async fn set_fixture_account_user_id(&self, user_id: i64) {
        self.state.lock().await.account.user_id = user_id;
    }

    #[doc(hidden)]
    pub async fn set_fixture_message_text(
        &self,
        chat_id: i64,
        message_id: i32,
        text: Option<String>,
        edit_date: Option<i32>,
    ) -> bool {
        let mut state = self.state.lock().await;
        let Some(message) = state.messages.get_mut(&chat_id).and_then(|messages| {
            messages.iter_mut().find(|message| message.message_id == message_id)
        }) else {
            return false;
        };
        message.text = text;
        message.edit_date = edit_date;
        true
    }

    #[doc(hidden)]
    pub async fn remove_fixture_message(&self, chat_id: i64, message_id: i32) -> bool {
        let mut state = self.state.lock().await;
        let Some(messages) = state.messages.get_mut(&chat_id) else {
            return false;
        };
        let before = messages.len();
        messages.retain(|message| message.message_id != message_id);
        messages.len() != before
    }

    #[doc(hidden)]
    pub async fn mutation_call_counts(&self) -> (usize, usize) {
        let state = self.state.lock().await;
        (state.edit_calls, state.delete_calls)
    }

    #[doc(hidden)]
    pub async fn history_observation(&self) -> (usize, usize) {
        let state = self.state.lock().await;
        (state.history_calls, state.history_rows_returned)
    }

    #[doc(hidden)]
    pub async fn list_dialogs_call_count(&self) -> usize {
        self.state.lock().await.list_dialogs_calls
    }

    /// Append a synthetic incoming message to one fixture dialog and advance
    /// its visible top message, mirroring what Telegram reports after a new
    /// post arrives.
    #[doc(hidden)]
    pub async fn push_fixture_message(&self, chat_id: i64, text: &str) -> Option<i32> {
        let mut state = self.state.lock().await;
        if !state.dialogs.iter().any(|dialog| dialog.chat_id == chat_id) {
            return None;
        }
        let messages = state.messages.entry(chat_id).or_default();
        let next_message_id =
            messages.iter().map(|message| message.message_id).max().unwrap_or(0) + 1;
        let next_date = messages.iter().map(|message| message.date).max().unwrap_or(0) + 1;
        messages.push(MessageSummary {
            message_id: next_message_id,
            chat_ref: Some(chat_id.to_string()),
            sender_ref: None,
            outgoing: false,
            pinned: false,
            silent: false,
            noforwards: false,
            text: Some(text.to_string()),
            date: next_date,
            edit_date: None,
            reply_to_message_id: None,
            views: None,
            forwards: None,
            reply_count: None,
            media_type: None,
            forward_origin: None,
        });
        if let Some(dialog) = state.dialogs.iter_mut().find(|dialog| dialog.chat_id == chat_id) {
            dialog.last_message_id = Some(next_message_id);
            dialog.last_message_text = Some(text.to_string());
            dialog.last_message_date = Some(next_date);
        }
        Some(next_message_id)
    }

    #[doc(hidden)]
    pub async fn pin_call_count(&self) -> usize {
        self.state.lock().await.pin_calls
    }

    #[doc(hidden)]
    pub async fn forward_call_count(&self) -> usize {
        self.state.lock().await.forward_calls
    }

    #[doc(hidden)]
    pub async fn reaction_call_count(&self) -> usize {
        self.state.lock().await.reaction_calls
    }

    #[doc(hidden)]
    pub async fn default_reaction_call_count(&self) -> usize {
        self.state.lock().await.default_reaction_calls
    }

    #[doc(hidden)]
    pub async fn make_next_default_reaction_outcome_uncertain(&self) {
        self.state.lock().await.default_reaction_uncertain_once = true;
    }

    #[doc(hidden)]
    pub async fn sticker_send_call_count(&self) -> usize {
        self.state.lock().await.sticker_send_calls
    }

    #[doc(hidden)]
    pub async fn make_next_sticker_send_outcome_uncertain(&self) {
        self.state.lock().await.sticker_send_uncertain_once = true;
    }

    #[doc(hidden)]
    pub async fn business_chat_link_call_counts(&self) -> (usize, usize, usize) {
        let state = self.state.lock().await;
        (
            state.business_chat_link_create_calls,
            state.business_chat_link_edit_calls,
            state.business_chat_link_delete_calls,
        )
    }

    #[doc(hidden)]
    pub async fn make_next_business_chat_link_create_outcome_uncertain(&self) {
        self.state.lock().await.business_chat_link_create_uncertain_once = true;
    }

    #[doc(hidden)]
    pub async fn make_next_business_chat_link_edit_outcome_uncertain(&self) {
        self.state.lock().await.business_chat_link_edit_uncertain_once = true;
    }

    #[doc(hidden)]
    pub async fn make_next_business_chat_link_delete_outcome_uncertain(&self) {
        self.state.lock().await.business_chat_link_delete_uncertain_once = true;
    }

    #[doc(hidden)]
    pub async fn business_settings_call_counts(&self) -> (usize, usize, usize, usize, usize) {
        let state = self.state.lock().await;
        (
            state.business_work_hours_calls,
            state.business_location_calls,
            state.business_greeting_calls,
            state.business_away_calls,
            state.business_intro_calls,
        )
    }

    #[doc(hidden)]
    pub async fn make_next_business_work_hours_outcome_uncertain(&self) {
        self.state.lock().await.business_work_hours_uncertain_once = true;
    }

    #[doc(hidden)]
    pub async fn make_next_business_location_outcome_uncertain(&self) {
        self.state.lock().await.business_location_uncertain_once = true;
    }

    #[doc(hidden)]
    pub async fn make_next_business_greeting_outcome_uncertain(&self) {
        self.state.lock().await.business_greeting_uncertain_once = true;
    }

    #[doc(hidden)]
    pub async fn make_next_business_away_outcome_uncertain(&self) {
        self.state.lock().await.business_away_uncertain_once = true;
    }

    #[doc(hidden)]
    pub async fn make_next_business_intro_outcome_uncertain(&self) {
        self.state.lock().await.business_intro_uncertain_once = true;
    }

    #[doc(hidden)]
    pub async fn star_gift_save_call_count(&self) -> usize {
        self.state.lock().await.star_gift_save_calls
    }

    #[doc(hidden)]
    pub async fn make_next_star_gift_save_outcome_uncertain(&self) {
        self.state.lock().await.star_gift_save_uncertain_once = true;
    }

    #[doc(hidden)]
    pub async fn star_gift_convert_call_count(&self) -> usize {
        self.state.lock().await.star_gift_convert_calls
    }

    #[doc(hidden)]
    pub async fn make_next_star_gift_convert_outcome_uncertain(&self) {
        self.state.lock().await.star_gift_convert_uncertain_once = true;
    }

    #[doc(hidden)]
    pub async fn stars_subscription_change_call_count(&self) -> usize {
        self.state.lock().await.stars_subscription_change_calls
    }

    #[doc(hidden)]
    pub async fn make_next_stars_subscription_change_outcome_uncertain(&self) {
        self.state.lock().await.stars_subscription_change_uncertain_once = true;
    }

    #[doc(hidden)]
    pub async fn sticker_state_call_counts(&self) -> (usize, usize, usize) {
        let state = self.state.lock().await;
        (state.sticker_favorite_calls, state.sticker_install_calls, state.sticker_uninstall_calls)
    }

    #[doc(hidden)]
    pub async fn recent_sticker_call_count(&self) -> usize {
        self.state.lock().await.recent_sticker_calls
    }

    #[doc(hidden)]
    pub async fn make_next_recent_sticker_outcome_uncertain(&self) {
        self.state.lock().await.recent_sticker_uncertain_once = true;
    }

    #[doc(hidden)]
    pub async fn make_next_sticker_favorite_outcome_uncertain(&self) {
        self.state.lock().await.sticker_favorite_uncertain_once = true;
    }

    #[doc(hidden)]
    pub async fn make_next_sticker_install_outcome_uncertain(&self) {
        self.state.lock().await.sticker_install_uncertain_once = true;
    }

    #[doc(hidden)]
    pub async fn rich_media_send_call_count(&self) -> usize {
        self.state.lock().await.rich_media_send_calls
    }

    #[doc(hidden)]
    pub async fn make_next_rich_media_send_outcome_uncertain(&self) {
        self.state.lock().await.rich_media_send_uncertain_once = true;
    }

    #[doc(hidden)]
    pub async fn voice_send_call_count(&self) -> usize {
        self.state.lock().await.voice_send_calls
    }

    #[doc(hidden)]
    pub async fn make_next_voice_send_outcome_uncertain(&self) {
        self.state.lock().await.voice_send_uncertain_once = true;
    }

    #[doc(hidden)]
    pub async fn video_note_send_call_count(&self) -> usize {
        self.state.lock().await.video_note_send_calls
    }

    #[doc(hidden)]
    pub async fn make_next_video_note_send_outcome_uncertain(&self) {
        self.state.lock().await.video_note_send_uncertain_once = true;
    }

    #[doc(hidden)]
    pub async fn album_send_call_count(&self) -> usize {
        self.state.lock().await.album_send_calls
    }

    #[doc(hidden)]
    pub async fn make_next_album_send_outcome_uncertain(&self) {
        self.state.lock().await.album_send_uncertain_once = true;
    }

    #[doc(hidden)]
    pub async fn boost_apply_call_count(&self) -> usize {
        self.state.lock().await.boost_apply_calls
    }

    #[doc(hidden)]
    pub async fn make_next_boost_apply_outcome_uncertain(&self) {
        self.state.lock().await.boost_apply_uncertain_once = true;
    }

    #[doc(hidden)]
    pub async fn translation_call_count(&self) -> usize {
        self.state.lock().await.translate_calls
    }

    #[doc(hidden)]
    pub async fn make_next_translation_outcome_uncertain(&self) {
        self.state.lock().await.translate_uncertain_once = true;
    }

    #[doc(hidden)]
    pub async fn transcription_call_counts(&self) -> (usize, usize, usize) {
        let state = self.state.lock().await;
        (
            state.transcription_start_calls,
            state.transcription_poll_calls,
            state.transcription_rate_calls,
        )
    }

    #[doc(hidden)]
    pub async fn make_next_transcription_outcome_uncertain(&self) {
        self.state.lock().await.transcription_uncertain_once = true;
    }

    #[doc(hidden)]
    pub async fn make_next_transcription_rating_outcome_uncertain(&self) {
        self.state.lock().await.transcription_rate_uncertain_once = true;
    }

    #[doc(hidden)]
    pub async fn transcription_rating(
        &self,
        chat_id: i64,
        message_id: i32,
        transcription_id: &str,
    ) -> Option<bool> {
        self.state
            .lock()
            .await
            .transcription_ratings
            .get(&(chat_id, message_id, transcription_id.to_string()))
            .copied()
    }

    #[doc(hidden)]
    pub async fn fact_check_mutation_call_counts(&self) -> (usize, usize) {
        let state = self.state.lock().await;
        (state.fact_check_edit_calls, state.fact_check_delete_calls)
    }

    #[doc(hidden)]
    pub async fn make_next_fact_check_edit_outcome_uncertain(&self) {
        self.state.lock().await.fact_check_edit_uncertain_once = true;
    }

    #[doc(hidden)]
    pub async fn make_next_fact_check_delete_outcome_uncertain(&self) {
        self.state.lock().await.fact_check_delete_uncertain_once = true;
    }

    #[doc(hidden)]
    pub async fn saved_messages_mutation_call_counts(&self) -> (usize, usize, usize) {
        let state = self.state.lock().await;
        (
            state.saved_history_delete_calls,
            state.saved_dialog_pin_calls,
            state.saved_reaction_tag_update_calls,
        )
    }

    #[doc(hidden)]
    pub async fn make_next_saved_history_delete_outcome_uncertain(&self) {
        self.state.lock().await.saved_history_delete_uncertain_once = true;
    }

    #[doc(hidden)]
    pub async fn make_next_saved_dialog_pin_outcome_uncertain(&self) {
        self.state.lock().await.saved_dialog_pin_uncertain_once = true;
    }

    #[doc(hidden)]
    pub async fn make_next_saved_reaction_tag_update_outcome_uncertain(&self) {
        self.state.lock().await.saved_reaction_tag_update_uncertain_once = true;
    }

    #[doc(hidden)]
    pub async fn typing_actions(&self) -> Vec<(String, TypingAction)> {
        self.state.lock().await.typing_actions.clone()
    }

    #[doc(hidden)]
    pub async fn typing_action_call_count(&self) -> usize {
        self.state.lock().await.typing_action_calls
    }

    #[doc(hidden)]
    pub async fn make_next_typing_action_outcome_uncertain(&self) {
        self.state.lock().await.typing_action_uncertain_once = true;
    }

    #[doc(hidden)]
    pub async fn pressed_callback_buttons(&self) -> Vec<CallbackButtonSnapshot> {
        self.state.lock().await.pressed_callback_buttons.clone()
    }

    #[doc(hidden)]
    pub async fn callback_button_press_call_count(&self) -> usize {
        self.state.lock().await.callback_button_press_calls
    }

    #[doc(hidden)]
    pub async fn make_next_callback_button_press_outcome_uncertain(&self) {
        self.state.lock().await.callback_button_press_uncertain_once = true;
    }

    #[doc(hidden)]
    pub async fn inline_query_call_count(&self) -> usize {
        self.state.lock().await.inline_query_calls
    }

    #[doc(hidden)]
    pub async fn sent_inline_results(&self) -> Vec<FakeInlineBotResultSend> {
        self.state.lock().await.sent_inline_results.clone()
    }

    #[doc(hidden)]
    pub async fn inline_result_send_call_count(&self) -> usize {
        self.state.lock().await.inline_result_send_calls
    }

    #[doc(hidden)]
    pub async fn make_next_inline_result_send_outcome_uncertain(&self) {
        self.state.lock().await.inline_result_send_uncertain_once = true;
    }

    #[doc(hidden)]
    pub async fn set_fixture_account_bot(&self, is_bot: bool) {
        self.state.lock().await.account_is_bot = is_bot;
    }

    #[doc(hidden)]
    pub async fn bot_command_set_call_count(&self) -> usize {
        self.state.lock().await.bot_command_set_calls
    }

    #[doc(hidden)]
    pub async fn make_next_bot_command_set_outcome_uncertain(&self) {
        self.state.lock().await.bot_command_set_uncertain_once = true;
    }

    #[doc(hidden)]
    pub async fn set_fixture_account_emoji_status(&self, status: EmojiStatusSummary) {
        self.state.lock().await.account_emoji_status = status;
    }

    #[doc(hidden)]
    pub async fn set_fixture_emoji_status_catalog(
        &self,
        kind: EmojiStatusCatalogKind,
        hash: i64,
        statuses: Vec<EmojiStatusSummary>,
    ) {
        self.state.lock().await.emoji_status_catalogs.insert(kind, (hash, statuses));
    }

    #[doc(hidden)]
    pub async fn emoji_status_set_call_count(&self) -> usize {
        self.state.lock().await.emoji_status_set_calls
    }

    #[doc(hidden)]
    pub async fn make_next_emoji_status_set_outcome_uncertain(&self) {
        self.state.lock().await.emoji_status_set_uncertain_once = true;
    }

    #[doc(hidden)]
    pub async fn emoji_recent_clear_call_count(&self) -> usize {
        self.state.lock().await.emoji_recent_clear_calls
    }

    #[doc(hidden)]
    pub async fn make_next_emoji_recent_clear_outcome_uncertain(&self) {
        self.state.lock().await.emoji_recent_clear_uncertain_once = true;
    }

    #[doc(hidden)]
    pub async fn set_fixture_account_peer_color(
        &self,
        for_profile: bool,
        color: Option<PeerColorSummary>,
    ) {
        let mut state = self.state.lock().await;
        if for_profile {
            state.account_profile.profile_color = color;
        } else {
            state.account_profile.name_color = color;
        }
    }

    #[doc(hidden)]
    pub async fn set_fixture_personal_channel(&self, channel: Option<PersonalChannelSummary>) {
        self.state.lock().await.account_profile.personal_channel = channel;
    }

    #[doc(hidden)]
    pub async fn account_identity_call_counts(&self) -> (usize, usize) {
        let state = self.state.lock().await;
        (state.account_color_update_calls, state.personal_channel_update_calls)
    }

    #[doc(hidden)]
    pub async fn make_next_account_color_outcome_uncertain(&self) {
        self.state.lock().await.account_color_update_uncertain_once = true;
    }

    #[doc(hidden)]
    pub async fn make_next_personal_channel_outcome_uncertain(&self) {
        self.state.lock().await.personal_channel_update_uncertain_once = true;
    }

    #[doc(hidden)]
    pub async fn set_fixture_draft(&self, chat_id: &str, text: Option<&str>) {
        let mut state = self.state.lock().await;
        let Some(canonical_id) = Self::resolve_dialog(&state.dialogs, chat_id) else {
            return;
        };
        match text {
            Some(text) => {
                state.drafts.insert(
                    canonical_id,
                    finalize_fixture_draft(DraftSummary {
                        message: text.to_string(),
                        date_unix: 1_700_009_999,
                        no_webpage: false,
                        invert_media: false,
                        has_reply_to: false,
                        has_entities: false,
                        has_media: false,
                        effect_id: None,
                        has_suggested_post: false,
                        has_rich_message: false,
                        state_sha256: String::new(),
                    }),
                );
            },
            None => {
                state.drafts.remove(&canonical_id);
            },
        }
    }

    #[doc(hidden)]
    pub async fn set_fixture_draft_summary(&self, chat_id: &str, draft: Option<DraftSummary>) {
        let mut state = self.state.lock().await;
        let Some(canonical_id) = Self::resolve_dialog(&state.dialogs, chat_id) else {
            return;
        };
        match draft {
            Some(draft) => {
                state.drafts.insert(canonical_id, draft);
            },
            None => {
                state.drafts.remove(&canonical_id);
            },
        }
    }

    #[doc(hidden)]
    pub async fn draft_call_count(&self) -> usize {
        self.state.lock().await.draft_calls
    }

    #[doc(hidden)]
    pub async fn draft_clear_all_call_count(&self) -> usize {
        self.state.lock().await.draft_clear_all_calls
    }

    #[doc(hidden)]
    pub async fn make_next_draft_clear_all_outcome_uncertain(&self) {
        self.state.lock().await.draft_clear_all_uncertain_once = true;
    }

    #[doc(hidden)]
    pub async fn dialog_state_call_counts(&self) -> (usize, usize, usize) {
        let state = self.state.lock().await;
        (state.dialog_unread_calls, state.dialog_archive_calls, state.dialog_pin_calls)
    }

    #[doc(hidden)]
    pub async fn message_read_call_counts(&self) -> (usize, usize) {
        let state = self.state.lock().await;
        (state.mark_messages_read_calls, state.mark_reactions_read_calls)
    }

    #[doc(hidden)]
    pub async fn chat_lifecycle_call_counts(&self) -> (usize, usize, usize, usize, usize, usize) {
        let state = self.state.lock().await;
        (
            state.chat_create_calls,
            state.chat_join_calls,
            state.chat_leave_calls,
            state.chat_delete_calls,
            state.chat_title_calls,
            state.chat_about_calls,
        )
    }

    #[doc(hidden)]
    pub async fn chat_photo_call_counts(&self) -> (usize, usize) {
        let state = self.state.lock().await;
        (state.chat_photo_update_calls, state.chat_photo_delete_calls)
    }

    #[doc(hidden)]
    pub async fn channel_settings_call_counts(&self) -> (usize, usize, usize, usize, usize) {
        let state = self.state.lock().await;
        (
            state.chat_username_calls,
            state.chat_slow_mode_calls,
            state.chat_signatures_calls,
            state.chat_pre_history_calls,
            state.chat_join_request_calls,
        )
    }

    #[doc(hidden)]
    pub async fn chat_moderation_call_counts(&self) -> (usize, usize, usize, usize) {
        let state = self.state.lock().await;
        (
            state.chat_permissions_calls,
            state.chat_reactions_policy_calls,
            state.chat_anti_spam_calls,
            state.chat_forum_mode_calls,
        )
    }

    #[doc(hidden)]
    pub async fn make_next_chat_permissions_outcome_uncertain(&self) {
        self.state.lock().await.chat_permissions_uncertain_once = true;
    }

    #[doc(hidden)]
    pub async fn make_next_chat_reactions_outcome_uncertain(&self) {
        self.state.lock().await.chat_reactions_policy_uncertain_once = true;
    }

    #[doc(hidden)]
    pub async fn make_next_chat_anti_spam_outcome_uncertain(&self) {
        self.state.lock().await.chat_anti_spam_uncertain_once = true;
    }

    #[doc(hidden)]
    pub async fn make_next_chat_forum_outcome_uncertain(&self) {
        self.state.lock().await.chat_forum_mode_uncertain_once = true;
    }

    #[doc(hidden)]
    pub async fn story_view_call_count(&self) -> usize {
        self.state.lock().await.story_view_calls
    }

    #[doc(hidden)]
    pub async fn story_mutation_call_counts(&self) -> (usize, usize, usize) {
        let state = self.state.lock().await;
        (state.story_view_calls, state.story_send_calls, state.story_delete_calls)
    }

    #[doc(hidden)]
    pub async fn story_extended_call_counts(&self) -> (usize, usize, usize, usize, usize) {
        let state = self.state.lock().await;
        (
            state.story_read_calls,
            state.story_reaction_calls,
            state.story_pin_calls,
            state.story_top_pin_calls,
            state.story_stealth_calls,
        )
    }

    #[doc(hidden)]
    pub async fn story_edit_call_count(&self) -> usize {
        self.state.lock().await.story_edit_calls
    }

    #[doc(hidden)]
    pub async fn story_report_call_count(&self) -> usize {
        self.state.lock().await.story_report_calls
    }

    #[doc(hidden)]
    pub async fn set_story_report_uncertain_once(&self) {
        self.state.lock().await.story_report_uncertain_once = true;
    }

    #[doc(hidden)]
    pub async fn block_call_count(&self) -> usize {
        self.state.lock().await.block_calls
    }

    #[doc(hidden)]
    pub async fn session_terminate_call_count(&self) -> usize {
        self.state.lock().await.session_terminate_calls
    }

    #[doc(hidden)]
    pub async fn set_fixture_sessions(&self, sessions: Vec<SessionSummary>) {
        self.state.lock().await.sessions = sessions;
    }

    #[doc(hidden)]
    pub async fn session_reset_call_count(&self) -> usize {
        self.state.lock().await.session_reset_calls
    }

    #[doc(hidden)]
    pub async fn make_next_session_reset_outcome_uncertain(&self) {
        self.state.lock().await.session_reset_uncertain_once = true;
    }

    #[doc(hidden)]
    pub async fn set_fixture_chat_auto_delete(&self, chat_id: &str, period_seconds: i32) {
        let mut state = self.state.lock().await;
        if let Some(chat_id) = Self::resolve_dialog(&state.dialogs, chat_id) {
            state.chat_auto_delete.insert(chat_id, period_seconds);
        }
    }

    #[doc(hidden)]
    pub async fn chat_auto_delete_call_count(&self) -> usize {
        self.state.lock().await.chat_auto_delete_calls
    }

    #[doc(hidden)]
    pub async fn make_next_chat_auto_delete_outcome_uncertain(&self) {
        self.state.lock().await.chat_auto_delete_uncertain_once = true;
    }

    #[doc(hidden)]
    pub async fn contact_call_count(&self) -> usize {
        self.state.lock().await.contact_calls
    }

    #[doc(hidden)]
    pub async fn privacy_call_count(&self) -> usize {
        self.state.lock().await.privacy_calls
    }

    #[doc(hidden)]
    pub async fn notification_call_count(&self) -> usize {
        self.state.lock().await.notification_calls
    }

    #[doc(hidden)]
    pub async fn scheduled_call_counts(&self) -> (usize, usize, usize, usize) {
        let state = self.state.lock().await;
        (
            state.schedule_create_calls,
            state.schedule_edit_calls,
            state.schedule_delete_calls,
            state.schedule_send_now_calls,
        )
    }

    #[doc(hidden)]
    pub async fn poll_call_counts(&self) -> (usize, usize, usize) {
        let state = self.state.lock().await;
        (state.poll_create_calls, state.poll_vote_calls, state.poll_stop_calls)
    }

    #[doc(hidden)]
    pub async fn forum_topic_call_counts(&self) -> (usize, usize, usize, usize) {
        let state = self.state.lock().await;
        (
            state.forum_topic_create_calls,
            state.forum_topic_edit_calls,
            state.forum_topic_pin_calls,
            state.forum_topic_delete_calls,
        )
    }

    #[doc(hidden)]
    pub async fn chat_member_call_counts(&self) -> (usize, usize, usize, usize) {
        let state = self.state.lock().await;
        (
            state.chat_member_add_calls,
            state.chat_member_remove_calls,
            state.chat_member_admin_calls,
            state.chat_member_restriction_calls,
        )
    }

    #[doc(hidden)]
    pub async fn add_fixture_contact(&self, contact: ContactSummary) {
        let mut state = self.state.lock().await;
        state.contacts.retain(|existing| existing.user_id != contact.user_id);
        state.contacts.push(contact);
    }

    /// Add a dialog to the fixture so a test can resolve an extra peer by
    /// username or ref (for example `@BotFather`, which the owner grant must
    /// never cover). A user dialog also needs a matching contact for
    /// `get_user_info`; add it separately with `add_fixture_contact`.
    #[doc(hidden)]
    pub async fn add_fixture_dialog(&self, dialog: DialogSummary) {
        let mut state = self.state.lock().await;
        state.dialogs.retain(|existing| existing.chat_id != dialog.chat_id);
        state.dialogs.push(dialog);
    }

    #[doc(hidden)]
    pub async fn set_fixture_chat_member(
        &self,
        chat_id: &str,
        user_id: i64,
        member: Option<ChatMemberSummary>,
    ) {
        let mut state = self.state.lock().await;
        let Some(canonical_id) = Self::resolve_known_chat(&state, chat_id) else {
            return;
        };
        match member {
            Some(member) => {
                if member.user.user_id != user_id {
                    return;
                }
                state.chat_members.insert((canonical_id, member.user.user_id), member);
            },
            None => {
                state.chat_members.remove(&(canonical_id, user_id));
            },
        }
    }

    #[doc(hidden)]
    pub async fn script_chat_member_invites(
        &self,
        chat_id: &str,
        user_id: i64,
        outcomes: Vec<FakeInviteOutcome>,
    ) {
        let mut state = self.state.lock().await;
        let Some(canonical_id) = Self::resolve_known_chat(&state, chat_id) else {
            return;
        };
        state.chat_member_invite_scripts.insert((canonical_id, user_id), outcomes.into());
    }

    #[doc(hidden)]
    pub async fn chat_member_invite_calls(&self) -> Vec<(String, i64)> {
        self.state.lock().await.chat_member_invite_calls.clone()
    }

    #[doc(hidden)]
    pub async fn invite_link_call_counts(&self) -> (usize, usize, usize) {
        let state = self.state.lock().await;
        (state.invite_export_calls, state.invite_revoke_calls, state.invite_delete_calls)
    }

    #[doc(hidden)]
    pub async fn join_request_call_count(&self) -> usize {
        self.state.lock().await.join_request_calls
    }

    #[doc(hidden)]
    pub async fn account_profile_call_counts(&self) -> (usize, usize, usize, usize) {
        let state = self.state.lock().await;
        (
            state.profile_update_calls,
            state.username_update_calls,
            state.birthday_update_calls,
            state.presence_update_calls,
        )
    }

    #[doc(hidden)]
    pub async fn profile_photo_call_counts(&self) -> (usize, usize) {
        let state = self.state.lock().await;
        (state.profile_photo_upload_calls, state.profile_photo_delete_calls)
    }

    #[doc(hidden)]
    pub async fn dialog_folder_call_counts(&self) -> (usize, usize, usize, usize) {
        let state = self.state.lock().await;
        (
            state.dialog_folder_upsert_calls,
            state.dialog_folder_delete_calls,
            state.dialog_folder_reorder_calls,
            state.dialog_folder_membership_calls,
        )
    }

    #[doc(hidden)]
    pub async fn folder_privacy_call_counts(&self) -> (usize, usize) {
        let state = self.state.lock().await;
        (state.dialog_folder_tags_calls, state.global_privacy_update_calls)
    }

    #[doc(hidden)]
    pub async fn set_fixture_dialog_folder_tags(&self, enabled: bool) {
        self.state.lock().await.dialog_folders.tags_enabled = enabled;
    }

    #[doc(hidden)]
    pub async fn set_fixture_suggested_dialog_folders(
        &self,
        suggestions: SuggestedDialogFolderList,
    ) {
        self.state.lock().await.suggested_dialog_folders = suggestions;
    }

    #[doc(hidden)]
    pub async fn set_fixture_global_privacy_settings(
        &self,
        settings: GlobalPrivacySettingsSummary,
    ) {
        self.state.lock().await.global_privacy_settings = settings;
    }

    #[doc(hidden)]
    pub async fn make_next_dialog_folder_tags_outcome_uncertain(&self) {
        self.state.lock().await.dialog_folder_tags_uncertain_once = true;
    }

    #[doc(hidden)]
    pub async fn make_next_global_privacy_outcome_uncertain(&self) {
        self.state.lock().await.global_privacy_update_uncertain_once = true;
    }

    #[doc(hidden)]
    pub async fn add_fixture_join_request(&self, chat_id: i64, user_id: i64, date_unix: i32) {
        let mut state = self.state.lock().await;
        state.join_requests.entry(chat_id).or_default().push(fixture_invite_importer(
            ContactSummary {
                user_id,
                first_name: format!("Fixture {user_id}"),
                last_name: None,
                display_name: format!("Fixture {user_id}"),
                username: Some(format!("fixture_join_{user_id}")),
                phone: None,
                is_contact: false,
                mutual_contact: false,
                bot: false,
                verified: false,
                premium: false,
                scam: false,
                restricted: false,
            },
            None,
            true,
            date_unix,
            Some("arrived after mutation preparation"),
            None,
        ));
    }

    fn resolve_dialog(dialogs: &[DialogSummary], identifier: &str) -> Option<i64> {
        let wanted = identifier.trim().trim_start_matches('@');
        dialogs.iter().find_map(|dialog| {
            let id_matches =
                wanted.parse::<i64>().is_ok_and(|peer_id| match dialog.chat_kind.as_str() {
                    "channel" => {
                        peer_id
                            .to_string()
                            .strip_prefix("-100")
                            .and_then(|value| value.parse::<i64>().ok())
                            == Some(dialog.chat_id)
                    },
                    "group" => peer_id.checked_abs() == Some(dialog.chat_id) && peer_id < 0,
                    _ => peer_id == dialog.chat_id && peer_id > 0,
                });
            let ref_matches = dialog.chat_ref.trim_start_matches('@').eq_ignore_ascii_case(wanted);
            let canonical_matches = dialog.canonical_chat_id == wanted;
            let username_matches = dialog
                .username
                .as_deref()
                .is_some_and(|username| username.eq_ignore_ascii_case(wanted));
            (id_matches || canonical_matches || ref_matches || username_matches)
                .then_some(dialog.chat_id)
        })
    }

    fn canonical_chat_ref(dialogs: &[DialogSummary], chat_id: i64) -> String {
        dialogs
            .iter()
            .find(|dialog| dialog.chat_id == chat_id)
            .map_or_else(|| chat_id.to_string(), |dialog| dialog.canonical_chat_id.clone())
    }

    fn resolve_known_chat(state: &FakeState, identifier: &str) -> Option<i64> {
        if let Some(chat_id) = Self::resolve_dialog(&state.dialogs, identifier) {
            return Some(chat_id);
        }
        let wanted = identifier.trim().trim_start_matches('@');
        state.known_chats.iter().find_map(|(chat_id, details)| {
            let id_matches = details.chat.chat_id == wanted;
            let username_matches = details
                .chat
                .username
                .as_deref()
                .is_some_and(|username| username.eq_ignore_ascii_case(wanted));
            (id_matches || username_matches).then_some(*chat_id)
        })
    }

    fn canonical_stars_peer_ref(
        state: &FakeState,
        peer_ref: &str,
    ) -> Result<String, TelegramError> {
        if ["me", "@me", "self", "@self"]
            .iter()
            .any(|alias| peer_ref.trim().eq_ignore_ascii_case(alias))
            || peer_ref.trim() == state.account.user_id.to_string()
        {
            return Ok(state.account.user_id.to_string());
        }
        let chat_id = Self::resolve_known_chat(state, peer_ref)
            .ok_or_else(|| TelegramError::PeerNotFound(peer_ref.to_string()))?;
        Ok(state
            .known_chats
            .get(&chat_id)
            .map(|details| details.chat.chat_id.clone())
            .unwrap_or_else(|| Self::canonical_chat_ref(&state.dialogs, chat_id)))
    }

    fn saved_star_gift_wallet_peer_ref(
        state: &FakeState,
        reference: &SavedStarGiftRef,
    ) -> Result<String, TelegramError> {
        match reference {
            SavedStarGiftRef::UserMessage { message_id } => {
                if *message_id <= 0 {
                    return Err(TelegramError::InvalidPeer(
                        "saved Star Gift message_id must be positive".to_string(),
                    ));
                }
                Ok(state.account.user_id.to_string())
            },
            SavedStarGiftRef::ChatSaved { chat_id, saved_id } => {
                let parsed = saved_id.parse::<i64>().map_err(|_| {
                    TelegramError::InvalidPeer(
                        "saved Star Gift saved_id must be a signed 64-bit decimal integer"
                            .to_string(),
                    )
                })?;
                if parsed <= 0 {
                    return Err(TelegramError::InvalidPeer(
                        "saved Star Gift saved_id must be positive".to_string(),
                    ));
                }
                Self::canonical_stars_peer_ref(state, chat_id)
            },
        }
    }

    fn saved_star_gift_matches(gift: &SavedStarGiftSummary, reference: &SavedStarGiftRef) -> bool {
        match reference {
            SavedStarGiftRef::UserMessage { message_id } => gift.message_id == Some(*message_id),
            SavedStarGiftRef::ChatSaved { saved_id, .. } => {
                gift.saved_id.as_deref() == Some(saved_id.as_str())
            },
        }
    }

    fn story_mut(state: &mut FakeState, chat_id: i64, story_id: i32) -> Option<&mut StorySummary> {
        if state
            .stories
            .get(&chat_id)
            .is_some_and(|stories| stories.iter().any(|story| story.story_id == story_id))
        {
            return state
                .stories
                .get_mut(&chat_id)
                .and_then(|stories| stories.iter_mut().find(|story| story.story_id == story_id));
        }
        state
            .story_archive
            .get_mut(&chat_id)
            .and_then(|stories| stories.iter_mut().find(|story| story.story_id == story_id))
    }

    fn dialog_from_chat(details: &ChatDetailsSummary) -> Result<DialogSummary, TelegramError> {
        let chat_id = details
            .chat
            .chat_id
            .strip_prefix("-100")
            .or_else(|| details.chat.chat_id.strip_prefix('-'))
            .unwrap_or(details.chat.chat_id.as_str())
            .parse::<i64>()
            .map_err(|_| TelegramError::InvalidPeer(details.chat.chat_id.clone()))?;
        Ok(DialogSummary {
            chat_id,
            canonical_chat_id: details.chat.chat_id.clone(),
            chat_ref: details
                .chat
                .username
                .as_ref()
                .map_or_else(|| details.chat.chat_id.clone(), |username| format!("@{username}")),
            chat_kind: details.chat.kind.clone(),
            title: details.chat.title.clone(),
            username: details.chat.username.clone(),
            last_message_id: None,
            last_message_text: None,
            last_message_outgoing: None,
            last_message_date: None,
            pinned: false,
            unread_mark: false,
            unread_count: 0,
            archived: false,
            folder_id: Some(0),
        })
    }

    fn channel_settings_snapshot(
        state: &FakeState,
        chat_id: i64,
    ) -> Result<ChannelSettingsSummary, TelegramError> {
        let details = state
            .known_chats
            .get(&chat_id)
            .cloned()
            .ok_or_else(|| TelegramError::PeerNotFound(chat_id.to_string()))?;
        let settings = state.channel_settings.get(&chat_id).ok_or_else(|| {
            TelegramError::InvalidPeer(
                "channel settings require a channel, supergroup, or gigagroup".to_string(),
            )
        })?;
        Ok(ChannelSettingsSummary {
            details,
            can_set_username: settings.can_set_username,
            can_view_stats: settings.can_view_stats,
            signatures_enabled: settings.signatures_enabled,
            signature_profiles_enabled: settings.signature_profiles_enabled,
            pre_history_hidden: settings.pre_history_hidden,
            join_request_required: settings.join_request_required,
            join_to_send_required: settings.join_to_send_required,
            slow_mode_seconds: settings.slow_mode_seconds,
            no_forwards: settings.no_forwards,
            forum_enabled: settings.forum_enabled,
            history_ttl_seconds: settings.history_ttl_seconds,
            linked_chat_id: settings.linked_chat_id.clone(),
        })
    }

    fn chat_moderation_settings_snapshot(
        state: &FakeState,
        chat_id: i64,
    ) -> Result<ChatModerationSettingsSummary, TelegramError> {
        let details = state
            .known_chats
            .get(&chat_id)
            .cloned()
            .ok_or_else(|| TelegramError::PeerNotFound(chat_id.to_string()))?;
        let settings = state.chat_moderation_settings.get(&chat_id).ok_or_else(|| {
            TelegramError::InvalidPeer(
                "chat moderation settings require a group, supergroup, or channel".to_string(),
            )
        })?;
        Ok(ChatModerationSettingsSummary {
            details,
            default_banned_rights: settings.default_banned_rights.clone(),
            available_reactions: settings.available_reactions.clone(),
            reactions_limit: settings.reactions_limit,
            anti_spam_enabled: settings.anti_spam_enabled,
            forum_enabled: settings.forum_enabled,
            forum_tabs_enabled: settings.forum_tabs_enabled,
        })
    }

    fn unread_fixture_page(
        state: &FakeState,
        request: &UnreadMessagesRequest,
        predicate: impl Fn(&MessageSummary) -> bool,
    ) -> Result<MessagePage, TelegramError> {
        let canonical_id = Self::resolve_dialog(&state.dialogs, &request.chat_id)
            .ok_or_else(|| TelegramError::PeerNotFound(request.chat_id.clone()))?;
        if let Some(saved_peer_id) = request.saved_peer_id.as_deref() {
            Self::resolve_known_chat(state, saved_peer_id)
                .ok_or_else(|| TelegramError::PeerNotFound(saved_peer_id.to_string()))?;
        }
        let skip = usize::try_from(request.add_offset.max(0)).unwrap_or(usize::MAX);
        let mut matching = state
            .messages
            .get(&canonical_id)
            .into_iter()
            .flatten()
            .filter(|message| {
                request.top_message_id.is_none_or(|top| message.reply_to_message_id == Some(top))
                    && (request.offset_id <= 0 || message.message_id < request.offset_id)
                    && (request.max_id <= 0 || message.message_id <= request.max_id)
                    && (request.min_id <= 0 || message.message_id >= request.min_id)
                    && predicate(message)
            })
            .cloned()
            .collect::<Vec<_>>();
        matching.sort_by_key(|right| std::cmp::Reverse(right.message_id));
        let total_matching = matching.len().saturating_sub(skip);
        let messages = matching.into_iter().skip(skip).take(request.limit).collect::<Vec<_>>();
        let next_offset_id = (messages.len() < total_matching)
            .then(|| messages.last().map(|message| message.message_id))
            .flatten();
        let completeness =
            if next_offset_id.is_some() { Completeness::Page } else { Completeness::Complete };
        Ok(MessagePage { messages, next_offset_id, completeness })
    }
}

#[async_trait]
impl TelegramBackend for FakeTelegramBackend {
    async fn get_me(
        &self,
        _session_path: Option<PathBuf>,
    ) -> Result<(UserInfo, PathBuf), TelegramError> {
        let state = self.state.lock().await;
        Ok((state.account.clone(), state.session_path.clone()))
    }

    async fn resolve_chat_id(
        &self,
        _session_path: Option<PathBuf>,
        chat_id: &str,
    ) -> Result<String, TelegramError> {
        let state = self.state.lock().await;
        if ["me", "@me", "self", "@self"]
            .iter()
            .any(|alias| chat_id.trim().eq_ignore_ascii_case(alias))
            || chat_id.trim() == state.account.user_id.to_string()
        {
            return Ok(state.account.user_id.to_string());
        }
        let canonical_id = Self::resolve_known_chat(&state, chat_id)
            .ok_or_else(|| TelegramError::PeerNotFound(chat_id.to_string()))?;
        Ok(state.known_chats.get(&canonical_id).map_or_else(
            || Self::canonical_chat_ref(&state.dialogs, canonical_id),
            |details| details.chat.chat_id.clone(),
        ))
    }

    async fn send_typing_action(
        &self,
        _session_path: Option<PathBuf>,
        chat_id: &str,
        action: TypingAction,
    ) -> Result<(), TelegramError> {
        let mut state = self.state.lock().await;
        let canonical_id = Self::resolve_dialog(&state.dialogs, chat_id)
            .ok_or_else(|| TelegramError::PeerNotFound(chat_id.to_string()))?;
        let canonical_chat_id = Self::canonical_chat_ref(&state.dialogs, canonical_id);
        state.typing_action_calls = state.typing_action_calls.saturating_add(1);
        state.typing_actions.push((canonical_chat_id, action));
        if state.typing_action_uncertain_once {
            state.typing_action_uncertain_once = false;
            return Err(TelegramError::DeliveryUncertain(
                "fixture accepted typing action before the transport receipt was lost".to_string(),
            ));
        }
        Ok(())
    }

    async fn list_dialogs(
        &self,
        _session_path: Option<PathBuf>,
        limit: usize,
    ) -> Result<Vec<DialogSummary>, TelegramError> {
        let mut state = self.state.lock().await;
        state.list_dialogs_calls = state.list_dialogs_calls.saturating_add(1);
        Ok(state.dialogs.iter().take(limit).cloned().collect())
    }

    async fn history(
        &self,
        _session_path: Option<PathBuf>,
        chat_id: &str,
        limit: usize,
        offset_id: Option<i32>,
        add_offset: i32,
    ) -> Result<Vec<MessageSummary>, TelegramError> {
        let mut state = self.state.lock().await;
        state.history_calls = state.history_calls.saturating_add(1);
        let canonical_id = Self::resolve_dialog(&state.dialogs, chat_id)
            .ok_or_else(|| TelegramError::PeerNotFound(chat_id.to_string()))?;
        let messages = state.messages.get(&canonical_id).cloned().unwrap_or_default();
        let base = offset_id
            .and_then(|offset| messages.iter().position(|message| message.message_id == offset))
            .map_or(0, |index| index.saturating_add(1));
        let offset = usize::try_from(add_offset.unsigned_abs()).unwrap_or(usize::MAX);
        let start = if add_offset.is_negative() {
            base.saturating_sub(offset)
        } else {
            base.saturating_add(offset)
        };
        let page = messages.into_iter().skip(start).take(limit).collect::<Vec<_>>();
        state.history_rows_returned = state.history_rows_returned.saturating_add(page.len());
        Ok(page)
    }

    async fn history_newer_than(
        &self,
        _session_path: Option<PathBuf>,
        chat_id: &str,
        min_message_id: i32,
        limit: usize,
        offset_id: Option<i32>,
    ) -> Result<Vec<MessageSummary>, TelegramError> {
        let mut state = self.state.lock().await;
        state.history_calls = state.history_calls.saturating_add(1);
        let canonical_id = Self::resolve_dialog(&state.dialogs, chat_id)
            .ok_or_else(|| TelegramError::PeerNotFound(chat_id.to_string()))?;
        let messages = state.messages.get(&canonical_id).cloned().unwrap_or_default();
        let base = offset_id
            .and_then(|offset| messages.iter().position(|message| message.message_id == offset))
            .map_or(0, |index| index.saturating_add(1));
        let page = messages
            .into_iter()
            .skip(base)
            .filter(|message| message.message_id > min_message_id)
            .take(limit)
            .collect::<Vec<_>>();
        state.history_rows_returned = state.history_rows_returned.saturating_add(page.len());
        Ok(page)
    }

    async fn get_updates_state(
        &self,
        _session_path: Option<PathBuf>,
    ) -> Result<UpdatesStateSummary, TelegramError> {
        Ok(UpdatesStateSummary {
            pts: FIXTURE_UPDATES_PTS,
            qts: FIXTURE_UPDATES_QTS,
            date: FIXTURE_UPDATES_DATE,
            seq: FIXTURE_UPDATES_SEQ,
            unread_count: 2,
        })
    }

    async fn get_updates_difference(
        &self,
        request: UpdatesDifferenceRequest,
    ) -> Result<UpdatesDifferenceSummary, TelegramError> {
        let state = self.state.lock().await;
        let request_summary = UpdatesDifferenceRequestSummary {
            pts: request.pts,
            qts: request.qts,
            date: request.date,
            pts_limit: request.pts_limit,
            pts_total_limit: request.pts_total_limit,
            qts_limit: request.qts_limit,
        };
        let related_chats = [202_i64, 505]
            .into_iter()
            .filter_map(|chat_id| state.known_chats.get(&chat_id).map(|chat| chat.chat.clone()))
            .collect::<Vec<_>>();
        let related_users = state.contacts.clone();
        let message = |chat_id: i64, message_id: i32| {
            state
                .messages
                .get(&chat_id)
                .and_then(|messages| {
                    messages.iter().find(|message| message.message_id == message_id)
                })
                .cloned()
        };

        Ok(match request.pts {
            FIXTURE_UPDATES_PTS.. => UpdatesDifferenceSummary {
                request: request_summary,
                kind: UpdatesDifferenceKind::Empty,
                is_final: true,
                continuation_required: false,
                requires_resync: false,
                next_cursor: Some(UpdatesCursorSummary {
                    pts: request.pts,
                    qts: request.qts,
                    date: FIXTURE_UPDATES_DATE,
                    seq: FIXTURE_UPDATES_SEQ,
                    unread_count: None,
                }),
                resync_pts: None,
                new_messages: Vec::new(),
                edited_messages: Vec::new(),
                encrypted_message_count: 0,
                deleted_messages: Vec::new(),
                other_updates: Vec::new(),
                related_chats: Vec::new(),
                related_users: Vec::new(),
            },
            119 => UpdatesDifferenceSummary {
                request: request_summary,
                kind: UpdatesDifferenceKind::Complete,
                is_final: true,
                continuation_required: false,
                requires_resync: false,
                next_cursor: Some(UpdatesCursorSummary {
                    pts: FIXTURE_UPDATES_PTS,
                    qts: FIXTURE_UPDATES_QTS,
                    date: FIXTURE_UPDATES_DATE,
                    seq: FIXTURE_UPDATES_SEQ,
                    unread_count: Some(2),
                }),
                resync_pts: None,
                new_messages: message(101, 11).into_iter().collect(),
                edited_messages: Vec::new(),
                encrypted_message_count: 1,
                deleted_messages: vec![
                    DeletedMessagesSummary::Global {
                        message_ids: vec![41, 42],
                        pts: FIXTURE_UPDATES_PTS,
                        pts_count: 2,
                    },
                    DeletedMessagesSummary::Channel {
                        chat_id: "-100505".to_string(),
                        message_ids: vec![49],
                        pts: FIXTURE_CHANNEL_UPDATES_PTS,
                        pts_count: 1,
                    },
                ],
                other_updates: vec![UpdateKindSummary { kind: "user_status".to_string() }],
                related_chats,
                related_users,
            },
            118 => UpdatesDifferenceSummary {
                request: request_summary,
                kind: UpdatesDifferenceKind::Slice,
                is_final: false,
                continuation_required: true,
                requires_resync: false,
                next_cursor: Some(UpdatesCursorSummary {
                    pts: 119,
                    qts: FIXTURE_UPDATES_QTS,
                    date: FIXTURE_UPDATES_DATE - 1,
                    seq: FIXTURE_UPDATES_SEQ - 1,
                    unread_count: Some(3),
                }),
                resync_pts: None,
                new_messages: message(101, 10).into_iter().collect(),
                edited_messages: Vec::new(),
                encrypted_message_count: 0,
                deleted_messages: Vec::new(),
                other_updates: vec![UpdateKindSummary { kind: "draft_message".to_string() }],
                related_chats,
                related_users,
            },
            _ => UpdatesDifferenceSummary {
                request: request_summary,
                kind: UpdatesDifferenceKind::TooLong,
                is_final: false,
                continuation_required: true,
                requires_resync: true,
                next_cursor: None,
                resync_pts: Some(FIXTURE_UPDATES_PTS),
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

    async fn get_channel_updates(
        &self,
        request: ChannelUpdatesRequest,
    ) -> Result<ChannelUpdatesDifferenceSummary, TelegramError> {
        let state = self.state.lock().await;
        let canonical_id = Self::resolve_known_chat(&state, &request.chat_id)
            .ok_or_else(|| TelegramError::PeerNotFound(request.chat_id.clone()))?;
        let details = state
            .known_chats
            .get(&canonical_id)
            .ok_or_else(|| TelegramError::PeerNotFound(request.chat_id.clone()))?;
        if !matches!(details.chat.kind.as_str(), "channel" | "supergroup" | "gigagroup") {
            return Err(TelegramError::InvalidPeer(
                "get_channel_updates requires a channel or supergroup".to_string(),
            ));
        }
        let request_summary = ChannelUpdatesRequestSummary {
            chat_id: details.chat.chat_id.clone(),
            pts: request.pts,
            limit: request.limit,
            force: request.force,
        };
        let fixture_message =
            state.messages.get(&canonical_id).and_then(|messages| messages.first()).cloned();
        let related_chats = vec![details.chat.clone()];
        let related_users = state.contacts.clone();

        Ok(match request.pts {
            FIXTURE_CHANNEL_UPDATES_PTS.. => ChannelUpdatesDifferenceSummary {
                request: request_summary,
                kind: ChannelUpdatesDifferenceKind::Empty,
                is_final: true,
                continuation_required: false,
                requires_resync: false,
                next_pts: Some(request.pts),
                resync_pts: None,
                timeout_seconds: Some(30),
                new_messages: Vec::new(),
                edited_messages: Vec::new(),
                snapshot_messages: Vec::new(),
                deleted_messages: Vec::new(),
                other_updates: Vec::new(),
                related_chats: Vec::new(),
                related_users: Vec::new(),
            },
            49 => ChannelUpdatesDifferenceSummary {
                request: request_summary,
                kind: ChannelUpdatesDifferenceKind::Difference,
                is_final: true,
                continuation_required: false,
                requires_resync: false,
                next_pts: Some(FIXTURE_CHANNEL_UPDATES_PTS),
                resync_pts: None,
                timeout_seconds: Some(30),
                new_messages: fixture_message.into_iter().collect(),
                edited_messages: Vec::new(),
                snapshot_messages: Vec::new(),
                deleted_messages: vec![DeletedMessagesSummary::Channel {
                    chat_id: details.chat.chat_id.clone(),
                    message_ids: vec![49],
                    pts: FIXTURE_CHANNEL_UPDATES_PTS,
                    pts_count: 1,
                }],
                other_updates: vec![UpdateKindSummary {
                    kind: "channel_message_views".to_string(),
                }],
                related_chats,
                related_users,
            },
            48 => ChannelUpdatesDifferenceSummary {
                request: request_summary,
                kind: ChannelUpdatesDifferenceKind::Difference,
                is_final: false,
                continuation_required: true,
                requires_resync: false,
                next_pts: Some(49),
                resync_pts: None,
                timeout_seconds: Some(5),
                new_messages: fixture_message.into_iter().collect(),
                edited_messages: Vec::new(),
                snapshot_messages: Vec::new(),
                deleted_messages: Vec::new(),
                other_updates: vec![UpdateKindSummary { kind: "new_channel_message".to_string() }],
                related_chats,
                related_users,
            },
            _ => ChannelUpdatesDifferenceSummary {
                request: request_summary,
                kind: ChannelUpdatesDifferenceKind::TooLong,
                is_final: false,
                continuation_required: true,
                requires_resync: true,
                next_pts: None,
                resync_pts: Some(FIXTURE_CHANNEL_UPDATES_PTS),
                timeout_seconds: Some(30),
                new_messages: Vec::new(),
                edited_messages: Vec::new(),
                snapshot_messages: state
                    .messages
                    .get(&canonical_id)
                    .cloned()
                    .unwrap_or_default()
                    .into_iter()
                    .take(usize::try_from(request.limit).unwrap_or_default())
                    .collect(),
                deleted_messages: Vec::new(),
                other_updates: Vec::new(),
                related_chats,
                related_users,
            },
        })
    }

    async fn get_quick_replies(
        &self,
        _session_path: Option<PathBuf>,
        hash: i64,
    ) -> Result<QuickReplyCatalogPage, TelegramError> {
        let state = self.state.lock().await;
        if hash == FIXTURE_QUICK_REPLIES_HASH {
            return Ok(QuickReplyCatalogPage {
                not_modified: true,
                shortcuts: Vec::new(),
                top_messages: Vec::new(),
                related_chats: Vec::new(),
                related_users: Vec::new(),
            });
        }
        let top_message_ids = state
            .quick_reply_shortcuts
            .iter()
            .map(|shortcut| shortcut.top_message_id)
            .collect::<HashSet<_>>();
        let top_messages = state
            .quick_reply_messages
            .values()
            .flatten()
            .filter(|message| top_message_ids.contains(&message.message.message_id))
            .cloned()
            .collect();
        Ok(QuickReplyCatalogPage {
            not_modified: false,
            shortcuts: state.quick_reply_shortcuts.clone(),
            top_messages,
            related_chats: [202_i64, 505]
                .into_iter()
                .filter_map(|chat_id| state.known_chats.get(&chat_id).map(|chat| chat.chat.clone()))
                .collect(),
            related_users: state.contacts.clone(),
        })
    }

    async fn get_quick_reply_messages(
        &self,
        _session_path: Option<PathBuf>,
        shortcut_id: i32,
        message_ids: Option<&[i32]>,
        hash: i64,
    ) -> Result<QuickReplyMessagesPage, TelegramError> {
        let state = self.state.lock().await;
        if hash == FIXTURE_QUICK_REPLY_MESSAGES_HASH {
            return Ok(QuickReplyMessagesPage {
                shortcut_id,
                not_modified: true,
                total_count: None,
                messages: Vec::new(),
                related_chats: Vec::new(),
                related_users: Vec::new(),
            });
        }
        let all_messages =
            state.quick_reply_messages.get(&shortcut_id).cloned().unwrap_or_default();
        let total_count = i32::try_from(all_messages.len()).ok();
        let messages = match message_ids {
            Some(message_ids) => all_messages
                .into_iter()
                .filter(|message| message_ids.contains(&message.message.message_id))
                .collect(),
            None => all_messages,
        };
        Ok(QuickReplyMessagesPage {
            shortcut_id,
            not_modified: false,
            total_count,
            messages,
            related_chats: [202_i64, 505]
                .into_iter()
                .filter_map(|chat_id| state.known_chats.get(&chat_id).map(|chat| chat.chat.clone()))
                .collect(),
            related_users: state.contacts.clone(),
        })
    }

    async fn get_group_call(
        &self,
        _session_path: Option<PathBuf>,
        chat_id: &str,
        participant_limit: i32,
    ) -> Result<GroupCallPage, TelegramError> {
        let state = self.state.lock().await;
        let canonical_id = Self::resolve_known_chat(&state, chat_id)
            .ok_or_else(|| TelegramError::PeerNotFound(chat_id.to_string()))?;
        if state
            .dialogs
            .iter()
            .any(|dialog| dialog.chat_id == canonical_id && dialog.chat_kind == "user")
        {
            return Err(TelegramError::InvalidPeer(
                "group calls are only available for groups, supergroups, and channels".to_string(),
            ));
        }
        let details = state
            .known_chats
            .get(&canonical_id)
            .ok_or_else(|| TelegramError::PeerNotFound(chat_id.to_string()))?;
        let (call, all_participants) = state.group_calls.get(&canonical_id).ok_or_else(|| {
            TelegramError::InvalidPeer(format!("{} has no active group call", details.chat.chat_id))
        })?;
        let limit = usize::try_from(participant_limit.max(0)).unwrap_or_default();
        let participants = all_participants.iter().take(limit).cloned().collect::<Vec<_>>();
        let participants_next_offset =
            (limit > 0 && participants.len() < all_participants.len()).then(|| limit.to_string());
        Ok(GroupCallPage {
            chat_id: details.chat.chat_id.clone(),
            call: call.clone(),
            participants,
            participants_next_offset,
            related_chats: [202_i64, 505]
                .into_iter()
                .filter_map(|id| state.known_chats.get(&id).map(|chat| chat.chat.clone()))
                .collect(),
            related_users: state.contacts.clone(),
        })
    }

    async fn get_group_call_participants(
        &self,
        _session_path: Option<PathBuf>,
        chat_id: &str,
        peer_refs: &[String],
        sources: &[i32],
        offset: &str,
        limit: i32,
    ) -> Result<GroupCallParticipantsPage, TelegramError> {
        let state = self.state.lock().await;
        let canonical_id = Self::resolve_known_chat(&state, chat_id)
            .ok_or_else(|| TelegramError::PeerNotFound(chat_id.to_string()))?;
        if state
            .dialogs
            .iter()
            .any(|dialog| dialog.chat_id == canonical_id && dialog.chat_kind == "user")
        {
            return Err(TelegramError::InvalidPeer(
                "group calls are only available for groups, supergroups, and channels".to_string(),
            ));
        }
        let details = state
            .known_chats
            .get(&canonical_id)
            .ok_or_else(|| TelegramError::PeerNotFound(chat_id.to_string()))?;
        let (_, all_participants) = state.group_calls.get(&canonical_id).ok_or_else(|| {
            TelegramError::InvalidPeer(format!("{} has no active group call", details.chat.chat_id))
        })?;
        let requested_peers = peer_refs
            .iter()
            .map(|peer_ref| {
                let id = Self::resolve_known_chat(&state, peer_ref)
                    .ok_or_else(|| TelegramError::PeerNotFound(peer_ref.clone()))?;
                Ok(Self::canonical_chat_ref(&state.dialogs, id))
            })
            .collect::<Result<HashSet<_>, TelegramError>>()?;
        let requested_sources = sources.iter().copied().collect::<HashSet<_>>();
        let filtered = all_participants
            .iter()
            .filter(|participant| {
                (requested_peers.is_empty() || requested_peers.contains(&participant.peer_ref))
                    && (requested_sources.is_empty()
                        || requested_sources.contains(&participant.source))
            })
            .cloned()
            .collect::<Vec<_>>();
        let start = if offset.is_empty() {
            0
        } else {
            offset.parse::<usize>().map_err(|_| {
                TelegramError::InvalidPeer("fixture group-call offset is invalid".to_string())
            })?
        };
        let limit = usize::try_from(limit.max(0)).unwrap_or_default();
        let participants = filtered.iter().skip(start).take(limit).cloned().collect::<Vec<_>>();
        let next = start.saturating_add(participants.len());
        Ok(GroupCallParticipantsPage {
            chat_id: details.chat.chat_id.clone(),
            count: i32::try_from(filtered.len()).unwrap_or(i32::MAX),
            participants,
            next_offset: (next < filtered.len()).then(|| next.to_string()),
            version: 7,
            related_chats: [202_i64, 505]
                .into_iter()
                .filter_map(|id| state.known_chats.get(&id).map(|chat| chat.chat.clone()))
                .collect(),
            related_users: state.contacts.clone(),
        })
    }

    async fn get_business_chat_links(
        &self,
        _session_path: Option<PathBuf>,
    ) -> Result<BusinessChatLinksPage, TelegramError> {
        Ok(self.state.lock().await.business_chat_links.clone())
    }

    async fn resolve_business_chat_link(
        &self,
        _session_path: Option<PathBuf>,
        slug: &str,
    ) -> Result<ResolvedBusinessChatLinkSummary, TelegramError> {
        let state = self.state.lock().await;
        let link = state
            .business_chat_links
            .links
            .iter()
            .find(|link| link.slug == slug)
            .ok_or_else(|| TelegramError::PeerNotFound(format!("Business chat link {slug}")))?;
        Ok(ResolvedBusinessChatLinkSummary {
            peer_ref: state.account.user_id.to_string(),
            message: link.message.clone(),
            entity_count: link.entity_count,
            entities_sha256: link.entities_sha256.clone(),
            related_chats: state.business_chat_links.related_chats.clone(),
            related_users: state.business_chat_links.related_users.clone(),
        })
    }

    async fn create_business_chat_link(
        &self,
        _session_path: Option<PathBuf>,
        content: &BusinessChatLinkContent,
    ) -> Result<BusinessChatLinkSummary, TelegramError> {
        let expected = crate::telegram::business_chat_link_content_from_source(
            &content.source_message,
            content.parse_mode,
            content.title.clone(),
        );
        if expected != *content {
            return Err(TelegramError::InvalidPeer(
                "fixture Business chat-link content binding is invalid".to_string(),
            ));
        }
        let mut state = self.state.lock().await;
        let slug = format!("fixture-created-{}", state.next_business_chat_link_id);
        state.next_business_chat_link_id = state.next_business_chat_link_id.saturating_add(1);
        let link = BusinessChatLinkSummary {
            slug: slug.clone(),
            link: format!("https://t.me/m/{slug}"),
            message: content.rendered_message.clone(),
            entity_count: content.entity_count,
            entities_sha256: content.entities_sha256.clone(),
            title: content.title.clone(),
            views: 0,
        };
        state.business_chat_links.links.push(link.clone());
        state.business_chat_links.count = state.business_chat_links.links.len();
        state.business_chat_link_create_calls =
            state.business_chat_link_create_calls.saturating_add(1);
        if state.business_chat_link_create_uncertain_once {
            state.business_chat_link_create_uncertain_once = false;
            return Err(TelegramError::DeliveryUncertain(
                "fixture created the Business chat link before the transport receipt was lost"
                    .to_string(),
            ));
        }
        Ok(link)
    }

    async fn edit_business_chat_link(
        &self,
        _session_path: Option<PathBuf>,
        slug: &str,
        content: &BusinessChatLinkContent,
    ) -> Result<BusinessChatLinkSummary, TelegramError> {
        let expected = crate::telegram::business_chat_link_content_from_source(
            &content.source_message,
            content.parse_mode,
            content.title.clone(),
        );
        if expected != *content {
            return Err(TelegramError::InvalidPeer(
                "fixture Business chat-link content binding is invalid".to_string(),
            ));
        }
        let mut state = self.state.lock().await;
        let link = state
            .business_chat_links
            .links
            .iter_mut()
            .find(|link| link.slug == slug)
            .ok_or_else(|| TelegramError::PeerNotFound(format!("Business chat link {slug}")))?;
        link.message = content.rendered_message.clone();
        link.entity_count = content.entity_count;
        link.entities_sha256 = content.entities_sha256.clone();
        link.title = content.title.clone();
        let result = link.clone();
        state.business_chat_link_edit_calls = state.business_chat_link_edit_calls.saturating_add(1);
        if state.business_chat_link_edit_uncertain_once {
            state.business_chat_link_edit_uncertain_once = false;
            return Err(TelegramError::DeliveryUncertain(
                "fixture edited the Business chat link before the transport receipt was lost"
                    .to_string(),
            ));
        }
        Ok(result)
    }

    async fn delete_business_chat_link(
        &self,
        _session_path: Option<PathBuf>,
        slug: &str,
    ) -> Result<(), TelegramError> {
        let mut state = self.state.lock().await;
        let index = state
            .business_chat_links
            .links
            .iter()
            .position(|link| link.slug == slug)
            .ok_or_else(|| TelegramError::PeerNotFound(format!("Business chat link {slug}")))?;
        state.business_chat_links.links.remove(index);
        state.business_chat_links.count = state.business_chat_links.links.len();
        state.business_chat_link_delete_calls =
            state.business_chat_link_delete_calls.saturating_add(1);
        if state.business_chat_link_delete_uncertain_once {
            state.business_chat_link_delete_uncertain_once = false;
            return Err(TelegramError::DeliveryUncertain(
                "fixture deleted the Business chat link before the transport receipt was lost"
                    .to_string(),
            ));
        }
        Ok(())
    }

    async fn set_business_work_hours(
        &self,
        _session_path: Option<PathBuf>,
        target: Option<&BusinessWorkHoursConfig>,
    ) -> Result<(), TelegramError> {
        let mut state = self.state.lock().await;
        state.account_profile.business.work_hours =
            target.cloned().map(|config| BusinessWorkHoursSummary { config, open_now: false });
        state.business_work_hours_calls = state.business_work_hours_calls.saturating_add(1);
        if state.business_work_hours_uncertain_once {
            state.business_work_hours_uncertain_once = false;
            return Err(TelegramError::DeliveryUncertain(
                "fixture updated Business work hours before the transport receipt was lost"
                    .to_string(),
            ));
        }
        Ok(())
    }

    async fn set_business_location(
        &self,
        _session_path: Option<PathBuf>,
        target: Option<&BusinessLocationSummary>,
    ) -> Result<(), TelegramError> {
        let mut state = self.state.lock().await;
        state.account_profile.business.location = target.cloned();
        state.business_location_calls = state.business_location_calls.saturating_add(1);
        if state.business_location_uncertain_once {
            state.business_location_uncertain_once = false;
            return Err(TelegramError::DeliveryUncertain(
                "fixture updated the Business location before the transport receipt was lost"
                    .to_string(),
            ));
        }
        Ok(())
    }

    async fn set_business_greeting(
        &self,
        _session_path: Option<PathBuf>,
        target: Option<&BusinessGreetingMessageSummary>,
    ) -> Result<(), TelegramError> {
        let mut state = self.state.lock().await;
        if let Some(target) = target
            && !state
                .quick_reply_shortcuts
                .iter()
                .any(|shortcut| shortcut.shortcut_id == target.shortcut_id)
        {
            return Err(TelegramError::PeerNotFound(format!(
                "quick reply shortcut {}",
                target.shortcut_id
            )));
        }
        state.account_profile.business.greeting = target.cloned();
        state.business_greeting_calls = state.business_greeting_calls.saturating_add(1);
        if state.business_greeting_uncertain_once {
            state.business_greeting_uncertain_once = false;
            return Err(TelegramError::DeliveryUncertain(
                "fixture updated the Business greeting before the transport receipt was lost"
                    .to_string(),
            ));
        }
        Ok(())
    }

    async fn set_business_away(
        &self,
        _session_path: Option<PathBuf>,
        target: Option<&BusinessAwayMessageSummary>,
    ) -> Result<(), TelegramError> {
        let mut state = self.state.lock().await;
        if let Some(target) = target
            && !state
                .quick_reply_shortcuts
                .iter()
                .any(|shortcut| shortcut.shortcut_id == target.shortcut_id)
        {
            return Err(TelegramError::PeerNotFound(format!(
                "quick reply shortcut {}",
                target.shortcut_id
            )));
        }
        state.account_profile.business.away = target.cloned();
        state.business_away_calls = state.business_away_calls.saturating_add(1);
        if state.business_away_uncertain_once {
            state.business_away_uncertain_once = false;
            return Err(TelegramError::DeliveryUncertain(
                "fixture updated the Business away message before the transport receipt was lost"
                    .to_string(),
            ));
        }
        Ok(())
    }

    async fn set_business_intro(
        &self,
        _session_path: Option<PathBuf>,
        target: Option<&BusinessIntroSpec>,
    ) -> Result<(), TelegramError> {
        let summary = match target {
            Some(target) => {
                match (target.sticker_set_short_name.as_deref(), target.sticker_handle.as_deref()) {
                    (None, None) => {},
                    (Some(set_short_name), Some(sticker_handle))
                        if set_short_name == "fixture_reactions"
                            && fixture_stickers()
                                .iter()
                                .any(|sticker| sticker.sticker_handle == sticker_handle) => {},
                    (Some(set_short_name), Some(sticker_handle)) => {
                        return Err(TelegramError::PeerNotFound(format!(
                            "sticker {set_short_name}:{sticker_handle}"
                        )));
                    },
                    _ => {
                        return Err(TelegramError::InvalidPeer(
                            "Business intro sticker set and opaque handle must be supplied together"
                                .to_string(),
                        ));
                    },
                }
                Some(BusinessIntroSummary {
                    title: target.title.clone(),
                    description: target.description.clone(),
                    sticker_handle: target.sticker_handle.clone(),
                })
            },
            None => None,
        };
        let mut state = self.state.lock().await;
        state.account_profile.business.intro = summary;
        state.business_intro_calls = state.business_intro_calls.saturating_add(1);
        if state.business_intro_uncertain_once {
            state.business_intro_uncertain_once = false;
            return Err(TelegramError::DeliveryUncertain(
                "fixture updated the Business intro before the transport receipt was lost"
                    .to_string(),
            ));
        }
        Ok(())
    }

    async fn get_stars_status(
        &self,
        _session_path: Option<PathBuf>,
        peer_ref: &str,
    ) -> Result<StarsStatusPage, TelegramError> {
        let state = self.state.lock().await;
        let canonical_peer_ref = if ["me", "@me", "self", "@self"]
            .iter()
            .any(|alias| peer_ref.trim().eq_ignore_ascii_case(alias))
            || peer_ref.trim() == state.account.user_id.to_string()
        {
            state.account.user_id.to_string()
        } else {
            let chat_id = Self::resolve_known_chat(&state, peer_ref)
                .ok_or_else(|| TelegramError::PeerNotFound(peer_ref.to_string()))?;
            state
                .known_chats
                .get(&chat_id)
                .map(|details| details.chat.chat_id.clone())
                .unwrap_or_else(|| chat_id.to_string())
        };
        state
            .stars_statuses
            .get(&canonical_peer_ref)
            .cloned()
            .ok_or_else(|| TelegramError::PeerNotFound(peer_ref.to_string()))
    }

    async fn get_stars_transactions(
        &self,
        _session_path: Option<PathBuf>,
        peer_ref: &str,
        inbound: bool,
        outbound: bool,
        ascending: bool,
        subscription_id: Option<&str>,
        offset: &str,
        limit: i32,
    ) -> Result<StarsStatusPage, TelegramError> {
        let state = self.state.lock().await;
        let canonical_peer_ref = if ["me", "@me", "self", "@self"]
            .iter()
            .any(|alias| peer_ref.trim().eq_ignore_ascii_case(alias))
            || peer_ref.trim() == state.account.user_id.to_string()
        {
            state.account.user_id.to_string()
        } else {
            let chat_id = Self::resolve_known_chat(&state, peer_ref)
                .ok_or_else(|| TelegramError::PeerNotFound(peer_ref.to_string()))?;
            state
                .known_chats
                .get(&chat_id)
                .map(|details| details.chat.chat_id.clone())
                .unwrap_or_else(|| chat_id.to_string())
        };
        let mut page = state
            .stars_statuses
            .get(&canonical_peer_ref)
            .cloned()
            .ok_or_else(|| TelegramError::PeerNotFound(peer_ref.to_string()))?;
        page.history.retain(|transaction| {
            let whole_amount = transaction.amount.amount.parse::<i64>().unwrap_or_default();
            let direction_matches = (!inbound && !outbound)
                || (inbound && whole_amount > 0)
                || (outbound && whole_amount < 0);
            let subscription_matches = subscription_id.is_none_or(|subscription_id| {
                state
                    .stars_transaction_subscription_ids
                    .get(&transaction.transaction_id)
                    .is_some_and(|bound| bound == subscription_id)
            });
            direction_matches && subscription_matches
        });
        page.history.sort_by_key(|transaction| transaction.date_unix);
        if !ascending {
            page.history.reverse();
        }
        let start = if offset.is_empty() {
            0
        } else {
            offset.parse::<usize>().map_err(|_| {
                TelegramError::InvalidPeer(
                    "fixture Stars transaction offset is invalid".to_string(),
                )
            })?
        };
        let limit = usize::try_from(limit.max(0)).unwrap_or_default();
        let filtered_count = page.history.len();
        page.history = page.history.into_iter().skip(start).take(limit).collect();
        let next = start.saturating_add(page.history.len());
        page.next_offset = (next < filtered_count).then(|| next.to_string());
        Ok(page)
    }

    async fn get_available_star_gifts(
        &self,
        _session_path: Option<PathBuf>,
    ) -> Result<AvailableStarGiftsPage, TelegramError> {
        Ok(self.state.lock().await.available_star_gifts.clone())
    }

    async fn get_saved_star_gifts(
        &self,
        request: SavedStarGiftsRequest,
    ) -> Result<SavedStarGiftsPage, TelegramError> {
        let state = self.state.lock().await;
        let canonical_peer_ref = if ["me", "@me", "self", "@self"]
            .iter()
            .any(|alias| request.peer_ref.trim().eq_ignore_ascii_case(alias))
            || request.peer_ref.trim() == state.account.user_id.to_string()
        {
            state.account.user_id.to_string()
        } else {
            let chat_id = Self::resolve_known_chat(&state, &request.peer_ref)
                .ok_or_else(|| TelegramError::PeerNotFound(request.peer_ref.clone()))?;
            state
                .known_chats
                .get(&chat_id)
                .map(|details| details.chat.chat_id.clone())
                .unwrap_or_else(|| chat_id.to_string())
        };
        let mut page = state
            .saved_star_gifts
            .get(&canonical_peer_ref)
            .cloned()
            .ok_or_else(|| TelegramError::PeerNotFound(request.peer_ref.clone()))?;
        page.gifts.retain(|saved| {
            let (limited, unique, peer_color_available, hosted) = match &saved.gift {
                StarGiftSummary::Regular { limited, peer_color_available, .. } => {
                    (*limited, false, *peer_color_available, false)
                },
                StarGiftSummary::Unique { theme_available, host_peer_ref, .. } => {
                    (false, true, *theme_available, host_peer_ref.is_some())
                },
            };
            (!request.exclude_unsaved || !saved.unsaved)
                && (!request.exclude_saved || saved.unsaved)
                && (!request.exclude_unlimited || limited || unique)
                && (!request.exclude_limited || !limited)
                && (!request.exclude_unique || !unique)
                && (!request.exclude_upgradable || !saved.can_upgrade)
                && (!request.exclude_unupgradable || saved.can_upgrade)
                && (!request.peer_color_available || peer_color_available)
                && (!request.exclude_hosted || !hosted)
                && request
                    .collection_id
                    .is_none_or(|collection_id| saved.collection_ids.contains(&collection_id))
        });
        if request.sort_by_value {
            page.gifts.sort_by(|left, right| {
                fake_star_gift_value(&right.gift)
                    .cmp(&fake_star_gift_value(&left.gift))
                    .then_with(|| right.date_unix.cmp(&left.date_unix))
            });
        } else {
            page.gifts.sort_by_key(|right| std::cmp::Reverse(right.date_unix));
        }
        let start = if request.offset.is_empty() {
            0
        } else {
            request.offset.parse::<usize>().map_err(|_| {
                TelegramError::InvalidPeer("fixture saved-Star-Gift offset is invalid".to_string())
            })?
        };
        let filtered_count = page.gifts.len();
        let limit = usize::try_from(request.limit.max(0)).unwrap_or_default();
        page.gifts = page.gifts.into_iter().skip(start).take(limit).collect();
        let next = start.saturating_add(page.gifts.len());
        page.count = i32::try_from(filtered_count).unwrap_or(i32::MAX);
        page.next_offset = (next < filtered_count).then(|| next.to_string());
        Ok(page)
    }

    async fn get_saved_star_gift(
        &self,
        _session_path: Option<PathBuf>,
        reference: &SavedStarGiftRef,
    ) -> Result<Option<SavedStarGiftSummary>, TelegramError> {
        let state = self.state.lock().await;
        let wallet_peer_ref = Self::saved_star_gift_wallet_peer_ref(&state, reference)?;
        Ok(state
            .saved_star_gifts
            .get(&wallet_peer_ref)
            .and_then(|page| {
                page.gifts.iter().find(|gift| Self::saved_star_gift_matches(gift, reference))
            })
            .cloned())
    }

    async fn set_star_gift_unsaved(
        &self,
        _session_path: Option<PathBuf>,
        reference: &SavedStarGiftRef,
        unsaved: bool,
    ) -> Result<(), TelegramError> {
        let mut state = self.state.lock().await;
        let wallet_peer_ref = Self::saved_star_gift_wallet_peer_ref(&state, reference)?;
        let gift = state
            .saved_star_gifts
            .get_mut(&wallet_peer_ref)
            .and_then(|page| {
                page.gifts.iter_mut().find(|gift| Self::saved_star_gift_matches(gift, reference))
            })
            .ok_or_else(|| TelegramError::PeerNotFound("saved Star Gift".to_string()))?;
        gift.unsaved = unsaved;
        state.star_gift_save_calls = state.star_gift_save_calls.saturating_add(1);
        if state.star_gift_save_uncertain_once {
            state.star_gift_save_uncertain_once = false;
            return Err(TelegramError::DeliveryUncertain(
                "fixture changed saved Star Gift visibility before the transport receipt was lost"
                    .to_string(),
            ));
        }
        Ok(())
    }

    async fn convert_star_gift(
        &self,
        _session_path: Option<PathBuf>,
        reference: &SavedStarGiftRef,
    ) -> Result<(), TelegramError> {
        let mut state = self.state.lock().await;
        let wallet_peer_ref = Self::saved_star_gift_wallet_peer_ref(&state, reference)?;
        let converted = state
            .saved_star_gifts
            .get(&wallet_peer_ref)
            .and_then(|page| {
                page.gifts.iter().find(|gift| Self::saved_star_gift_matches(gift, reference))
            })
            .ok_or_else(|| TelegramError::PeerNotFound("saved Star Gift".to_string()))?;
        if converted.refunded {
            return Err(TelegramError::Telegram(
                "fixture refuses to convert a refunded Star Gift".to_string(),
            ));
        }
        let converted = converted
            .convert_stars
            .as_deref()
            .ok_or_else(|| {
                TelegramError::Telegram("fixture Star Gift has no conversion value".to_string())
            })?
            .parse::<i64>()
            .map_err(|_| {
                TelegramError::Telegram("fixture Star Gift conversion value is invalid".to_string())
            })?;
        if converted <= 0 {
            return Err(TelegramError::Telegram(
                "fixture Star Gift conversion value must be positive".to_string(),
            ));
        }
        let before_balance = state
            .stars_statuses
            .get(&wallet_peer_ref)
            .ok_or_else(|| TelegramError::PeerNotFound(wallet_peer_ref.clone()))?
            .balance
            .amount
            .parse::<i64>()
            .map_err(|_| {
                TelegramError::Telegram("fixture Stars wallet balance is invalid".to_string())
            })?;
        let updated_balance = before_balance
            .checked_add(converted)
            .ok_or_else(|| TelegramError::Telegram("fixture Stars balance overflow".to_string()))?;
        {
            let page = state
                .saved_star_gifts
                .get_mut(&wallet_peer_ref)
                .ok_or_else(|| TelegramError::PeerNotFound("saved Star Gift".to_string()))?;
            let index = page
                .gifts
                .iter()
                .position(|gift| Self::saved_star_gift_matches(gift, reference))
                .ok_or_else(|| TelegramError::PeerNotFound("saved Star Gift".to_string()))?;
            page.gifts.remove(index);
            page.count = page.count.saturating_sub(1);
        }
        let wallet = state
            .stars_statuses
            .get_mut(&wallet_peer_ref)
            .ok_or_else(|| TelegramError::PeerNotFound(wallet_peer_ref.clone()))?;
        wallet.balance.amount = updated_balance.to_string();
        state.star_gift_convert_calls = state.star_gift_convert_calls.saturating_add(1);
        if state.star_gift_convert_uncertain_once {
            state.star_gift_convert_uncertain_once = false;
            return Err(TelegramError::DeliveryUncertain(
                "fixture converted the Star Gift before the transport receipt was lost".to_string(),
            ));
        }
        Ok(())
    }

    async fn get_stars_topup_options(
        &self,
        _session_path: Option<PathBuf>,
    ) -> Result<StarsTopupOptionsPage, TelegramError> {
        Ok(self.state.lock().await.stars_topup_options.clone())
    }

    async fn get_stars_subscriptions(
        &self,
        _session_path: Option<PathBuf>,
        peer_ref: &str,
        missing_balance: bool,
        offset: &str,
    ) -> Result<StarsSubscriptionsPage, TelegramError> {
        let state = self.state.lock().await;
        let canonical_peer_ref = if ["me", "@me", "self", "@self"]
            .iter()
            .any(|alias| peer_ref.trim().eq_ignore_ascii_case(alias))
            || peer_ref.trim() == state.account.user_id.to_string()
        {
            state.account.user_id.to_string()
        } else {
            let chat_id = Self::resolve_known_chat(&state, peer_ref)
                .ok_or_else(|| TelegramError::PeerNotFound(peer_ref.to_string()))?;
            state
                .known_chats
                .get(&chat_id)
                .map(|details| details.chat.chat_id.clone())
                .unwrap_or_else(|| chat_id.to_string())
        };
        let status = state
            .stars_statuses
            .get(&canonical_peer_ref)
            .ok_or_else(|| TelegramError::PeerNotFound(peer_ref.to_string()))?;
        let mut subscriptions = status.subscriptions.clone();
        if missing_balance {
            subscriptions.retain(|subscription| subscription.missing_balance);
        }
        let start = if offset.is_empty() {
            0
        } else {
            offset.parse::<usize>().map_err(|_| {
                TelegramError::InvalidPeer(
                    "fixture Stars subscription offset is invalid".to_string(),
                )
            })?
        };
        let page_size = 1;
        let page_subscriptions =
            subscriptions.iter().skip(start).take(page_size).cloned().collect::<Vec<_>>();
        let next = start.saturating_add(page_subscriptions.len());
        Ok(StarsSubscriptionsPage {
            peer_ref: canonical_peer_ref,
            balance: status.balance.clone(),
            subscriptions: page_subscriptions,
            next_offset: (next < subscriptions.len()).then(|| next.to_string()),
            subscriptions_missing_balance: status.subscriptions_missing_balance.clone(),
            related_chats: status.related_chats.clone(),
            related_users: status.related_users.clone(),
        })
    }

    async fn get_stars_subscription(
        &self,
        _session_path: Option<PathBuf>,
        peer_ref: &str,
        subscription_id: &str,
    ) -> Result<Option<StarsSubscriptionSummary>, TelegramError> {
        let state = self.state.lock().await;
        let canonical_peer_ref = Self::canonical_stars_peer_ref(&state, peer_ref)?;
        Ok(state
            .stars_statuses
            .get(&canonical_peer_ref)
            .and_then(|status| {
                status
                    .subscriptions
                    .iter()
                    .find(|subscription| subscription.subscription_id == subscription_id)
            })
            .cloned())
    }

    async fn change_stars_subscription(
        &self,
        _session_path: Option<PathBuf>,
        peer_ref: &str,
        subscription_id: &str,
        canceled: bool,
    ) -> Result<(), TelegramError> {
        let mut state = self.state.lock().await;
        let canonical_peer_ref = Self::canonical_stars_peer_ref(&state, peer_ref)?;
        let subscription = state
            .stars_statuses
            .get_mut(&canonical_peer_ref)
            .and_then(|status| {
                status
                    .subscriptions
                    .iter_mut()
                    .find(|subscription| subscription.subscription_id == subscription_id)
            })
            .ok_or_else(|| TelegramError::PeerNotFound(subscription_id.to_string()))?;
        subscription.canceled = canceled;
        state.stars_subscription_change_calls =
            state.stars_subscription_change_calls.saturating_add(1);
        if state.stars_subscription_change_uncertain_once {
            state.stars_subscription_change_uncertain_once = false;
            return Err(TelegramError::DeliveryUncertain(
                "fixture changed the Stars subscription before the transport receipt was lost"
                    .to_string(),
            ));
        }
        Ok(())
    }

    async fn search_messages(
        &self,
        request: SearchMessagesRequest,
    ) -> Result<MessagePage, TelegramError> {
        let state = self.state.lock().await;
        let query = request.query.to_lowercase();
        let chat_scope = request
            .chat_id
            .as_deref()
            .map(|chat_id| {
                Self::resolve_dialog(&state.dialogs, chat_id)
                    .ok_or_else(|| TelegramError::PeerNotFound(chat_id.to_string()))
            })
            .transpose()?;
        let mut messages = state
            .messages
            .iter()
            .filter(|(chat_id, _)| chat_scope.is_none_or(|scope| **chat_id == scope))
            .flat_map(|(_, messages)| messages.iter().cloned())
            .filter(|message| {
                request.offset_id.is_none_or(|offset| message.message_id < offset)
                    && message
                        .text
                        .as_deref()
                        .is_some_and(|text| text.to_lowercase().contains(&query))
            })
            .collect::<Vec<_>>();
        messages.sort_by(|left, right| {
            right.date.cmp(&left.date).then_with(|| right.message_id.cmp(&left.message_id))
        });
        messages.truncate(request.limit);
        let next_offset_id = chat_scope
            .is_some()
            .then(|| {
                (messages.len() == request.limit).then(|| messages.last().map(|m| m.message_id))
            })
            .flatten()
            .flatten();
        let completeness = if chat_scope.is_none() {
            Completeness::FirstPageOnly
        } else if next_offset_id.is_some() {
            Completeness::Page
        } else {
            Completeness::Complete
        };
        Ok(MessagePage { messages, next_offset_id, completeness })
    }

    async fn search_saved_messages(
        &self,
        request: SavedMessagesSearchRequest,
    ) -> Result<MessagePage, TelegramError> {
        let state = self.state.lock().await;
        let query = request.query.to_lowercase();
        let peer_scope = request
            .saved_peer_id
            .as_deref()
            .map(|peer_ref| {
                Self::resolve_dialog(&state.dialogs, peer_ref)
                    .ok_or_else(|| TelegramError::PeerNotFound(peer_ref.to_string()))
            })
            .transpose()?;
        let mut messages = state
            .saved_histories
            .iter()
            .filter(|(peer_id, _)| peer_scope.is_none_or(|scope| **peer_id == scope))
            .flat_map(|(_, messages)| messages.iter().cloned())
            .filter(|message| {
                request.offset_id.is_none_or(|offset| message.message_id < offset)
                    && request.min_date.is_none_or(|date| message.date >= date)
                    && request.max_date.is_none_or(|date| message.date <= date)
                    && (query.is_empty()
                        || message
                            .text
                            .as_deref()
                            .is_some_and(|text| text.to_lowercase().contains(&query)))
                    && (request.tag_reactions.is_empty()
                        || state.saved_message_tags.get(&message.message_id).is_some_and(|tags| {
                            request.tag_reactions.iter().any(|requested| tags.contains(requested))
                        }))
            })
            .collect::<Vec<_>>();
        messages.sort_by(|left, right| {
            right.date.cmp(&left.date).then_with(|| right.message_id.cmp(&left.message_id))
        });
        let total = messages.len();
        messages.truncate(request.limit);
        let next_offset_id = (total > messages.len())
            .then(|| messages.last().map(|message| message.message_id))
            .flatten();
        Ok(MessagePage {
            messages,
            next_offset_id,
            completeness: if next_offset_id.is_some() {
                Completeness::Page
            } else {
                Completeness::Complete
            },
        })
    }

    async fn get_saved_dialogs(
        &self,
        _session_path: Option<PathBuf>,
        limit: usize,
        exclude_pinned: bool,
        offset_id: Option<i32>,
    ) -> Result<SavedDialogsPage, TelegramError> {
        let state = self.state.lock().await;
        let mut dialogs = state
            .saved_dialogs
            .iter()
            .filter(|dialog| !exclude_pinned || !dialog.pinned)
            .filter(|dialog| offset_id.is_none_or(|offset| dialog.top_message_id < offset))
            .cloned()
            .collect::<Vec<_>>();
        dialogs.sort_by(|left, right| {
            right
                .pinned
                .cmp(&left.pinned)
                .then_with(|| right.top_message_id.cmp(&left.top_message_id))
        });
        let total_count = i32::try_from(dialogs.len()).ok();
        let total = dialogs.len();
        dialogs.truncate(limit);
        let next_offset_id = (total > dialogs.len())
            .then(|| dialogs.last().map(|dialog| dialog.top_message_id))
            .flatten();
        Ok(SavedDialogsPage {
            dialogs,
            total_count,
            next_offset_id,
            completeness: if next_offset_id.is_some() {
                Completeness::Page
            } else {
                Completeness::Complete
            },
        })
    }

    async fn get_saved_dialog(
        &self,
        _session_path: Option<PathBuf>,
        peer_ref: &str,
    ) -> Result<Option<SavedDialogSummary>, TelegramError> {
        let state = self.state.lock().await;
        let peer_id = Self::resolve_dialog(&state.dialogs, peer_ref)
            .ok_or_else(|| TelegramError::PeerNotFound(peer_ref.to_string()))?;
        Ok(state
            .saved_dialogs
            .iter()
            .find(|dialog| Self::resolve_dialog(&state.dialogs, &dialog.peer_ref) == Some(peer_id))
            .cloned())
    }

    async fn get_saved_history(
        &self,
        _session_path: Option<PathBuf>,
        peer_ref: &str,
        limit: usize,
        offset_id: Option<i32>,
    ) -> Result<SavedHistoryPage, TelegramError> {
        let state = self.state.lock().await;
        let peer_id = Self::resolve_dialog(&state.dialogs, peer_ref)
            .ok_or_else(|| TelegramError::PeerNotFound(peer_ref.to_string()))?;
        let canonical_peer_ref = Self::canonical_chat_ref(&state.dialogs, peer_id);
        let mut messages = state
            .saved_histories
            .get(&peer_id)
            .into_iter()
            .flatten()
            .filter(|message| offset_id.is_none_or(|offset| message.message_id < offset))
            .cloned()
            .collect::<Vec<_>>();
        messages.sort_by_key(|right| std::cmp::Reverse(right.message_id));
        let total = messages.len();
        messages.truncate(limit);
        let next_offset_id = (total > messages.len())
            .then(|| messages.last().map(|message| message.message_id))
            .flatten();
        Ok(SavedHistoryPage {
            peer_ref: canonical_peer_ref,
            messages,
            total_count: i32::try_from(total).ok(),
            next_offset_id,
            completeness: if next_offset_id.is_some() {
                Completeness::Page
            } else {
                Completeness::Complete
            },
        })
    }

    async fn delete_saved_history(
        &self,
        _session_path: Option<PathBuf>,
        peer_ref: &str,
        max_id: i32,
        min_date: Option<i32>,
        max_date: Option<i32>,
    ) -> Result<(), TelegramError> {
        let mut state = self.state.lock().await;
        let peer_id = Self::resolve_dialog(&state.dialogs, peer_ref)
            .ok_or_else(|| TelegramError::PeerNotFound(peer_ref.to_string()))?;
        let canonical_peer_ref = Self::canonical_chat_ref(&state.dialogs, peer_id);
        let remaining_top = {
            let history = state.saved_histories.entry(peer_id).or_default();
            history.retain(|message| {
                let selected = (max_id <= 0 || message.message_id <= max_id)
                    && min_date.is_none_or(|date| message.date >= date)
                    && max_date.is_none_or(|date| message.date <= date);
                !selected
            });
            history.iter().map(|message| message.message_id).max()
        };
        match remaining_top {
            Some(top_message_id) => {
                if let Some(dialog) = state
                    .saved_dialogs
                    .iter_mut()
                    .find(|dialog| dialog.peer_ref == canonical_peer_ref)
                {
                    dialog.top_message_id = top_message_id;
                }
            },
            None => {
                state.saved_dialogs.retain(|dialog| dialog.peer_ref != canonical_peer_ref);
            },
        }
        state.saved_history_delete_calls = state.saved_history_delete_calls.saturating_add(1);
        if std::mem::take(&mut state.saved_history_delete_uncertain_once) {
            return Err(TelegramError::DeliveryUncertain(
                "fixture deleted Saved Messages history before the response receipt was lost"
                    .to_string(),
            ));
        }
        Ok(())
    }

    async fn toggle_saved_dialog_pin(
        &self,
        _session_path: Option<PathBuf>,
        peer_ref: &str,
        pinned: bool,
    ) -> Result<(), TelegramError> {
        let mut state = self.state.lock().await;
        let peer_id = Self::resolve_dialog(&state.dialogs, peer_ref)
            .ok_or_else(|| TelegramError::PeerNotFound(peer_ref.to_string()))?;
        let dialog = state
            .saved_dialogs
            .iter_mut()
            .find(|dialog| {
                dialog.peer_ref == peer_ref
                    || dialog.peer_ref.parse::<i64>().ok().is_some_and(|id| id.abs() == peer_id)
            })
            .ok_or_else(|| TelegramError::PeerNotFound(peer_ref.to_string()))?;
        dialog.pinned = pinned;
        state.saved_dialog_pin_calls = state.saved_dialog_pin_calls.saturating_add(1);
        if std::mem::take(&mut state.saved_dialog_pin_uncertain_once) {
            return Err(TelegramError::DeliveryUncertain(
                "fixture updated the Saved Messages pin before the response receipt was lost"
                    .to_string(),
            ));
        }
        Ok(())
    }

    async fn get_saved_reaction_tags(
        &self,
        _session_path: Option<PathBuf>,
        peer_ref: Option<&str>,
        hash: i64,
    ) -> Result<SavedReactionTagsPage, TelegramError> {
        let state = self.state.lock().await;
        if let Some(peer_ref) = peer_ref {
            Self::resolve_dialog(&state.dialogs, peer_ref)
                .ok_or_else(|| TelegramError::PeerNotFound(peer_ref.to_string()))?;
        }
        if hash == 7_007 {
            return Ok(SavedReactionTagsPage { tags: Vec::new(), hash: None, not_modified: true });
        }
        Ok(SavedReactionTagsPage {
            tags: state.saved_reaction_tags.clone(),
            hash: Some("7007".to_string()),
            not_modified: false,
        })
    }

    async fn update_saved_reaction_tag(
        &self,
        _session_path: Option<PathBuf>,
        reaction: &ReactionIdentity,
        title: Option<&str>,
    ) -> Result<(), TelegramError> {
        if matches!(reaction, ReactionIdentity::Empty | ReactionIdentity::Paid) {
            return Err(TelegramError::InvalidPeer(
                "Saved Messages tags require an emoji or custom-emoji reaction".to_string(),
            ));
        }
        let mut state = self.state.lock().await;
        if let Some(tag) =
            state.saved_reaction_tags.iter_mut().find(|tag| tag.reaction == *reaction)
        {
            tag.title = title.map(str::to_string);
        } else {
            state.saved_reaction_tags.push(SavedReactionTagSummary {
                reaction: reaction.clone(),
                title: title.map(str::to_string),
                count: 0,
            });
        }
        state.saved_reaction_tag_update_calls =
            state.saved_reaction_tag_update_calls.saturating_add(1);
        if std::mem::take(&mut state.saved_reaction_tag_update_uncertain_once) {
            return Err(TelegramError::DeliveryUncertain(
                "fixture updated the Saved Messages reaction tag before the response receipt was lost"
                    .to_string(),
            ));
        }
        Ok(())
    }

    async fn get_default_tag_reactions(
        &self,
        _session_path: Option<PathBuf>,
        hash: i64,
    ) -> Result<ReactionCatalogPage, TelegramError> {
        if hash == 7_008 {
            return Ok(ReactionCatalogPage {
                not_modified: true,
                hash: None,
                reactions: Vec::new(),
            });
        }
        Ok(ReactionCatalogPage {
            not_modified: false,
            hash: Some(7_008),
            reactions: vec![
                ReactionIdentity::Emoji { emoji: "🧠".to_string() },
                ReactionIdentity::Emoji { emoji: "📚".to_string() },
                ReactionIdentity::CustomEmoji { document_id: 7_001 },
            ],
        })
    }

    async fn get_unread_mentions(
        &self,
        request: UnreadMessagesRequest,
    ) -> Result<MessagePage, TelegramError> {
        let state = self.state.lock().await;
        Self::unread_fixture_page(&state, &request, |message| message.message_id == 11)
    }

    async fn get_unread_reactions(
        &self,
        request: UnreadMessagesRequest,
    ) -> Result<MessagePage, TelegramError> {
        let state = self.state.lock().await;
        Self::unread_fixture_page(&state, &request, |message| message.message_id == 21)
    }

    async fn get_unread_poll_votes(
        &self,
        request: UnreadMessagesRequest,
    ) -> Result<MessagePage, TelegramError> {
        let state = self.state.lock().await;
        Self::unread_fixture_page(&state, &request, |message| message.message_id == 20)
    }

    async fn get_message(
        &self,
        _session_path: Option<PathBuf>,
        chat_id: &str,
        message_id: i32,
    ) -> Result<Option<MessageSummary>, TelegramError> {
        let state = self.state.lock().await;
        let canonical_id = Self::resolve_dialog(&state.dialogs, chat_id)
            .ok_or_else(|| TelegramError::PeerNotFound(chat_id.to_string()))?;
        Ok(state
            .messages
            .get(&canonical_id)
            .and_then(|messages| messages.iter().find(|message| message.message_id == message_id))
            .cloned())
    }

    async fn get_message_html(
        &self,
        _session_path: Option<PathBuf>,
        chat_id: &str,
        message_id: i32,
    ) -> Result<String, TelegramError> {
        let state = self.state.lock().await;
        let canonical_id = Self::resolve_dialog(&state.dialogs, chat_id)
            .ok_or_else(|| TelegramError::PeerNotFound(chat_id.to_string()))?;
        let message = state
            .messages
            .get(&canonical_id)
            .and_then(|messages| messages.iter().find(|message| message.message_id == message_id))
            .ok_or_else(|| TelegramError::MessageNotFound(format!("{chat_id}:{message_id}")))?;
        if let Some(html) = state.message_html.get(&(canonical_id, message_id)) {
            return Ok(html.clone());
        }
        Ok(ferogram::parsers::generate_html(message.text.as_deref().unwrap_or_default(), &[]))
    }

    async fn get_message_buttons(
        &self,
        _session_path: Option<PathBuf>,
        chat_id: &str,
        message_id: Option<i32>,
        limit: usize,
    ) -> Result<MessageButtonInventory, TelegramError> {
        let state = self.state.lock().await;
        let canonical_id = Self::resolve_dialog(&state.dialogs, chat_id)
            .ok_or_else(|| TelegramError::PeerNotFound(chat_id.to_string()))?;
        let canonical_chat_id = Self::canonical_chat_ref(&state.dialogs, canonical_id);
        if let Some(message_id) = message_id {
            if let Some(inventory) = state.message_buttons.get(&(canonical_id, message_id)) {
                return Ok(inventory.clone());
            }
            let message = state
                .messages
                .get(&canonical_id)
                .and_then(|messages| {
                    messages.iter().find(|message| message.message_id == message_id)
                })
                .ok_or_else(|| {
                    TelegramError::MessageNotFound(format!("{canonical_chat_id}:{message_id}"))
                })?;
            return Ok(MessageButtonInventory {
                chat_id: canonical_chat_id,
                message_id,
                message_date_unix: message.date,
                markup_type: MessageButtonMarkupType::None,
                buttons: Vec::new(),
            });
        }
        let inventory = state
            .messages
            .get(&canonical_id)
            .into_iter()
            .flatten()
            .take(limit)
            .find_map(|message| {
                state.message_buttons.get(&(canonical_id, message.message_id)).cloned()
            })
            .ok_or_else(|| {
                TelegramError::MessageNotFound(format!(
                    "no message with buttons found in the latest {limit} messages of {canonical_chat_id}"
                ))
            })?;
        Ok(inventory)
    }

    async fn press_callback_button(
        &self,
        _session_path: Option<PathBuf>,
        button: &CallbackButtonSnapshot,
    ) -> Result<BotCallbackAnswerSummary, TelegramError> {
        let mut state = self.state.lock().await;
        let canonical_id = Self::resolve_dialog(&state.dialogs, &button.chat_id)
            .ok_or_else(|| TelegramError::PeerNotFound(button.chat_id.clone()))?;
        let canonical_chat_id = Self::canonical_chat_ref(&state.dialogs, canonical_id);
        if canonical_chat_id != button.chat_id {
            return Err(TelegramError::InvalidPeer(format!(
                "callback target resolved to {canonical_chat_id}, expected {}",
                button.chat_id
            )));
        }
        let inventory =
            state.message_buttons.get(&(canonical_id, button.message_id)).ok_or_else(|| {
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
        state.callback_button_press_calls = state.callback_button_press_calls.saturating_add(1);
        state.pressed_callback_buttons.push(button.clone());
        if std::mem::take(&mut state.callback_button_press_uncertain_once) {
            return Err(TelegramError::DeliveryUncertain(
                "fixture pressed the callback button before the response receipt was lost"
                    .to_string(),
            ));
        }
        Ok(BotCallbackAnswerSummary {
            alert: true,
            has_url: true,
            native_ui: false,
            message: Some("Contract approval recorded".to_string()),
            url: Some("https://example.com/contract/11/receipt".to_string()),
            cache_time_seconds: 30,
        })
    }

    async fn get_inline_bot_results(
        &self,
        _session_path: Option<PathBuf>,
        bot_ref: &str,
        context_chat_id: &str,
        query: &str,
        offset: &str,
    ) -> Result<InlineBotResults, TelegramError> {
        let mut state = self.state.lock().await;
        let wanted_bot = bot_ref.trim().trim_start_matches('@');
        let bot = state
            .contacts
            .iter()
            .find(|contact| {
                contact.user_id.to_string() == wanted_bot
                    || contact
                        .username
                        .as_deref()
                        .is_some_and(|username| username.eq_ignore_ascii_case(wanted_bot))
            })
            .cloned()
            .ok_or_else(|| TelegramError::PeerNotFound(bot_ref.to_string()))?;
        if !bot.bot {
            return Err(TelegramError::InvalidPeer(format!(
                "{bot_ref} resolved to a Telegram user that is not a bot"
            )));
        }
        let canonical_context_chat_id = if context_chat_id == state.account.user_id.to_string() {
            context_chat_id.to_string()
        } else {
            let context_id = Self::resolve_dialog(&state.dialogs, context_chat_id)
                .ok_or_else(|| TelegramError::PeerNotFound(context_chat_id.to_string()))?;
            Self::canonical_chat_ref(&state.dialogs, context_id)
        };
        state.inline_query_calls = state.inline_query_calls.saturating_add(1);
        let query_id = state.next_inline_query_id;
        state.next_inline_query_id = state.next_inline_query_id.saturating_add(1);
        let query_label = if query.is_empty() { "empty query" } else { query };
        let results = vec![
            InlineBotResultSummary {
                id: "article:0".to_string(),
                result_type: "article".to_string(),
                source_kind: InlineBotResultSourceKind::Result,
                title: Some(format!("Inline answer for {query_label}")),
                description: Some("Deterministic fixture article result".to_string()),
                url: Some("https://example.com/inline/article".to_string()),
                media_kind: Some("web_document".to_string()),
                has_thumbnail: true,
                has_content: true,
                send_message_kind: "text".to_string(),
                send_message_text: Some(format!("Inline answer: {query_label}")),
                send_message_url: None,
                send_message_geo: None,
                send_message_has_reply_markup: true,
            },
            InlineBotResultSummary {
                id: "photo:1".to_string(),
                result_type: "photo".to_string(),
                source_kind: InlineBotResultSourceKind::MediaResult,
                title: Some("Fixture release image".to_string()),
                description: Some("A deterministic inline media result".to_string()),
                url: None,
                media_kind: Some("photo".to_string()),
                has_thumbnail: false,
                has_content: true,
                send_message_kind: "media_auto".to_string(),
                send_message_text: Some(format!("Release image for {query_label}")),
                send_message_url: None,
                send_message_geo: None,
                send_message_has_reply_markup: false,
            },
        ];
        state.inline_queries.insert(query_id, results.clone());
        Ok(InlineBotResults {
            bot,
            context_chat_id: canonical_context_chat_id,
            query: query.to_string(),
            offset: offset.to_string(),
            query_id: query_id.to_string(),
            next_offset: offset.is_empty().then(|| "next:1".to_string()),
            cache_time_seconds: 300,
            gallery: true,
            switch_pm: Some(InlineBotSwitchPmSummary {
                text: "Open fixture bot".to_string(),
                start_param: "inline_fixture".to_string(),
            }),
            switch_webview: Some(InlineBotSwitchWebViewSummary {
                text: "Open fixture web view".to_string(),
                url: "https://example.com/inline/webview".to_string(),
            }),
            results,
        })
    }

    async fn send_inline_bot_result_with_random_id(
        &self,
        _session_path: Option<PathBuf>,
        chat_id: &str,
        query_id: &str,
        result_id: &str,
        reply_to_message_id: Option<i32>,
        silent: bool,
        hide_via: bool,
        clear_draft: bool,
        random_id: i64,
    ) -> Result<i32, TelegramError> {
        let mut state = self.state.lock().await;
        if let Some(message_id) = state.sent_by_random_id.get(&random_id) {
            return Ok(*message_id);
        }
        let query_id_value = query_id.parse::<i64>().map_err(|_| {
            TelegramError::InvalidPeer("inline query id is not a signed 64-bit integer".to_string())
        })?;
        if query_id_value.to_string() != query_id {
            return Err(TelegramError::InvalidPeer(
                "inline query id is not canonical decimal text".to_string(),
            ));
        }
        let selected = state
            .inline_queries
            .get(&query_id_value)
            .and_then(|results| results.iter().find(|result| result.id == result_id))
            .cloned()
            .ok_or_else(|| {
                TelegramError::InvalidPeer(format!(
                    "inline result {result_id} was not issued for query {query_id}"
                ))
            })?;
        let canonical_id = Self::resolve_dialog(&state.dialogs, chat_id)
            .ok_or_else(|| TelegramError::PeerNotFound(chat_id.to_string()))?;
        if let Some(reply_to_message_id) = reply_to_message_id
            && !state.messages.get(&canonical_id).is_some_and(|messages| {
                messages.iter().any(|message| message.message_id == reply_to_message_id)
            })
        {
            return Err(TelegramError::MessageNotFound(format!("{chat_id}:{reply_to_message_id}")));
        }
        state.inline_result_send_calls = state.inline_result_send_calls.saturating_add(1);
        let message_id = state.next_message_id;
        state.next_message_id = state.next_message_id.saturating_add(1);
        state.sent_by_random_id.insert(random_id, message_id);
        let canonical_chat_id = Self::canonical_chat_ref(&state.dialogs, canonical_id);
        let text = selected
            .send_message_text
            .clone()
            .or_else(|| selected.title.clone())
            .unwrap_or_default();
        state.sent.push(FakeSentMessage {
            chat_id: canonical_chat_id.clone(),
            text: text.clone(),
            message_id,
            file_name: None,
            reply_to_message_id,
            file_mode: None,
        });
        let account_user_id = state.account.user_id;
        state.messages.entry(canonical_id).or_default().insert(
            0,
            MessageSummary {
                message_id,
                chat_ref: Some(canonical_chat_id.clone()),
                sender_ref: Some(account_user_id.to_string()),
                outgoing: true,
                pinned: false,
                silent,
                noforwards: false,
                text: (!text.is_empty()).then_some(text),
                date: 1_700_000_000_i32.saturating_add(message_id),
                edit_date: None,
                reply_to_message_id,
                views: None,
                forwards: None,
                reply_count: None,
                media_type: selected.media_kind.clone(),
                forward_origin: None,
            },
        );
        if clear_draft {
            state.drafts.remove(&canonical_id);
        }
        state.sent_inline_results.push(FakeInlineBotResultSend {
            chat_id: canonical_chat_id,
            query_id: query_id.to_string(),
            result_id: result_id.to_string(),
            message_id,
            reply_to_message_id,
            silent,
            hide_via,
            clear_draft,
            random_id,
        });
        if std::mem::take(&mut state.inline_result_send_uncertain_once) {
            return Err(TelegramError::DeliveryUncertain(
                "fixture sent the inline result before the transport receipt was lost".to_string(),
            ));
        }
        Ok(message_id)
    }

    async fn translate_messages(
        &self,
        _session_path: Option<PathBuf>,
        chat_id: &str,
        message_ids: &[i32],
        to_language: &str,
    ) -> Result<MessageTranslationBatch, TelegramError> {
        let mut state = self.state.lock().await;
        let canonical_id = Self::resolve_dialog(&state.dialogs, chat_id)
            .ok_or_else(|| TelegramError::PeerNotFound(chat_id.to_string()))?;
        let canonical_chat_id = state
            .dialogs
            .iter()
            .find(|dialog| dialog.chat_id == canonical_id)
            .map(|dialog| dialog.canonical_chat_id.clone())
            .ok_or_else(|| TelegramError::PeerNotFound(chat_id.to_string()))?;
        let translations = {
            let messages = state
                .messages
                .get(&canonical_id)
                .ok_or_else(|| TelegramError::PeerNotFound(chat_id.to_string()))?;
            let mut translations = Vec::with_capacity(message_ids.len());
            for message_id in message_ids {
                let message = messages
                    .iter()
                    .find(|message| message.message_id == *message_id)
                    .ok_or_else(|| TelegramError::MessageNotFound(message_id.to_string()))?;
                let source = message.text.as_deref().unwrap_or_default();
                let text = match (to_language, source) {
                    ("es", "Here is the latest contract") => {
                        "Aquí está el contrato más reciente".to_string()
                    },
                    ("es", "Please check contract.pdf") => {
                        "Por favor, revisa contract.pdf".to_string()
                    },
                    _ => format!("[{to_language}] {source}"),
                };
                translations.push(MessageTranslationSummary { message_id: *message_id, text });
            }
            translations
        };
        state.translate_calls = state.translate_calls.saturating_add(1);
        let uncertain = std::mem::take(&mut state.translate_uncertain_once);
        let result = MessageTranslationBatch {
            chat_id: canonical_chat_id,
            to_language: to_language.to_string(),
            translations,
        };
        if uncertain {
            return Err(TelegramError::DeliveryUncertain(
                "fixture consumed the translation request before the response receipt was lost"
                    .to_string(),
            ));
        }
        Ok(result)
    }

    async fn transcribe_audio(
        &self,
        _session_path: Option<PathBuf>,
        chat_id: &str,
        message_id: i32,
    ) -> Result<AudioTranscriptionSummary, TelegramError> {
        let mut state = self.state.lock().await;
        let canonical_id = Self::resolve_dialog(&state.dialogs, chat_id)
            .ok_or_else(|| TelegramError::PeerNotFound(chat_id.to_string()))?;
        let canonical_chat_id = state
            .dialogs
            .iter()
            .find(|dialog| dialog.chat_id == canonical_id)
            .map(|dialog| dialog.canonical_chat_id.clone())
            .ok_or_else(|| TelegramError::PeerNotFound(chat_id.to_string()))?;
        let message = state
            .messages
            .get(&canonical_id)
            .and_then(|messages| messages.iter().find(|message| message.message_id == message_id))
            .ok_or_else(|| TelegramError::MessageNotFound(message_id.to_string()))?;
        if !matches!(
            message.media_type.as_deref(),
            Some("voice" | "video_note" | "video" | "document")
        ) {
            return Err(TelegramError::Telegram(
                "fixture message is not eligible for Telegram transcription".to_string(),
            ));
        }
        state.transcription_poll_calls = state.transcription_poll_calls.saturating_add(1);
        let key = (canonical_id, message_id);
        let result = if let Some(existing) = state.transcriptions.get_mut(&key) {
            if existing.pending {
                existing.pending = false;
                existing.text = "Fixture voice transcription complete".to_string();
            }
            existing.clone()
        } else {
            state.transcription_start_calls = state.transcription_start_calls.saturating_add(1);
            let transcription = AudioTranscriptionSummary {
                chat_id: canonical_chat_id,
                message_id,
                transcription_id: format!("9{canonical_id}{message_id}"),
                text: String::new(),
                pending: true,
                trial_remains_num: Some(2),
                trial_remains_until_unix: Some(1_800_000_000),
            };
            state.transcriptions.insert(key, transcription.clone());
            transcription
        };
        let uncertain = std::mem::take(&mut state.transcription_uncertain_once);
        if uncertain {
            return Err(TelegramError::DeliveryUncertain(
                "fixture started transcription before the response receipt was lost".to_string(),
            ));
        }
        Ok(result)
    }

    async fn rate_transcription(
        &self,
        _session_path: Option<PathBuf>,
        chat_id: &str,
        message_id: i32,
        transcription_id: i64,
        good: bool,
    ) -> Result<(), TelegramError> {
        let mut state = self.state.lock().await;
        let canonical_id = Self::resolve_dialog(&state.dialogs, chat_id)
            .ok_or_else(|| TelegramError::PeerNotFound(chat_id.to_string()))?;
        let exact_id = transcription_id.to_string();
        let current = state
            .transcriptions
            .get(&(canonical_id, message_id))
            .ok_or_else(|| TelegramError::MessageNotFound(message_id.to_string()))?;
        if current.transcription_id != exact_id {
            return Err(TelegramError::Telegram(
                "fixture transcription id does not match the message".to_string(),
            ));
        }
        state.transcription_ratings.insert((canonical_id, message_id, exact_id), good);
        state.transcription_rate_calls = state.transcription_rate_calls.saturating_add(1);
        if std::mem::take(&mut state.transcription_rate_uncertain_once) {
            return Err(TelegramError::DeliveryUncertain(
                "fixture stored the transcription rating before the response receipt was lost"
                    .to_string(),
            ));
        }
        Ok(())
    }

    async fn get_fact_checks(
        &self,
        _session_path: Option<PathBuf>,
        chat_id: &str,
        message_ids: &[i32],
    ) -> Result<FactCheckPage, TelegramError> {
        let state = self.state.lock().await;
        let canonical_id = Self::resolve_dialog(&state.dialogs, chat_id)
            .ok_or_else(|| TelegramError::PeerNotFound(chat_id.to_string()))?;
        let canonical_chat_id = state
            .dialogs
            .iter()
            .find(|dialog| dialog.chat_id == canonical_id)
            .map(|dialog| dialog.canonical_chat_id.clone())
            .ok_or_else(|| TelegramError::PeerNotFound(chat_id.to_string()))?;
        let messages = state
            .messages
            .get(&canonical_id)
            .ok_or_else(|| TelegramError::PeerNotFound(chat_id.to_string()))?;
        let mut fact_checks = Vec::with_capacity(message_ids.len());
        for message_id in message_ids {
            if !messages.iter().any(|message| message.message_id == *message_id) {
                return Err(TelegramError::MessageNotFound(message_id.to_string()));
            }
            fact_checks.push(
                state.fact_checks.get(&(canonical_id, *message_id)).cloned().unwrap_or(
                    FactCheckSummary {
                        message_id: *message_id,
                        need_check: true,
                        country: None,
                        text: None,
                        hash: "0".to_string(),
                    },
                ),
            );
        }
        Ok(FactCheckPage {
            chat_id: canonical_chat_id,
            returned_count: fact_checks.len(),
            fact_checks,
        })
    }

    async fn edit_fact_check(
        &self,
        _session_path: Option<PathBuf>,
        chat_id: &str,
        message_id: i32,
        text: &str,
    ) -> Result<(), TelegramError> {
        let mut state = self.state.lock().await;
        let canonical_id = Self::resolve_dialog(&state.dialogs, chat_id)
            .ok_or_else(|| TelegramError::PeerNotFound(chat_id.to_string()))?;
        if !state
            .messages
            .get(&canonical_id)
            .is_some_and(|messages| messages.iter().any(|message| message.message_id == message_id))
        {
            return Err(TelegramError::MessageNotFound(message_id.to_string()));
        }
        let previous = state.fact_checks.get(&(canonical_id, message_id)).cloned();
        let next_hash = previous
            .as_ref()
            .and_then(|fact_check| fact_check.hash.parse::<i64>().ok())
            .unwrap_or_default()
            .saturating_add(1)
            .to_string();
        state.fact_checks.insert(
            (canonical_id, message_id),
            FactCheckSummary {
                message_id,
                need_check: false,
                country: previous.and_then(|fact_check| fact_check.country),
                text: Some(text.to_string()),
                hash: next_hash,
            },
        );
        state.fact_check_edit_calls = state.fact_check_edit_calls.saturating_add(1);
        if std::mem::take(&mut state.fact_check_edit_uncertain_once) {
            return Err(TelegramError::DeliveryUncertain(
                "fixture edited the fact-check before the response receipt was lost".to_string(),
            ));
        }
        Ok(())
    }

    async fn delete_fact_check(
        &self,
        _session_path: Option<PathBuf>,
        chat_id: &str,
        message_id: i32,
    ) -> Result<(), TelegramError> {
        let mut state = self.state.lock().await;
        let canonical_id = Self::resolve_dialog(&state.dialogs, chat_id)
            .ok_or_else(|| TelegramError::PeerNotFound(chat_id.to_string()))?;
        if !state
            .messages
            .get(&canonical_id)
            .is_some_and(|messages| messages.iter().any(|message| message.message_id == message_id))
        {
            return Err(TelegramError::MessageNotFound(message_id.to_string()));
        }
        let next_hash = state
            .fact_checks
            .get(&(canonical_id, message_id))
            .and_then(|fact_check| fact_check.hash.parse::<i64>().ok())
            .unwrap_or_default()
            .saturating_add(1)
            .to_string();
        state.fact_checks.insert(
            (canonical_id, message_id),
            FactCheckSummary {
                message_id,
                need_check: false,
                country: None,
                text: None,
                hash: next_hash,
            },
        );
        state.fact_check_delete_calls = state.fact_check_delete_calls.saturating_add(1);
        if std::mem::take(&mut state.fact_check_delete_uncertain_once) {
            return Err(TelegramError::DeliveryUncertain(
                "fixture deleted the fact-check before the response receipt was lost".to_string(),
            ));
        }
        Ok(())
    }

    async fn get_discussion_message(
        &self,
        _session_path: Option<PathBuf>,
        chat_id: &str,
        message_id: i32,
    ) -> Result<DiscussionMessageSummary, TelegramError> {
        let state = self.state.lock().await;
        let canonical_id = Self::resolve_dialog(&state.dialogs, chat_id)
            .ok_or_else(|| TelegramError::PeerNotFound(chat_id.to_string()))?;
        let source = state
            .messages
            .get(&canonical_id)
            .and_then(|messages| messages.iter().find(|message| message.message_id == message_id))
            .ok_or_else(|| TelegramError::MessageNotFound(message_id.to_string()))?;
        let mut messages = state
            .messages
            .get(&canonical_id)
            .into_iter()
            .flatten()
            .filter(|message| {
                message.message_id == source.message_id
                    || message.reply_to_message_id == Some(source.message_id)
            })
            .cloned()
            .collect::<Vec<_>>();
        messages.sort_by_key(|message| message.message_id);
        let read_inbox_max_id = state.dialog_read_inbox_max_ids.get(&canonical_id).copied();
        Ok(DiscussionMessageSummary {
            source_chat_id: Self::canonical_chat_ref(&state.dialogs, canonical_id),
            source_message_id: message_id,
            max_id: messages.last().map(|message| message.message_id),
            read_inbox_max_id,
            read_outbox_max_id: messages
                .iter()
                .filter(|message| message.outgoing)
                .map(|message| message.message_id)
                .max(),
            unread_count: state
                .dialogs
                .iter()
                .find(|dialog| dialog.chat_id == canonical_id)
                .map_or(0, |dialog| dialog.unread_count),
            messages,
        })
    }

    async fn export_message_link(
        &self,
        _session_path: Option<PathBuf>,
        chat_id: &str,
        message_id: i32,
        grouped: bool,
        thread: bool,
    ) -> Result<MessageLinkSummary, TelegramError> {
        let state = self.state.lock().await;
        let canonical_id = Self::resolve_dialog(&state.dialogs, chat_id)
            .ok_or_else(|| TelegramError::PeerNotFound(chat_id.to_string()))?;
        let dialog = state
            .dialogs
            .iter()
            .find(|dialog| dialog.chat_id == canonical_id)
            .ok_or_else(|| TelegramError::PeerNotFound(chat_id.to_string()))?;
        if !matches!(dialog.chat_kind.as_str(), "channel" | "supergroup" | "gigagroup") {
            return Err(TelegramError::InvalidPeer(
                "message links require a channel or supergroup".to_string(),
            ));
        }
        let exists = state.messages.get(&canonical_id).is_some_and(|messages| {
            messages.iter().any(|message| message.message_id == message_id)
        });
        if !exists {
            return Err(TelegramError::MessageNotFound(format!("{chat_id}:{message_id}")));
        }
        let link = format!("https://t.me/c/{canonical_id}/{message_id}");
        Ok(MessageLinkSummary {
            chat_id: dialog.canonical_chat_id.clone(),
            message_id,
            grouped,
            thread,
            html: format!("<a href=\"{link}\">message {message_id}</a>"),
            link,
        })
    }

    async fn get_web_page_preview(
        &self,
        _session_path: Option<PathBuf>,
        message: &str,
    ) -> Result<WebPagePreviewSummary, TelegramError> {
        let url = message
            .split_whitespace()
            .map(|part| {
                part.trim_matches(|character: char| {
                    matches!(character, ',' | '.' | ')' | ']' | '}' | '>' | '"' | '\'')
                })
            })
            .find(|part| part.starts_with("https://") || part.starts_with("http://"))
            .map(str::to_string);
        let Some(url) = url else {
            return Ok(WebPagePreviewSummary {
                state: "empty".to_string(),
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
                safe: true,
                related_chats: Vec::new(),
                related_users: Vec::new(),
            });
        };
        let display_url = url
            .strip_prefix("https://")
            .or_else(|| url.strip_prefix("http://"))
            .unwrap_or(url.as_str())
            .to_string();
        Ok(WebPagePreviewSummary {
            state: "loaded".to_string(),
            webpage_id: Some(9_001),
            pending_until_unix: None,
            url: Some(url),
            display_url: Some(display_url),
            page_type: Some("article".to_string()),
            site_name: Some("Fixture Web".to_string()),
            title: Some("Release preview".to_string()),
            description: Some("Deterministic Telegram webpage preview".to_string()),
            author: Some("Fixture".to_string()),
            embed_url: None,
            embed_type: None,
            embed_width: None,
            embed_height: None,
            duration_seconds: None,
            has_photo: true,
            has_document: false,
            has_cached_page: false,
            has_large_media: true,
            video_cover_photo: false,
            force_large_media: false,
            force_small_media: false,
            manual: false,
            safe: true,
            related_chats: Vec::new(),
            related_users: Vec::new(),
        })
    }

    async fn get_groups_for_discussion(
        &self,
        _session_path: Option<PathBuf>,
    ) -> Result<Vec<ChatSummary>, TelegramError> {
        let state = self.state.lock().await;
        let mut chats = state
            .known_chats
            .values()
            .map(|details| details.chat.clone())
            .filter(|chat| matches!(chat.kind.as_str(), "group" | "supergroup"))
            .collect::<Vec<_>>();
        chats.sort_by(|left, right| left.chat_id.cmp(&right.chat_id));
        Ok(chats)
    }

    async fn list_contacts(
        &self,
        _session_path: Option<PathBuf>,
        limit: usize,
    ) -> Result<Vec<ContactSummary>, TelegramError> {
        Ok(self
            .state
            .lock()
            .await
            .contacts
            .iter()
            .filter(|contact| contact.is_contact)
            .take(limit)
            .cloned()
            .collect())
    }

    async fn search_contacts(
        &self,
        _session_path: Option<PathBuf>,
        query: &str,
        limit: usize,
    ) -> Result<Vec<ContactSummary>, TelegramError> {
        let query = query.trim().trim_start_matches('@').to_lowercase();
        Ok(self
            .state
            .lock()
            .await
            .contacts
            .iter()
            .filter(|contact| contact.is_contact)
            .filter(|contact| {
                contact.display_name.to_lowercase().contains(&query)
                    || contact
                        .username
                        .as_deref()
                        .is_some_and(|username| username.to_lowercase().contains(&query))
                    || contact.user_id.to_string() == query
            })
            .take(limit)
            .cloned()
            .collect())
    }

    async fn get_pinned_message(
        &self,
        _session_path: Option<PathBuf>,
        chat_id: &str,
    ) -> Result<Option<MessageSummary>, TelegramError> {
        let state = self.state.lock().await;
        let canonical_id = Self::resolve_dialog(&state.dialogs, chat_id)
            .ok_or_else(|| TelegramError::PeerNotFound(chat_id.to_string()))?;
        let Some(message_id) = state.pinned_messages.get(&canonical_id) else {
            return Ok(None);
        };
        Ok(state
            .messages
            .get(&canonical_id)
            .and_then(|messages| messages.iter().find(|message| message.message_id == *message_id))
            .cloned())
    }

    async fn get_scheduled_messages(
        &self,
        _session_path: Option<PathBuf>,
        chat_id: &str,
    ) -> Result<Vec<MessageSummary>, TelegramError> {
        let state = self.state.lock().await;
        let canonical_id = Self::resolve_dialog(&state.dialogs, chat_id)
            .ok_or_else(|| TelegramError::PeerNotFound(chat_id.to_string()))?;
        Ok(state.scheduled_messages.get(&canonical_id).cloned().unwrap_or_default())
    }

    async fn get_poll(
        &self,
        _session_path: Option<PathBuf>,
        chat_id: &str,
        message_id: i32,
    ) -> Result<Option<PollSummary>, TelegramError> {
        let state = self.state.lock().await;
        let canonical_id = Self::resolve_dialog(&state.dialogs, chat_id)
            .ok_or_else(|| TelegramError::PeerNotFound(chat_id.to_string()))?;
        Ok(state.polls.get(&(canonical_id, message_id)).cloned())
    }

    async fn get_poll_votes(
        &self,
        _session_path: Option<PathBuf>,
        chat_id: &str,
        message_id: i32,
        option_index: Option<usize>,
        limit: usize,
        offset: Option<&str>,
    ) -> Result<PollVoterPage, TelegramError> {
        let state = self.state.lock().await;
        let canonical_id = Self::resolve_dialog(&state.dialogs, chat_id)
            .ok_or_else(|| TelegramError::PeerNotFound(chat_id.to_string()))?;
        let poll = state
            .polls
            .get(&(canonical_id, message_id))
            .ok_or_else(|| TelegramError::MessageNotFound(format!("{chat_id}:{message_id}")))?;
        if let Some(index) = option_index
            && !poll.answers.iter().any(|answer| answer.option.index == index)
        {
            return Err(TelegramError::InvalidPeer(format!(
                "poll option index {index} was not found"
            )));
        }
        let matching = state
            .poll_voters
            .get(&(canonical_id, message_id))
            .into_iter()
            .flatten()
            .filter(|voter| option_index.is_none_or(|index| voter.option_indices.contains(&index)))
            .cloned()
            .collect::<Vec<_>>();
        let total_count = i32::try_from(matching.len()).unwrap_or(i32::MAX);
        let voters = if offset.is_some() {
            Vec::new()
        } else {
            matching.into_iter().take(limit).collect::<Vec<_>>()
        };
        let next_offset = (offset.is_none()
            && voters.len() < usize::try_from(total_count).unwrap_or(usize::MAX))
        .then(|| "fixture-next".to_string());
        Ok(PollVoterPage { total_count, voters, next_offset })
    }

    async fn create_poll_with_random_id(
        &self,
        _session_path: Option<PathBuf>,
        chat_id: &str,
        poll: &PollCreateSpec,
        reply_to_message_id: Option<i32>,
        silent: bool,
        noforwards: bool,
        random_id: i64,
    ) -> Result<i32, TelegramError> {
        let mut state = self.state.lock().await;
        if let Some(message_id) = state.sent_by_random_id.get(&random_id) {
            return Ok(*message_id);
        }
        let canonical_id = Self::resolve_dialog(&state.dialogs, chat_id)
            .ok_or_else(|| TelegramError::PeerNotFound(chat_id.to_string()))?;
        if let Some(reply_to_message_id) = reply_to_message_id
            && !state.messages.get(&canonical_id).is_some_and(|messages| {
                messages.iter().any(|message| message.message_id == reply_to_message_id)
            })
        {
            return Err(TelegramError::MessageNotFound(format!("{chat_id}:{reply_to_message_id}")));
        }
        state.poll_create_calls = state.poll_create_calls.saturating_add(1);
        let message_id = state.next_message_id;
        state.next_message_id = state.next_message_id.saturating_add(1);
        state.sent_by_random_id.insert(random_id, message_id);
        let poll_id = 10_000_i64.saturating_add(i64::from(message_id));
        let answers = poll
            .answers
            .iter()
            .enumerate()
            .map(|(index, text)| PollAnswerSummary {
                option: PollOptionBinding {
                    index,
                    text: text.clone(),
                    option_hex: hex_encode(&[u8::try_from(index).unwrap_or(u8::MAX)]),
                },
                media_type: None,
                added_by_ref: None,
                date_unix: None,
                chosen: false,
                correct: poll.correct_option_index == Some(index),
                voters: Some(0),
                recent_voter_refs: Vec::new(),
            })
            .collect::<Vec<_>>();
        let canonical_chat_id = Self::canonical_chat_ref(&state.dialogs, canonical_id);
        let summary = PollSummary {
            chat_id: canonical_chat_id.clone(),
            message_id,
            poll_id,
            closed: false,
            public_voters: poll.public_voters,
            multiple_choice: poll.multiple_choice,
            quiz: poll.quiz,
            open_answers: poll.open_answers,
            revoting_disabled: poll.revoting_disabled,
            shuffle_answers: poll.shuffle_answers,
            hide_results_until_close: poll.hide_results_until_close,
            creator: true,
            subscribers_only: poll.subscribers_only,
            question: poll.question.clone(),
            answers,
            unmatched_result_option_hex: Vec::new(),
            close_period_seconds: poll.close_period_seconds,
            close_date_unix: poll.close_date_unix,
            countries_iso2: poll.countries_iso2.clone(),
            poll_hash: poll_id,
            results_minimal: false,
            has_unread_votes: false,
            can_view_stats: poll.public_voters,
            total_voters: Some(0),
            recent_voter_refs: Vec::new(),
            solution: poll.solution.clone(),
            solution_has_media: false,
            attached_media_type: None,
        };
        let account_user_id = state.account.user_id;
        state.messages.entry(canonical_id).or_default().insert(
            0,
            MessageSummary {
                message_id,
                chat_ref: Some(canonical_chat_id),
                sender_ref: Some(account_user_id.to_string()),
                outgoing: true,
                pinned: false,
                silent,
                noforwards,
                text: None,
                date: 1_700_000_000_i32.saturating_add(message_id),
                edit_date: None,
                reply_to_message_id,
                views: None,
                forwards: None,
                reply_count: None,
                media_type: Some("poll".to_string()),
                forward_origin: None,
            },
        );
        state.polls.insert((canonical_id, message_id), summary);
        state.poll_voters.insert((canonical_id, message_id), Vec::new());
        state.sent.push(FakeSentMessage {
            chat_id: canonical_id.to_string(),
            text: poll.question.clone(),
            message_id,
            file_name: None,
            reply_to_message_id,
            file_mode: None,
        });
        Ok(message_id)
    }

    async fn set_poll_vote(
        &self,
        _session_path: Option<PathBuf>,
        chat_id: &str,
        message_id: i32,
        options: &[Vec<u8>],
    ) -> Result<(), TelegramError> {
        let mut state = self.state.lock().await;
        let canonical_id = Self::resolve_dialog(&state.dialogs, chat_id)
            .ok_or_else(|| TelegramError::PeerNotFound(chat_id.to_string()))?;
        let requested_hex = options.iter().map(|option| hex_encode(option)).collect::<Vec<_>>();
        let current_poll = state
            .polls
            .get(&(canonical_id, message_id))
            .ok_or_else(|| TelegramError::MessageNotFound(format!("{chat_id}:{message_id}")))?;
        if current_poll.closed {
            return Err(TelegramError::SendRejected("poll is closed".to_string()));
        }
        let available = current_poll
            .answers
            .iter()
            .map(|answer| (answer.option.option_hex.clone(), answer.option.index))
            .collect::<HashMap<_, _>>();
        let option_indices = requested_hex
            .iter()
            .map(|option| {
                available.get(option).copied().ok_or_else(|| {
                    TelegramError::InvalidPeer(format!("unknown poll option {option}"))
                })
            })
            .collect::<Result<Vec<_>, _>>()?;
        let account_user_id = state.account.user_id;
        let account_ref = account_user_id.to_string();
        let voters = state.poll_voters.entry((canonical_id, message_id)).or_default();
        voters.retain(|voter| voter.peer_ref != account_ref);
        if !option_indices.is_empty() {
            voters.push(PollVoterSummary {
                peer_ref: account_ref.clone(),
                option_indices: option_indices.clone(),
                unknown_option_hex: Vec::new(),
                submitted_open_answer: false,
                date_unix: 1_700_003_000_i32.saturating_add(message_id),
            });
        }
        let voter_snapshot = voters.clone();
        let poll = state
            .polls
            .get_mut(&(canonical_id, message_id))
            .ok_or_else(|| TelegramError::MessageNotFound(format!("{chat_id}:{message_id}")))?;
        for answer in &mut poll.answers {
            answer.chosen = option_indices.contains(&answer.option.index);
            let matching = voter_snapshot
                .iter()
                .filter(|voter| voter.option_indices.contains(&answer.option.index))
                .collect::<Vec<_>>();
            answer.voters = Some(i32::try_from(matching.len()).unwrap_or(i32::MAX));
            answer.recent_voter_refs =
                matching.into_iter().rev().take(3).map(|voter| voter.peer_ref.clone()).collect();
        }
        poll.total_voters = Some(i32::try_from(voter_snapshot.len()).unwrap_or(i32::MAX));
        poll.recent_voter_refs =
            voter_snapshot.iter().rev().take(3).map(|voter| voter.peer_ref.clone()).collect();
        poll.poll_hash = poll.poll_hash.saturating_add(1);
        state.poll_vote_calls = state.poll_vote_calls.saturating_add(1);
        Ok(())
    }

    async fn stop_poll(
        &self,
        _session_path: Option<PathBuf>,
        chat_id: &str,
        message_id: i32,
    ) -> Result<(), TelegramError> {
        let mut state = self.state.lock().await;
        let canonical_id = Self::resolve_dialog(&state.dialogs, chat_id)
            .ok_or_else(|| TelegramError::PeerNotFound(chat_id.to_string()))?;
        let poll = state
            .polls
            .get_mut(&(canonical_id, message_id))
            .ok_or_else(|| TelegramError::MessageNotFound(format!("{chat_id}:{message_id}")))?;
        poll.closed = true;
        poll.poll_hash = poll.poll_hash.saturating_add(1);
        state.poll_stop_calls = state.poll_stop_calls.saturating_add(1);
        Ok(())
    }

    async fn schedule_text_with_random_id(
        &self,
        _session_path: Option<PathBuf>,
        chat_id: &str,
        text: &str,
        parse_mode: CaptionParseMode,
        reply_to_message_id: Option<i32>,
        schedule_date_unix: i32,
        silent: bool,
        noforwards: bool,
        random_id: i64,
    ) -> Result<i32, TelegramError> {
        let mut state = self.state.lock().await;
        if let Some(message_id) = state.sent_by_random_id.get(&random_id) {
            return Ok(*message_id);
        }
        let canonical_id = Self::resolve_dialog(&state.dialogs, chat_id)
            .ok_or_else(|| TelegramError::PeerNotFound(chat_id.to_string()))?;
        if let Some(reply_to_message_id) = reply_to_message_id
            && !state.messages.get(&canonical_id).is_some_and(|messages| {
                messages.iter().any(|message| message.message_id == reply_to_message_id)
            })
        {
            return Err(TelegramError::PeerNotFound(format!(
                "reply message {reply_to_message_id} in {chat_id}"
            )));
        }
        state.schedule_create_calls = state.schedule_create_calls.saturating_add(1);
        let message_id = state.next_message_id;
        state.next_message_id = state.next_message_id.saturating_add(1);
        state.sent_by_random_id.insert(random_id, message_id);
        let account_user_id = state.account.user_id;
        let chat_ref = Self::canonical_chat_ref(&state.dialogs, canonical_id);
        let rendered_text = match parse_mode {
            CaptionParseMode::Plain => text.to_string(),
            CaptionParseMode::Markdown => ferogram::InputMessage::markdown(text).text,
            CaptionParseMode::Html => ferogram::InputMessage::html(text).text,
            CaptionParseMode::RichMarkdown => String::new(),
        };
        state.scheduled_messages.entry(canonical_id).or_default().push(MessageSummary {
            message_id,
            chat_ref: Some(chat_ref.clone()),
            sender_ref: Some(account_user_id.to_string()),
            outgoing: true,
            pinned: false,
            silent,
            noforwards,
            text: (!matches!(parse_mode, CaptionParseMode::RichMarkdown)).then_some(rendered_text),
            date: schedule_date_unix,
            edit_date: None,
            reply_to_message_id,
            views: None,
            forwards: None,
            reply_count: None,
            media_type: None,
            forward_origin: None,
        });
        if matches!(parse_mode, CaptionParseMode::RichMarkdown) {
            let normalized_text = crate::telegram::normalize_rich_markdown(text);
            let block_count = ferogram::parsers::parse_rich_markdown(&normalized_text).len();
            let mut state_hasher = Sha256::new();
            state_hasher.update(b"fake-rich-message:v1\0");
            state_hasher.update(text.as_bytes());
            state.rich_messages.insert(
                (canonical_id, message_id),
                RichMessageSnapshot {
                    chat_id: chat_ref,
                    message_id,
                    rtl: false,
                    part: false,
                    block_count,
                    photo_count: 0,
                    document_count: 0,
                    state_sha256: format!("{:x}", state_hasher.finalize()),
                },
            );
        }
        Ok(message_id)
    }

    #[allow(clippy::too_many_arguments)]
    async fn schedule_file_with_random_id(
        &self,
        _session_path: Option<PathBuf>,
        chat_id: &str,
        file_path: &std::path::Path,
        caption: &str,
        caption_parse_mode: CaptionParseMode,
        reply_to_message_id: Option<i32>,
        schedule_date_unix: i32,
        silent: bool,
        noforwards: bool,
        mode: FileSendMode,
        random_id: i64,
    ) -> Result<i32, TelegramError> {
        let is_rich = matches!(caption_parse_mode, CaptionParseMode::RichMarkdown);
        if is_rich && !matches!(mode, FileSendMode::Auto | FileSendMode::Photo) {
            return Err(TelegramError::InvalidPeer(
                "native RichMessage media currently supports photo uploads only".to_string(),
            ));
        }
        let metadata = tokio::fs::metadata(file_path).await?;
        if !metadata.is_file() {
            return Err(TelegramError::Io(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "upload path is not a regular file",
            )));
        }
        let bytes = tokio::fs::read(file_path).await?;
        let file_name =
            file_path.file_name().and_then(|value| value.to_str()).unwrap_or("attachment.bin");
        let mime = ferogram::media::detect_mime_from_bytes(&bytes, file_name);
        if is_rich && (!mime.starts_with("image/") || mime == "image/gif") {
            return Err(TelegramError::InvalidPeer(format!(
                "native RichMessage media requires a static image; detected {mime}"
            )));
        }
        let media_type = match mode {
            FileSendMode::Photo => "photo",
            FileSendMode::Gif => {
                if mime != "image/gif" {
                    return Err(TelegramError::InvalidPeer(format!(
                        "GIF mode requires image/gif bytes; detected {mime}"
                    )));
                }
                "gif"
            },
            FileSendMode::Document => "document",
            FileSendMode::Auto if mime.starts_with("video/") => "video",
            FileSendMode::Auto if is_rich && mime.starts_with("image/") => "photo",
            FileSendMode::Auto => "document",
        };
        let rendered_caption = match caption_parse_mode {
            CaptionParseMode::Plain => caption.to_string(),
            CaptionParseMode::Markdown => ferogram::InputMessage::markdown(caption).text,
            CaptionParseMode::Html => ferogram::InputMessage::html(caption).text,
            CaptionParseMode::RichMarkdown => String::new(),
        };
        let mut state = self.state.lock().await;
        if let Some(message_id) = state.sent_by_random_id.get(&random_id) {
            return Ok(*message_id);
        }
        let canonical_id = Self::resolve_dialog(&state.dialogs, chat_id)
            .ok_or_else(|| TelegramError::PeerNotFound(chat_id.to_string()))?;
        if let Some(reply_to_message_id) = reply_to_message_id
            && !state.messages.get(&canonical_id).is_some_and(|messages| {
                messages.iter().any(|message| message.message_id == reply_to_message_id)
            })
        {
            return Err(TelegramError::PeerNotFound(format!(
                "reply message {reply_to_message_id} in {chat_id}"
            )));
        }
        state.schedule_create_calls = state.schedule_create_calls.saturating_add(1);
        let message_id = state.next_message_id;
        state.next_message_id = state.next_message_id.saturating_add(1);
        state.sent_by_random_id.insert(random_id, message_id);
        let account_user_id = state.account.user_id;
        let chat_ref = Self::canonical_chat_ref(&state.dialogs, canonical_id);
        let rich_chat_ref = chat_ref.clone();
        state.scheduled_messages.entry(canonical_id).or_default().push(MessageSummary {
            message_id,
            chat_ref: Some(chat_ref),
            sender_ref: Some(account_user_id.to_string()),
            outgoing: true,
            pinned: false,
            silent,
            noforwards,
            text: (!is_rich).then_some(rendered_caption),
            date: schedule_date_unix,
            edit_date: None,
            reply_to_message_id,
            views: None,
            forwards: None,
            reply_count: None,
            media_type: (!is_rich).then_some(media_type.to_string()),
            forward_origin: None,
        });
        if is_rich {
            let normalized_caption = crate::telegram::normalize_rich_markdown(caption);
            let block_count = ferogram::parsers::parse_rich_markdown(&normalized_caption).len();
            let mut state_hasher = Sha256::new();
            state_hasher.update(b"fake-rich-file-message:v1\0");
            state_hasher.update(caption.as_bytes());
            state_hasher.update(&bytes);
            state.rich_messages.insert(
                (canonical_id, message_id),
                RichMessageSnapshot {
                    chat_id: rich_chat_ref,
                    message_id,
                    rtl: false,
                    part: false,
                    block_count: block_count.saturating_add(1),
                    photo_count: 1,
                    document_count: 0,
                    state_sha256: format!("{:x}", state_hasher.finalize()),
                },
            );
        } else {
            state.attachments.insert(
                (canonical_id, message_id),
                (
                    AttachmentDescriptor {
                        attachment_id: format!("{chat_id}:{message_id}:0"),
                        chat_id: chat_id.to_string(),
                        message_id,
                        kind: if media_type == "photo" {
                            "photo".to_string()
                        } else {
                            "document".to_string()
                        },
                        file_name: Some(file_name.to_string()),
                        mime_type: Some(mime),
                        size_bytes: Some(u64::try_from(bytes.len()).unwrap_or(u64::MAX)),
                        downloadable: true,
                    },
                    bytes,
                ),
            );
        }
        Ok(message_id)
    }

    async fn edit_scheduled_message(
        &self,
        _session_path: Option<PathBuf>,
        chat_id: &str,
        message_id: i32,
        text: &str,
        parse_mode: CaptionParseMode,
        schedule_date_unix: i32,
    ) -> Result<(), TelegramError> {
        let mut state = self.state.lock().await;
        let canonical_id = Self::resolve_dialog(&state.dialogs, chat_id)
            .ok_or_else(|| TelegramError::PeerNotFound(chat_id.to_string()))?;
        let rendered_text = match parse_mode {
            CaptionParseMode::Plain => text.to_string(),
            CaptionParseMode::Markdown => ferogram::InputMessage::markdown(text).text,
            CaptionParseMode::Html => ferogram::InputMessage::html(text).text,
            CaptionParseMode::RichMarkdown => String::new(),
        };
        let is_rich = matches!(parse_mode, CaptionParseMode::RichMarkdown);
        let message = state
            .scheduled_messages
            .get_mut(&canonical_id)
            .and_then(|messages| {
                messages.iter_mut().find(|message| message.message_id == message_id)
            })
            .ok_or_else(|| {
                TelegramError::PeerNotFound(format!("scheduled message {message_id}"))
            })?;
        message.text = (!is_rich).then_some(rendered_text);
        message.date = schedule_date_unix;
        message.edit_date = Some(1_700_000_000_i32.saturating_add(message_id));
        let chat_ref = message.chat_ref.clone().unwrap_or_else(|| chat_id.to_string());
        if is_rich {
            let normalized_text = crate::telegram::normalize_rich_markdown(text);
            let block_count = ferogram::parsers::parse_rich_markdown(&normalized_text).len();
            let mut state_hasher = Sha256::new();
            state_hasher.update(b"fake-rich-message:v1\0");
            state_hasher.update(text.as_bytes());
            state.rich_messages.insert(
                (canonical_id, message_id),
                RichMessageSnapshot {
                    chat_id: chat_ref,
                    message_id,
                    rtl: false,
                    part: false,
                    block_count,
                    photo_count: 0,
                    document_count: 0,
                    state_sha256: format!("{:x}", state_hasher.finalize()),
                },
            );
        } else {
            state.rich_messages.remove(&(canonical_id, message_id));
        }
        state.schedule_edit_calls = state.schedule_edit_calls.saturating_add(1);
        Ok(())
    }

    async fn delete_scheduled_messages(
        &self,
        _session_path: Option<PathBuf>,
        chat_id: &str,
        message_ids: &[i32],
    ) -> Result<(), TelegramError> {
        let mut state = self.state.lock().await;
        let canonical_id = Self::resolve_dialog(&state.dialogs, chat_id)
            .ok_or_else(|| TelegramError::PeerNotFound(chat_id.to_string()))?;
        let scheduled = state.scheduled_messages.entry(canonical_id).or_default();
        if message_ids
            .iter()
            .any(|message_id| !scheduled.iter().any(|message| message.message_id == *message_id))
        {
            return Err(TelegramError::PeerNotFound(
                "one or more scheduled messages were not found".to_string(),
            ));
        }
        let ids = message_ids.iter().copied().collect::<HashSet<_>>();
        scheduled.retain(|message| !ids.contains(&message.message_id));
        state.schedule_delete_calls = state.schedule_delete_calls.saturating_add(1);
        Ok(())
    }

    async fn send_scheduled_messages_now(
        &self,
        _session_path: Option<PathBuf>,
        chat_id: &str,
        message_ids: &[i32],
    ) -> Result<Vec<i32>, TelegramError> {
        let mut state = self.state.lock().await;
        let canonical_id = Self::resolve_dialog(&state.dialogs, chat_id)
            .ok_or_else(|| TelegramError::PeerNotFound(chat_id.to_string()))?;
        let scheduled = state.scheduled_messages.entry(canonical_id).or_default();
        let selected = message_ids
            .iter()
            .map(|message_id| {
                scheduled
                    .iter()
                    .find(|message| message.message_id == *message_id)
                    .cloned()
                    .ok_or_else(|| {
                        TelegramError::PeerNotFound(format!("scheduled message {message_id}"))
                    })
            })
            .collect::<Result<Vec<_>, _>>()?;
        let ids = message_ids.iter().copied().collect::<HashSet<_>>();
        scheduled.retain(|message| !ids.contains(&message.message_id));

        let account_user_id = state.account.user_id;
        let chat_ref = Self::canonical_chat_ref(&state.dialogs, canonical_id);
        let mut sent_message_ids = Vec::with_capacity(selected.len());
        for scheduled in selected {
            let message_id = state.next_message_id;
            state.next_message_id = state.next_message_id.saturating_add(1);
            state.sent.push(FakeSentMessage {
                chat_id: canonical_id.to_string(),
                text: scheduled.text.clone().unwrap_or_default(),
                message_id,
                file_name: None,
                reply_to_message_id: scheduled.reply_to_message_id,
                file_mode: None,
            });
            state.messages.entry(canonical_id).or_default().insert(
                0,
                MessageSummary {
                    message_id,
                    chat_ref: Some(chat_ref.clone()),
                    sender_ref: Some(account_user_id.to_string()),
                    outgoing: true,
                    pinned: false,
                    silent: scheduled.silent,
                    noforwards: scheduled.noforwards,
                    text: scheduled.text,
                    date: 1_700_000_000_i32.saturating_add(message_id),
                    edit_date: None,
                    reply_to_message_id: scheduled.reply_to_message_id,
                    views: None,
                    forwards: None,
                    reply_count: None,
                    media_type: scheduled.media_type,
                    forward_origin: None,
                },
            );
            sent_message_ids.push(message_id);
        }
        state.schedule_send_now_calls = state.schedule_send_now_calls.saturating_add(1);
        Ok(sent_message_ids)
    }

    async fn list_drafts(
        &self,
        _session_path: Option<PathBuf>,
        limit: usize,
    ) -> Result<Vec<DialogDraftSummary>, TelegramError> {
        let state = self.state.lock().await;
        Ok(state
            .dialogs
            .iter()
            .filter_map(|dialog| {
                state.drafts.get(&dialog.chat_id).cloned().map(|draft| DialogDraftSummary {
                    chat_id: dialog.canonical_chat_id.clone(),
                    title: dialog.title.clone(),
                    draft,
                })
            })
            .take(limit)
            .collect())
    }

    async fn get_draft(
        &self,
        _session_path: Option<PathBuf>,
        chat_id: &str,
    ) -> Result<Option<DraftSummary>, TelegramError> {
        let state = self.state.lock().await;
        let canonical_id = Self::resolve_dialog(&state.dialogs, chat_id)
            .ok_or_else(|| TelegramError::PeerNotFound(chat_id.to_string()))?;
        Ok(state.drafts.get(&canonical_id).cloned())
    }

    async fn save_draft(
        &self,
        _session_path: Option<PathBuf>,
        chat_id: &str,
        text: Option<&str>,
    ) -> Result<(), TelegramError> {
        let mut state = self.state.lock().await;
        let canonical_id = Self::resolve_dialog(&state.dialogs, chat_id)
            .ok_or_else(|| TelegramError::PeerNotFound(chat_id.to_string()))?;
        state.draft_calls = state.draft_calls.saturating_add(1);
        if let Some(text) = text {
            let date_unix = 1_700_002_000_i32
                .saturating_add(i32::try_from(state.draft_calls).unwrap_or(i32::MAX));
            state.drafts.insert(
                canonical_id,
                finalize_fixture_draft(DraftSummary {
                    message: text.to_string(),
                    date_unix,
                    no_webpage: false,
                    invert_media: false,
                    has_reply_to: false,
                    has_entities: false,
                    has_media: false,
                    effect_id: None,
                    has_suggested_post: false,
                    has_rich_message: false,
                    state_sha256: String::new(),
                }),
            );
        } else {
            state.drafts.remove(&canonical_id);
        }
        Ok(())
    }

    async fn clear_all_drafts(&self, _session_path: Option<PathBuf>) -> Result<(), TelegramError> {
        let mut state = self.state.lock().await;
        state.draft_clear_all_calls = state.draft_clear_all_calls.saturating_add(1);
        state.drafts.clear();
        if std::mem::take(&mut state.draft_clear_all_uncertain_once) {
            return Err(TelegramError::DeliveryUncertain(
                "fixture cleared all drafts before the transport receipt was lost".to_string(),
            ));
        }
        Ok(())
    }

    async fn get_dialog_state(
        &self,
        _session_path: Option<PathBuf>,
        chat_id: &str,
    ) -> Result<DialogStateSnapshot, TelegramError> {
        let state = self.state.lock().await;
        let canonical_id = Self::resolve_dialog(&state.dialogs, chat_id)
            .ok_or_else(|| TelegramError::PeerNotFound(chat_id.to_string()))?;
        let dialog = state
            .dialogs
            .iter()
            .find(|dialog| dialog.chat_id == canonical_id)
            .ok_or_else(|| TelegramError::PeerNotFound(chat_id.to_string()))?;
        Ok(DialogStateSnapshot {
            pinned: dialog.pinned,
            unread_mark: dialog.unread_mark,
            top_message_id: dialog.last_message_id.unwrap_or(0),
            read_inbox_max_id: state
                .dialog_read_inbox_max_ids
                .get(&canonical_id)
                .copied()
                .unwrap_or(0),
            unread_count: dialog.unread_count,
            unread_reactions_count: state
                .dialog_unread_reactions
                .get(&canonical_id)
                .copied()
                .unwrap_or(0),
            archived: dialog.archived,
            folder_id: dialog.folder_id,
        })
    }

    async fn set_dialog_unread(
        &self,
        _session_path: Option<PathBuf>,
        chat_id: &str,
        unread: bool,
    ) -> Result<(), TelegramError> {
        let mut state = self.state.lock().await;
        let canonical_id = Self::resolve_dialog(&state.dialogs, chat_id)
            .ok_or_else(|| TelegramError::PeerNotFound(chat_id.to_string()))?;
        state.dialog_unread_calls = state.dialog_unread_calls.saturating_add(1);
        let dialog = state
            .dialogs
            .iter_mut()
            .find(|dialog| dialog.chat_id == canonical_id)
            .ok_or_else(|| TelegramError::PeerNotFound(chat_id.to_string()))?;
        dialog.unread_mark = unread;
        if !unread {
            dialog.unread_count = 0;
            let top_message_id = dialog.last_message_id.unwrap_or(0);
            state.dialog_read_inbox_max_ids.insert(canonical_id, top_message_id);
        }
        Ok(())
    }

    async fn mark_messages_read(
        &self,
        _session_path: Option<PathBuf>,
        chat_id: &str,
        max_id: i32,
    ) -> Result<(), TelegramError> {
        let mut state = self.state.lock().await;
        let canonical_id = Self::resolve_dialog(&state.dialogs, chat_id)
            .ok_or_else(|| TelegramError::PeerNotFound(chat_id.to_string()))?;
        state.mark_messages_read_calls = state.mark_messages_read_calls.saturating_add(1);
        let previous = state.dialog_read_inbox_max_ids.get(&canonical_id).copied().unwrap_or(0);
        state.dialog_read_inbox_max_ids.insert(canonical_id, previous.max(max_id));
        let dialog = state
            .dialogs
            .iter_mut()
            .find(|dialog| dialog.chat_id == canonical_id)
            .ok_or_else(|| TelegramError::PeerNotFound(chat_id.to_string()))?;
        let top_message_id = dialog.last_message_id.unwrap_or(0);
        if max_id >= top_message_id {
            dialog.unread_mark = false;
            dialog.unread_count = 0;
        } else if max_id > previous {
            dialog.unread_count = dialog.unread_count.saturating_sub(1);
        }
        Ok(())
    }

    async fn mark_reactions_read(
        &self,
        _session_path: Option<PathBuf>,
        chat_id: &str,
    ) -> Result<MessageReactionReadReceipt, TelegramError> {
        let mut state = self.state.lock().await;
        let canonical_id = Self::resolve_dialog(&state.dialogs, chat_id)
            .ok_or_else(|| TelegramError::PeerNotFound(chat_id.to_string()))?;
        state.mark_reactions_read_calls = state.mark_reactions_read_calls.saturating_add(1);
        state.dialog_unread_reactions.insert(canonical_id, 0);
        Ok(MessageReactionReadReceipt {
            pts: 1_000_i32.saturating_add(state.mark_reactions_read_calls as i32),
            pts_count: 1,
            offset: 0,
        })
    }

    async fn set_dialog_archived(
        &self,
        _session_path: Option<PathBuf>,
        chat_id: &str,
        archived: bool,
    ) -> Result<(), TelegramError> {
        let mut state = self.state.lock().await;
        let canonical_id = Self::resolve_dialog(&state.dialogs, chat_id)
            .ok_or_else(|| TelegramError::PeerNotFound(chat_id.to_string()))?;
        state.dialog_archive_calls = state.dialog_archive_calls.saturating_add(1);
        let dialog = state
            .dialogs
            .iter_mut()
            .find(|dialog| dialog.chat_id == canonical_id)
            .ok_or_else(|| TelegramError::PeerNotFound(chat_id.to_string()))?;
        dialog.archived = archived;
        dialog.folder_id = Some(if archived { 1 } else { 0 });
        Ok(())
    }

    async fn set_dialog_pinned(
        &self,
        _session_path: Option<PathBuf>,
        chat_id: &str,
        pinned: bool,
    ) -> Result<(), TelegramError> {
        let mut state = self.state.lock().await;
        let canonical_id = Self::resolve_dialog(&state.dialogs, chat_id)
            .ok_or_else(|| TelegramError::PeerNotFound(chat_id.to_string()))?;
        state.dialog_pin_calls = state.dialog_pin_calls.saturating_add(1);
        let dialog = state
            .dialogs
            .iter_mut()
            .find(|dialog| dialog.chat_id == canonical_id)
            .ok_or_else(|| TelegramError::PeerNotFound(chat_id.to_string()))?;
        dialog.pinned = pinned;
        Ok(())
    }

    async fn list_dialog_folders(
        &self,
        _session_path: Option<PathBuf>,
    ) -> Result<DialogFolderList, TelegramError> {
        Ok(self.state.lock().await.dialog_folders.clone())
    }

    async fn get_suggested_dialog_folders(
        &self,
        _session_path: Option<PathBuf>,
    ) -> Result<SuggestedDialogFolderList, TelegramError> {
        Ok(self.state.lock().await.suggested_dialog_folders.clone())
    }

    async fn set_dialog_folder_tags(
        &self,
        _session_path: Option<PathBuf>,
        enabled: bool,
    ) -> Result<(), TelegramError> {
        let mut state = self.state.lock().await;
        state.dialog_folder_tags_calls = state.dialog_folder_tags_calls.saturating_add(1);
        state.dialog_folders.tags_enabled = enabled;
        if std::mem::take(&mut state.dialog_folder_tags_uncertain_once) {
            return Err(TelegramError::DeliveryUncertain(
                "fixture changed folder tags before the transport receipt was lost".to_string(),
            ));
        }
        Ok(())
    }

    async fn get_global_privacy_settings(
        &self,
        _session_path: Option<PathBuf>,
    ) -> Result<GlobalPrivacySettingsSummary, TelegramError> {
        Ok(self.state.lock().await.global_privacy_settings.clone())
    }

    async fn set_global_privacy_settings(
        &self,
        _session_path: Option<PathBuf>,
        settings: &GlobalPrivacySettingsSummary,
    ) -> Result<(), TelegramError> {
        if let Some(stars) = settings.noncontact_peers_paid_stars.as_deref() {
            stars.parse::<i64>().ok().filter(|value| *value > 0).ok_or_else(|| {
                TelegramError::InvalidPeer(
                    "noncontact_peers_paid_stars must be a positive Telegram i64 amount"
                        .to_string(),
                )
            })?;
        }
        let mut state = self.state.lock().await;
        state.global_privacy_update_calls = state.global_privacy_update_calls.saturating_add(1);
        state.global_privacy_settings = settings.clone();
        if std::mem::take(&mut state.global_privacy_update_uncertain_once) {
            return Err(TelegramError::DeliveryUncertain(
                "fixture changed global privacy before the transport receipt was lost".to_string(),
            ));
        }
        Ok(())
    }

    async fn upsert_dialog_folder(
        &self,
        _session_path: Option<PathBuf>,
        folder: &DialogFolderSummary,
        preserve_title_entities: bool,
    ) -> Result<(), TelegramError> {
        let mut state = self.state.lock().await;
        let mut target = folder.clone();
        if let Some(existing) = state
            .dialog_folders
            .folders
            .iter()
            .find(|candidate| candidate.folder_id == folder.folder_id)
            && preserve_title_entities
            && existing.title == folder.title
        {
            target.title_has_entities = existing.title_has_entities;
        }
        if let Some(existing) = state
            .dialog_folders
            .folders
            .iter_mut()
            .find(|candidate| candidate.folder_id == folder.folder_id)
        {
            *existing = target;
        } else {
            state.dialog_folders.folders.push(target);
        }
        state.dialog_folder_upsert_calls = state.dialog_folder_upsert_calls.saturating_add(1);
        Ok(())
    }

    async fn delete_dialog_folder(
        &self,
        _session_path: Option<PathBuf>,
        folder_id: i32,
    ) -> Result<(), TelegramError> {
        let mut state = self.state.lock().await;
        state.dialog_folders.folders.retain(|folder| folder.folder_id != folder_id);
        state.dialog_folder_delete_calls = state.dialog_folder_delete_calls.saturating_add(1);
        Ok(())
    }

    async fn reorder_dialog_folders(
        &self,
        _session_path: Option<PathBuf>,
        folder_ids: &[i32],
    ) -> Result<(), TelegramError> {
        let mut state = self.state.lock().await;
        let default = state
            .dialog_folders
            .folders
            .iter()
            .find(|folder| folder.kind == DialogFolderKind::Default)
            .cloned();
        let by_id = state
            .dialog_folders
            .folders
            .iter()
            .filter(|folder| folder.kind != DialogFolderKind::Default)
            .map(|folder| (folder.folder_id, folder.clone()))
            .collect::<HashMap<_, _>>();
        if folder_ids.iter().any(|folder_id| !by_id.contains_key(folder_id)) {
            return Err(TelegramError::InvalidPeer("unknown dialog folder id".into()));
        }
        let mut folders = default.into_iter().collect::<Vec<_>>();
        folders.extend(folder_ids.iter().filter_map(|folder_id| by_id.get(folder_id).cloned()));
        state.dialog_folders.folders = folders;
        state.dialog_folder_reorder_calls = state.dialog_folder_reorder_calls.saturating_add(1);
        Ok(())
    }

    async fn set_dialog_folder_membership(
        &self,
        _session_path: Option<PathBuf>,
        folder_id: i32,
        chat_id: &str,
        included: bool,
    ) -> Result<(), TelegramError> {
        let mut state = self.state.lock().await;
        let canonical_id = Self::resolve_dialog(&state.dialogs, chat_id)
            .ok_or_else(|| TelegramError::PeerNotFound(chat_id.to_string()))?;
        let canonical = Self::canonical_chat_ref(&state.dialogs, canonical_id);
        let folder = state
            .dialog_folders
            .folders
            .iter_mut()
            .find(|folder| folder.folder_id == folder_id)
            .ok_or_else(|| TelegramError::InvalidPeer(format!("folder {folder_id}")))?;
        if included {
            folder.excluded_chat_ids.retain(|chat_id| chat_id != &canonical);
            if !folder.pinned_chat_ids.contains(&canonical)
                && !folder.included_chat_ids.contains(&canonical)
            {
                folder.included_chat_ids.push(canonical);
            }
        } else {
            folder.pinned_chat_ids.retain(|chat_id| chat_id != &canonical);
            folder.included_chat_ids.retain(|chat_id| chat_id != &canonical);
            if folder.kind == DialogFolderKind::Custom
                && !folder.excluded_chat_ids.contains(&canonical)
            {
                folder.excluded_chat_ids.push(canonical);
            }
        }
        state.dialog_folder_membership_calls =
            state.dialog_folder_membership_calls.saturating_add(1);
        Ok(())
    }

    async fn set_user_blocked(
        &self,
        _session_path: Option<PathBuf>,
        user_ref: &str,
        blocked: bool,
    ) -> Result<(), TelegramError> {
        let mut state = self.state.lock().await;
        let wanted = user_ref.trim().trim_start_matches('@');
        let user_id = state
            .contacts
            .iter()
            .find(|contact| {
                contact.user_id.to_string() == wanted
                    || contact
                        .username
                        .as_deref()
                        .is_some_and(|username| username.eq_ignore_ascii_case(wanted))
            })
            .map(|contact| contact.user_id)
            .ok_or_else(|| TelegramError::PeerNotFound(user_ref.to_string()))?;
        state.block_calls = state.block_calls.saturating_add(1);
        if blocked {
            state.blocked_users.insert(user_id);
        } else {
            state.blocked_users.remove(&user_id);
        }
        Ok(())
    }

    async fn get_replies(
        &self,
        _session_path: Option<PathBuf>,
        chat_id: &str,
        message_id: i32,
        limit: usize,
        offset_id: Option<i32>,
    ) -> Result<MessagePage, TelegramError> {
        let state = self.state.lock().await;
        let canonical_id = Self::resolve_dialog(&state.dialogs, chat_id)
            .ok_or_else(|| TelegramError::PeerNotFound(chat_id.to_string()))?;
        let mut messages = state
            .messages
            .get(&canonical_id)
            .into_iter()
            .flatten()
            .filter(|message| message.reply_to_message_id == Some(message_id))
            .filter(|message| offset_id.is_none_or(|offset| message.message_id < offset))
            .take(limit)
            .cloned()
            .collect::<Vec<_>>();
        messages.sort_by_key(|right| std::cmp::Reverse(right.message_id));
        let next_offset_id = (messages.len() == limit)
            .then(|| messages.last().map(|message| message.message_id))
            .flatten();
        let completeness =
            if next_offset_id.is_some() { Completeness::Page } else { Completeness::Complete };
        Ok(MessagePage { messages, next_offset_id, completeness })
    }

    async fn get_message_read_receipts(
        &self,
        _session_path: Option<PathBuf>,
        chat_id: &str,
        message_id: i32,
    ) -> Result<Vec<MessageReadReceipt>, TelegramError> {
        let state = self.state.lock().await;
        let canonical_id = Self::resolve_dialog(&state.dialogs, chat_id)
            .ok_or_else(|| TelegramError::PeerNotFound(chat_id.to_string()))?;
        Ok(state.read_receipts.get(&(canonical_id, message_id)).cloned().unwrap_or_default())
    }

    async fn get_outbox_read_date(
        &self,
        _session_path: Option<PathBuf>,
        chat_id: &str,
        message_id: i32,
    ) -> Result<OutboxReadDateSummary, TelegramError> {
        let state = self.state.lock().await;
        let canonical_id = Self::resolve_dialog(&state.dialogs, chat_id)
            .ok_or_else(|| TelegramError::PeerNotFound(chat_id.to_string()))?;
        let dialog = state
            .dialogs
            .iter()
            .find(|dialog| dialog.chat_id == canonical_id)
            .ok_or_else(|| TelegramError::PeerNotFound(chat_id.to_string()))?;
        if dialog.chat_kind != "user" {
            return Err(TelegramError::InvalidPeer(
                "outbox read dates are available only for private chats".to_string(),
            ));
        }
        let message = state
            .messages
            .get(&canonical_id)
            .and_then(|messages| messages.iter().find(|message| message.message_id == message_id))
            .ok_or_else(|| TelegramError::MessageNotFound(format!("{chat_id}:{message_id}")))?;
        if !message.outgoing {
            return Err(TelegramError::InvalidPeer(
                "outbox read dates require an outgoing message".to_string(),
            ));
        }
        Ok(OutboxReadDateSummary {
            chat_id: dialog.canonical_chat_id.clone(),
            message_id,
            read_at_unix: 1_700_000_020,
        })
    }

    async fn get_user_info(
        &self,
        _session_path: Option<PathBuf>,
        user_ref: &str,
    ) -> Result<UserDetails, TelegramError> {
        let state = self.state.lock().await;
        let wanted = user_ref.trim().trim_start_matches('@');
        let contact = if state.account.user_id.to_string() == wanted
            || state
                .account
                .username
                .as_deref()
                .is_some_and(|username| username.eq_ignore_ascii_case(wanted))
        {
            let mut contact = fixture_account_contact();
            contact.first_name = state.account.first_name.clone();
            contact.display_name = state.account.first_name.clone();
            contact.username = state.account.username.clone();
            contact.bot = state.account_is_bot;
            contact
        } else {
            state
                .contacts
                .iter()
                .find(|contact| {
                    contact.user_id.to_string() == wanted
                        || contact
                            .username
                            .as_deref()
                            .is_some_and(|username| username.eq_ignore_ascii_case(wanted))
                })
                .cloned()
                .ok_or_else(|| TelegramError::PeerNotFound(user_ref.to_string()))?
        };
        let blocked = state.blocked_users.contains(&contact.user_id);
        Ok(UserDetails {
            user: contact,
            about: Some("Fixture contact used by deterministic MCP E2E tests".to_string()),
            blocked,
            common_chats_count: 1,
            phone_calls_available: true,
            phone_calls_private: false,
            status: Some("recently".to_string()),
            status_timestamp_unix: None,
        })
    }

    async fn list_participants(
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

    async fn list_chat_members(
        &self,
        _session_path: Option<PathBuf>,
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
        let state = self.state.lock().await;
        let canonical_id = Self::resolve_dialog(&state.dialogs, chat_id)
            .ok_or_else(|| TelegramError::PeerNotFound(chat_id.to_string()))?;
        let dialog = state
            .dialogs
            .iter()
            .find(|dialog| dialog.chat_id == canonical_id)
            .ok_or_else(|| TelegramError::PeerNotFound(chat_id.to_string()))?;
        if dialog.chat_kind == "user" {
            return Err(TelegramError::InvalidPeer(
                "list_participants requires a group, supergroup, or channel".to_string(),
            ));
        }
        let canonical_chat_id = Self::canonical_chat_ref(&state.dialogs, canonical_id);
        let mut members = state
            .chat_members
            .iter()
            .filter(|((member_chat_id, _), _)| *member_chat_id == canonical_id)
            .map(|(_, member)| member.clone())
            .collect::<Vec<_>>();
        if members.is_empty() {
            let self_contact = ContactSummary {
                user_id: state.account.user_id,
                first_name: state.account.first_name.clone(),
                last_name: None,
                display_name: state.account.first_name.clone(),
                username: state.account.username.clone(),
                phone: None,
                is_contact: false,
                mutual_contact: false,
                bot: false,
                verified: false,
                premium: false,
                scam: false,
                restricted: false,
            };
            members.push(ChatMemberSummary {
                chat_id: canonical_chat_id.clone(),
                user: self_contact,
                role: ChatMemberRole::Creator,
                can_edit: false,
                is_self: true,
                left: false,
                joined_date_unix: None,
                inviter_user_id: None,
                promoted_by_user_id: None,
                kicked_by_user_id: None,
                rank: Some("Owner".to_string()),
                subscription_until_date_unix: None,
                via_request: false,
                admin_rights: Some(fixture_admin_rights()),
                banned_rights: None,
            });
            members.extend(state.contacts.iter().cloned().map(|user| ChatMemberSummary {
                chat_id: canonical_chat_id.clone(),
                user,
                role: ChatMemberRole::Member,
                can_edit: false,
                is_self: false,
                left: false,
                joined_date_unix: Some(1_700_000_100),
                inviter_user_id: Some(state.account.user_id),
                promoted_by_user_id: None,
                kicked_by_user_id: None,
                rank: None,
                subscription_until_date_unix: None,
                via_request: false,
                admin_rights: None,
                banned_rights: None,
            }));
        }
        members.sort_by_key(|member| member.user.user_id);
        members.retain(|member| participant_filter_matches(member, filter));
        let query = query.map(str::trim).filter(|value| !value.is_empty());
        if let Some(query) = query {
            members.retain(|member| participant_query_matches(member, query));
        }
        let total_count = members.len();
        let start = offset.min(total_count);
        let end = start.saturating_add(limit).min(total_count);
        let participants = members[start..end].to_vec();
        let returned_count = participants.len();
        let has_more = end < total_count;
        Ok(ParticipantPage {
            chat_id: canonical_chat_id,
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
        })
    }

    async fn get_chat_member(
        &self,
        _session_path: Option<PathBuf>,
        chat_id: &str,
        user_ref: &str,
    ) -> Result<Option<ChatMemberSummary>, TelegramError> {
        let state = self.state.lock().await;
        let canonical_id = Self::resolve_dialog(&state.dialogs, chat_id)
            .ok_or_else(|| TelegramError::PeerNotFound(chat_id.to_string()))?;
        let wanted = user_ref.trim().trim_start_matches('@');
        let user_id = if state.account.user_id.to_string() == wanted
            || state
                .account
                .username
                .as_deref()
                .is_some_and(|username| username.eq_ignore_ascii_case(wanted))
        {
            state.account.user_id
        } else {
            state
                .contacts
                .iter()
                .find(|contact| {
                    contact.user_id.to_string() == wanted
                        || contact
                            .username
                            .as_deref()
                            .is_some_and(|username| username.eq_ignore_ascii_case(wanted))
                })
                .map(|contact| contact.user_id)
                .ok_or_else(|| TelegramError::PeerNotFound(user_ref.to_string()))?
        };
        Ok(state.chat_members.get(&(canonical_id, user_id)).cloned())
    }

    async fn add_chat_member(
        &self,
        _session_path: Option<PathBuf>,
        chat_id: &str,
        user_ref: &str,
    ) -> Result<(), TelegramError> {
        let mut state = self.state.lock().await;
        let canonical_id = Self::resolve_dialog(&state.dialogs, chat_id)
            .ok_or_else(|| TelegramError::PeerNotFound(chat_id.to_string()))?;
        let wanted = user_ref.trim().trim_start_matches('@');
        let user = state
            .contacts
            .iter()
            .find(|contact| {
                contact.user_id.to_string() == wanted
                    || contact
                        .username
                        .as_deref()
                        .is_some_and(|username| username.eq_ignore_ascii_case(wanted))
            })
            .cloned()
            .ok_or_else(|| TelegramError::PeerNotFound(user_ref.to_string()))?;
        let user_id = user.user_id;
        let canonical_chat_id = Self::canonical_chat_ref(&state.dialogs, canonical_id);
        state.chat_member_add_calls = state.chat_member_add_calls.saturating_add(1);
        state.chat_member_invite_calls.push((canonical_chat_id.clone(), user_id));
        let scripted = state
            .chat_member_invite_scripts
            .get_mut(&(canonical_id, user_id))
            .and_then(VecDeque::pop_front)
            .unwrap_or(FakeInviteOutcome::Success);
        if scripted == FakeInviteOutcome::Rejected {
            return Err(TelegramError::SendRejected(
                "fixture rejected the selected member invitation".to_string(),
            ));
        }
        if scripted == FakeInviteOutcome::RetryableNoEffect {
            return Err(TelegramError::SendRetryable(
                "fixture proved the invitation was not dispatched".to_string(),
            ));
        }
        if scripted == FakeInviteOutcome::UncertainBeforeApply {
            return Err(TelegramError::DeliveryUncertain(
                "fixture lost the invitation receipt before applying it".to_string(),
            ));
        }
        if state
            .chat_members
            .get(&(canonical_id, user_id))
            .is_some_and(|member| member.role == ChatMemberRole::Banned)
        {
            return Err(TelegramError::SendRejected(
                "a banned user must be unbanned before they can be invited".to_string(),
            ));
        }
        let inviter_user_id = state.account.user_id;
        state.chat_members.insert(
            (canonical_id, user_id),
            ChatMemberSummary {
                chat_id: canonical_chat_id,
                user,
                role: ChatMemberRole::Member,
                can_edit: true,
                is_self: false,
                left: false,
                joined_date_unix: Some(1_700_005_000),
                inviter_user_id: Some(inviter_user_id),
                promoted_by_user_id: None,
                kicked_by_user_id: None,
                rank: None,
                subscription_until_date_unix: None,
                via_request: false,
                admin_rights: None,
                banned_rights: None,
            },
        );
        if scripted == FakeInviteOutcome::UncertainAfterApply {
            return Err(TelegramError::DeliveryUncertain(
                "fixture applied the invitation but lost its receipt".to_string(),
            ));
        }
        Ok(())
    }

    async fn remove_chat_member(
        &self,
        _session_path: Option<PathBuf>,
        chat_id: &str,
        user_ref: &str,
    ) -> Result<(), TelegramError> {
        let mut state = self.state.lock().await;
        let canonical_id = Self::resolve_dialog(&state.dialogs, chat_id)
            .ok_or_else(|| TelegramError::PeerNotFound(chat_id.to_string()))?;
        let wanted = user_ref.trim().trim_start_matches('@');
        let user_id = state
            .contacts
            .iter()
            .find(|contact| {
                contact.user_id.to_string() == wanted
                    || contact
                        .username
                        .as_deref()
                        .is_some_and(|username| username.eq_ignore_ascii_case(wanted))
            })
            .map(|contact| contact.user_id)
            .ok_or_else(|| TelegramError::PeerNotFound(user_ref.to_string()))?;
        let account_user_id = state.account.user_id;
        let member = state
            .chat_members
            .get_mut(&(canonical_id, user_id))
            .ok_or_else(|| TelegramError::PeerNotFound(user_id.to_string()))?;
        if member.role == ChatMemberRole::Creator {
            return Err(TelegramError::SendRejected(
                "the chat creator cannot be removed".to_string(),
            ));
        }
        member.role = ChatMemberRole::Left;
        member.left = true;
        member.can_edit = false;
        member.kicked_by_user_id = Some(account_user_id);
        member.admin_rights = None;
        member.banned_rights = None;
        member.rank = None;
        state.chat_member_remove_calls = state.chat_member_remove_calls.saturating_add(1);
        Ok(())
    }

    async fn set_chat_member_admin(
        &self,
        _session_path: Option<PathBuf>,
        chat_id: &str,
        user_ref: &str,
        rights: &ChatAdminRightsSummary,
        rank: Option<&str>,
    ) -> Result<(), TelegramError> {
        let mut state = self.state.lock().await;
        let canonical_id = Self::resolve_dialog(&state.dialogs, chat_id)
            .ok_or_else(|| TelegramError::PeerNotFound(chat_id.to_string()))?;
        let wanted = user_ref.trim().trim_start_matches('@');
        let user_id = state
            .contacts
            .iter()
            .find(|contact| {
                contact.user_id.to_string() == wanted
                    || contact
                        .username
                        .as_deref()
                        .is_some_and(|username| username.eq_ignore_ascii_case(wanted))
            })
            .map(|contact| contact.user_id)
            .ok_or_else(|| TelegramError::PeerNotFound(user_ref.to_string()))?;
        let account_user_id = state.account.user_id;
        let member = state
            .chat_members
            .get_mut(&(canonical_id, user_id))
            .ok_or_else(|| TelegramError::PeerNotFound(user_id.to_string()))?;
        if member.role == ChatMemberRole::Creator {
            return Err(TelegramError::SendRejected(
                "the chat creator rights cannot be changed".to_string(),
            ));
        }
        if member.left || matches!(member.role, ChatMemberRole::Banned | ChatMemberRole::Left) {
            return Err(TelegramError::SendRejected(
                "only a current member can be made an administrator".to_string(),
            ));
        }
        if rights.any() {
            member.role = ChatMemberRole::Admin;
            member.admin_rights = Some(rights.clone());
            member.rank = rank.map(str::to_string);
            member.promoted_by_user_id = Some(account_user_id);
        } else {
            member.role = ChatMemberRole::Member;
            member.admin_rights = None;
            member.rank = None;
            member.promoted_by_user_id = None;
        }
        state.chat_member_admin_calls = state.chat_member_admin_calls.saturating_add(1);
        Ok(())
    }

    async fn set_chat_member_restrictions(
        &self,
        _session_path: Option<PathBuf>,
        chat_id: &str,
        user_ref: &str,
        rights: &ChatBannedRightsSummary,
    ) -> Result<(), TelegramError> {
        let mut state = self.state.lock().await;
        let canonical_id = Self::resolve_dialog(&state.dialogs, chat_id)
            .ok_or_else(|| TelegramError::PeerNotFound(chat_id.to_string()))?;
        let wanted = user_ref.trim().trim_start_matches('@');
        let account_user_id = state.account.user_id;
        let user = state
            .contacts
            .iter()
            .find(|contact| {
                contact.user_id.to_string() == wanted
                    || contact
                        .username
                        .as_deref()
                        .is_some_and(|username| username.eq_ignore_ascii_case(wanted))
            })
            .cloned()
            .ok_or_else(|| TelegramError::PeerNotFound(user_ref.to_string()))?;
        let user_id = user.user_id;
        let member = state.chat_members.entry((canonical_id, user_id)).or_insert_with(|| {
            ChatMemberSummary {
                chat_id: chat_id.to_string(),
                user,
                role: ChatMemberRole::Left,
                can_edit: false,
                is_self: false,
                left: true,
                joined_date_unix: None,
                inviter_user_id: None,
                promoted_by_user_id: None,
                kicked_by_user_id: None,
                rank: None,
                subscription_until_date_unix: None,
                via_request: false,
                admin_rights: None,
                banned_rights: None,
            }
        });
        if matches!(member.role, ChatMemberRole::Creator | ChatMemberRole::Admin) {
            return Err(TelegramError::SendRejected(
                "administrators must be demoted before restrictions can be applied".to_string(),
            ));
        }
        if rights.any_restriction() {
            member.role = if rights.view_messages {
                ChatMemberRole::Banned
            } else {
                ChatMemberRole::Restricted
            };
            member.left = rights.view_messages;
            member.can_edit = true;
            member.kicked_by_user_id = Some(account_user_id);
            member.banned_rights = Some(rights.clone());
        } else {
            let was_left = member.left || member.role == ChatMemberRole::Banned;
            member.role = if was_left { ChatMemberRole::Left } else { ChatMemberRole::Member };
            member.left = was_left;
            member.kicked_by_user_id = None;
            member.banned_rights = None;
        }
        state.chat_member_restriction_calls = state.chat_member_restriction_calls.saturating_add(1);
        Ok(())
    }

    async fn list_invite_links(
        &self,
        _session_path: Option<PathBuf>,
        chat_id: &str,
        admin_user_id: i64,
        revoked: bool,
        limit: usize,
    ) -> Result<Vec<InviteLinkSummary>, TelegramError> {
        let state = self.state.lock().await;
        let canonical_id = Self::resolve_dialog(&state.dialogs, chat_id)
            .ok_or_else(|| TelegramError::PeerNotFound(chat_id.to_string()))?;
        Ok(state
            .invite_links
            .get(&canonical_id)
            .into_iter()
            .flatten()
            .filter(|invite| {
                invite.summary.revoked == revoked
                    && invite.summary.admin_user_id == Some(admin_user_id)
            })
            .take(limit)
            .map(|invite| invite.summary.clone())
            .collect())
    }

    async fn get_invite_link(
        &self,
        _session_path: Option<PathBuf>,
        chat_id: &str,
        admin_user_id: i64,
        invite_handle: &str,
    ) -> Result<Option<InviteLinkSummary>, TelegramError> {
        let state = self.state.lock().await;
        let canonical_id = Self::resolve_dialog(&state.dialogs, chat_id)
            .ok_or_else(|| TelegramError::PeerNotFound(chat_id.to_string()))?;
        Ok(state
            .invite_links
            .get(&canonical_id)
            .into_iter()
            .flatten()
            .find(|invite| {
                invite.summary.invite_handle == invite_handle
                    && invite.summary.admin_user_id == Some(admin_user_id)
            })
            .map(|invite| invite.summary.clone()))
    }

    async fn export_invite_link(
        &self,
        _session_path: Option<PathBuf>,
        chat_id: &str,
        expire_date_unix: Option<i32>,
        usage_limit: Option<i32>,
        request_needed: bool,
        title: Option<&str>,
    ) -> Result<InviteLinkSummary, TelegramError> {
        let mut state = self.state.lock().await;
        let canonical_id = Self::resolve_dialog(&state.dialogs, chat_id)
            .ok_or_else(|| TelegramError::PeerNotFound(chat_id.to_string()))?;
        state.invite_export_calls = state.invite_export_calls.saturating_add(1);
        let raw_link =
            format!("fixture-generated-invite-{canonical_id}-{}", state.invite_export_calls);
        let summary = InviteLinkSummary {
            invite_handle: opaque_invite_handle(&raw_link),
            kind: InviteLinkKind::Exported,
            revoked: false,
            permanent: expire_date_unix.is_none() && usage_limit.is_none() && !request_needed,
            request_needed,
            admin_user_id: Some(state.account.user_id),
            date_unix: Some(1_700_000_100 + state.invite_export_calls as i32),
            start_date_unix: None,
            expire_date_unix,
            usage_limit,
            usage_count: Some(0),
            requested_count: Some(0),
            subscription_expired_count: None,
            title: title.map(str::to_string),
            subscription_pricing: None,
        };
        state
            .invite_links
            .entry(canonical_id)
            .or_default()
            .push(FakeInviteLink { _raw_link: raw_link, summary: summary.clone() });
        Ok(summary)
    }

    async fn revoke_invite_link(
        &self,
        _session_path: Option<PathBuf>,
        chat_id: &str,
        admin_user_id: i64,
        invite_handle: &str,
    ) -> Result<InviteLinkSummary, TelegramError> {
        let mut state = self.state.lock().await;
        let canonical_id = Self::resolve_dialog(&state.dialogs, chat_id)
            .ok_or_else(|| TelegramError::PeerNotFound(chat_id.to_string()))?;
        let invite = state
            .invite_links
            .get_mut(&canonical_id)
            .and_then(|invites| {
                invites.iter_mut().find(|invite| {
                    invite.summary.invite_handle == invite_handle
                        && invite.summary.admin_user_id == Some(admin_user_id)
                })
            })
            .ok_or_else(|| TelegramError::InvalidPeer("invite handle was not found".to_string()))?;
        if invite.summary.revoked {
            return Ok(invite.summary.clone());
        }
        invite.summary.revoked = true;
        let summary = invite.summary.clone();
        state.invite_revoke_calls = state.invite_revoke_calls.saturating_add(1);
        Ok(summary)
    }

    async fn delete_revoked_invite_link(
        &self,
        _session_path: Option<PathBuf>,
        chat_id: &str,
        admin_user_id: i64,
        invite_handle: &str,
    ) -> Result<(), TelegramError> {
        let mut state = self.state.lock().await;
        let canonical_id = Self::resolve_dialog(&state.dialogs, chat_id)
            .ok_or_else(|| TelegramError::PeerNotFound(chat_id.to_string()))?;
        let invites = state.invite_links.entry(canonical_id).or_default();
        let index = invites
            .iter()
            .position(|invite| {
                invite.summary.invite_handle == invite_handle
                    && invite.summary.admin_user_id == Some(admin_user_id)
            })
            .ok_or_else(|| TelegramError::InvalidPeer("invite handle was not found".to_string()))?;
        if !invites[index].summary.revoked {
            return Err(TelegramError::SendRejected(
                "an active invite must be revoked before deletion".to_string(),
            ));
        }
        invites.remove(index);
        state.invite_delete_calls = state.invite_delete_calls.saturating_add(1);
        Ok(())
    }

    async fn list_invite_joiners(
        &self,
        _session_path: Option<PathBuf>,
        chat_id: &str,
        admin_user_id: i64,
        invite_handle: Option<&str>,
        requested: bool,
        limit: usize,
        offset_date_unix: i32,
        offset_user_id: i64,
    ) -> Result<InviteJoinerPage, TelegramError> {
        let state = self.state.lock().await;
        let canonical_id = Self::resolve_dialog(&state.dialogs, chat_id)
            .ok_or_else(|| TelegramError::PeerNotFound(chat_id.to_string()))?;
        if let Some(invite_handle) = invite_handle
            && !state.invite_links.get(&canonical_id).into_iter().flatten().any(|invite| {
                invite.summary.invite_handle == invite_handle
                    && invite.summary.admin_user_id == Some(admin_user_id)
            })
        {
            return Err(TelegramError::InvalidPeer(
                "invite handle was not found for the selected administrator".to_string(),
            ));
        }
        let source = if requested { &state.join_requests } else { &state.invite_joiners };
        let mut matching = source
            .get(&canonical_id)
            .into_iter()
            .flatten()
            .filter(|entry| {
                invite_handle.is_none_or(|handle| entry.invite_handle.as_deref() == Some(handle))
            })
            .cloned()
            .collect::<Vec<_>>();
        matching.sort_by(|left, right| {
            right
                .summary
                .date_unix
                .cmp(&left.summary.date_unix)
                .then_with(|| right.summary.user_id.cmp(&left.summary.user_id))
        });
        let total_count = matching.len();
        let start = if offset_date_unix == 0 && offset_user_id == 0 {
            0
        } else {
            matching
                .iter()
                .position(|entry| {
                    entry.summary.date_unix == offset_date_unix
                        && entry.summary.user_id == offset_user_id
                })
                .map(|index| index + 1)
                .ok_or_else(|| {
                    TelegramError::InvalidPeer(
                        "the invite-joiner pagination cursor no longer exists".to_string(),
                    )
                })?
        };
        let mut entries = matching
            .into_iter()
            .skip(start)
            .take(limit)
            .map(|entry| {
                let mut summary = entry.summary;
                summary.source_invite_handle = invite_handle.map(str::to_string);
                summary
            })
            .collect::<Vec<_>>();
        let has_more = start.saturating_add(entries.len()) < total_count;
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
            entries: std::mem::take(&mut entries),
            next_offset_date_unix,
            next_offset_user_id,
            completeness: if has_more { Completeness::Page } else { Completeness::Complete },
        })
    }

    async fn process_join_request(
        &self,
        _session_path: Option<PathBuf>,
        chat_id: &str,
        user_id: i64,
        approve: bool,
    ) -> Result<(), TelegramError> {
        let mut state = self.state.lock().await;
        let canonical_id = Self::resolve_dialog(&state.dialogs, chat_id)
            .ok_or_else(|| TelegramError::PeerNotFound(chat_id.to_string()))?;
        let mut request = {
            let requests = state.join_requests.entry(canonical_id).or_default();
            let index = requests
                .iter()
                .position(|request| request.summary.user_id == user_id)
                .ok_or_else(|| {
                    TelegramError::InvalidPeer(format!(
                        "pending join request for user {user_id} was not found"
                    ))
                })?;
            requests.remove(index)
        };
        state.join_request_calls = state.join_request_calls.saturating_add(1);
        if approve {
            request.summary.requested = false;
            request.summary.approved_by_user_id = Some(state.account.user_id);
            state.invite_joiners.entry(canonical_id).or_default().push(request);
        }
        Ok(())
    }

    async fn get_account_profile(
        &self,
        _session_path: Option<PathBuf>,
    ) -> Result<AccountProfileSummary, TelegramError> {
        Ok(self.state.lock().await.account_profile.clone())
    }

    async fn get_bot_commands(
        &self,
        _session_path: Option<PathBuf>,
        language_code: &str,
    ) -> Result<Vec<BotCommandSummary>, TelegramError> {
        let state = self.state.lock().await;
        if !state.account_is_bot {
            return Err(TelegramError::InvalidPeer(
                "the authenticated Telegram account is not a bot".to_string(),
            ));
        }
        Ok(state.bot_commands.get(language_code).cloned().unwrap_or_default())
    }

    async fn set_bot_commands(
        &self,
        _session_path: Option<PathBuf>,
        language_code: &str,
        commands: &[BotCommandSummary],
    ) -> Result<(), TelegramError> {
        let mut state = self.state.lock().await;
        if !state.account_is_bot {
            return Err(TelegramError::InvalidPeer(
                "the authenticated Telegram account is not a bot".to_string(),
            ));
        }
        state.bot_command_set_calls = state.bot_command_set_calls.saturating_add(1);
        if commands.is_empty() {
            state.bot_commands.remove(language_code);
        } else {
            state.bot_commands.insert(language_code.to_string(), commands.to_vec());
        }
        if state.bot_command_set_uncertain_once {
            state.bot_command_set_uncertain_once = false;
            return Err(TelegramError::DeliveryUncertain(
                "fixture changed bot commands before the transport receipt was lost".to_string(),
            ));
        }
        Ok(())
    }

    async fn get_account_emoji_status(
        &self,
        _session_path: Option<PathBuf>,
    ) -> Result<EmojiStatusSummary, TelegramError> {
        Ok(self.state.lock().await.account_emoji_status.clone())
    }

    async fn list_emoji_statuses(
        &self,
        _session_path: Option<PathBuf>,
        kind: EmojiStatusCatalogKind,
        hash: i64,
    ) -> Result<EmojiStatusCatalogPage, TelegramError> {
        let state = self.state.lock().await;
        let (response_hash, statuses) =
            state.emoji_status_catalogs.get(&kind).cloned().unwrap_or_else(|| (0, Vec::new()));
        if hash != 0 && hash == response_hash {
            return Ok(EmojiStatusCatalogPage {
                kind,
                request_hash: hash.to_string(),
                response_hash: None,
                not_modified: true,
                available_count: None,
                returned_count: 0,
                truncated: false,
                statuses: Vec::new(),
            });
        }
        let count = statuses.len();
        Ok(EmojiStatusCatalogPage {
            kind,
            request_hash: hash.to_string(),
            response_hash: Some(response_hash.to_string()),
            not_modified: false,
            available_count: Some(count),
            returned_count: count,
            truncated: false,
            statuses,
        })
    }

    async fn set_account_emoji_status(
        &self,
        _session_path: Option<PathBuf>,
        status: &AccountEmojiStatusSpec,
    ) -> Result<(), TelegramError> {
        let mut state = self.state.lock().await;
        state.emoji_status_set_calls = state.emoji_status_set_calls.saturating_add(1);
        let mut summary = match status {
            AccountEmojiStatusSpec::Empty => empty_emoji_status(),
            AccountEmojiStatusSpec::Default { document_id, until_unix } => {
                let document_id = document_id.parse::<i64>().map_err(|_| {
                    TelegramError::InvalidPeer("invalid fixture emoji document id".to_string())
                })?;
                let mut summary = fixture_default_emoji_status(document_id);
                summary.until_unix = *until_unix;
                summary
            },
            AccountEmojiStatusSpec::Collectible { collectible_id, until_unix } => {
                let collectible_id = collectible_id.parse::<i64>().map_err(|_| {
                    TelegramError::InvalidPeer("invalid fixture collectible id".to_string())
                })?;
                let mut summary = state
                    .emoji_status_catalogs
                    .values()
                    .flat_map(|(_, statuses)| statuses.iter())
                    .find(|status| {
                        status.collectible_id.as_deref()
                            == Some(collectible_id.to_string().as_str())
                    })
                    .cloned()
                    .unwrap_or_else(|| {
                        fixture_collectible_emoji_status(
                            collectible_id,
                            collectible_id.saturating_add(1_000),
                        )
                    });
                summary.until_unix = *until_unix;
                summary
            },
        };
        if matches!(status, AccountEmojiStatusSpec::Empty) {
            summary.until_unix = None;
        }
        state.account_emoji_status = summary;
        if state.emoji_status_set_uncertain_once {
            state.emoji_status_set_uncertain_once = false;
            return Err(TelegramError::DeliveryUncertain(
                "fixture changed the emoji status before the transport receipt was lost"
                    .to_string(),
            ));
        }
        Ok(())
    }

    async fn clear_recent_emoji_statuses(
        &self,
        _session_path: Option<PathBuf>,
    ) -> Result<(), TelegramError> {
        let mut state = self.state.lock().await;
        state.emoji_recent_clear_calls = state.emoji_recent_clear_calls.saturating_add(1);
        let entry = state
            .emoji_status_catalogs
            .entry(EmojiStatusCatalogKind::Recent)
            .or_insert_with(|| (0, Vec::new()));
        entry.0 = entry.0.wrapping_add(1);
        entry.1.clear();
        if state.emoji_recent_clear_uncertain_once {
            state.emoji_recent_clear_uncertain_once = false;
            return Err(TelegramError::DeliveryUncertain(
                "fixture cleared recent emoji statuses before the transport receipt was lost"
                    .to_string(),
            ));
        }
        Ok(())
    }

    async fn update_account_profile(
        &self,
        _session_path: Option<PathBuf>,
        first_name: Option<&str>,
        last_name: Option<&str>,
        about: Option<&str>,
    ) -> Result<(), TelegramError> {
        let mut state = self.state.lock().await;
        if let Some(first_name) = first_name {
            state.account.first_name = first_name.to_string();
            state.account_profile.first_name = first_name.to_string();
        }
        if let Some(last_name) = last_name {
            state.account_profile.last_name =
                (!last_name.is_empty()).then(|| last_name.to_string());
        }
        if let Some(about) = about {
            state.account_profile.about = (!about.is_empty()).then(|| about.to_string());
        }
        state.profile_update_calls = state.profile_update_calls.saturating_add(1);
        Ok(())
    }

    async fn update_account_username(
        &self,
        _session_path: Option<PathBuf>,
        username: Option<&str>,
    ) -> Result<(), TelegramError> {
        let mut state = self.state.lock().await;
        let username = username.filter(|username| !username.is_empty()).map(str::to_string);
        state.account.username = username.clone();
        state.account_profile.username = username;
        state.username_update_calls = state.username_update_calls.saturating_add(1);
        Ok(())
    }

    async fn update_account_birthday(
        &self,
        _session_path: Option<PathBuf>,
        birthday: Option<&BirthdaySummary>,
    ) -> Result<(), TelegramError> {
        let mut state = self.state.lock().await;
        state.account_profile.birthday = birthday.cloned();
        state.birthday_update_calls = state.birthday_update_calls.saturating_add(1);
        Ok(())
    }

    async fn update_account_color(
        &self,
        _session_path: Option<PathBuf>,
        for_profile: bool,
        color: Option<i32>,
        background_emoji_id: Option<&str>,
    ) -> Result<(), TelegramError> {
        let background_emoji_id = background_emoji_id
            .map(|value| {
                value.parse::<i64>().ok().filter(|value| *value > 0).ok_or_else(|| {
                    TelegramError::InvalidPeer(
                        "background_emoji_id must be a positive Telegram i64 id".to_string(),
                    )
                })
            })
            .transpose()?;
        let target = if color.is_none() && background_emoji_id.is_none() {
            None
        } else {
            Some(PeerColorSummary::Simple {
                color,
                background_emoji_id: background_emoji_id.map(|value| value.to_string()),
            })
        };
        let mut state = self.state.lock().await;
        state.account_color_update_calls = state.account_color_update_calls.saturating_add(1);
        if for_profile {
            state.account_profile.profile_color = target;
        } else {
            state.account_profile.name_color = target;
        }
        if std::mem::take(&mut state.account_color_update_uncertain_once) {
            return Err(TelegramError::DeliveryUncertain(
                "fixture changed the account peer color before the transport receipt was lost"
                    .to_string(),
            ));
        }
        Ok(())
    }

    async fn update_personal_channel(
        &self,
        _session_path: Option<PathBuf>,
        channel_id: Option<&str>,
    ) -> Result<(), TelegramError> {
        let mut state = self.state.lock().await;
        let target = if let Some(channel_id) = channel_id {
            let canonical_id = Self::resolve_known_chat(&state, channel_id)
                .ok_or_else(|| TelegramError::PeerNotFound(channel_id.to_string()))?;
            let details = state
                .known_chats
                .get(&canonical_id)
                .cloned()
                .ok_or_else(|| TelegramError::PeerNotFound(channel_id.to_string()))?;
            if !matches!(details.chat.kind.as_str(), "channel" | "supergroup" | "gigagroup") {
                return Err(TelegramError::InvalidPeer(
                    "personal channel requires a Telegram channel or supergroup".to_string(),
                ));
            }
            let message_id = state
                .dialogs
                .iter()
                .find(|dialog| dialog.chat_id == canonical_id)
                .and_then(|dialog| dialog.last_message_id)
                .or_else(|| {
                    state.messages.get(&canonical_id).and_then(|messages| {
                        messages.iter().map(|message| message.message_id).max()
                    })
                })
                .unwrap_or_default();
            Some(PersonalChannelSummary { chat: details.chat, message_id })
        } else {
            None
        };
        state.personal_channel_update_calls = state.personal_channel_update_calls.saturating_add(1);
        state.account_profile.personal_channel = target;
        if std::mem::take(&mut state.personal_channel_update_uncertain_once) {
            return Err(TelegramError::DeliveryUncertain(
                "fixture changed the personal channel before the transport receipt was lost"
                    .to_string(),
            ));
        }
        Ok(())
    }

    async fn set_account_presence(
        &self,
        _session_path: Option<PathBuf>,
        _online: bool,
    ) -> Result<(), TelegramError> {
        let mut state = self.state.lock().await;
        state.presence_update_calls = state.presence_update_calls.saturating_add(1);
        Ok(())
    }

    async fn list_profile_photos(
        &self,
        _session_path: Option<PathBuf>,
        offset: usize,
        limit: usize,
    ) -> Result<ProfilePhotoPage, TelegramError> {
        let state = self.state.lock().await;
        let total_count = state.profile_photos.len();
        let photos =
            state.profile_photos.iter().skip(offset).take(limit).cloned().collect::<Vec<_>>();
        let next_offset = (offset.saturating_add(photos.len()) < total_count)
            .then_some(offset.saturating_add(photos.len()));
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

    async fn get_profile_photo(
        &self,
        _session_path: Option<PathBuf>,
        photo_handle: &str,
    ) -> Result<Option<ProfilePhotoSummary>, TelegramError> {
        Ok(self
            .state
            .lock()
            .await
            .profile_photos
            .iter()
            .find(|photo| photo.photo_handle == photo_handle)
            .cloned())
    }

    async fn upload_profile_photo(
        &self,
        _session_path: Option<PathBuf>,
        _file_path: &std::path::Path,
        _fallback: bool,
    ) -> Result<ProfilePhotoSummary, TelegramError> {
        let mut state = self.state.lock().await;
        let photo_id = state.next_profile_photo_id;
        state.next_profile_photo_id = state.next_profile_photo_id.saturating_add(1);
        let photo = ProfilePhotoSummary {
            photo_handle: opaque_profile_photo_handle(photo_id),
            date_unix: 1_800_000_000_i32.saturating_add(
                i32::try_from(state.profile_photo_upload_calls).unwrap_or(i32::MAX),
            ),
            has_stickers: false,
            has_video: false,
        };
        state.profile_photos.insert(0, photo.clone());
        state.profile_photo_upload_calls = state.profile_photo_upload_calls.saturating_add(1);
        Ok(photo)
    }

    async fn delete_profile_photos(
        &self,
        _session_path: Option<PathBuf>,
        photo_handles: &[String],
    ) -> Result<(), TelegramError> {
        let requested = photo_handles.iter().map(String::as_str).collect::<HashSet<_>>();
        let mut state = self.state.lock().await;
        state.profile_photos.retain(|photo| !requested.contains(photo.photo_handle.as_str()));
        state.profile_photo_delete_calls = state.profile_photo_delete_calls.saturating_add(1);
        Ok(())
    }

    async fn get_sessions(
        &self,
        _session_path: Option<PathBuf>,
    ) -> Result<Vec<SessionSummary>, TelegramError> {
        Ok(self.state.lock().await.sessions.clone())
    }

    async fn terminate_session(
        &self,
        _session_path: Option<PathBuf>,
        authorization_id: i64,
    ) -> Result<(), TelegramError> {
        let mut state = self.state.lock().await;
        let session = state
            .sessions
            .iter()
            .find(|session| session.authorization_id == authorization_id)
            .cloned()
            .ok_or_else(|| {
                TelegramError::InvalidPeer(format!(
                    "authorization {authorization_id} was not found"
                ))
            })?;
        if session.current {
            return Err(TelegramError::InvalidPeer(
                "the current MCP authorization cannot terminate itself".to_string(),
            ));
        }
        state.session_terminate_calls = state.session_terminate_calls.saturating_add(1);
        state.sessions.retain(|candidate| candidate.authorization_id != authorization_id);
        Ok(())
    }

    async fn terminate_all_other_sessions(
        &self,
        _session_path: Option<PathBuf>,
    ) -> Result<(), TelegramError> {
        let mut state = self.state.lock().await;
        state.session_reset_calls = state.session_reset_calls.saturating_add(1);
        state.sessions.retain(|session| session.current);
        if state.session_reset_uncertain_once {
            state.session_reset_uncertain_once = false;
            return Err(TelegramError::DeliveryUncertain(
                "fixture terminated all other sessions before the transport receipt was lost"
                    .to_string(),
            ));
        }
        Ok(())
    }

    async fn get_chat_auto_delete(
        &self,
        _session_path: Option<PathBuf>,
        chat_id: &str,
    ) -> Result<ChatAutoDeleteSummary, TelegramError> {
        let state = self.state.lock().await;
        let resolved = Self::resolve_dialog(&state.dialogs, chat_id)
            .ok_or_else(|| TelegramError::PeerNotFound(chat_id.to_string()))?;
        Ok(ChatAutoDeleteSummary {
            chat_id: Self::canonical_chat_ref(&state.dialogs, resolved),
            period_seconds: state.chat_auto_delete.get(&resolved).copied().unwrap_or_default(),
        })
    }

    async fn set_chat_auto_delete(
        &self,
        _session_path: Option<PathBuf>,
        chat_id: &str,
        period_seconds: i32,
    ) -> Result<(), TelegramError> {
        if period_seconds < 0 {
            return Err(TelegramError::InvalidPeer(
                "auto-delete period must be non-negative".to_string(),
            ));
        }
        let mut state = self.state.lock().await;
        let resolved = Self::resolve_dialog(&state.dialogs, chat_id)
            .ok_or_else(|| TelegramError::PeerNotFound(chat_id.to_string()))?;
        state.chat_auto_delete_calls = state.chat_auto_delete_calls.saturating_add(1);
        state.chat_auto_delete.insert(resolved, period_seconds);
        if state.chat_auto_delete_uncertain_once {
            state.chat_auto_delete_uncertain_once = false;
            return Err(TelegramError::DeliveryUncertain(
                "fixture changed chat auto-delete before the transport receipt was lost"
                    .to_string(),
            ));
        }
        Ok(())
    }

    async fn set_contact(
        &self,
        _session_path: Option<PathBuf>,
        user_id: i64,
        contact: bool,
        first_name: &str,
        last_name: &str,
        phone: &str,
        _add_phone_privacy_exception: bool,
    ) -> Result<(), TelegramError> {
        let mut state = self.state.lock().await;
        state.contact_calls = state.contact_calls.saturating_add(1);
        let user = state
            .contacts
            .iter_mut()
            .find(|user| user.user_id == user_id)
            .ok_or_else(|| TelegramError::PeerNotFound(user_id.to_string()))?;
        user.is_contact = contact;
        if contact {
            user.first_name = first_name.to_string();
            user.last_name = (!last_name.is_empty()).then(|| last_name.to_string());
            user.phone = (!phone.is_empty()).then(|| phone.to_string());
            user.display_name =
                format!("{} {}", user.first_name, user.last_name.as_deref().unwrap_or_default())
                    .trim()
                    .to_string();
        }
        Ok(())
    }

    async fn get_common_chats(
        &self,
        _session_path: Option<PathBuf>,
        user_ref: &str,
        max_id: i64,
        limit: usize,
    ) -> Result<Vec<ChatSummary>, TelegramError> {
        let state = self.state.lock().await;
        let wanted = user_ref.trim().trim_start_matches('@');
        if !state.contacts.iter().any(|contact| {
            contact.user_id.to_string() == wanted
                || contact
                    .username
                    .as_deref()
                    .is_some_and(|username| username.eq_ignore_ascii_case(wanted))
        }) {
            return Err(TelegramError::PeerNotFound(user_ref.to_string()));
        }
        Ok(state
            .dialogs
            .iter()
            .filter(|dialog| dialog.chat_kind != "user")
            .filter(|dialog| max_id <= 0 || dialog.chat_id < max_id)
            .take(limit)
            .map(|dialog| ChatSummary {
                chat_id: dialog.canonical_chat_id.clone(),
                title: dialog.title.clone(),
                kind: dialog.chat_kind.clone(),
                username: dialog.username.clone(),
                participants_count: Some(3),
                creator: true,
                verified: false,
                restricted: false,
            })
            .collect())
    }

    async fn get_chat_lifecycle_state(
        &self,
        _session_path: Option<PathBuf>,
        chat_id: &str,
    ) -> Result<ChatLifecycleState, TelegramError> {
        let state = self.state.lock().await;
        let canonical_id = Self::resolve_known_chat(&state, chat_id)
            .ok_or_else(|| TelegramError::PeerNotFound(chat_id.to_string()))?;
        let details = state
            .known_chats
            .get(&canonical_id)
            .cloned()
            .ok_or_else(|| TelegramError::PeerNotFound(chat_id.to_string()))?;
        Ok(ChatLifecycleState {
            details,
            in_dialogs: state.dialogs.iter().any(|dialog| dialog.chat_id == canonical_id),
        })
    }

    async fn create_group(
        &self,
        _session_path: Option<PathBuf>,
        title: &str,
        user_ids: &[i64],
    ) -> Result<ChatDetailsSummary, TelegramError> {
        let mut state = self.state.lock().await;
        if user_ids
            .iter()
            .any(|user_id| !state.contacts.iter().any(|contact| contact.user_id == *user_id))
        {
            return Err(TelegramError::PeerNotFound(
                "one or more group members are unknown".to_string(),
            ));
        }
        let chat_id = state.next_chat_id;
        state.next_chat_id = state.next_chat_id.saturating_add(1);
        let details = ChatDetailsSummary {
            chat: ChatSummary {
                chat_id: format!("-{chat_id}"),
                title: title.to_string(),
                kind: "group".to_string(),
                username: None,
                participants_count: Some(
                    i32::try_from(user_ids.len().saturating_add(1)).unwrap_or(i32::MAX),
                ),
                creator: true,
                verified: false,
                restricted: false,
            },
            about: None,
            photo_handle: None,
        };
        state.dialogs.push(Self::dialog_from_chat(&details)?);
        state.known_chats.insert(chat_id, details.clone());
        state.chat_moderation_settings.insert(chat_id, FakeChatModerationState::default());
        state.dialog_read_inbox_max_ids.insert(chat_id, 0);
        state.dialog_unread_reactions.insert(chat_id, 0);
        state.chat_create_calls = state.chat_create_calls.saturating_add(1);
        Ok(details)
    }

    async fn create_channel(
        &self,
        _session_path: Option<PathBuf>,
        title: &str,
        about: &str,
        broadcast: bool,
    ) -> Result<ChatDetailsSummary, TelegramError> {
        let mut state = self.state.lock().await;
        let chat_id = state.next_chat_id;
        state.next_chat_id = state.next_chat_id.saturating_add(1);
        let details = ChatDetailsSummary {
            chat: ChatSummary {
                chat_id: format!("-100{chat_id}"),
                title: title.to_string(),
                kind: if broadcast { "channel" } else { "supergroup" }.to_string(),
                username: None,
                participants_count: Some(1),
                creator: true,
                verified: false,
                restricted: false,
            },
            about: (!about.is_empty()).then(|| about.to_string()),
            photo_handle: None,
        };
        state.dialogs.push(Self::dialog_from_chat(&details)?);
        state.known_chats.insert(chat_id, details.clone());
        state.channel_settings.insert(chat_id, FakeChannelSettingsState::default());
        state.chat_moderation_settings.insert(
            chat_id,
            FakeChatModerationState {
                anti_spam_enabled: (!broadcast).then_some(false),
                forum_enabled: (!broadcast).then_some(false),
                forum_tabs_enabled: (!broadcast).then_some(false),
                ..FakeChatModerationState::default()
            },
        );
        state.dialog_read_inbox_max_ids.insert(chat_id, 0);
        state.dialog_unread_reactions.insert(chat_id, 0);
        state.chat_create_calls = state.chat_create_calls.saturating_add(1);
        Ok(details)
    }

    async fn join_chat(
        &self,
        _session_path: Option<PathBuf>,
        chat_id: &str,
    ) -> Result<ChatLifecycleState, TelegramError> {
        let mut state = self.state.lock().await;
        let canonical_id = Self::resolve_known_chat(&state, chat_id)
            .ok_or_else(|| TelegramError::PeerNotFound(chat_id.to_string()))?;
        let details = state
            .known_chats
            .get(&canonical_id)
            .cloned()
            .ok_or_else(|| TelegramError::PeerNotFound(chat_id.to_string()))?;
        if !state.dialogs.iter().any(|dialog| dialog.chat_id == canonical_id) {
            state.dialogs.push(Self::dialog_from_chat(&details)?);
            state.chat_join_calls = state.chat_join_calls.saturating_add(1);
        }
        Ok(ChatLifecycleState { details, in_dialogs: true })
    }

    async fn leave_chat(
        &self,
        _session_path: Option<PathBuf>,
        chat_id: &str,
    ) -> Result<ChatLifecycleState, TelegramError> {
        let mut state = self.state.lock().await;
        let canonical_id = Self::resolve_known_chat(&state, chat_id)
            .ok_or_else(|| TelegramError::PeerNotFound(chat_id.to_string()))?;
        let details = state
            .known_chats
            .get(&canonical_id)
            .cloned()
            .ok_or_else(|| TelegramError::PeerNotFound(chat_id.to_string()))?;
        state.dialogs.retain(|dialog| dialog.chat_id != canonical_id);
        state.chat_leave_calls = state.chat_leave_calls.saturating_add(1);
        Ok(ChatLifecycleState { details, in_dialogs: false })
    }

    async fn delete_chat(
        &self,
        _session_path: Option<PathBuf>,
        chat_id: &str,
    ) -> Result<ChatLifecycleState, TelegramError> {
        let mut state = self.state.lock().await;
        let canonical_id = Self::resolve_known_chat(&state, chat_id)
            .ok_or_else(|| TelegramError::PeerNotFound(chat_id.to_string()))?;
        let details = state
            .known_chats
            .remove(&canonical_id)
            .ok_or_else(|| TelegramError::PeerNotFound(chat_id.to_string()))?;
        state.channel_settings.remove(&canonical_id);
        state.chat_moderation_settings.remove(&canonical_id);
        state.dialogs.retain(|dialog| dialog.chat_id != canonical_id);
        state.chat_delete_calls = state.chat_delete_calls.saturating_add(1);
        Ok(ChatLifecycleState { details, in_dialogs: false })
    }

    async fn update_chat_title(
        &self,
        _session_path: Option<PathBuf>,
        chat_id: &str,
        title: &str,
    ) -> Result<ChatDetailsSummary, TelegramError> {
        let mut state = self.state.lock().await;
        let canonical_id = Self::resolve_known_chat(&state, chat_id)
            .ok_or_else(|| TelegramError::PeerNotFound(chat_id.to_string()))?;
        let details = state
            .known_chats
            .get_mut(&canonical_id)
            .ok_or_else(|| TelegramError::PeerNotFound(chat_id.to_string()))?;
        details.chat.title = title.to_string();
        let updated = details.clone();
        if let Some(dialog) = state.dialogs.iter_mut().find(|dialog| dialog.chat_id == canonical_id)
        {
            dialog.title = title.to_string();
        }
        state.chat_title_calls = state.chat_title_calls.saturating_add(1);
        Ok(updated)
    }

    async fn update_chat_about(
        &self,
        _session_path: Option<PathBuf>,
        chat_id: &str,
        about: &str,
    ) -> Result<ChatDetailsSummary, TelegramError> {
        let mut state = self.state.lock().await;
        let canonical_id = Self::resolve_known_chat(&state, chat_id)
            .ok_or_else(|| TelegramError::PeerNotFound(chat_id.to_string()))?;
        let details = state
            .known_chats
            .get_mut(&canonical_id)
            .ok_or_else(|| TelegramError::PeerNotFound(chat_id.to_string()))?;
        details.about = (!about.is_empty()).then(|| about.to_string());
        let updated = details.clone();
        state.chat_about_calls = state.chat_about_calls.saturating_add(1);
        Ok(updated)
    }

    async fn update_chat_photo(
        &self,
        _session_path: Option<PathBuf>,
        chat_id: &str,
        _file_path: &std::path::Path,
    ) -> Result<ChatDetailsSummary, TelegramError> {
        let mut state = self.state.lock().await;
        let canonical_id = Self::resolve_known_chat(&state, chat_id)
            .ok_or_else(|| TelegramError::PeerNotFound(chat_id.to_string()))?;
        let photo_id = state.next_chat_photo_id;
        state.next_chat_photo_id = state.next_chat_photo_id.saturating_add(1);
        let details = state
            .known_chats
            .get_mut(&canonical_id)
            .ok_or_else(|| TelegramError::PeerNotFound(chat_id.to_string()))?;
        details.photo_handle = Some(opaque_chat_photo_handle(&details.chat.chat_id, photo_id));
        let updated = details.clone();
        state.chat_photo_update_calls = state.chat_photo_update_calls.saturating_add(1);
        Ok(updated)
    }

    async fn delete_chat_photo(
        &self,
        _session_path: Option<PathBuf>,
        chat_id: &str,
    ) -> Result<ChatDetailsSummary, TelegramError> {
        let mut state = self.state.lock().await;
        let canonical_id = Self::resolve_known_chat(&state, chat_id)
            .ok_or_else(|| TelegramError::PeerNotFound(chat_id.to_string()))?;
        let details = state
            .known_chats
            .get_mut(&canonical_id)
            .ok_or_else(|| TelegramError::PeerNotFound(chat_id.to_string()))?;
        details.photo_handle = None;
        let updated = details.clone();
        state.chat_photo_delete_calls = state.chat_photo_delete_calls.saturating_add(1);
        Ok(updated)
    }

    async fn get_channel_settings(
        &self,
        _session_path: Option<PathBuf>,
        chat_id: &str,
    ) -> Result<ChannelSettingsSummary, TelegramError> {
        let state = self.state.lock().await;
        let canonical_id = Self::resolve_known_chat(&state, chat_id)
            .ok_or_else(|| TelegramError::PeerNotFound(chat_id.to_string()))?;
        Self::channel_settings_snapshot(&state, canonical_id)
    }

    async fn get_chat_moderation_settings(
        &self,
        _session_path: Option<PathBuf>,
        chat_id: &str,
    ) -> Result<ChatModerationSettingsSummary, TelegramError> {
        let state = self.state.lock().await;
        let canonical_id = Self::resolve_known_chat(&state, chat_id)
            .ok_or_else(|| TelegramError::PeerNotFound(chat_id.to_string()))?;
        Self::chat_moderation_settings_snapshot(&state, canonical_id)
    }

    async fn get_channel_stats(
        &self,
        _session_path: Option<PathBuf>,
        chat_id: &str,
    ) -> Result<ChannelStatsSummary, TelegramError> {
        let state = self.state.lock().await;
        let canonical_id = Self::resolve_known_chat(&state, chat_id)
            .ok_or_else(|| TelegramError::PeerNotFound(chat_id.to_string()))?;
        let details = state
            .known_chats
            .get(&canonical_id)
            .ok_or_else(|| TelegramError::PeerNotFound(chat_id.to_string()))?;
        let period =
            StatsDateRangeSummary { min_date_unix: 1_699_395_200, max_date_unix: 1_700_000_000 };
        let graph = |name: &str| StatsGraphSummary {
            name: name.to_string(),
            state: "loaded".to_string(),
            data: Some(serde_json::json!({"columns": [["x", 1, 2], ["y0", 3, 5]]})),
            token: None,
            error: None,
        };
        if details.chat.kind == "channel" {
            Ok(ChannelStatsSummary::Broadcast {
                chat_id: details.chat.chat_id.clone(),
                period,
                followers: StatsValueSummary { current: 1200.0, previous: 1100.0 },
                views_per_post: StatsValueSummary { current: 840.0, previous: 790.0 },
                shares_per_post: StatsValueSummary { current: 18.0, previous: 15.0 },
                reactions_per_post: StatsValueSummary { current: 72.0, previous: 66.0 },
                views_per_story: StatsValueSummary { current: 430.0, previous: 410.0 },
                shares_per_story: StatsValueSummary { current: 9.0, previous: 8.0 },
                reactions_per_story: StatsValueSummary { current: 31.0, previous: 27.0 },
                enabled_notifications: StatsPercentSummary {
                    part: 840.0,
                    total: 1200.0,
                    percentage: Some(70.0),
                },
                graphs: vec![graph("growth"), graph("followers"), graph("interactions")],
                recent_interactions: vec![StatsPostInteractionSummary::Message {
                    message_id: 50,
                    views: 900,
                    forwards: 24,
                    reactions: 81,
                }],
            })
        } else {
            Ok(ChannelStatsSummary::Megagroup {
                chat_id: details.chat.chat_id.clone(),
                period,
                members: StatsValueSummary { current: 320.0, previous: 300.0 },
                messages: StatsValueSummary { current: 1450.0, previous: 1320.0 },
                viewers: StatsValueSummary { current: 280.0, previous: 265.0 },
                posters: StatsValueSummary { current: 96.0, previous: 90.0 },
                graphs: vec![graph("growth"), graph("messages"), graph("weekdays")],
                top_posters: vec![StatsTopPosterSummary {
                    user_id: 101,
                    messages: 84,
                    average_characters: 67,
                }],
                top_admins: vec![StatsTopAdminSummary {
                    user_id: 7,
                    deleted_messages: 4,
                    kicked_users: 1,
                    banned_users: 0,
                }],
                top_inviters: vec![StatsTopInviterSummary { user_id: 101, invitations: 12 }],
            })
        }
    }

    async fn get_my_boosts(
        &self,
        _session_path: Option<PathBuf>,
    ) -> Result<MyBoostsSummary, TelegramError> {
        let slots = self.state.lock().await.my_boosts.clone();
        Ok(MyBoostsSummary { returned_count: slots.len(), slots })
    }

    #[allow(clippy::too_many_arguments)]
    async fn get_admin_log(
        &self,
        _session_path: Option<PathBuf>,
        chat_id: &str,
        query: &str,
        events: Option<AdminLogEventFilter>,
        admin_refs: &[String],
        max_id: i64,
        min_id: i64,
        limit: usize,
    ) -> Result<AdminLogPage, TelegramError> {
        let state = self.state.lock().await;
        let canonical_id = Self::resolve_known_chat(&state, chat_id)
            .ok_or_else(|| TelegramError::PeerNotFound(chat_id.to_string()))?;
        let details = state
            .known_chats
            .get(&canonical_id)
            .ok_or_else(|| TelegramError::PeerNotFound(chat_id.to_string()))?;
        if !matches!(details.chat.kind.as_str(), "channel" | "supergroup" | "gigagroup") {
            return Err(TelegramError::InvalidPeer(
                "get_admin_log requires a channel or supergroup".to_string(),
            ));
        }
        let account = fixture_account_contact();
        let contact_by_id = |user_id| {
            (user_id == account.user_id)
                .then(|| account.clone())
                .or_else(|| state.contacts.iter().find(|user| user.user_id == user_id).cloned())
        };
        let mut admin_ids = HashSet::with_capacity(admin_refs.len());
        for admin_ref in admin_refs {
            let wanted = admin_ref.trim().trim_start_matches('@');
            let user = wanted
                .parse::<i64>()
                .ok()
                .and_then(contact_by_id)
                .or_else(|| {
                    std::iter::once(&account)
                        .chain(state.contacts.iter())
                        .find(|user| {
                            user.username
                                .as_deref()
                                .is_some_and(|username| username.eq_ignore_ascii_case(wanted))
                        })
                        .cloned()
                })
                .ok_or_else(|| TelegramError::PeerNotFound(admin_ref.clone()))?;
            admin_ids.insert(user.user_id);
        }
        let mut log = vec![
            AdminLogEventSummary {
                event_id: 9_004,
                date_unix: 1_700_000_400,
                actor_user_id: 7,
                actor: contact_by_id(7),
                action: AdminLogActionKind::ChangeTitle,
                details: Some("\"Release room\" → \"Release operations\"".to_string()),
                target_user_id: None,
                target_user: None,
                message_id: None,
                topic_id: None,
            },
            AdminLogEventSummary {
                event_id: 9_003,
                date_unix: 1_700_000_300,
                actor_user_id: 303,
                actor: contact_by_id(303),
                action: AdminLogActionKind::ParticipantToggleBan,
                details: Some("banned user Alex Morgan (@alex)".to_string()),
                target_user_id: Some(101),
                target_user: contact_by_id(101),
                message_id: None,
                topic_id: None,
            },
            AdminLogEventSummary {
                event_id: 9_002,
                date_unix: 1_700_000_200,
                actor_user_id: 303,
                actor: contact_by_id(303),
                action: AdminLogActionKind::EditMessage,
                details: Some("message #21".to_string()),
                target_user_id: None,
                target_user: None,
                message_id: Some(21),
                topic_id: None,
            },
            AdminLogEventSummary {
                event_id: 9_001,
                date_unix: 1_700_000_100,
                actor_user_id: 7,
                actor: contact_by_id(7),
                action: AdminLogActionKind::CreateTopic,
                details: Some("topic \"Incidents\"".to_string()),
                target_user_id: None,
                target_user: None,
                message_id: None,
                topic_id: Some(44),
            },
        ];
        if max_id > 0 {
            log.retain(|event| event.event_id < max_id);
        }
        if min_id > 0 {
            log.retain(|event| event.event_id > min_id);
        }
        if !admin_ids.is_empty() {
            log.retain(|event| admin_ids.contains(&event.actor_user_id));
        }
        if let Some(filter) = events {
            log.retain(|event| match event.action {
                AdminLogActionKind::ChangeTitle => filter.info,
                AdminLogActionKind::ParticipantToggleBan => filter.ban,
                AdminLogActionKind::EditMessage => filter.edit,
                AdminLogActionKind::CreateTopic => filter.forums,
                _ => false,
            });
        }
        let query = query.trim().to_lowercase();
        if !query.is_empty() {
            log.retain(|event| {
                format!("{:?} {}", event.action, event.details.as_deref().unwrap_or_default())
                    .to_lowercase()
                    .contains(&query)
            });
        }
        let events = log.into_iter().take(limit).collect::<Vec<_>>();
        let next_max_id =
            (events.len() == limit).then(|| events.last().map(|event| event.event_id)).flatten();
        let completeness =
            if next_max_id.is_some() { Completeness::Page } else { Completeness::Complete };
        Ok(AdminLogPage {
            chat_id: details.chat.chat_id.clone(),
            returned_count: events.len(),
            events,
            next_max_id,
            completeness,
        })
    }

    async fn get_boost_status(
        &self,
        _session_path: Option<PathBuf>,
        chat_id: &str,
    ) -> Result<BoostStatusSummary, TelegramError> {
        let state = self.state.lock().await;
        let canonical_id = Self::resolve_known_chat(&state, chat_id)
            .ok_or_else(|| TelegramError::PeerNotFound(chat_id.to_string()))?;
        let details = state
            .known_chats
            .get(&canonical_id)
            .ok_or_else(|| TelegramError::PeerNotFound(chat_id.to_string()))?;
        let my_boost_slots = state
            .my_boosts
            .iter()
            .filter(|slot| slot.chat_id.as_deref() == Some(details.chat.chat_id.as_str()))
            .map(|slot| slot.slot)
            .collect::<Vec<_>>();
        Ok(BoostStatusSummary {
            chat_id: details.chat.chat_id.clone(),
            level: 2,
            current_level_boosts: 5,
            boosts: 8,
            gift_boosts: Some(2),
            next_level_boosts: Some(10),
            premium_audience: Some(StatsPercentSummary {
                part: 180.0,
                total: 320.0,
                percentage: Some(56.25),
            }),
            boost_url: "https://t.me/boost/fixture-ops".to_string(),
            prepaid_giveaway_count: 1,
            my_boost: !my_boost_slots.is_empty(),
            my_boost_slots,
        })
    }

    async fn list_boosts(
        &self,
        _session_path: Option<PathBuf>,
        chat_id: &str,
        gifts_only: bool,
        offset: &str,
        limit: usize,
    ) -> Result<BoostPage, TelegramError> {
        let state = self.state.lock().await;
        Self::resolve_known_chat(&state, chat_id)
            .ok_or_else(|| TelegramError::PeerNotFound(chat_id.to_string()))?;
        let mut boosts = vec![
            BoostSummary {
                boost_id: "boost-fixture-1".to_string(),
                user_id: Some(101),
                applied_at_unix: 1_699_900_000,
                expires_at_unix: 1_702_500_000,
                gift: false,
                giveaway: false,
                unclaimed: false,
                giveaway_message_id: None,
                used_gift_slug: None,
                multiplier: Some(1),
                stars: None,
            },
            BoostSummary {
                boost_id: "boost-fixture-gift".to_string(),
                user_id: None,
                applied_at_unix: 1_699_950_000,
                expires_at_unix: 1_702_550_000,
                gift: true,
                giveaway: true,
                unclaimed: false,
                giveaway_message_id: Some(50),
                used_gift_slug: Some("fixture-gift".to_string()),
                multiplier: Some(2),
                stars: Some(500),
            },
        ];
        if gifts_only {
            boosts.retain(|boost| boost.gift);
        }
        let total_count = i32::try_from(boosts.len()).unwrap_or(i32::MAX);
        let start = if offset.is_empty() {
            0
        } else {
            offset.parse::<usize>().map_err(|_| {
                TelegramError::Telegram(
                    "fixture boost offset must be a non-negative integer".into(),
                )
            })?
        };
        let page = boosts.into_iter().skip(start).take(limit).collect::<Vec<_>>();
        let next_offset = (start.saturating_add(page.len())
            < usize::try_from(total_count).unwrap_or(0))
        .then(|| start.saturating_add(page.len()).to_string());
        Ok(BoostPage { total_count, returned_count: page.len(), boosts: page, next_offset })
    }

    async fn apply_boosts(
        &self,
        _session_path: Option<PathBuf>,
        chat_id: &str,
        slots: &[i32],
    ) -> Result<MyBoostsSummary, TelegramError> {
        let mut state = self.state.lock().await;
        let canonical_id = Self::resolve_known_chat(&state, chat_id)
            .ok_or_else(|| TelegramError::PeerNotFound(chat_id.to_string()))?;
        let target_chat_id = state
            .known_chats
            .get(&canonical_id)
            .ok_or_else(|| TelegramError::PeerNotFound(chat_id.to_string()))?
            .chat
            .chat_id
            .clone();
        for requested in slots {
            let slot = state.my_boosts.iter_mut().find(|slot| slot.slot == *requested).ok_or_else(
                || {
                    TelegramError::Telegram(format!(
                        "fixture boost slot {requested} does not exist"
                    ))
                },
            )?;
            slot.chat_id = Some(target_chat_id.clone());
            slot.applied_at_unix = 1_700_000_100;
            slot.cooldown_until_unix = None;
        }
        state.boost_apply_calls = state.boost_apply_calls.saturating_add(1);
        let uncertain = std::mem::take(&mut state.boost_apply_uncertain_once);
        let boosts = MyBoostsSummary {
            returned_count: state.my_boosts.len(),
            slots: state.my_boosts.clone(),
        };
        if uncertain {
            return Err(TelegramError::DeliveryUncertain(
                "fixture applied boost slots before the transport receipt was lost".to_string(),
            ));
        }
        Ok(boosts)
    }

    async fn check_chat_username(
        &self,
        _session_path: Option<PathBuf>,
        chat_id: &str,
        username: &str,
    ) -> Result<bool, TelegramError> {
        let state = self.state.lock().await;
        let canonical_id = Self::resolve_known_chat(&state, chat_id)
            .ok_or_else(|| TelegramError::PeerNotFound(chat_id.to_string()))?;
        Self::channel_settings_snapshot(&state, canonical_id)?;
        Ok(!state.known_chats.iter().any(|(candidate_id, details)| {
            *candidate_id != canonical_id
                && details
                    .chat
                    .username
                    .as_deref()
                    .is_some_and(|candidate| candidate.eq_ignore_ascii_case(username))
        }))
    }

    async fn update_chat_username(
        &self,
        _session_path: Option<PathBuf>,
        chat_id: &str,
        username: Option<&str>,
    ) -> Result<ChannelSettingsSummary, TelegramError> {
        let mut state = self.state.lock().await;
        let canonical_id = Self::resolve_known_chat(&state, chat_id)
            .ok_or_else(|| TelegramError::PeerNotFound(chat_id.to_string()))?;
        Self::channel_settings_snapshot(&state, canonical_id)?;
        let username = username.map(str::to_string);
        state
            .known_chats
            .get_mut(&canonical_id)
            .ok_or_else(|| TelegramError::PeerNotFound(chat_id.to_string()))?
            .chat
            .username = username.clone();
        if let Some(dialog) = state.dialogs.iter_mut().find(|dialog| dialog.chat_id == canonical_id)
        {
            dialog.username = username.clone();
            dialog.chat_ref = username
                .as_ref()
                .map_or_else(|| dialog.canonical_chat_id.clone(), |value| format!("@{value}"));
        }
        state.chat_username_calls = state.chat_username_calls.saturating_add(1);
        Self::channel_settings_snapshot(&state, canonical_id)
    }

    async fn set_chat_slow_mode(
        &self,
        _session_path: Option<PathBuf>,
        chat_id: &str,
        seconds: i32,
    ) -> Result<ChannelSettingsSummary, TelegramError> {
        let mut state = self.state.lock().await;
        let canonical_id = Self::resolve_known_chat(&state, chat_id)
            .ok_or_else(|| TelegramError::PeerNotFound(chat_id.to_string()))?;
        state
            .channel_settings
            .get_mut(&canonical_id)
            .ok_or_else(|| TelegramError::InvalidPeer("slow mode requires a supergroup".into()))?
            .slow_mode_seconds = seconds;
        state.chat_slow_mode_calls = state.chat_slow_mode_calls.saturating_add(1);
        Self::channel_settings_snapshot(&state, canonical_id)
    }

    async fn toggle_chat_signatures(
        &self,
        _session_path: Option<PathBuf>,
        chat_id: &str,
        signatures_enabled: bool,
        profiles_enabled: bool,
    ) -> Result<ChannelSettingsSummary, TelegramError> {
        let mut state = self.state.lock().await;
        let canonical_id = Self::resolve_known_chat(&state, chat_id)
            .ok_or_else(|| TelegramError::PeerNotFound(chat_id.to_string()))?;
        let settings = state.channel_settings.get_mut(&canonical_id).ok_or_else(|| {
            TelegramError::InvalidPeer("signatures require a broadcast channel".into())
        })?;
        settings.signatures_enabled = signatures_enabled;
        settings.signature_profiles_enabled = profiles_enabled;
        state.chat_signatures_calls = state.chat_signatures_calls.saturating_add(1);
        Self::channel_settings_snapshot(&state, canonical_id)
    }

    async fn toggle_chat_pre_history_hidden(
        &self,
        _session_path: Option<PathBuf>,
        chat_id: &str,
        enabled: bool,
    ) -> Result<ChannelSettingsSummary, TelegramError> {
        let mut state = self.state.lock().await;
        let canonical_id = Self::resolve_known_chat(&state, chat_id)
            .ok_or_else(|| TelegramError::PeerNotFound(chat_id.to_string()))?;
        state
            .channel_settings
            .get_mut(&canonical_id)
            .ok_or_else(|| TelegramError::InvalidPeer("pre-history requires a supergroup".into()))?
            .pre_history_hidden = enabled;
        state.chat_pre_history_calls = state.chat_pre_history_calls.saturating_add(1);
        Self::channel_settings_snapshot(&state, canonical_id)
    }

    async fn toggle_chat_join_request(
        &self,
        _session_path: Option<PathBuf>,
        chat_id: &str,
        enabled: bool,
    ) -> Result<ChannelSettingsSummary, TelegramError> {
        let mut state = self.state.lock().await;
        let canonical_id = Self::resolve_known_chat(&state, chat_id)
            .ok_or_else(|| TelegramError::PeerNotFound(chat_id.to_string()))?;
        state
            .channel_settings
            .get_mut(&canonical_id)
            .ok_or_else(|| {
                TelegramError::InvalidPeer("join requests require a channel or supergroup".into())
            })?
            .join_request_required = enabled;
        state.chat_join_request_calls = state.chat_join_request_calls.saturating_add(1);
        Self::channel_settings_snapshot(&state, canonical_id)
    }

    async fn set_chat_default_permissions(
        &self,
        _session_path: Option<PathBuf>,
        chat_id: &str,
        target: &ChatBannedRightsSummary,
    ) -> Result<ChatModerationSettingsSummary, TelegramError> {
        let mut state = self.state.lock().await;
        let canonical_id = Self::resolve_known_chat(&state, chat_id)
            .ok_or_else(|| TelegramError::PeerNotFound(chat_id.to_string()))?;
        state
            .chat_moderation_settings
            .get_mut(&canonical_id)
            .ok_or_else(|| {
                TelegramError::InvalidPeer(
                    "default permissions require a group, supergroup, or channel".to_string(),
                )
            })?
            .default_banned_rights = Some(target.clone());
        state.chat_permissions_calls = state.chat_permissions_calls.saturating_add(1);
        let uncertain = std::mem::take(&mut state.chat_permissions_uncertain_once);
        let summary = Self::chat_moderation_settings_snapshot(&state, canonical_id)?;
        if uncertain {
            return Err(TelegramError::DeliveryUncertain(
                "fixture changed default chat permissions before the transport receipt was lost"
                    .to_string(),
            ));
        }
        Ok(summary)
    }

    async fn set_chat_available_reactions(
        &self,
        _session_path: Option<PathBuf>,
        chat_id: &str,
        target: &ChatReactionsPolicy,
    ) -> Result<ChatModerationSettingsSummary, TelegramError> {
        let mut state = self.state.lock().await;
        let canonical_id = Self::resolve_known_chat(&state, chat_id)
            .ok_or_else(|| TelegramError::PeerNotFound(chat_id.to_string()))?;
        state
            .chat_moderation_settings
            .get_mut(&canonical_id)
            .ok_or_else(|| {
                TelegramError::InvalidPeer(
                    "available reactions require a group, supergroup, or channel".to_string(),
                )
            })?
            .available_reactions = Some(target.clone());
        state.chat_reactions_policy_calls = state.chat_reactions_policy_calls.saturating_add(1);
        let uncertain = std::mem::take(&mut state.chat_reactions_policy_uncertain_once);
        let summary = Self::chat_moderation_settings_snapshot(&state, canonical_id)?;
        if uncertain {
            return Err(TelegramError::DeliveryUncertain(
                "fixture changed the chat reaction policy before the transport receipt was lost"
                    .to_string(),
            ));
        }
        Ok(summary)
    }

    async fn toggle_chat_anti_spam(
        &self,
        _session_path: Option<PathBuf>,
        chat_id: &str,
        enabled: bool,
    ) -> Result<ChatModerationSettingsSummary, TelegramError> {
        let mut state = self.state.lock().await;
        let canonical_id = Self::resolve_known_chat(&state, chat_id)
            .ok_or_else(|| TelegramError::PeerNotFound(chat_id.to_string()))?;
        let settings = state.chat_moderation_settings.get_mut(&canonical_id).ok_or_else(|| {
            TelegramError::InvalidPeer("native anti-spam requires a supergroup".to_string())
        })?;
        let Some(current) = settings.anti_spam_enabled.as_mut() else {
            return Err(TelegramError::InvalidPeer(
                "native anti-spam requires a supergroup".to_string(),
            ));
        };
        *current = enabled;
        state.chat_anti_spam_calls = state.chat_anti_spam_calls.saturating_add(1);
        let uncertain = std::mem::take(&mut state.chat_anti_spam_uncertain_once);
        let summary = Self::chat_moderation_settings_snapshot(&state, canonical_id)?;
        if uncertain {
            return Err(TelegramError::DeliveryUncertain(
                "fixture toggled native anti-spam before the transport receipt was lost"
                    .to_string(),
            ));
        }
        Ok(summary)
    }

    async fn toggle_chat_forum_mode(
        &self,
        _session_path: Option<PathBuf>,
        chat_id: &str,
        enabled: bool,
        tabs_enabled: bool,
    ) -> Result<ChatModerationSettingsSummary, TelegramError> {
        let mut state = self.state.lock().await;
        let canonical_id = Self::resolve_known_chat(&state, chat_id)
            .ok_or_else(|| TelegramError::PeerNotFound(chat_id.to_string()))?;
        let settings = state.chat_moderation_settings.get_mut(&canonical_id).ok_or_else(|| {
            TelegramError::InvalidPeer("forum mode requires a supergroup".to_string())
        })?;
        let Some(forum_enabled) = settings.forum_enabled.as_mut() else {
            return Err(TelegramError::InvalidPeer("forum mode requires a supergroup".to_string()));
        };
        *forum_enabled = enabled;
        settings.forum_tabs_enabled = Some(enabled && tabs_enabled);
        if let Some(channel_settings) = state.channel_settings.get_mut(&canonical_id) {
            channel_settings.forum_enabled = enabled;
        }
        state.chat_forum_mode_calls = state.chat_forum_mode_calls.saturating_add(1);
        let uncertain = std::mem::take(&mut state.chat_forum_mode_uncertain_once);
        let summary = Self::chat_moderation_settings_snapshot(&state, canonical_id)?;
        if uncertain {
            return Err(TelegramError::DeliveryUncertain(
                "fixture toggled forum mode before the transport receipt was lost".to_string(),
            ));
        }
        Ok(summary)
    }

    async fn list_stories(
        &self,
        _session_path: Option<PathBuf>,
        _next: bool,
        _hidden: bool,
        state_token: Option<&str>,
    ) -> Result<StoryListPage, TelegramError> {
        let state = self.state.lock().await;
        let canonical_state = "fixture-story-state-v1";
        let mut peers = state
            .stories
            .iter()
            .filter_map(|(chat_id, stories)| {
                let details = state.known_chats.get(chat_id)?;
                Some(StoryPeerSummary {
                    peer_ref: details.chat.chat_id.clone(),
                    max_read_id: state.story_max_read_ids.get(chat_id).copied(),
                    stories: stories.clone(),
                })
            })
            .collect::<Vec<_>>();
        peers.sort_by(|left, right| left.peer_ref.cmp(&right.peer_ref));
        Ok(StoryListPage {
            state: Some(state_token.unwrap_or(canonical_state).to_string()),
            has_more: false,
            count: peers.iter().map(|peer| peer.stories.len()).sum::<usize>() as i32,
            peers,
            stealth_mode_active_until_unix: state.story_stealth_mode.active_until_unix,
            stealth_mode_cooldown_until_unix: state.story_stealth_mode.cooldown_until_unix,
        })
    }

    async fn get_peer_stories(
        &self,
        _session_path: Option<PathBuf>,
        chat_id: &str,
    ) -> Result<StoryPeerPage, TelegramError> {
        let state = self.state.lock().await;
        let canonical_id = Self::resolve_known_chat(&state, chat_id)
            .ok_or_else(|| TelegramError::PeerNotFound(chat_id.to_string()))?;
        let details = state
            .known_chats
            .get(&canonical_id)
            .ok_or_else(|| TelegramError::PeerNotFound(chat_id.to_string()))?;
        let peer = StoryPeerSummary {
            peer_ref: details.chat.chat_id.clone(),
            max_read_id: state.story_max_read_ids.get(&canonical_id).copied(),
            stories: state.stories.get(&canonical_id).cloned().unwrap_or_default(),
        };
        Ok(StoryPeerPage { count: peer.stories.len(), peer })
    }

    async fn get_stories_by_ids(
        &self,
        _session_path: Option<PathBuf>,
        chat_id: &str,
        story_ids: &[i32],
    ) -> Result<StoryCollectionPage, TelegramError> {
        let state = self.state.lock().await;
        let canonical_id = Self::resolve_known_chat(&state, chat_id)
            .ok_or_else(|| TelegramError::PeerNotFound(chat_id.to_string()))?;
        let peer_ref = state
            .known_chats
            .get(&canonical_id)
            .map(|details| details.chat.chat_id.clone())
            .ok_or_else(|| TelegramError::PeerNotFound(chat_id.to_string()))?;
        let stories = story_ids
            .iter()
            .filter_map(|story_id| {
                state
                    .stories
                    .get(&canonical_id)
                    .into_iter()
                    .flatten()
                    .chain(state.story_archive.get(&canonical_id).into_iter().flatten())
                    .find(|story| story.story_id == *story_id)
                    .cloned()
            })
            .collect::<Vec<_>>();
        Ok(StoryCollectionPage {
            peer_ref,
            count: i32::try_from(stories.len()).unwrap_or(i32::MAX),
            stories,
            pinned_to_top_ids: state
                .story_pinned_to_top
                .get(&canonical_id)
                .cloned()
                .unwrap_or_default(),
            next_offset_id: None,
            completeness: Completeness::Complete,
        })
    }

    async fn get_stories_archive(
        &self,
        _session_path: Option<PathBuf>,
        chat_id: &str,
        offset_id: i32,
        limit: usize,
    ) -> Result<StoryCollectionPage, TelegramError> {
        let state = self.state.lock().await;
        let canonical_id = Self::resolve_known_chat(&state, chat_id)
            .ok_or_else(|| TelegramError::PeerNotFound(chat_id.to_string()))?;
        let peer_ref = state
            .known_chats
            .get(&canonical_id)
            .map(|details| details.chat.chat_id.clone())
            .ok_or_else(|| TelegramError::PeerNotFound(chat_id.to_string()))?;
        let all = state.story_archive.get(&canonical_id).cloned().unwrap_or_default();
        let mut stories = all
            .iter()
            .filter(|story| offset_id == 0 || story.story_id < offset_id)
            .take(limit)
            .cloned()
            .collect::<Vec<_>>();
        stories.sort_by_key(|right| std::cmp::Reverse(right.story_id));
        let has_more = stories.len() == limit
            && stories
                .last()
                .is_some_and(|last| all.iter().any(|story| story.story_id < last.story_id));
        Ok(StoryCollectionPage {
            peer_ref,
            count: i32::try_from(all.len()).unwrap_or(i32::MAX),
            next_offset_id: has_more.then(|| stories.last().map(|story| story.story_id)).flatten(),
            completeness: if has_more { Completeness::Page } else { Completeness::Complete },
            stories,
            pinned_to_top_ids: state
                .story_pinned_to_top
                .get(&canonical_id)
                .cloned()
                .unwrap_or_default(),
        })
    }

    async fn export_story_link(
        &self,
        _session_path: Option<PathBuf>,
        chat_id: &str,
        story_id: i32,
    ) -> Result<StoryLinkSummary, TelegramError> {
        let state = self.state.lock().await;
        let canonical_id = Self::resolve_known_chat(&state, chat_id)
            .ok_or_else(|| TelegramError::PeerNotFound(chat_id.to_string()))?;
        let peer_ref = state
            .known_chats
            .get(&canonical_id)
            .map(|details| details.chat.chat_id.clone())
            .ok_or_else(|| TelegramError::PeerNotFound(chat_id.to_string()))?;
        let exists = state
            .stories
            .get(&canonical_id)
            .into_iter()
            .flatten()
            .chain(state.story_archive.get(&canonical_id).into_iter().flatten())
            .any(|story| story.story_id == story_id && story.state != "deleted");
        if !exists {
            return Err(TelegramError::PeerNotFound(format!("story {story_id}")));
        }
        Ok(StoryLinkSummary {
            peer_ref,
            story_id,
            link: format!("https://t.me/c/{canonical_id}/{story_id}"),
        })
    }

    #[allow(clippy::too_many_arguments)]
    async fn get_story_viewers(
        &self,
        _session_path: Option<PathBuf>,
        chat_id: &str,
        story_id: i32,
        offset: &str,
        limit: usize,
        just_contacts: bool,
        reactions_first: bool,
        _forwards_first: bool,
        query: Option<&str>,
    ) -> Result<StoryViewersPage, TelegramError> {
        let state = self.state.lock().await;
        let canonical_id = Self::resolve_known_chat(&state, chat_id)
            .ok_or_else(|| TelegramError::PeerNotFound(chat_id.to_string()))?;
        let story = state
            .stories
            .get(&canonical_id)
            .and_then(|stories| stories.iter().find(|story| story.story_id == story_id))
            .ok_or_else(|| TelegramError::PeerNotFound(format!("story {story_id}")))?;
        let start = if offset.trim().is_empty() {
            0
        } else {
            offset.parse::<usize>().map_err(|_| {
                TelegramError::InvalidPeer(
                    "story viewers offset must be a non-negative index".into(),
                )
            })?
        };
        let query = query.map(str::to_lowercase);
        let mut viewers = state
            .story_viewers
            .get(&(canonical_id, story_id))
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .filter(|viewer| {
                (!just_contacts || viewer.user.as_ref().is_some_and(|user| user.is_contact))
                    && query.as_deref().is_none_or(|query| {
                        viewer.user.as_ref().is_some_and(|user| {
                            user.display_name.to_lowercase().contains(query)
                                || user
                                    .username
                                    .as_deref()
                                    .is_some_and(|username| username.to_lowercase().contains(query))
                        })
                    })
            })
            .collect::<Vec<_>>();
        if reactions_first {
            viewers.sort_by_key(|viewer| viewer.reaction_kind.is_none());
        }
        let total = viewers.len();
        let page = viewers.into_iter().skip(start).take(limit).collect::<Vec<_>>();
        let next_offset = (start.saturating_add(page.len()) < total)
            .then(|| start.saturating_add(page.len()).to_string());
        Ok(StoryViewersPage {
            story_id,
            count: total as i32,
            views_count: story.views_count.unwrap_or(total as i32),
            forwards_count: story.forwards_count.unwrap_or(0),
            reactions_count: story.reactions_count.unwrap_or(0),
            viewers: page,
            next_offset,
        })
    }

    async fn view_story(
        &self,
        _session_path: Option<PathBuf>,
        chat_id: &str,
        story_ids: &[i32],
    ) -> Result<(), TelegramError> {
        let mut state = self.state.lock().await;
        let canonical_id = Self::resolve_known_chat(&state, chat_id)
            .ok_or_else(|| TelegramError::PeerNotFound(chat_id.to_string()))?;
        let stories = state
            .stories
            .get(&canonical_id)
            .ok_or_else(|| TelegramError::PeerNotFound(chat_id.to_string()))?;
        if let Some(missing) = story_ids
            .iter()
            .find(|story_id| !stories.iter().any(|story| story.story_id == **story_id))
        {
            return Err(TelegramError::PeerNotFound(format!("story {missing}")));
        }
        let max_read_id = state.story_max_read_ids.entry(canonical_id).or_default();
        if let Some(highest) = story_ids.iter().copied().max() {
            *max_read_id = (*max_read_id).max(highest);
        }
        state.story_view_calls = state.story_view_calls.saturating_add(1);
        Ok(())
    }

    async fn read_stories(
        &self,
        _session_path: Option<PathBuf>,
        chat_id: &str,
        max_id: i32,
    ) -> Result<Vec<i32>, TelegramError> {
        let mut state = self.state.lock().await;
        let canonical_id = Self::resolve_known_chat(&state, chat_id)
            .ok_or_else(|| TelegramError::PeerNotFound(chat_id.to_string()))?;
        let exists = state
            .stories
            .get(&canonical_id)
            .into_iter()
            .flatten()
            .chain(state.story_archive.get(&canonical_id).into_iter().flatten())
            .any(|story| story.story_id == max_id && story.state != "deleted");
        if !exists {
            return Err(TelegramError::PeerNotFound(format!("story {max_id}")));
        }
        let before = state.story_max_read_ids.entry(canonical_id).or_default();
        let previous = *before;
        *before = (*before).max(max_id);
        state.story_read_calls = state.story_read_calls.saturating_add(1);
        Ok(((previous + 1)..=max_id).collect())
    }

    async fn set_story_reaction(
        &self,
        _session_path: Option<PathBuf>,
        chat_id: &str,
        story_id: i32,
        reaction: &ReactionSpec,
    ) -> Result<(), TelegramError> {
        let mut state = self.state.lock().await;
        let canonical_id = Self::resolve_known_chat(&state, chat_id)
            .ok_or_else(|| TelegramError::PeerNotFound(chat_id.to_string()))?;
        let target = match reaction {
            ReactionSpec::Remove => None,
            ReactionSpec::Emoji { emoji, .. } => {
                Some(ReactionSpec::Emoji { emoji: emoji.clone(), big: false, add_to_recent: false })
            },
            ReactionSpec::CustomEmoji { document_id, .. } => Some(ReactionSpec::CustomEmoji {
                document_id: *document_id,
                big: false,
                add_to_recent: false,
            }),
        };
        let story = Self::story_mut(&mut state, canonical_id, story_id)
            .ok_or_else(|| TelegramError::PeerNotFound(format!("story {story_id}")))?;
        story.sent_reaction = target;
        state.story_reaction_calls = state.story_reaction_calls.saturating_add(1);
        Ok(())
    }

    async fn set_stories_pinned(
        &self,
        _session_path: Option<PathBuf>,
        chat_id: &str,
        story_ids: &[i32],
        pinned: bool,
    ) -> Result<Vec<i32>, TelegramError> {
        let mut state = self.state.lock().await;
        let canonical_id = Self::resolve_known_chat(&state, chat_id)
            .ok_or_else(|| TelegramError::PeerNotFound(chat_id.to_string()))?;
        for story_id in story_ids {
            if Self::story_mut(&mut state, canonical_id, *story_id).is_none() {
                return Err(TelegramError::PeerNotFound(format!("story {story_id}")));
            }
        }
        for story_id in story_ids {
            if let Some(story) = Self::story_mut(&mut state, canonical_id, *story_id) {
                story.pinned = pinned;
            }
        }
        state.story_pin_calls = state.story_pin_calls.saturating_add(1);
        Ok(story_ids.to_vec())
    }

    async fn toggle_stories_pinned_to_top(
        &self,
        _session_path: Option<PathBuf>,
        chat_id: &str,
        story_ids: &[i32],
    ) -> Result<bool, TelegramError> {
        let mut state = self.state.lock().await;
        let canonical_id = Self::resolve_known_chat(&state, chat_id)
            .ok_or_else(|| TelegramError::PeerNotFound(chat_id.to_string()))?;
        for story_id in story_ids {
            if Self::story_mut(&mut state, canonical_id, *story_id).is_none() {
                return Err(TelegramError::PeerNotFound(format!("story {story_id}")));
            }
        }
        let pinned = state.story_pinned_to_top.entry(canonical_id).or_default();
        for story_id in story_ids {
            if let Some(index) = pinned.iter().position(|current| current == story_id) {
                pinned.remove(index);
            } else {
                pinned.push(*story_id);
            }
        }
        state.story_top_pin_calls = state.story_top_pin_calls.saturating_add(1);
        Ok(true)
    }

    async fn activate_story_stealth_mode(
        &self,
        _session_path: Option<PathBuf>,
        past: bool,
        future: bool,
    ) -> Result<(), TelegramError> {
        if !past && !future {
            return Err(TelegramError::InvalidPeer(
                "story stealth mode requires past or future".into(),
            ));
        }
        let mut state = self.state.lock().await;
        state.story_stealth_mode = StoryStealthModeSummary {
            active_until_unix: Some(2_000_000_000),
            cooldown_until_unix: Some(2_000_001_500),
        };
        state.story_stealth_calls = state.story_stealth_calls.saturating_add(1);
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    async fn send_story_with_random_id(
        &self,
        _session_path: Option<PathBuf>,
        chat_id: &str,
        file_path: &std::path::Path,
        caption: &str,
        privacy: &StoryPrivacySpec,
        pinned: bool,
        noforwards: bool,
        period_seconds: Option<i32>,
        random_id: i64,
    ) -> Result<i32, TelegramError> {
        let metadata = tokio::fs::metadata(file_path).await?;
        if !metadata.is_file() {
            return Err(TelegramError::Io(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "story upload path is not a regular file",
            )));
        }
        let bytes = tokio::fs::read(file_path).await?;
        let name = file_path.file_name().and_then(|value| value.to_str()).unwrap_or("story.bin");
        let mime = ferogram::media::detect_mime_from_bytes(&bytes[..bytes.len().min(512)], name);
        let media_kind = if mime.starts_with("image/") {
            "photo"
        } else if mime.starts_with("video/") {
            "document"
        } else {
            return Err(TelegramError::SendRejected(format!(
                "story media must be an image or video; detected {mime}"
            )));
        };
        let mut state = self.state.lock().await;
        if let Some((existing_chat_id, story_id)) = state.story_by_random_id.get(&random_id) {
            if *existing_chat_id == Self::resolve_known_chat(&state, chat_id).unwrap_or_default() {
                return Ok(*story_id);
            }
            return Err(TelegramError::SendRejected(
                "story random_id is already bound to another peer".into(),
            ));
        }
        let canonical_id = Self::resolve_known_chat(&state, chat_id)
            .ok_or_else(|| TelegramError::PeerNotFound(chat_id.to_string()))?;
        let story_id = state.next_story_id;
        state.next_story_id = state.next_story_id.saturating_add(1);
        let date_unix = 1_700_000_500_i32.saturating_add(story_id);
        let story = StorySummary {
            state: "active".to_string(),
            story_id,
            date_unix,
            expire_date_unix: date_unix.saturating_add(period_seconds.unwrap_or(86_400)),
            from_ref: Some(state.account.user_id.to_string()),
            caption: (!caption.is_empty()).then(|| caption.to_string()),
            pinned,
            public: privacy.kind == "everyone",
            close_friends: privacy.kind == "close_friends",
            noforwards,
            edited: false,
            contacts: privacy.kind == "contacts",
            selected_contacts: privacy.kind == "selected_contacts",
            outgoing: true,
            live: false,
            media_kind: Some(media_kind.to_string()),
            views_count: Some(0),
            forwards_count: Some(0),
            reactions_count: Some(0),
            sent_reaction: None,
        };
        state.stories.entry(canonical_id).or_default().insert(0, story);
        state.story_by_random_id.insert(random_id, (canonical_id, story_id));
        state.story_send_calls = state.story_send_calls.saturating_add(1);
        Ok(story_id)
    }

    #[allow(clippy::too_many_arguments)]
    async fn edit_story(
        &self,
        _session_path: Option<PathBuf>,
        chat_id: &str,
        story_id: i32,
        file_path: Option<&std::path::Path>,
        caption: Option<&str>,
        privacy: Option<&StoryPrivacySpec>,
    ) -> Result<(), TelegramError> {
        let target_media_kind = if let Some(file_path) = file_path {
            let metadata = tokio::fs::metadata(file_path).await?;
            if !metadata.is_file() {
                return Err(TelegramError::Io(std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    "story upload path is not a regular file",
                )));
            }
            let bytes = tokio::fs::read(file_path).await?;
            let name =
                file_path.file_name().and_then(|value| value.to_str()).unwrap_or("story.bin");
            let mime =
                ferogram::media::detect_mime_from_bytes(&bytes[..bytes.len().min(512)], name);
            Some(if mime.starts_with("image/") && mime != "image/gif" {
                "photo".to_string()
            } else if mime.starts_with("video/") {
                "document".to_string()
            } else {
                return Err(TelegramError::SendRejected(format!(
                    "story media must be a supported image or video; detected {mime}"
                )));
            })
        } else {
            None
        };
        let mut state = self.state.lock().await;
        let canonical_id = Self::resolve_known_chat(&state, chat_id)
            .ok_or_else(|| TelegramError::PeerNotFound(chat_id.to_string()))?;
        let story = Self::story_mut(&mut state, canonical_id, story_id)
            .ok_or_else(|| TelegramError::PeerNotFound(format!("story {story_id}")))?;
        if let Some(media_kind) = target_media_kind {
            story.media_kind = Some(media_kind);
        }
        if let Some(caption) = caption {
            story.caption = (!caption.is_empty()).then(|| caption.to_string());
        }
        if let Some(privacy) = privacy {
            story.public = privacy.kind == "everyone";
            story.contacts = privacy.kind == "contacts";
            story.close_friends = privacy.kind == "close_friends";
            story.selected_contacts = privacy.kind == "selected_contacts";
        }
        story.edited = true;
        state.story_edit_calls = state.story_edit_calls.saturating_add(1);
        Ok(())
    }

    async fn report_stories(
        &self,
        _session_path: Option<PathBuf>,
        chat_id: &str,
        story_ids: &[i32],
        option_hex: &str,
        message: &str,
    ) -> Result<StoryReportResult, TelegramError> {
        let mut state = self.state.lock().await;
        let canonical_id = Self::resolve_known_chat(&state, chat_id)
            .ok_or_else(|| TelegramError::PeerNotFound(chat_id.to_string()))?;
        let all_stories = state
            .stories
            .get(&canonical_id)
            .into_iter()
            .flatten()
            .chain(state.story_archive.get(&canonical_id).into_iter().flatten());
        let known_ids = all_stories.map(|story| story.story_id).collect::<HashSet<_>>();
        if story_ids.iter().any(|story_id| !known_ids.contains(story_id)) {
            return Err(TelegramError::PeerNotFound(format!(
                "one or more stories were not found for {chat_id}"
            )));
        }
        let result = match (option_hex, message.is_empty()) {
            ("", _) => StoryReportResult::ChooseOption {
                title: "Why are you reporting these stories?".to_string(),
                options: vec![
                    StoryReportOption { text: "Spam".to_string(), option_hex: "01".to_string() },
                    StoryReportOption {
                        text: "Violence".to_string(),
                        option_hex: "02".to_string(),
                    },
                ],
            },
            ("01", _) => StoryReportResult::Reported,
            ("02", true) => {
                StoryReportResult::AddComment { optional: false, option_hex: "02".to_string() }
            },
            ("02", false) => StoryReportResult::Reported,
            _ => {
                return Err(TelegramError::SendRejected(
                    "unknown fake story report option".to_string(),
                ));
            },
        };
        state.story_report_calls = state.story_report_calls.saturating_add(1);
        if state.story_report_uncertain_once {
            state.story_report_uncertain_once = false;
            return Err(TelegramError::DeliveryUncertain(
                "fake Telegram accepted the report step but lost the response".to_string(),
            ));
        }
        Ok(result)
    }

    async fn delete_stories(
        &self,
        _session_path: Option<PathBuf>,
        chat_id: &str,
        story_ids: &[i32],
    ) -> Result<Vec<i32>, TelegramError> {
        let mut state = self.state.lock().await;
        let canonical_id = Self::resolve_known_chat(&state, chat_id)
            .ok_or_else(|| TelegramError::PeerNotFound(chat_id.to_string()))?;
        let stories = state.stories.entry(canonical_id).or_default();
        let deleted = story_ids
            .iter()
            .copied()
            .filter(|story_id| stories.iter().any(|story| story.story_id == *story_id))
            .collect::<Vec<_>>();
        stories.retain(|story| !story_ids.contains(&story.story_id));
        state.story_delete_calls = state.story_delete_calls.saturating_add(1);
        Ok(deleted)
    }

    async fn get_story(
        &self,
        _session_path: Option<PathBuf>,
        chat_id: &str,
        story_id: i32,
    ) -> Result<Option<StorySummary>, TelegramError> {
        let state = self.state.lock().await;
        let canonical_id = Self::resolve_known_chat(&state, chat_id)
            .ok_or_else(|| TelegramError::PeerNotFound(chat_id.to_string()))?;
        Ok(state
            .stories
            .get(&canonical_id)
            .into_iter()
            .flatten()
            .chain(state.story_archive.get(&canonical_id).into_iter().flatten())
            .find(|story| story.story_id == story_id)
            .cloned())
    }

    async fn get_chat_administrators(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
    ) -> Result<Vec<ParticipantSummary>, TelegramError> {
        let participants = self.list_participants(session_path, chat_id, 200).await?;
        Ok(participants
            .into_iter()
            .filter(|participant| matches!(participant.role.as_str(), "creator" | "admin"))
            .collect())
    }

    async fn get_online_count(
        &self,
        _session_path: Option<PathBuf>,
        chat_id: &str,
    ) -> Result<i32, TelegramError> {
        let state = self.state.lock().await;
        let canonical_id = Self::resolve_dialog(&state.dialogs, chat_id)
            .ok_or_else(|| TelegramError::PeerNotFound(chat_id.to_string()))?;
        let dialog = state
            .dialogs
            .iter()
            .find(|dialog| dialog.chat_id == canonical_id)
            .ok_or_else(|| TelegramError::PeerNotFound(chat_id.to_string()))?;
        if dialog.chat_kind == "user" {
            return Err(TelegramError::InvalidPeer(
                "get_online_count requires a group, supergroup, or channel".to_string(),
            ));
        }
        Ok(2)
    }

    async fn get_blocked_users(
        &self,
        _session_path: Option<PathBuf>,
        offset: usize,
        limit: usize,
    ) -> Result<Vec<ContactSummary>, TelegramError> {
        let state = self.state.lock().await;
        Ok(state
            .contacts
            .iter()
            .filter(|contact| state.blocked_users.contains(&contact.user_id))
            .skip(offset)
            .take(limit)
            .cloned()
            .collect())
    }

    async fn list_forum_topics(
        &self,
        _session_path: Option<PathBuf>,
        chat_id: &str,
        query: Option<&str>,
        limit: usize,
        _offset_date: i32,
        _offset_id: i32,
        _offset_topic: i32,
    ) -> Result<Vec<ForumTopicSummary>, TelegramError> {
        let state = self.state.lock().await;
        let canonical_id = Self::resolve_dialog(&state.dialogs, chat_id)
            .ok_or_else(|| TelegramError::PeerNotFound(chat_id.to_string()))?;
        let dialog = state
            .dialogs
            .iter()
            .find(|dialog| dialog.chat_id == canonical_id)
            .ok_or_else(|| TelegramError::PeerNotFound(chat_id.to_string()))?;
        if dialog.chat_kind == "user" {
            return Err(TelegramError::InvalidPeer(
                "list_forum_topics requires a forum supergroup".to_string(),
            ));
        }
        let normalized_query =
            query.map(str::trim).filter(|query| !query.is_empty()).map(str::to_lowercase);
        Ok(state
            .forum_topics
            .get(&canonical_id)
            .into_iter()
            .flatten()
            .filter(|topic| !topic.deleted)
            .filter(|topic| {
                normalized_query.as_ref().is_none_or(|query| {
                    topic.title.as_deref().is_some_and(|title| title.to_lowercase().contains(query))
                })
            })
            .take(limit)
            .cloned()
            .collect())
    }

    async fn get_forum_topic(
        &self,
        _session_path: Option<PathBuf>,
        chat_id: &str,
        topic_id: i32,
    ) -> Result<Option<ForumTopicSummary>, TelegramError> {
        let state = self.state.lock().await;
        let canonical_id = Self::resolve_dialog(&state.dialogs, chat_id)
            .ok_or_else(|| TelegramError::PeerNotFound(chat_id.to_string()))?;
        Ok(state
            .forum_topics
            .get(&canonical_id)
            .and_then(|topics| topics.iter().find(|topic| topic.topic_id == topic_id))
            .cloned())
    }

    async fn create_forum_topic_with_random_id(
        &self,
        _session_path: Option<PathBuf>,
        chat_id: &str,
        title: &str,
        icon_color: Option<i32>,
        icon_emoji_id: Option<i64>,
        random_id: i64,
    ) -> Result<i32, TelegramError> {
        let mut state = self.state.lock().await;
        if let Some(topic_id) = state.forum_topic_by_random_id.get(&random_id) {
            return Ok(*topic_id);
        }
        let canonical_id = Self::resolve_dialog(&state.dialogs, chat_id)
            .ok_or_else(|| TelegramError::PeerNotFound(chat_id.to_string()))?;
        let dialog = state
            .dialogs
            .iter()
            .find(|dialog| dialog.chat_id == canonical_id)
            .ok_or_else(|| TelegramError::PeerNotFound(chat_id.to_string()))?;
        if dialog.chat_kind == "user" {
            return Err(TelegramError::InvalidPeer(
                "create_forum_topic requires a forum supergroup".to_string(),
            ));
        }
        let topic_id = state.next_message_id;
        state.next_message_id = state.next_message_id.saturating_add(1);
        let topic = ForumTopicSummary {
            topic_id,
            deleted: false,
            title: Some(title.to_string()),
            date_unix: Some(1_700_004_000_i32.saturating_add(topic_id)),
            creator_ref: Some(state.account.user_id.to_string()),
            my: true,
            closed: false,
            pinned: false,
            short: false,
            hidden: false,
            title_missing: false,
            icon_color: Some(icon_color.unwrap_or(0x006f_bad0)),
            icon_emoji_id,
            top_message_id: Some(topic_id),
            read_inbox_max_id: Some(topic_id),
            read_outbox_max_id: Some(topic_id),
            unread_count: Some(0),
            unread_mentions_count: Some(0),
            unread_reactions_count: Some(0),
            unread_poll_votes_count: Some(0),
            notification_settings: Some(default_notification_settings()),
            draft: None,
        };
        state.forum_topics.entry(canonical_id).or_default().push(topic);
        state.forum_topic_by_random_id.insert(random_id, topic_id);
        state.forum_topic_create_calls = state.forum_topic_create_calls.saturating_add(1);
        Ok(topic_id)
    }

    #[allow(clippy::too_many_arguments)]
    async fn edit_forum_topic(
        &self,
        _session_path: Option<PathBuf>,
        chat_id: &str,
        topic_id: i32,
        title: Option<&str>,
        icon_emoji_id: Option<i64>,
        closed: Option<bool>,
        hidden: Option<bool>,
    ) -> Result<(), TelegramError> {
        let mut state = self.state.lock().await;
        let canonical_id = Self::resolve_dialog(&state.dialogs, chat_id)
            .ok_or_else(|| TelegramError::PeerNotFound(chat_id.to_string()))?;
        let topic = state
            .forum_topics
            .get_mut(&canonical_id)
            .and_then(|topics| topics.iter_mut().find(|topic| topic.topic_id == topic_id))
            .ok_or_else(|| TelegramError::MessageNotFound(format!("{chat_id}:{topic_id}")))?;
        if topic.deleted {
            return Err(TelegramError::MessageNotFound(format!("{chat_id}:{topic_id}")));
        }
        if let Some(title) = title {
            topic.title = Some(title.to_string());
        }
        if let Some(icon_emoji_id) = icon_emoji_id {
            topic.icon_emoji_id = (icon_emoji_id != 0).then_some(icon_emoji_id);
        }
        if let Some(closed) = closed {
            topic.closed = closed;
        }
        if let Some(hidden) = hidden {
            topic.hidden = hidden;
        }
        state.forum_topic_edit_calls = state.forum_topic_edit_calls.saturating_add(1);
        Ok(())
    }

    async fn set_forum_topic_pinned(
        &self,
        _session_path: Option<PathBuf>,
        chat_id: &str,
        topic_id: i32,
        pinned: bool,
    ) -> Result<(), TelegramError> {
        let mut state = self.state.lock().await;
        let canonical_id = Self::resolve_dialog(&state.dialogs, chat_id)
            .ok_or_else(|| TelegramError::PeerNotFound(chat_id.to_string()))?;
        let topic = state
            .forum_topics
            .get_mut(&canonical_id)
            .and_then(|topics| topics.iter_mut().find(|topic| topic.topic_id == topic_id))
            .ok_or_else(|| TelegramError::MessageNotFound(format!("{chat_id}:{topic_id}")))?;
        if topic.deleted {
            return Err(TelegramError::MessageNotFound(format!("{chat_id}:{topic_id}")));
        }
        topic.pinned = pinned;
        state.forum_topic_pin_calls = state.forum_topic_pin_calls.saturating_add(1);
        Ok(())
    }

    async fn delete_forum_topic(
        &self,
        _session_path: Option<PathBuf>,
        chat_id: &str,
        topic_id: i32,
    ) -> Result<(), TelegramError> {
        let mut state = self.state.lock().await;
        let canonical_id = Self::resolve_dialog(&state.dialogs, chat_id)
            .ok_or_else(|| TelegramError::PeerNotFound(chat_id.to_string()))?;
        let topic = state
            .forum_topics
            .get_mut(&canonical_id)
            .and_then(|topics| topics.iter_mut().find(|topic| topic.topic_id == topic_id))
            .ok_or_else(|| TelegramError::MessageNotFound(format!("{chat_id}:{topic_id}")))?;
        topic.deleted = true;
        topic.title = None;
        topic.creator_ref = None;
        topic.my = false;
        topic.closed = false;
        topic.pinned = false;
        topic.hidden = false;
        topic.icon_color = None;
        topic.icon_emoji_id = None;
        topic.top_message_id = None;
        topic.notification_settings = None;
        topic.draft = None;
        state.forum_topic_delete_calls = state.forum_topic_delete_calls.saturating_add(1);
        Ok(())
    }

    async fn get_privacy_rules(
        &self,
        _session_path: Option<PathBuf>,
        key: PrivacyKey,
    ) -> Result<Vec<PrivacyRuleSummary>, TelegramError> {
        let state = self.state.lock().await;
        Ok(state.privacy_rules.get(&key).cloned().unwrap_or_else(|| {
            vec![PrivacyRuleSummary {
                kind: "allow_contacts".to_string(),
                user_ids: Vec::new(),
                chat_ids: Vec::new(),
            }]
        }))
    }

    async fn set_privacy_rules(
        &self,
        _session_path: Option<PathBuf>,
        key: PrivacyKey,
        rules: &[PrivacyRuleSummary],
    ) -> Result<Vec<PrivacyRuleSummary>, TelegramError> {
        let mut state = self.state.lock().await;
        state.privacy_calls += 1;
        state.privacy_rules.insert(key, rules.to_vec());
        Ok(rules.to_vec())
    }

    async fn get_notification_settings(
        &self,
        _session_path: Option<PathBuf>,
        chat_id: &str,
    ) -> Result<NotificationSettingsSummary, TelegramError> {
        let state = self.state.lock().await;
        let canonical_id = Self::resolve_dialog(&state.dialogs, chat_id)
            .ok_or_else(|| TelegramError::PeerNotFound(chat_id.to_string()))?;
        Ok(state
            .notification_settings
            .get(&canonical_id)
            .cloned()
            .unwrap_or_else(default_notification_settings))
    }

    async fn set_notification_settings(
        &self,
        _session_path: Option<PathBuf>,
        chat_id: &str,
        patch: &NotificationSettingsPatch,
    ) -> Result<(), TelegramError> {
        let mut state = self.state.lock().await;
        let canonical_id = Self::resolve_dialog(&state.dialogs, chat_id)
            .ok_or_else(|| TelegramError::PeerNotFound(chat_id.to_string()))?;
        state.notification_calls += 1;
        let settings = state
            .notification_settings
            .entry(canonical_id)
            .or_insert_with(default_notification_settings);
        if let Some(value) = patch.show_previews {
            settings.show_previews = Some(value);
        }
        if let Some(value) = patch.silent {
            settings.silent = Some(value);
        }
        if let Some(value) = patch.mute_until_unix {
            settings.mute_until_unix = Some(value);
        }
        if let Some(sound) = &patch.sound {
            settings.ios_sound = Some(sound.clone());
            settings.android_sound = Some(sound.clone());
            settings.other_sound = Some(sound.clone());
        }
        if let Some(value) = patch.stories_muted {
            settings.stories_muted = Some(value);
        }
        if let Some(value) = patch.stories_hide_sender {
            settings.stories_hide_sender = Some(value);
        }
        if let Some(sound) = &patch.stories_sound {
            settings.stories_ios_sound = Some(sound.clone());
            settings.stories_android_sound = Some(sound.clone());
            settings.stories_other_sound = Some(sound.clone());
        }
        Ok(())
    }

    async fn list_sticker_sets(
        &self,
        _session_path: Option<PathBuf>,
        hash: i64,
    ) -> Result<Option<Vec<StickerSetSummary>>, TelegramError> {
        let state = self.state.lock().await;
        if !state.sticker_set_installed {
            return Ok(Some(Vec::new()));
        }
        if hash == 4242 {
            return Ok(None);
        }
        let mut sticker_set = fixture_sticker_set_summary();
        sticker_set.archived = state.sticker_set_archived;
        Ok(Some(vec![sticker_set]))
    }

    async fn get_recent_stickers(
        &self,
        _session_path: Option<PathBuf>,
        attached: bool,
        limit: usize,
        hash: i64,
    ) -> Result<StickerCatalogPage, TelegramError> {
        let state = self.state.lock().await;
        let saved = if attached { &state.attached_recent_stickers } else { &state.recent_stickers };
        let all = fixture_stickers();
        let catalog_hash = all.iter().enumerate().fold(5_151_i64, |hash, (index, sticker)| {
            if saved.contains(&sticker.sticker_handle) {
                hash
            } else {
                hash.wrapping_add(i64::try_from(index + 1).unwrap_or(i64::MAX))
            }
        });
        if hash == catalog_hash {
            return Ok(StickerCatalogPage {
                not_modified: true,
                hash: Some(catalog_hash),
                attached,
                total_count: 0,
                returned_count: 0,
                has_more: false,
                stickers: Vec::new(),
            });
        }
        let mut stickers = all
            .into_iter()
            .enumerate()
            .filter_map(|(index, mut sticker)| {
                saved.contains(&sticker.sticker_handle).then(|| {
                    sticker.recent_date_unix =
                        Some(1_700_000_301 - i32::try_from(index).unwrap_or(0));
                    sticker
                })
            })
            .collect::<Vec<_>>();
        let total_count = stickers.len();
        stickers.truncate(limit);
        Ok(StickerCatalogPage {
            not_modified: false,
            hash: Some(catalog_hash),
            attached,
            total_count,
            returned_count: stickers.len(),
            has_more: stickers.len() < total_count,
            stickers,
        })
    }

    async fn get_sticker_set(
        &self,
        _session_path: Option<PathBuf>,
        short_name: &str,
        hash: i32,
    ) -> Result<StickerSetDetailsPage, TelegramError> {
        if short_name != "fixture_reactions" {
            return Err(TelegramError::PeerNotFound(format!("sticker set {short_name}")));
        }
        if hash == 4242 {
            return Ok(StickerSetDetailsPage {
                not_modified: true,
                sticker_set: None,
                packs: Vec::new(),
                keywords: Vec::new(),
                stickers: Vec::new(),
            });
        }
        let state = self.state.lock().await;
        let mut details = fixture_sticker_set_details();
        let sticker_set = details.sticker_set.as_mut().expect("fixture sticker set");
        sticker_set.installed_date_unix = state.sticker_set_installed.then_some(1_700_000_000);
        sticker_set.archived = state.sticker_set_installed && state.sticker_set_archived;
        Ok(details)
    }

    async fn search_sticker_sets(
        &self,
        _session_path: Option<PathBuf>,
        query: &str,
        _exclude_featured: bool,
        limit: usize,
        hash: i64,
    ) -> Result<StickerSetSearchPage, TelegramError> {
        const SEARCH_HASH: i64 = 6_262;
        if hash == SEARCH_HASH {
            return Ok(StickerSetSearchPage {
                not_modified: true,
                hash: Some(SEARCH_HASH),
                total_count: 0,
                returned_count: 0,
                has_more: false,
                sticker_sets: Vec::new(),
            });
        }
        let normalized = query.trim().to_ascii_lowercase();
        let mut sticker_sets = ["fixture", "reaction", "release", "fire"]
            .iter()
            .any(|needle| normalized.contains(needle))
            .then(fixture_sticker_set_summary)
            .into_iter()
            .collect::<Vec<_>>();
        let total_count = sticker_sets.len();
        sticker_sets.truncate(limit);
        Ok(StickerSetSearchPage {
            not_modified: false,
            hash: Some(SEARCH_HASH),
            total_count,
            returned_count: sticker_sets.len(),
            has_more: sticker_sets.len() < total_count,
            sticker_sets,
        })
    }

    async fn get_favorite_stickers(
        &self,
        _session_path: Option<PathBuf>,
        limit: usize,
        hash: i64,
    ) -> Result<StickerCatalogPage, TelegramError> {
        let state = self.state.lock().await;
        let all = fixture_stickers();
        let catalog_hash = all.iter().enumerate().fold(7_171_i64, |value, (index, sticker)| {
            if state.favorite_stickers.contains(&sticker.sticker_handle) {
                value.wrapping_mul(31).wrapping_add(i64::try_from(index + 1).unwrap_or(i64::MAX))
            } else {
                value
            }
        });
        if hash == catalog_hash {
            return Ok(StickerCatalogPage {
                not_modified: true,
                hash: Some(catalog_hash),
                attached: false,
                total_count: 0,
                returned_count: 0,
                has_more: false,
                stickers: Vec::new(),
            });
        }
        let mut stickers = all
            .into_iter()
            .filter(|sticker| state.favorite_stickers.contains(&sticker.sticker_handle))
            .collect::<Vec<_>>();
        let total_count = stickers.len();
        stickers.truncate(limit);
        Ok(StickerCatalogPage {
            not_modified: false,
            hash: Some(catalog_hash),
            attached: false,
            total_count,
            returned_count: stickers.len(),
            has_more: stickers.len() < total_count,
            stickers,
        })
    }

    async fn set_sticker_favorite(
        &self,
        _session_path: Option<PathBuf>,
        set_short_name: &str,
        sticker_handle: &str,
        favorite: bool,
    ) -> Result<(), TelegramError> {
        if set_short_name != "fixture_reactions"
            || !fixture_stickers().iter().any(|sticker| sticker.sticker_handle == sticker_handle)
        {
            return Err(TelegramError::PeerNotFound(format!(
                "sticker {set_short_name}:{sticker_handle}"
            )));
        }
        let mut state = self.state.lock().await;
        state.sticker_favorite_calls = state.sticker_favorite_calls.saturating_add(1);
        if favorite {
            state.favorite_stickers.insert(sticker_handle.to_string());
        } else {
            state.favorite_stickers.remove(sticker_handle);
        }
        if state.sticker_favorite_uncertain_once {
            state.sticker_favorite_uncertain_once = false;
            return Err(TelegramError::DeliveryUncertain(
                "fixture updated sticker favourite before the transport receipt was lost"
                    .to_string(),
            ));
        }
        Ok(())
    }

    async fn set_recent_sticker(
        &self,
        _session_path: Option<PathBuf>,
        set_short_name: &str,
        sticker_handle: &str,
        attached: bool,
        saved: bool,
    ) -> Result<(), TelegramError> {
        if set_short_name != "fixture_reactions"
            || !fixture_stickers().iter().any(|sticker| sticker.sticker_handle == sticker_handle)
        {
            return Err(TelegramError::PeerNotFound(format!(
                "sticker {set_short_name}:{sticker_handle}"
            )));
        }
        let mut state = self.state.lock().await;
        state.recent_sticker_calls = state.recent_sticker_calls.saturating_add(1);
        let recent =
            if attached { &mut state.attached_recent_stickers } else { &mut state.recent_stickers };
        if saved {
            recent.insert(sticker_handle.to_string());
        } else {
            recent.remove(sticker_handle);
        }
        if state.recent_sticker_uncertain_once {
            state.recent_sticker_uncertain_once = false;
            return Err(TelegramError::DeliveryUncertain(
                "fixture updated the recent-sticker list before the transport receipt was lost"
                    .to_string(),
            ));
        }
        Ok(())
    }

    async fn install_sticker_set(
        &self,
        _session_path: Option<PathBuf>,
        short_name: &str,
        archived: bool,
    ) -> Result<(), TelegramError> {
        if short_name != "fixture_reactions" {
            return Err(TelegramError::PeerNotFound(format!("sticker set {short_name}")));
        }
        let mut state = self.state.lock().await;
        state.sticker_install_calls = state.sticker_install_calls.saturating_add(1);
        state.sticker_set_installed = true;
        state.sticker_set_archived = archived;
        if state.sticker_install_uncertain_once {
            state.sticker_install_uncertain_once = false;
            return Err(TelegramError::DeliveryUncertain(
                "fixture installed sticker set before the transport receipt was lost".to_string(),
            ));
        }
        Ok(())
    }

    async fn uninstall_sticker_set(
        &self,
        _session_path: Option<PathBuf>,
        short_name: &str,
    ) -> Result<(), TelegramError> {
        if short_name != "fixture_reactions" {
            return Err(TelegramError::PeerNotFound(format!("sticker set {short_name}")));
        }
        let mut state = self.state.lock().await;
        state.sticker_uninstall_calls = state.sticker_uninstall_calls.saturating_add(1);
        state.sticker_set_installed = false;
        state.sticker_set_archived = false;
        Ok(())
    }

    async fn get_custom_emoji_documents(
        &self,
        _session_path: Option<PathBuf>,
        document_ids: &[i64],
    ) -> Result<Vec<CustomEmojiDocumentSummary>, TelegramError> {
        let size_hint = self.state.lock().await.custom_emoji_size_hint;
        Ok(document_ids
            .iter()
            .filter_map(|document_id| {
                let mut document = fixture_custom_emoji(*document_id)?;
                if let Some(size_bytes) = size_hint {
                    document.sticker.size_bytes = size_bytes;
                }
                Some(document)
            })
            .collect())
    }

    async fn download_custom_emoji_to(
        &self,
        _session_path: Option<PathBuf>,
        document_id: i64,
        destination: &std::path::Path,
        max_bytes: u64,
    ) -> Result<u64, TelegramError> {
        let bytes = fixture_custom_emoji_bytes(document_id).ok_or_else(|| {
            TelegramError::AttachmentNotFound(format!("custom emoji {document_id}"))
        })?;
        let byte_count = u64::try_from(bytes.len()).unwrap_or(u64::MAX);
        let mut file = tokio::fs::File::create(destination).await?;
        restrict_private_file(destination)?;
        if byte_count > max_bytes {
            let allowed = usize::try_from(max_bytes).unwrap_or(usize::MAX).min(bytes.len());
            let partial = bytes.get(..allowed).ok_or_else(|| {
                TelegramError::Io(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "fixture custom emoji limit exceeded its source buffer",
                ))
            })?;
            file.write_all(partial).await?;
            file.flush().await?;
            file.sync_all().await?;
            return Err(TelegramError::AttachmentTooLarge { max_bytes });
        }
        file.write_all(bytes).await?;
        file.flush().await?;
        file.sync_all().await?;
        Ok(byte_count)
    }

    async fn get_dice_emojis(
        &self,
        _session_path: Option<PathBuf>,
        hash: i32,
    ) -> Result<Option<DiceEmojiCatalog>, TelegramError> {
        const DICE_HASH: i32 = 12_345;
        if hash == DICE_HASH {
            return Ok(None);
        }
        Ok(Some(DiceEmojiCatalog {
            hash: DICE_HASH,
            emojis: ["🎲", "🎯", "🏀", "⚽", "🎳", "🎰"].into_iter().map(str::to_string).collect(),
        }))
    }

    async fn send_rich_media_with_random_id(
        &self,
        _session_path: Option<PathBuf>,
        chat_id: &str,
        media: &RichMediaSpec,
        caption: &str,
        reply_to_message_id: Option<i32>,
        topic_id: Option<i32>,
        silent: bool,
        noforwards: bool,
        random_id: i64,
    ) -> Result<i32, TelegramError> {
        let mut state = self.state.lock().await;
        if let Some(message_id) = state.sent_by_random_id.get(&random_id) {
            return Ok(*message_id);
        }
        let canonical_id = Self::resolve_dialog(&state.dialogs, chat_id)
            .ok_or_else(|| TelegramError::PeerNotFound(chat_id.to_string()))?;
        if let Some(reply_to_message_id) = reply_to_message_id
            && !state.messages.get(&canonical_id).is_some_and(|messages| {
                messages.iter().any(|message| message.message_id == reply_to_message_id)
            })
        {
            return Err(TelegramError::MessageNotFound(format!("{chat_id}:{reply_to_message_id}")));
        }
        state.rich_media_send_calls = state.rich_media_send_calls.saturating_add(1);
        let effective_reply_to_message_id = reply_to_message_id.or(topic_id);
        let message_id = state.next_message_id;
        state.next_message_id = state.next_message_id.saturating_add(1);
        state.sent_by_random_id.insert(random_id, message_id);
        state.sent.push(FakeSentMessage {
            chat_id: canonical_id.to_string(),
            text: caption.to_string(),
            message_id,
            file_name: None,
            reply_to_message_id: effective_reply_to_message_id,
            file_mode: None,
        });
        let summary = SentRichMediaSummary {
            media: media.clone(),
            dice_value: matches!(media, RichMediaSpec::Dice { .. }).then_some(4),
        };
        state.rich_media.insert((canonical_id, message_id), summary);
        if let Some(topic_id) = topic_id {
            state.message_topics.insert((canonical_id, message_id), topic_id);
        }
        let account_user_id = state.account.user_id;
        let chat_ref = Self::canonical_chat_ref(&state.dialogs, canonical_id);
        let media_type = match media {
            RichMediaSpec::Location { .. } => "geo",
            RichMediaSpec::Venue { .. } => "venue",
            RichMediaSpec::Contact { .. } => "contact",
            RichMediaSpec::Dice { .. } => "dice",
        };
        state.messages.entry(canonical_id).or_default().insert(
            0,
            MessageSummary {
                message_id,
                chat_ref: Some(chat_ref),
                sender_ref: Some(account_user_id.to_string()),
                outgoing: true,
                pinned: false,
                silent,
                noforwards,
                text: Some(caption.to_string()),
                date: 1_700_000_000_i32.saturating_add(message_id),
                edit_date: None,
                reply_to_message_id: effective_reply_to_message_id,
                views: None,
                forwards: None,
                reply_count: None,
                media_type: Some(media_type.to_string()),
                forward_origin: None,
            },
        );
        if state.rich_media_send_uncertain_once {
            state.rich_media_send_uncertain_once = false;
            return Err(TelegramError::DeliveryUncertain(
                "fixture sent rich media before the transport receipt was lost".to_string(),
            ));
        }
        Ok(message_id)
    }

    async fn get_message_rich_media(
        &self,
        _session_path: Option<PathBuf>,
        chat_id: &str,
        message_id: i32,
    ) -> Result<Option<SentRichMediaSummary>, TelegramError> {
        let state = self.state.lock().await;
        let canonical_id = Self::resolve_dialog(&state.dialogs, chat_id)
            .ok_or_else(|| TelegramError::PeerNotFound(chat_id.to_string()))?;
        Ok(state.rich_media.get(&(canonical_id, message_id)).cloned())
    }

    async fn list_message_reactions(
        &self,
        _session_path: Option<PathBuf>,
        chat_id: &str,
        message_id: i32,
        limit: usize,
        offset: Option<&str>,
    ) -> Result<MessageReactionPage, TelegramError> {
        let state = self.state.lock().await;
        let canonical_id = Self::resolve_dialog(&state.dialogs, chat_id)
            .ok_or_else(|| TelegramError::PeerNotFound(chat_id.to_string()))?;
        if !state
            .messages
            .get(&canonical_id)
            .is_some_and(|messages| messages.iter().any(|message| message.message_id == message_id))
        {
            return Err(TelegramError::MessageNotFound(format!("{chat_id}:{message_id}")));
        }
        let mut all_reactions = vec![MessageReactionSummary {
            peer_ref: "101".to_string(),
            date_unix: 1_700_000_031,
            big: false,
            unread: false,
            my: false,
            kind: "emoji".to_string(),
            emoji: Some("👍".to_string()),
            custom_emoji_id: None,
        }];
        if let Some(reaction) = state.my_reactions.get(&(canonical_id, message_id)) {
            let (kind, emoji, custom_emoji_id, big) = match reaction {
                ReactionSpec::Remove => unreachable!("removed reactions are not stored"),
                ReactionSpec::Emoji { emoji, big, .. } => {
                    ("emoji", Some(emoji.clone()), None, *big)
                },
                ReactionSpec::CustomEmoji { document_id, big, .. } => {
                    ("custom_emoji", None, Some(*document_id), *big)
                },
            };
            all_reactions.push(MessageReactionSummary {
                peer_ref: state.account.user_id.to_string(),
                date_unix: 1_700_000_032,
                big,
                unread: false,
                my: true,
                kind: kind.to_string(),
                emoji,
                custom_emoji_id,
            });
        }
        let total_count = i32::try_from(all_reactions.len()).unwrap_or(i32::MAX);
        let reactions = if offset.is_some() || limit == 0 {
            Vec::new()
        } else {
            all_reactions.into_iter().take(limit).collect()
        };
        Ok(MessageReactionPage {
            total_count,
            next_offset: (reactions.len() < usize::try_from(total_count).unwrap_or(usize::MAX))
                .then(|| "fixture-next".to_string()),
            reactions,
            counts: Vec::new(),
            viewer_list_forbidden: None,
        })
    }

    async fn get_default_reaction(
        &self,
        _session_path: Option<PathBuf>,
    ) -> Result<DefaultReactionSummary, TelegramError> {
        Ok(DefaultReactionSummary { reaction: self.state.lock().await.default_reaction.clone() })
    }

    async fn get_top_reactions(
        &self,
        _session_path: Option<PathBuf>,
        limit: usize,
        hash: i64,
    ) -> Result<ReactionCatalogPage, TelegramError> {
        const FIXTURE_HASH: i64 = 11_001;
        if hash == FIXTURE_HASH {
            return Ok(ReactionCatalogPage {
                not_modified: true,
                hash: None,
                reactions: Vec::new(),
            });
        }
        let reactions = vec![
            ReactionIdentity::Emoji { emoji: "👍".to_string() },
            ReactionIdentity::Emoji { emoji: "❤️".to_string() },
            ReactionIdentity::Paid,
            ReactionIdentity::CustomEmoji { document_id: 7_001 },
        ]
        .into_iter()
        .take(limit)
        .collect();
        Ok(ReactionCatalogPage { not_modified: false, hash: Some(FIXTURE_HASH), reactions })
    }

    async fn get_recent_reactions(
        &self,
        _session_path: Option<PathBuf>,
        limit: usize,
        hash: i64,
    ) -> Result<ReactionCatalogPage, TelegramError> {
        const FIXTURE_HASH: i64 = 11_002;
        if hash == FIXTURE_HASH {
            return Ok(ReactionCatalogPage {
                not_modified: true,
                hash: None,
                reactions: Vec::new(),
            });
        }
        let reactions = vec![
            ReactionIdentity::CustomEmoji { document_id: 8_001 },
            ReactionIdentity::Emoji { emoji: "🔥".to_string() },
            ReactionIdentity::Emoji { emoji: "🎉".to_string() },
        ]
        .into_iter()
        .take(limit)
        .collect();
        Ok(ReactionCatalogPage { not_modified: false, hash: Some(FIXTURE_HASH), reactions })
    }

    async fn set_default_reaction(
        &self,
        _session_path: Option<PathBuf>,
        reaction: &DefaultReactionSpec,
    ) -> Result<(), TelegramError> {
        let mut state = self.state.lock().await;
        state.default_reaction_calls = state.default_reaction_calls.saturating_add(1);
        state.default_reaction = Some(match reaction {
            DefaultReactionSpec::Emoji { emoji } => {
                ReactionIdentity::Emoji { emoji: emoji.clone() }
            },
            DefaultReactionSpec::CustomEmoji { document_id } => {
                ReactionIdentity::CustomEmoji { document_id: *document_id }
            },
        });
        if state.default_reaction_uncertain_once {
            state.default_reaction_uncertain_once = false;
            return Err(TelegramError::DeliveryUncertain(
                "fixture lost the set-default-reaction receipt after applying the state"
                    .to_string(),
            ));
        }
        Ok(())
    }

    async fn get_my_reaction(
        &self,
        _session_path: Option<PathBuf>,
        chat_id: &str,
        message_id: i32,
    ) -> Result<Option<ReactionSpec>, TelegramError> {
        let state = self.state.lock().await;
        let canonical_id = Self::resolve_dialog(&state.dialogs, chat_id)
            .ok_or_else(|| TelegramError::PeerNotFound(chat_id.to_string()))?;
        if !state
            .messages
            .get(&canonical_id)
            .is_some_and(|messages| messages.iter().any(|message| message.message_id == message_id))
        {
            return Err(TelegramError::MessageNotFound(format!("{chat_id}:{message_id}")));
        }
        Ok(state.my_reactions.get(&(canonical_id, message_id)).cloned())
    }

    async fn set_message_reaction(
        &self,
        _session_path: Option<PathBuf>,
        chat_id: &str,
        message_id: i32,
        reaction: &ReactionSpec,
    ) -> Result<(), TelegramError> {
        let mut state = self.state.lock().await;
        let canonical_id = Self::resolve_dialog(&state.dialogs, chat_id)
            .ok_or_else(|| TelegramError::PeerNotFound(chat_id.to_string()))?;
        if !state
            .messages
            .get(&canonical_id)
            .is_some_and(|messages| messages.iter().any(|message| message.message_id == message_id))
        {
            return Err(TelegramError::MessageNotFound(format!("{chat_id}:{message_id}")));
        }
        state.reaction_calls = state.reaction_calls.saturating_add(1);
        match reaction {
            ReactionSpec::Remove => {
                state.my_reactions.remove(&(canonical_id, message_id));
            },
            reaction => {
                state.my_reactions.insert((canonical_id, message_id), reaction.clone());
            },
        }
        Ok(())
    }

    async fn edit_message(
        &self,
        _session_path: Option<PathBuf>,
        chat_id: &str,
        message_id: i32,
        text: &str,
        parse_mode: CaptionParseMode,
    ) -> Result<i32, TelegramError> {
        let mut state = self.state.lock().await;
        state.edit_calls = state.edit_calls.saturating_add(1);
        let canonical_id = Self::resolve_dialog(&state.dialogs, chat_id)
            .ok_or_else(|| TelegramError::PeerNotFound(chat_id.to_string()))?;
        let parsed = match parse_mode {
            CaptionParseMode::Plain => ferogram::InputMessage::text(text),
            CaptionParseMode::Markdown => ferogram::InputMessage::markdown(text),
            CaptionParseMode::Html => ferogram::InputMessage::html(text),
            CaptionParseMode::RichMarkdown => {
                return Err(TelegramError::Telegram(
                    "rich_markdown parse mode is not supported for message edits".to_string(),
                ));
            },
        };
        let html = ferogram::parsers::generate_html(
            &parsed.text,
            parsed.entities.as_deref().unwrap_or_default(),
        );
        let message = state
            .messages
            .get_mut(&canonical_id)
            .and_then(|messages| {
                messages.iter_mut().find(|message| message.message_id == message_id)
            })
            .ok_or_else(|| TelegramError::MessageNotFound(format!("{chat_id}:{message_id}")))?;
        message.text = Some(parsed.text);
        message.edit_date = Some(1_700_001_000_i32.saturating_add(message_id));
        state.message_html.insert((canonical_id, message_id), html);
        Ok(message_id)
    }

    async fn delete_messages(
        &self,
        _session_path: Option<PathBuf>,
        chat_id: &str,
        message_ids: &[i32],
        _revoke: bool,
    ) -> Result<(), TelegramError> {
        let mut state = self.state.lock().await;
        state.delete_calls = state.delete_calls.saturating_add(1);
        let canonical_id = Self::resolve_dialog(&state.dialogs, chat_id)
            .ok_or_else(|| TelegramError::PeerNotFound(chat_id.to_string()))?;
        let messages = state
            .messages
            .get_mut(&canonical_id)
            .ok_or_else(|| TelegramError::PeerNotFound(chat_id.to_string()))?;
        messages.retain(|message| !message_ids.contains(&message.message_id));
        Ok(())
    }

    async fn set_message_pinned(
        &self,
        _session_path: Option<PathBuf>,
        chat_id: &str,
        message_id: i32,
        pinned: bool,
        _silent: bool,
        _one_sided: bool,
    ) -> Result<(), TelegramError> {
        let mut state = self.state.lock().await;
        state.pin_calls = state.pin_calls.saturating_add(1);
        let canonical_id = Self::resolve_dialog(&state.dialogs, chat_id)
            .ok_or_else(|| TelegramError::PeerNotFound(chat_id.to_string()))?;
        let message = state
            .messages
            .get_mut(&canonical_id)
            .and_then(|messages| {
                messages.iter_mut().find(|message| message.message_id == message_id)
            })
            .ok_or_else(|| TelegramError::MessageNotFound(format!("{chat_id}:{message_id}")))?;
        message.pinned = pinned;
        if pinned {
            state.pinned_messages.insert(canonical_id, message_id);
        } else if state.pinned_messages.get(&canonical_id) == Some(&message_id) {
            state.pinned_messages.remove(&canonical_id);
        }
        Ok(())
    }

    async fn forward_messages_with_random_ids(
        &self,
        _session_path: Option<PathBuf>,
        source_chat_id: &str,
        message_ids: &[i32],
        destination_chat_id: &str,
        _silent: bool,
        _drop_author: bool,
        drop_media_captions: bool,
        _noforwards: bool,
        random_ids: &[i64],
    ) -> Result<Vec<i32>, TelegramError> {
        if message_ids.is_empty() || message_ids.len() != random_ids.len() {
            return Err(TelegramError::InvalidPeer(
                "forward message ids and random ids must be non-empty and have equal length"
                    .to_string(),
            ));
        }
        let mut state = self.state.lock().await;
        state.forward_calls = state.forward_calls.saturating_add(1);
        let source_id = Self::resolve_dialog(&state.dialogs, source_chat_id)
            .ok_or_else(|| TelegramError::PeerNotFound(source_chat_id.to_string()))?;
        let destination_id = Self::resolve_dialog(&state.dialogs, destination_chat_id)
            .ok_or_else(|| TelegramError::PeerNotFound(destination_chat_id.to_string()))?;
        let source_messages = message_ids
            .iter()
            .map(|message_id| {
                state
                    .messages
                    .get(&source_id)
                    .and_then(|messages| {
                        messages.iter().find(|message| message.message_id == *message_id)
                    })
                    .cloned()
                    .ok_or_else(|| {
                        TelegramError::MessageNotFound(format!("{source_chat_id}:{message_id}"))
                    })
            })
            .collect::<Result<Vec<_>, _>>()?;
        let account_user_id = state.account.user_id;
        let destination_ref = Self::canonical_chat_ref(&state.dialogs, destination_id);
        let mut destination_message_ids = Vec::with_capacity(message_ids.len());
        for (source, random_id) in source_messages.into_iter().zip(random_ids.iter().copied()) {
            if let Some(message_id) = state.sent_by_random_id.get(&random_id).copied() {
                destination_message_ids.push(message_id);
                continue;
            }
            let message_id = state.next_message_id;
            state.next_message_id = state.next_message_id.saturating_add(1);
            state.sent_by_random_id.insert(random_id, message_id);
            let text = if drop_media_captions && source.media_type.is_some() {
                None
            } else {
                source.text.clone()
            };
            state.sent.push(FakeSentMessage {
                chat_id: destination_id.to_string(),
                text: text.clone().unwrap_or_default(),
                message_id,
                file_name: None,
                reply_to_message_id: None,
                file_mode: None,
            });
            state.messages.entry(destination_id).or_default().insert(
                0,
                MessageSummary {
                    message_id,
                    chat_ref: Some(destination_ref.clone()),
                    sender_ref: Some(account_user_id.to_string()),
                    outgoing: true,
                    pinned: false,
                    silent: _silent,
                    noforwards: _noforwards,
                    text,
                    date: 1_700_000_000_i32.saturating_add(message_id),
                    edit_date: None,
                    reply_to_message_id: None,
                    views: None,
                    forwards: None,
                    reply_count: None,
                    media_type: source.media_type,
                    forward_origin: if _drop_author {
                        None
                    } else {
                        Some(MessageForwardOrigin {
                            from_ref: source.sender_ref,
                            source_message_id: Some(source.message_id),
                            saved_from_ref: None,
                            saved_from_message_id: None,
                            date: source.date,
                        })
                    },
                },
            );
            destination_message_ids.push(message_id);
        }
        Ok(destination_message_ids)
    }

    async fn list_attachments(
        &self,
        _session_path: Option<PathBuf>,
        chat_id: &str,
        message_id: i32,
    ) -> Result<Vec<AttachmentDescriptor>, TelegramError> {
        let state = self.state.lock().await;
        let canonical_id = Self::resolve_dialog(&state.dialogs, chat_id)
            .ok_or_else(|| TelegramError::PeerNotFound(chat_id.to_string()))?;
        Ok(state
            .attachments
            .get(&(canonical_id, message_id))
            .map(|(descriptor, _)| vec![descriptor.clone()])
            .unwrap_or_default())
    }

    async fn download_attachment_to(
        &self,
        _session_path: Option<PathBuf>,
        chat_id: &str,
        message_id: i32,
        destination: &std::path::Path,
        max_bytes: u64,
    ) -> Result<u64, TelegramError> {
        let bytes = {
            let state = self.state.lock().await;
            let canonical_id = Self::resolve_dialog(&state.dialogs, chat_id)
                .ok_or_else(|| TelegramError::PeerNotFound(chat_id.to_string()))?;
            state
                .attachments
                .get(&(canonical_id, message_id))
                .map(|(_, bytes)| bytes.clone())
                .ok_or_else(|| {
                    TelegramError::AttachmentNotFound(format!("{chat_id}:{message_id}"))
                })?
        };
        let byte_count = u64::try_from(bytes.len()).unwrap_or(u64::MAX);
        let mut file = tokio::fs::File::create(destination).await?;
        restrict_private_file(destination)?;
        if byte_count > max_bytes {
            let allowed = usize::try_from(max_bytes).unwrap_or(usize::MAX).min(bytes.len());
            let partial = bytes.get(..allowed).ok_or_else(|| {
                TelegramError::Io(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "fixture download limit exceeded its source buffer",
                ))
            })?;
            file.write_all(partial).await?;
            file.flush().await?;
            file.sync_all().await?;
            return Err(TelegramError::AttachmentTooLarge { max_bytes });
        }
        file.write_all(&bytes).await?;
        file.flush().await?;
        file.sync_all().await?;
        Ok(byte_count)
    }

    async fn send_file(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        file_path: &std::path::Path,
        caption: &str,
        reply_to_message_id: Option<i32>,
        mode: FileSendMode,
    ) -> Result<i32, TelegramError> {
        let metadata = tokio::fs::metadata(file_path).await?;
        if !metadata.is_file() {
            return Err(TelegramError::Io(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "upload path is not a regular file",
            )));
        }
        let file_name =
            file_path.file_name().and_then(|value| value.to_str()).unwrap_or("attachment");
        let message_id = self.send_message(session_path, chat_id, caption).await?;
        let mut state = self.state.lock().await;
        let canonical_id = Self::resolve_dialog(&state.dialogs, chat_id)
            .ok_or_else(|| TelegramError::PeerNotFound(chat_id.to_string()))?;
        if let Some(sent) = state.sent.iter_mut().find(|sent| sent.message_id == message_id) {
            sent.file_name = Some(file_name.to_string());
            sent.reply_to_message_id = reply_to_message_id;
            sent.file_mode = Some(mode);
        }
        if let Some(messages) = state.messages.get_mut(&canonical_id)
            && let Some(message) =
                messages.iter_mut().find(|message| message.message_id == message_id)
        {
            message.reply_to_message_id = reply_to_message_id;
            message.media_type = Some(
                match mode {
                    FileSendMode::Photo => "photo",
                    FileSendMode::Gif => "gif",
                    FileSendMode::Auto | FileSendMode::Document => "document",
                }
                .to_string(),
            );
        }
        Ok(message_id)
    }

    async fn send_file_with_random_id(
        &self,
        _session_path: Option<PathBuf>,
        chat_id: &str,
        file_path: &std::path::Path,
        caption: &str,
        reply_to_message_id: Option<i32>,
        mode: FileSendMode,
        random_id: i64,
    ) -> Result<i32, TelegramError> {
        let metadata = tokio::fs::metadata(file_path).await?;
        if !metadata.is_file() {
            return Err(TelegramError::Io(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "upload path is not a regular file",
            )));
        }
        let file_name =
            file_path.file_name().and_then(|value| value.to_str()).unwrap_or("attachment");
        let mut state = self.state.lock().await;
        if let Some(message_id) = state.sent_by_random_id.get(&random_id) {
            return Ok(*message_id);
        }
        let canonical_id = Self::resolve_dialog(&state.dialogs, chat_id)
            .ok_or_else(|| TelegramError::PeerNotFound(chat_id.to_string()))?;
        let message_id = state.next_message_id;
        state.next_message_id = state.next_message_id.saturating_add(1);
        state.sent_by_random_id.insert(random_id, message_id);
        state.sent.push(FakeSentMessage {
            chat_id: canonical_id.to_string(),
            text: caption.to_string(),
            message_id,
            file_name: Some(file_name.to_string()),
            reply_to_message_id,
            file_mode: Some(mode),
        });
        let account_user_id = state.account.user_id;
        let chat_ref = Self::canonical_chat_ref(&state.dialogs, canonical_id);
        state.messages.entry(canonical_id).or_default().insert(
            0,
            MessageSummary {
                message_id,
                chat_ref: Some(chat_ref),
                sender_ref: Some(account_user_id.to_string()),
                outgoing: true,
                pinned: false,
                silent: false,
                noforwards: false,
                text: Some(caption.to_string()),
                date: 1_700_000_000_i32.saturating_add(message_id),
                edit_date: None,
                reply_to_message_id,
                views: None,
                forwards: None,
                reply_count: None,
                media_type: Some(
                    match mode {
                        FileSendMode::Photo => "photo",
                        FileSendMode::Gif => "gif",
                        FileSendMode::Auto | FileSendMode::Document => "document",
                    }
                    .to_string(),
                ),
                forward_origin: None,
            },
        );
        Ok(message_id)
    }

    async fn send_file_html_with_random_id(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        file_path: &std::path::Path,
        caption_html: &str,
        reply_to_message_id: Option<i32>,
        topic_id: Option<i32>,
        silent: bool,
        noforwards: bool,
        mode: FileSendMode,
        random_id: i64,
    ) -> Result<i32, TelegramError> {
        let caption = ferogram::InputMessage::html(caption_html).text;
        let effective_reply_to = reply_to_message_id.or(topic_id);
        let message_id = self
            .send_file_with_random_id(
                session_path,
                chat_id,
                file_path,
                &caption,
                effective_reply_to,
                mode,
                random_id,
            )
            .await?;
        let bytes = tokio::fs::read(file_path).await?;
        let file_name =
            file_path.file_name().and_then(|value| value.to_str()).unwrap_or("attachment.bin");
        let mime = ferogram::media::detect_mime_from_bytes(&bytes, file_name);
        let media_type = match mode {
            FileSendMode::Photo => "photo",
            FileSendMode::Gif => "gif",
            FileSendMode::Document => "document",
            FileSendMode::Auto if mime.starts_with("video/") => "video",
            FileSendMode::Auto => "document",
        };
        let mut state = self.state.lock().await;
        let canonical_id = Self::resolve_dialog(&state.dialogs, chat_id)
            .ok_or_else(|| TelegramError::PeerNotFound(chat_id.to_string()))?;
        if let Some(sent) = state.sent.iter_mut().find(|sent| sent.message_id == message_id) {
            sent.reply_to_message_id = effective_reply_to;
        }
        let message = state
            .messages
            .get_mut(&canonical_id)
            .and_then(|messages| {
                messages.iter_mut().find(|message| message.message_id == message_id)
            })
            .ok_or_else(|| TelegramError::MessageNotFound(format!("{chat_id}:{message_id}")))?;
        message.silent = silent;
        message.noforwards = noforwards;
        message.reply_to_message_id = effective_reply_to;
        message.media_type = Some(media_type.to_string());
        if let Some(topic_id) = topic_id {
            state.message_topics.insert((canonical_id, message_id), topic_id);
        }
        Ok(message_id)
    }

    async fn send_voice_with_random_id(
        &self,
        _session_path: Option<PathBuf>,
        chat_id: &str,
        file_path: &std::path::Path,
        caption: &str,
        caption_parse_mode: CaptionParseMode,
        reply_to_message_id: Option<i32>,
        topic_id: Option<i32>,
        silent: bool,
        noforwards: bool,
        random_id: i64,
    ) -> Result<i32, TelegramError> {
        if matches!(caption_parse_mode, CaptionParseMode::RichMarkdown) {
            return Err(TelegramError::InvalidPeer(
                "media does not support native RichMessage captions".to_string(),
            ));
        }
        let metadata = tokio::fs::metadata(file_path).await?;
        if !metadata.is_file() {
            return Err(TelegramError::Io(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "upload path is not a regular file",
            )));
        }
        let file_name =
            file_path.file_name().and_then(|value| value.to_str()).unwrap_or("voice.ogg");
        let rendered_caption = match caption_parse_mode {
            CaptionParseMode::Plain => caption.to_string(),
            CaptionParseMode::Markdown => ferogram::InputMessage::markdown(caption).text,
            CaptionParseMode::Html => ferogram::InputMessage::html(caption).text,
            CaptionParseMode::RichMarkdown => String::new(),
        };
        let mut state = self.state.lock().await;
        if let Some(message_id) = state.sent_by_random_id.get(&random_id) {
            return Ok(*message_id);
        }
        let canonical_id = Self::resolve_dialog(&state.dialogs, chat_id)
            .ok_or_else(|| TelegramError::PeerNotFound(chat_id.to_string()))?;
        if let Some(reply_to_message_id) = reply_to_message_id
            && !state.messages.get(&canonical_id).is_some_and(|messages| {
                messages.iter().any(|message| message.message_id == reply_to_message_id)
            })
        {
            return Err(TelegramError::MessageNotFound(format!("{chat_id}:{reply_to_message_id}")));
        }
        state.voice_send_calls = state.voice_send_calls.saturating_add(1);
        let effective_reply_to_message_id = reply_to_message_id.or(topic_id);
        let message_id = state.next_message_id;
        state.next_message_id = state.next_message_id.saturating_add(1);
        state.sent_by_random_id.insert(random_id, message_id);
        state.sent.push(FakeSentMessage {
            chat_id: canonical_id.to_string(),
            text: rendered_caption.clone(),
            message_id,
            file_name: Some(file_name.to_string()),
            reply_to_message_id: effective_reply_to_message_id,
            file_mode: None,
        });
        state.document_media.insert(
            (canonical_id, message_id),
            SentDocumentMediaSummary {
                media_type: "voice".to_string(),
                mime_type: "audio/ogg".to_string(),
                duration_seconds: Some(1),
                width_px: None,
                height_px: None,
            },
        );
        if let Some(topic_id) = topic_id {
            state.message_topics.insert((canonical_id, message_id), topic_id);
        }
        let account_user_id = state.account.user_id;
        let chat_ref = Self::canonical_chat_ref(&state.dialogs, canonical_id);
        state.messages.entry(canonical_id).or_default().insert(
            0,
            MessageSummary {
                message_id,
                chat_ref: Some(chat_ref),
                sender_ref: Some(account_user_id.to_string()),
                outgoing: true,
                pinned: false,
                silent,
                noforwards,
                text: Some(rendered_caption),
                date: 1_700_000_000_i32.saturating_add(message_id),
                edit_date: None,
                reply_to_message_id: effective_reply_to_message_id,
                views: None,
                forwards: None,
                reply_count: None,
                media_type: Some("voice".to_string()),
                forward_origin: None,
            },
        );
        if state.voice_send_uncertain_once {
            state.voice_send_uncertain_once = false;
            return Err(TelegramError::DeliveryUncertain(
                "fixture sent voice before the transport receipt was lost".to_string(),
            ));
        }
        Ok(message_id)
    }

    async fn send_video_note_with_random_id(
        &self,
        _session_path: Option<PathBuf>,
        chat_id: &str,
        file_path: &std::path::Path,
        duration_seconds: i32,
        length_pixels: i32,
        reply_to_message_id: Option<i32>,
        topic_id: Option<i32>,
        silent: bool,
        noforwards: bool,
        random_id: i64,
    ) -> Result<i32, TelegramError> {
        let metadata = tokio::fs::metadata(file_path).await?;
        if !metadata.is_file() {
            return Err(TelegramError::Io(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "upload path is not a regular file",
            )));
        }
        let file_name =
            file_path.file_name().and_then(|value| value.to_str()).unwrap_or("video-note.mp4");
        let mut state = self.state.lock().await;
        if let Some(message_id) = state.sent_by_random_id.get(&random_id) {
            return Ok(*message_id);
        }
        let canonical_id = Self::resolve_dialog(&state.dialogs, chat_id)
            .ok_or_else(|| TelegramError::PeerNotFound(chat_id.to_string()))?;
        if let Some(reply_to_message_id) = reply_to_message_id
            && !state.messages.get(&canonical_id).is_some_and(|messages| {
                messages.iter().any(|message| message.message_id == reply_to_message_id)
            })
        {
            return Err(TelegramError::MessageNotFound(format!("{chat_id}:{reply_to_message_id}")));
        }
        state.video_note_send_calls = state.video_note_send_calls.saturating_add(1);
        let effective_reply_to_message_id = reply_to_message_id.or(topic_id);
        let message_id = state.next_message_id;
        state.next_message_id = state.next_message_id.saturating_add(1);
        state.sent_by_random_id.insert(random_id, message_id);
        state.sent.push(FakeSentMessage {
            chat_id: canonical_id.to_string(),
            text: String::new(),
            message_id,
            file_name: Some(file_name.to_string()),
            reply_to_message_id: effective_reply_to_message_id,
            file_mode: None,
        });
        state.document_media.insert(
            (canonical_id, message_id),
            SentDocumentMediaSummary {
                media_type: "video_note".to_string(),
                mime_type: "video/mp4".to_string(),
                duration_seconds: Some(duration_seconds),
                width_px: Some(length_pixels),
                height_px: Some(length_pixels),
            },
        );
        if let Some(topic_id) = topic_id {
            state.message_topics.insert((canonical_id, message_id), topic_id);
        }
        let account_user_id = state.account.user_id;
        let chat_ref = Self::canonical_chat_ref(&state.dialogs, canonical_id);
        state.messages.entry(canonical_id).or_default().insert(
            0,
            MessageSummary {
                message_id,
                chat_ref: Some(chat_ref),
                sender_ref: Some(account_user_id.to_string()),
                outgoing: true,
                pinned: false,
                silent,
                noforwards,
                text: Some(String::new()),
                date: 1_700_000_000_i32.saturating_add(message_id),
                edit_date: None,
                reply_to_message_id: effective_reply_to_message_id,
                views: None,
                forwards: None,
                reply_count: None,
                media_type: Some("video_note".to_string()),
                forward_origin: None,
            },
        );
        if state.video_note_send_uncertain_once {
            state.video_note_send_uncertain_once = false;
            return Err(TelegramError::DeliveryUncertain(
                "fixture sent video note before the transport receipt was lost".to_string(),
            ));
        }
        Ok(message_id)
    }

    async fn get_message_document_media(
        &self,
        _session_path: Option<PathBuf>,
        chat_id: &str,
        message_id: i32,
    ) -> Result<Option<SentDocumentMediaSummary>, TelegramError> {
        let state = self.state.lock().await;
        let canonical_id = Self::resolve_dialog(&state.dialogs, chat_id)
            .ok_or_else(|| TelegramError::PeerNotFound(chat_id.to_string()))?;
        Ok(state.document_media.get(&(canonical_id, message_id)).cloned())
    }

    async fn get_message_topic_id(
        &self,
        _session_path: Option<PathBuf>,
        chat_id: &str,
        message_id: i32,
    ) -> Result<Option<i32>, TelegramError> {
        let state = self.state.lock().await;
        let canonical_id = Self::resolve_dialog(&state.dialogs, chat_id)
            .ok_or_else(|| TelegramError::PeerNotFound(chat_id.to_string()))?;
        Ok(state.message_topics.get(&(canonical_id, message_id)).copied())
    }

    async fn send_album_with_random_ids(
        &self,
        _session_path: Option<PathBuf>,
        chat_id: &str,
        items: &[AlbumUploadSpec],
        caption_parse_mode: CaptionParseMode,
        reply_to_message_id: Option<i32>,
        topic_id: Option<i32>,
        silent: bool,
        noforwards: bool,
        random_ids: &[i64],
    ) -> Result<Vec<i32>, TelegramError> {
        if matches!(caption_parse_mode, CaptionParseMode::RichMarkdown) {
            return Err(TelegramError::InvalidPeer(
                "album does not support native RichMessage captions".to_string(),
            ));
        }
        if !(2..=10).contains(&items.len()) || items.len() != random_ids.len() {
            return Err(TelegramError::InvalidPeer(
                "album requires 2-10 items and one random id per item".to_string(),
            ));
        }
        let mut item_payloads = Vec::with_capacity(items.len());
        for item in items {
            let metadata = tokio::fs::metadata(&item.file_path).await?;
            if !metadata.is_file() {
                return Err(TelegramError::Io(std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    "album upload path is not a regular file",
                )));
            }
            let bytes = tokio::fs::read(&item.file_path).await?;
            let file_name = item
                .file_path
                .file_name()
                .and_then(|value| value.to_str())
                .unwrap_or("album-item.bin")
                .to_string();
            let mime_type = ferogram::media::detect_mime_from_bytes(&bytes, &file_name).to_string();
            item_payloads.push((bytes, file_name, mime_type));
        }
        let mut state = self.state.lock().await;
        let canonical_id = Self::resolve_dialog(&state.dialogs, chat_id)
            .ok_or_else(|| TelegramError::PeerNotFound(chat_id.to_string()))?;
        if let Some(reply_to_message_id) = reply_to_message_id
            && !state.messages.get(&canonical_id).is_some_and(|messages| {
                messages.iter().any(|message| message.message_id == reply_to_message_id)
            })
        {
            return Err(TelegramError::MessageNotFound(format!("{chat_id}:{reply_to_message_id}")));
        }
        state.album_send_calls = state.album_send_calls.saturating_add(1);
        let existing_grouped_id = random_ids.iter().find_map(|random_id| {
            state
                .sent_by_random_id
                .get(random_id)
                .and_then(|message_id| state.message_grouped_ids.get(&(canonical_id, *message_id)))
                .copied()
        });
        let grouped_id = existing_grouped_id
            .unwrap_or_else(|| 900_000_i64.saturating_add(i64::from(state.next_message_id)));
        let effective_reply_to_message_id = reply_to_message_id.or(topic_id);
        let account_user_id = state.account.user_id;
        let chat_ref = Self::canonical_chat_ref(&state.dialogs, canonical_id);
        let mut message_ids = Vec::with_capacity(items.len());
        for ((item, random_id), (bytes, captured_file_name, mime_type)) in
            items.iter().zip(random_ids.iter().copied()).zip(item_payloads)
        {
            if let Some(message_id) = state.sent_by_random_id.get(&random_id).copied() {
                message_ids.push(message_id);
                continue;
            }
            let message_id = state.next_message_id;
            state.next_message_id = state.next_message_id.saturating_add(1);
            state.sent_by_random_id.insert(random_id, message_id);
            let file_name =
                item.file_path.file_name().and_then(|value| value.to_str()).unwrap_or("album-item");
            let rendered_caption = match caption_parse_mode {
                CaptionParseMode::Plain => item.caption.clone(),
                CaptionParseMode::Markdown => ferogram::InputMessage::markdown(&item.caption).text,
                CaptionParseMode::Html => ferogram::InputMessage::html(&item.caption).text,
                CaptionParseMode::RichMarkdown => String::new(),
            };
            state.sent.push(FakeSentMessage {
                chat_id: canonical_id.to_string(),
                text: rendered_caption.clone(),
                message_id,
                file_name: Some(file_name.to_string()),
                reply_to_message_id: effective_reply_to_message_id,
                file_mode: None,
            });
            state.message_grouped_ids.insert((canonical_id, message_id), grouped_id);
            state.attachments.insert(
                (canonical_id, message_id),
                (
                    AttachmentDescriptor {
                        attachment_id: format!("{chat_id}:{message_id}:0"),
                        chat_id: chat_id.to_string(),
                        message_id,
                        kind: match item.media_kind {
                            AlbumMediaKind::Photo => "photo",
                            AlbumMediaKind::Video => "video",
                        }
                        .to_string(),
                        file_name: Some(captured_file_name),
                        mime_type: Some(mime_type),
                        size_bytes: Some(u64::try_from(bytes.len()).unwrap_or(u64::MAX)),
                        downloadable: true,
                    },
                    bytes,
                ),
            );
            if let Some(topic_id) = topic_id {
                state.message_topics.insert((canonical_id, message_id), topic_id);
            }
            state.messages.entry(canonical_id).or_default().insert(
                0,
                MessageSummary {
                    message_id,
                    chat_ref: Some(chat_ref.clone()),
                    sender_ref: Some(account_user_id.to_string()),
                    outgoing: true,
                    pinned: false,
                    silent,
                    noforwards,
                    text: Some(rendered_caption),
                    date: 1_700_000_000_i32.saturating_add(message_id),
                    edit_date: None,
                    reply_to_message_id: effective_reply_to_message_id,
                    views: None,
                    forwards: None,
                    reply_count: None,
                    media_type: Some(
                        match item.media_kind {
                            AlbumMediaKind::Photo => "photo",
                            AlbumMediaKind::Video => "video",
                        }
                        .to_string(),
                    ),
                    forward_origin: None,
                },
            );
            message_ids.push(message_id);
        }
        if state.album_send_uncertain_once {
            state.album_send_uncertain_once = false;
            return Err(TelegramError::DeliveryUncertain(
                "fixture sent album before the transport receipt was lost".to_string(),
            ));
        }
        Ok(message_ids)
    }

    async fn get_message_grouped_id(
        &self,
        _session_path: Option<PathBuf>,
        chat_id: &str,
        message_id: i32,
    ) -> Result<Option<i64>, TelegramError> {
        let state = self.state.lock().await;
        let canonical_id = Self::resolve_dialog(&state.dialogs, chat_id)
            .ok_or_else(|| TelegramError::PeerNotFound(chat_id.to_string()))?;
        Ok(state.message_grouped_ids.get(&(canonical_id, message_id)).copied())
    }

    async fn send_sticker_with_random_id(
        &self,
        _session_path: Option<PathBuf>,
        chat_id: &str,
        set_short_name: &str,
        sticker_handle: &str,
        reply_to_message_id: Option<i32>,
        silent: bool,
        noforwards: bool,
        random_id: i64,
    ) -> Result<i32, TelegramError> {
        let mut state = self.state.lock().await;
        if let Some(message_id) = state.sent_by_random_id.get(&random_id) {
            return Ok(*message_id);
        }
        if set_short_name != "fixture_reactions"
            || !fixture_stickers().iter().any(|sticker| sticker.sticker_handle == sticker_handle)
        {
            return Err(TelegramError::PeerNotFound(format!(
                "sticker {set_short_name}:{sticker_handle}"
            )));
        }
        let canonical_id = Self::resolve_dialog(&state.dialogs, chat_id)
            .ok_or_else(|| TelegramError::PeerNotFound(chat_id.to_string()))?;
        if let Some(reply_to_message_id) = reply_to_message_id
            && !state.messages.get(&canonical_id).is_some_and(|messages| {
                messages.iter().any(|message| message.message_id == reply_to_message_id)
            })
        {
            return Err(TelegramError::MessageNotFound(format!("{chat_id}:{reply_to_message_id}")));
        }
        state.sticker_send_calls = state.sticker_send_calls.saturating_add(1);
        let message_id = state.next_message_id;
        state.next_message_id = state.next_message_id.saturating_add(1);
        state.sent_by_random_id.insert(random_id, message_id);
        state.sent.push(FakeSentMessage {
            chat_id: canonical_id.to_string(),
            text: String::new(),
            message_id,
            file_name: None,
            reply_to_message_id,
            file_mode: None,
        });
        let account_user_id = state.account.user_id;
        let chat_ref = Self::canonical_chat_ref(&state.dialogs, canonical_id);
        state.messages.entry(canonical_id).or_default().insert(
            0,
            MessageSummary {
                message_id,
                chat_ref: Some(chat_ref),
                sender_ref: Some(account_user_id.to_string()),
                outgoing: true,
                pinned: false,
                silent,
                noforwards,
                text: None,
                date: 1_700_000_000_i32.saturating_add(message_id),
                edit_date: None,
                reply_to_message_id,
                views: None,
                forwards: None,
                reply_count: None,
                media_type: Some("sticker".to_string()),
                forward_origin: None,
            },
        );
        if state.sticker_send_uncertain_once {
            state.sticker_send_uncertain_once = false;
            return Err(TelegramError::DeliveryUncertain(
                "fixture accepted sticker send before the transport receipt was lost".to_string(),
            ));
        }
        Ok(message_id)
    }

    async fn send_text_with_random_id(
        &self,
        _session_path: Option<PathBuf>,
        chat_id: &str,
        text: &str,
        reply_to_message_id: Option<i32>,
        random_id: i64,
    ) -> Result<i32, TelegramError> {
        let mut state = self.state.lock().await;
        if let Some(message_id) = state.sent_by_random_id.get(&random_id) {
            return Ok(*message_id);
        }
        let canonical_id = Self::resolve_dialog(&state.dialogs, chat_id)
            .ok_or_else(|| TelegramError::PeerNotFound(chat_id.to_string()))?;
        let message_id = state.next_message_id;
        state.next_message_id = state.next_message_id.saturating_add(1);
        state.sent_by_random_id.insert(random_id, message_id);
        state.sent.push(FakeSentMessage {
            chat_id: canonical_id.to_string(),
            text: text.to_string(),
            message_id,
            file_name: None,
            reply_to_message_id: None,
            file_mode: None,
        });
        let account_user_id = state.account.user_id;
        let chat_ref = Self::canonical_chat_ref(&state.dialogs, canonical_id);
        state.messages.entry(canonical_id).or_default().insert(
            0,
            MessageSummary {
                message_id,
                chat_ref: Some(chat_ref),
                sender_ref: Some(account_user_id.to_string()),
                outgoing: true,
                pinned: false,
                silent: false,
                noforwards: false,
                text: Some(text.to_string()),
                date: 1_700_000_000_i32.saturating_add(message_id),
                edit_date: None,
                reply_to_message_id,
                views: None,
                forwards: None,
                reply_count: None,
                media_type: None,
                forward_origin: None,
            },
        );
        Ok(message_id)
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
        let text = ferogram::InputMessage::html(html).text;
        let effective_reply_to = reply_to_message_id.or(topic_id);
        let message_id = self
            .send_text_with_random_id(session_path, chat_id, &text, effective_reply_to, random_id)
            .await?;
        let mut state = self.state.lock().await;
        let canonical_id = Self::resolve_dialog(&state.dialogs, chat_id)
            .ok_or_else(|| TelegramError::PeerNotFound(chat_id.to_string()))?;
        if let Some(sent) = state.sent.iter_mut().find(|sent| sent.message_id == message_id) {
            sent.reply_to_message_id = effective_reply_to;
        }
        let message = state
            .messages
            .get_mut(&canonical_id)
            .and_then(|messages| {
                messages.iter_mut().find(|message| message.message_id == message_id)
            })
            .ok_or_else(|| TelegramError::MessageNotFound(format!("{chat_id}:{message_id}")))?;
        message.silent = silent;
        message.noforwards = noforwards;
        message.reply_to_message_id = effective_reply_to;
        if let Some(topic_id) = topic_id {
            state.message_topics.insert((canonical_id, message_id), topic_id);
        }
        Ok(message_id)
    }

    async fn send_rich_markdown_with_random_id(
        &self,
        _session_path: Option<PathBuf>,
        chat_id: &str,
        markdown: &str,
        reply_to_message_id: Option<i32>,
        random_id: i64,
    ) -> Result<i32, TelegramError> {
        let mut state = self.state.lock().await;
        if let Some(message_id) = state.sent_by_random_id.get(&random_id) {
            return Ok(*message_id);
        }
        let self_chat = ["me", "@me", "self", "@self"]
            .iter()
            .any(|alias| chat_id.trim().eq_ignore_ascii_case(alias))
            || chat_id.trim() == state.account.user_id.to_string();
        let canonical_id = if self_chat {
            state.account.user_id
        } else {
            Self::resolve_dialog(&state.dialogs, chat_id)
                .ok_or_else(|| TelegramError::PeerNotFound(chat_id.to_string()))?
        };
        if let Some(reply_to_message_id) = reply_to_message_id
            && !state.messages.get(&canonical_id).is_some_and(|messages| {
                messages.iter().any(|message| message.message_id == reply_to_message_id)
            })
        {
            return Err(TelegramError::MessageNotFound(format!("{chat_id}:{reply_to_message_id}")));
        }
        if markdown.trim().is_empty() {
            return Err(TelegramError::InvalidPeer(
                "rich-message markdown must not be empty".to_string(),
            ));
        }

        let message_id = state.next_message_id;
        state.next_message_id = state.next_message_id.saturating_add(1);
        state.sent_by_random_id.insert(random_id, message_id);
        let canonical_chat_id = if self_chat {
            state.account.user_id.to_string()
        } else {
            Self::canonical_chat_ref(&state.dialogs, canonical_id)
        };
        state.sent.push(FakeSentMessage {
            chat_id: canonical_chat_id.clone(),
            text: markdown.to_string(),
            message_id,
            file_name: None,
            reply_to_message_id,
            file_mode: None,
        });
        let account_user_id = state.account.user_id;
        state.messages.entry(canonical_id).or_default().insert(
            0,
            MessageSummary {
                message_id,
                chat_ref: Some(canonical_chat_id.clone()),
                sender_ref: Some(account_user_id.to_string()),
                outgoing: true,
                pinned: false,
                silent: false,
                noforwards: false,
                text: None,
                date: 1_700_000_000_i32.saturating_add(message_id),
                edit_date: None,
                reply_to_message_id,
                views: None,
                forwards: None,
                reply_count: None,
                media_type: None,
                forward_origin: None,
            },
        );
        let normalized_markdown = crate::telegram::normalize_rich_markdown(markdown);
        let block_count =
            normalized_markdown.lines().filter(|line| !line.trim().is_empty()).count().max(1);
        let mut state_hasher = Sha256::new();
        state_hasher.update(b"fake-rich-message:v1\0");
        state_hasher.update(markdown.as_bytes());
        state.rich_messages.insert(
            (canonical_id, message_id),
            RichMessageSnapshot {
                chat_id: canonical_chat_id,
                message_id,
                rtl: false,
                part: false,
                block_count,
                photo_count: 0,
                document_count: 0,
                state_sha256: format!("{:x}", state_hasher.finalize()),
            },
        );
        Ok(message_id)
    }

    async fn get_rich_message_snapshot(
        &self,
        _session_path: Option<PathBuf>,
        chat_id: &str,
        message_id: i32,
    ) -> Result<Option<RichMessageSnapshot>, TelegramError> {
        let state = self.state.lock().await;
        let self_chat = ["me", "@me", "self", "@self"]
            .iter()
            .any(|alias| chat_id.trim().eq_ignore_ascii_case(alias))
            || chat_id.trim() == state.account.user_id.to_string();
        let canonical_id = if self_chat {
            state.account.user_id
        } else {
            Self::resolve_dialog(&state.dialogs, chat_id)
                .ok_or_else(|| TelegramError::PeerNotFound(chat_id.to_string()))?
        };
        Ok(state.rich_messages.get(&(canonical_id, message_id)).cloned())
    }

    async fn send_message(
        &self,
        _session_path: Option<PathBuf>,
        chat_id: &str,
        text: &str,
    ) -> Result<i32, TelegramError> {
        let mut state = self.state.lock().await;
        let canonical_id = Self::resolve_dialog(&state.dialogs, chat_id)
            .ok_or_else(|| TelegramError::PeerNotFound(chat_id.to_string()))?;
        let message_id = state.next_message_id;
        state.next_message_id = state.next_message_id.saturating_add(1);
        state.sent.push(FakeSentMessage {
            chat_id: canonical_id.to_string(),
            text: text.to_string(),
            message_id,
            file_name: None,
            reply_to_message_id: None,
            file_mode: None,
        });
        let account_user_id = state.account.user_id;
        let chat_ref = Self::canonical_chat_ref(&state.dialogs, canonical_id);
        state.messages.entry(canonical_id).or_default().insert(
            0,
            MessageSummary {
                message_id,
                chat_ref: Some(chat_ref),
                sender_ref: Some(account_user_id.to_string()),
                outgoing: true,
                pinned: false,
                silent: false,
                noforwards: false,
                text: Some(text.to_string()),
                date: 1_700_000_000_i32.saturating_add(message_id),
                edit_date: None,
                reply_to_message_id: None,
                views: None,
                forwards: None,
                reply_count: None,
                media_type: None,
                forward_origin: None,
            },
        );
        if let Some(dialog) = state.dialogs.iter_mut().find(|dialog| dialog.chat_id == canonical_id)
        {
            dialog.last_message_id = Some(message_id);
            dialog.last_message_text = Some(text.to_string());
            dialog.last_message_outgoing = Some(true);
        }
        Ok(message_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn fixture_supports_read_and_send() {
        let backend = FakeTelegramBackend::fixture();
        let messages = backend.history(None, "@alex", 10, None, 0).await.expect("read fixture");
        assert_eq!(messages.len(), 3);

        let sent_id = backend
            .send_message(None, "@alex", "Updated contract attached")
            .await
            .expect("send fixture");
        assert_eq!(sent_id, 100);
        assert_eq!(backend.sent_messages().await.len(), 1);
    }

    #[tokio::test]
    async fn fixture_stops_at_download_limit_with_a_partial_file_for_wrapper_cleanup() {
        let backend = FakeTelegramBackend::fixture();
        let temp = tempfile::tempdir().expect("tempdir");
        let destination = temp.path().join("contract.pdf");

        let error = backend
            .download_attachment_to(None, "@alex", 11, &destination, 10)
            .await
            .expect_err("fixture attachment exceeds limit");
        assert!(matches!(error, TelegramError::AttachmentTooLarge { max_bytes: 10 }));
        assert_eq!(std::fs::metadata(destination).expect("partial metadata").len(), 10);
    }
}
