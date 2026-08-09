pub mod approval;
pub mod backend;
pub mod config;
pub mod daemon;
pub mod domain;
pub mod fake;
mod fs_security;
mod invite;
pub mod ledger;
pub mod mcp;
pub mod message_cache;
pub mod mutation_ledger;
mod profile_photo;
pub mod protocol;
pub mod state_db;
mod sticker;
pub mod telegram;

pub use approval::{
    ApprovalAction, ApprovalBot, ApprovalPolicy, BotApiApprovalBot, FakeApprovalBot,
    FanoutApprovalBot,
};
pub use backend::TelegramBackend;
pub use config::Config;
pub use domain::{
    AccountEmojiStatusSpec, AdminLogActionKind, AdminLogEventFilter, AdminLogEventSummary,
    AdminLogPage, AlbumMediaKind, AlbumUploadSpec, AttachmentDescriptor, AudioTranscriptionSummary,
    AvailableStarGiftsPage, BoostPage, BoostSlotSummary, BoostStatusSummary, BoostSummary,
    BotCallbackAnswerSummary, BotCommandSetSummary, BotCommandSummary, CallbackButtonSnapshot,
    CaptionParseMode, ChannelStatsSummary, ChatAutoDeleteSummary, ChatDetailsSummary,
    ChatLifecycleState, ChatSummary, Completeness, ContactSummary, CustomEmojiDocumentSummary,
    CustomEmojiDocumentsPage, DefaultReactionSpec, DefaultReactionSummary,
    DeletedMessageRestoreMedia, DeletedMessageRestoreSnapshot, DialogFolderKind, DialogFolderList,
    DialogFolderSummary, DialogStateSnapshot, DiceEmojiCatalog, DisallowedGiftsSettingsSummary,
    DiscussionMessageSummary, DownloadedAttachment, DownloadedCustomEmoji, DraftSummary,
    EmojiStatusCatalogKind, EmojiStatusCatalogPage, EmojiStatusKind, EmojiStatusSummary,
    FactCheckPage, FactCheckSummary, FileSendMode, ForumTopicSummary, GeoPointSpec,
    GlobalPrivacySettingsSummary, GroupCallInfoSummary, GroupCallPage, GroupCallParticipantSummary,
    GroupCallParticipantVideoSourceGroupSummary, GroupCallParticipantVideoSummary,
    GroupCallParticipantsPage, InlineBotGeoSummary, InlineBotQuerySnapshot,
    InlineBotResultSourceKind, InlineBotResultSummary, InlineBotResults, InlineBotResultsPage,
    InlineBotSwitchPmSummary, InlineBotSwitchWebViewSummary, InviteBatchProgress,
    InviteChatBinding, InviteChatKind, InviteConfirmation, InviteTargetProgress, InviteTargetState,
    InviteUserBinding, MessageButtonInventory, MessageButtonKind, MessageButtonMarkupType,
    MessageButtonStyleSummary, MessageButtonSummary, MessageContext, MessagePage,
    MessageReactionPage, MessageReactionReadReceipt, MessageReactionSummary, MessageReadReceipt,
    MessageTranslationBatch, MessageTranslationSummary, MutationAction, MutationAlbumItem,
    MutationApproval, MutationApprovalState, MutationFileArtifact, MutationMessageSnapshot,
    MutationResult, MyBoostsSummary, NotificationSettingsPatch, NotificationSettingsSummary,
    NotificationSoundPlatform, NotificationSoundSummary, NotificationSoundTarget,
    OutboxReadDateSummary, ParticipantFilter, ParticipantPage, ParticipantSummary,
    PeerColorSummary, PersonalChannelSummary, PreparedMutation, PreparedMutationState,
    PreparedSend, PreparedSendKind, PreparedSendState, PrivacyKey, PrivacyRuleSummary,
    PrivacyUserBinding, ProfilePhotoPage, ProfilePhotoSummary, QuickReplyCatalogPage,
    QuickReplyMessagesPage, QuickReplyShortcutSummary, QuickReplyStoredMessageSummary,
    ReactionCatalogPage, ReactionIdentity, ReactionSpec, RestoredMessageMapping, RichMediaSpec,
    RichMessageSnapshot, SavedStarGiftSummary, SavedStarGiftsPage, SavedStarGiftsRequest,
    SearchMessagesRequest, SendApproval, SendApprovalState, SentDocumentMediaSummary,
    SentRichMediaSummary, SessionSummary, StarGiftAttributeSummary, StarGiftRaritySummary,
    StarGiftSummary, StarsAmountKind, StarsAmountSummary, StarsStatusPage,
    StarsSubscriptionPricingSummary, StarsSubscriptionSummary, StarsSubscriptionsPage,
    StarsTopupOptionSummary, StarsTopupOptionsPage, StarsTransactionGiftSummary,
    StarsTransactionPeerSummary, StarsTransactionSummary, StatsDateRangeSummary, StatsGraphSummary,
    StatsPercentSummary, StatsPostInteractionSummary, StatsTopAdminSummary, StatsTopInviterSummary,
    StatsTopPosterSummary, StatsValueSummary, StickerCatalogPage, StickerDocumentSummary,
    StickerEmojiPackSummary, StickerKeywordSummary, StickerSetDetailsPage, StickerSetSearchPage,
    StickerSetSummary, SuggestedDialogFolderList, SuggestedDialogFolderSummary, TypingAction,
    UnreadMessagesRequest, UserDetails,
};
pub use fake::{FakeInlineBotResultSend, FakeInviteOutcome, FakeTelegramBackend};
pub use ledger::{ApprovalRequestRecord, PreparedSendRecord, SendLedger};
pub use mcp::{McpServer, McpServerSettings, MessageCacheWorker};
pub use message_cache::{
    AccountCacheStats, AccountMetadata, CacheSyncPhase, CacheUpdateBatch, CachedMessage,
    CachedMessageInput, ChatCacheStats, ClearAccountResult, DialogCheckpoint, DialogMetadata,
    GlobalUpdateCursor, IncomingEventCursor, IncomingEventWindow, IncomingMessageEvent,
    IncomingMessageEventInput, MAX_MESSAGE_CACHE_AGE_DAYS, MessageCache, MessageCacheError,
    MessageRetentionResult, MessageSource, MessageWindow, RegexSearchPage, RegexSearchRequest,
    SenderCount, SourceExport, TimelineGranularity, TimelinePoint, UpdateBatchApplyResult,
};
pub use mutation_ledger::{MutationApprovalRequestRecord, MutationLedger, PreparedMutationRecord};
pub use telegram::{
    LoginCodeChallenge, LoginCodeOutcome, LoginPasswordChallenge, LoginRequestOutcome,
    LoginSession, TelegramClient, TelegramError, UserInfo,
};
