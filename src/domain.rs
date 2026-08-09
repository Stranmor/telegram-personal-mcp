use crate::telegram::{MessageForwardOrigin, MessageSummary};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct SearchMessagesRequest {
    pub session_path: Option<PathBuf>,
    pub chat_id: Option<String>,
    pub query: String,
    pub limit: usize,
    pub offset_id: Option<i32>,
}

#[derive(Debug, Clone)]
pub struct SavedMessagesSearchRequest {
    pub session_path: Option<PathBuf>,
    pub query: String,
    pub tag_reactions: Vec<ReactionIdentity>,
    pub saved_peer_id: Option<String>,
    pub min_date: Option<i32>,
    pub max_date: Option<i32>,
    pub limit: usize,
    pub offset_id: Option<i32>,
}

#[derive(Debug, Clone)]
pub struct SavedStarGiftsRequest {
    pub session_path: Option<PathBuf>,
    pub peer_ref: String,
    pub exclude_unsaved: bool,
    pub exclude_saved: bool,
    pub exclude_unlimited: bool,
    pub exclude_limited: bool,
    pub exclude_unique: bool,
    pub sort_by_value: bool,
    pub exclude_upgradable: bool,
    pub exclude_unupgradable: bool,
    pub peer_color_available: bool,
    pub exclude_hosted: bool,
    pub collection_id: Option<i32>,
    pub offset: String,
    pub limit: i32,
}

#[derive(Debug, Clone)]
pub struct UnreadMessagesRequest {
    pub session_path: Option<PathBuf>,
    pub chat_id: String,
    pub top_message_id: Option<i32>,
    pub saved_peer_id: Option<String>,
    pub offset_id: i32,
    pub add_offset: i32,
    pub limit: usize,
    pub max_id: i32,
    pub min_id: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Completeness {
    Complete,
    Page,
    FirstPageOnly,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct MessagePage {
    pub messages: Vec<MessageSummary>,
    pub next_offset_id: Option<i32>,
    pub completeness: Completeness,
}

#[derive(Debug, Clone)]
pub struct UpdatesDifferenceRequest {
    pub session_path: Option<PathBuf>,
    pub pts: i32,
    pub qts: i32,
    pub date: i32,
    pub pts_limit: Option<i32>,
    pub pts_total_limit: Option<i32>,
    pub qts_limit: Option<i32>,
}

#[derive(Debug, Clone)]
pub struct ChannelUpdatesRequest {
    pub session_path: Option<PathBuf>,
    pub chat_id: String,
    pub pts: i32,
    pub limit: i32,
    pub force: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct UpdatesStateSummary {
    pub pts: i32,
    pub qts: i32,
    pub date: i32,
    pub seq: i32,
    pub unread_count: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct UpdatesCursorSummary {
    pub pts: i32,
    pub qts: i32,
    pub date: i32,
    pub seq: i32,
    #[schemars(
        description = "Present when Telegram returned a complete updates.State; absent for differenceEmpty, whose constructor does not carry unread_count."
    )]
    pub unread_count: Option<i32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct UpdatesDifferenceRequestSummary {
    pub pts: i32,
    pub qts: i32,
    pub date: i32,
    pub pts_limit: Option<i32>,
    pub pts_total_limit: Option<i32>,
    pub qts_limit: Option<i32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct ChannelUpdatesRequestSummary {
    pub chat_id: String,
    pub pts: i32,
    pub limit: i32,
    pub force: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum UpdatesDifferenceKind {
    Empty,
    Complete,
    Slice,
    TooLong,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ChannelUpdatesDifferenceKind {
    Empty,
    Difference,
    TooLong,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(tag = "scope", rename_all = "snake_case")]
pub enum DeletedMessagesSummary {
    Global { message_ids: Vec<i32>, pts: i32, pts_count: i32 },
    Channel { chat_id: String, message_ids: Vec<i32>, pts: i32, pts_count: i32 },
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct UpdateKindSummary {
    #[schemars(
        description = "Stable snake_case Telegram Update constructor kind. Raw provider payloads are not exposed."
    )]
    pub kind: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct UpdatesDifferenceSummary {
    pub request: UpdatesDifferenceRequestSummary,
    pub kind: UpdatesDifferenceKind,
    pub is_final: bool,
    pub continuation_required: bool,
    pub requires_resync: bool,
    pub next_cursor: Option<UpdatesCursorSummary>,
    #[schemars(
        description = "Server pts returned by differenceTooLong. Use it as the next global pts only as part of an explicit resynchronization flow."
    )]
    pub resync_pts: Option<i32>,
    pub new_messages: Vec<MessageSummary>,
    #[schemars(
        description = "Typed edited-message payloads carried by Telegram update constructors."
    )]
    pub edited_messages: Vec<MessageSummary>,
    #[schemars(
        description = "Number of secret-chat encrypted messages omitted from this safe typed surface."
    )]
    pub encrypted_message_count: usize,
    pub deleted_messages: Vec<DeletedMessagesSummary>,
    pub other_updates: Vec<UpdateKindSummary>,
    pub related_chats: Vec<ChatSummary>,
    pub related_users: Vec<ContactSummary>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ChannelUpdatesDifferenceSummary {
    pub request: ChannelUpdatesRequestSummary,
    pub kind: ChannelUpdatesDifferenceKind,
    pub is_final: bool,
    pub continuation_required: bool,
    pub requires_resync: bool,
    pub next_pts: Option<i32>,
    #[schemars(description = "Channel pts recovered from the dialog in channelDifferenceTooLong.")]
    pub resync_pts: Option<i32>,
    pub timeout_seconds: Option<i32>,
    pub new_messages: Vec<MessageSummary>,
    #[schemars(
        description = "Typed edited-message payloads carried by Telegram update constructors."
    )]
    pub edited_messages: Vec<MessageSummary>,
    #[schemars(
        description = "Current channel snapshot messages returned by channelDifferenceTooLong; these are not a delta."
    )]
    pub snapshot_messages: Vec<MessageSummary>,
    pub deleted_messages: Vec<DeletedMessagesSummary>,
    pub other_updates: Vec<UpdateKindSummary>,
    pub related_chats: Vec<ChatSummary>,
    pub related_users: Vec<ContactSummary>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct QuickReplyShortcutSummary {
    pub shortcut_id: i32,
    pub shortcut: String,
    pub top_message_id: i32,
    pub count: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct QuickReplyStoredMessageSummary {
    pub message: MessageSummary,
    pub is_service: bool,
    pub service_action_kind: Option<String>,
    pub quick_reply_shortcut_id: Option<i32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct QuickReplyCatalogPage {
    pub not_modified: bool,
    pub shortcuts: Vec<QuickReplyShortcutSummary>,
    pub top_messages: Vec<QuickReplyStoredMessageSummary>,
    pub related_chats: Vec<ChatSummary>,
    pub related_users: Vec<ContactSummary>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct QuickReplyMessagesPage {
    pub shortcut_id: i32,
    pub not_modified: bool,
    pub total_count: Option<i32>,
    pub messages: Vec<QuickReplyStoredMessageSummary>,
    pub related_chats: Vec<ChatSummary>,
    pub related_users: Vec<ContactSummary>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct BusinessChatLinkSummary {
    pub slug: String,
    #[schemars(description = "Untrusted Telegram Business link; never open it automatically.")]
    pub link: String,
    pub message: String,
    pub entity_count: usize,
    #[schemars(description = "SHA-256 of the exact serialized Telegram message-entity vector.")]
    pub entities_sha256: String,
    pub title: Option<String>,
    pub views: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct BusinessChatLinksPage {
    pub count: usize,
    pub links: Vec<BusinessChatLinkSummary>,
    pub related_chats: Vec<ChatSummary>,
    pub related_users: Vec<ContactSummary>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct ResolvedBusinessChatLinkSummary {
    pub peer_ref: String,
    pub message: String,
    pub entity_count: usize,
    #[schemars(description = "SHA-256 of the exact serialized Telegram message-entity vector.")]
    pub entities_sha256: String,
    pub related_chats: Vec<ChatSummary>,
    pub related_users: Vec<ContactSummary>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct BusinessChatLinkContent {
    pub source_message: String,
    pub parse_mode: CaptionParseMode,
    pub rendered_message: String,
    pub entity_count: usize,
    #[schemars(description = "SHA-256 of the exact serialized Telegram message-entity vector.")]
    pub entities_sha256: String,
    pub title: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct BusinessWeeklyOpenSummary {
    #[schemars(description = "Opening minute in the Telegram week, where Monday 00:00 is 0.")]
    pub start_minute: i32,
    #[schemars(
        description = "Exclusive closing minute; may exceed one week for a Sunday-to-Monday interval."
    )]
    pub end_minute: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct BusinessWorkHoursConfig {
    pub timezone_id: String,
    pub weekly_open: Vec<BusinessWeeklyOpenSummary>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct BusinessWorkHoursSummary {
    pub config: BusinessWorkHoursConfig,
    #[schemars(
        description = "Server-derived current open state; it is never accepted as write input."
    )]
    pub open_now: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct BusinessLocationSummary {
    pub address: String,
    pub point: Option<GeoPointSpec>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct BusinessRecipientsSummary {
    pub existing_chats: bool,
    pub new_chats: bool,
    pub contacts: bool,
    pub non_contacts: bool,
    pub exclude_selected: bool,
    pub user_ids: Vec<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct BusinessGreetingMessageSummary {
    pub shortcut_id: i32,
    pub recipients: BusinessRecipientsSummary,
    pub no_activity_days: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum BusinessAwayScheduleSummary {
    Always,
    OutsideHours,
    Custom { start_date_unix: i32, end_date_unix: i32 },
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct BusinessAwayMessageSummary {
    pub offline_only: bool,
    pub shortcut_id: i32,
    pub schedule: BusinessAwayScheduleSummary,
    pub recipients: BusinessRecipientsSummary,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct BusinessIntroSummary {
    pub title: String,
    pub description: String,
    #[schemars(
        description = "Opaque account-scoped sticker handle; Telegram document ids, access hashes, and file references are never exposed."
    )]
    pub sticker_handle: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct BusinessIntroSpec {
    pub title: String,
    pub description: String,
    pub sticker_set_short_name: Option<String>,
    #[schemars(
        description = "Opaque sticker handle returned by this MCP; raw Telegram document credentials are never accepted."
    )]
    pub sticker_handle: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct BusinessAccountSettingsSummary {
    pub work_hours: Option<BusinessWorkHoursSummary>,
    pub location: Option<BusinessLocationSummary>,
    pub greeting: Option<BusinessGreetingMessageSummary>,
    pub away: Option<BusinessAwayMessageSummary>,
    pub intro: Option<BusinessIntroSummary>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct GroupCallParticipantVideoSourceGroupSummary {
    pub semantics: String,
    pub sources: Vec<i32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct GroupCallParticipantVideoSummary {
    pub paused: bool,
    pub endpoint: String,
    pub source_groups: Vec<GroupCallParticipantVideoSourceGroupSummary>,
    pub audio_source: Option<i32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct GroupCallParticipantSummary {
    pub peer_ref: String,
    pub date_unix: i32,
    pub active_date_unix: Option<i32>,
    pub source: i32,
    pub volume: Option<i32>,
    pub muted: bool,
    pub left: bool,
    pub can_self_unmute: bool,
    pub just_joined: bool,
    pub versioned: bool,
    pub min: bool,
    pub self_participant: bool,
    pub muted_by_you: bool,
    pub volume_by_admin: bool,
    pub video_joined: bool,
    pub about: Option<String>,
    #[schemars(description = "Exact Telegram raise-hand rating encoded as decimal text.")]
    pub raise_hand_rating: Option<String>,
    pub video: Option<GroupCallParticipantVideoSummary>,
    pub presentation: Option<GroupCallParticipantVideoSummary>,
    #[schemars(description = "Exact paid Stars total encoded as decimal text.")]
    pub paid_stars_total: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum GroupCallInfoSummary {
    Active {
        #[schemars(description = "Exact Telegram group-call id encoded as decimal text.")]
        call_id: String,
        participants_count: i32,
        title: Option<String>,
        stream_dc_id: Option<i32>,
        record_start_date_unix: Option<i32>,
        schedule_date_unix: Option<i32>,
        unmuted_video_count: Option<i32>,
        unmuted_video_limit: i32,
        version: i32,
        join_muted: bool,
        can_change_join_muted: bool,
        join_date_asc: bool,
        schedule_start_subscribed: bool,
        can_start_video: bool,
        record_video_active: bool,
        rtmp_stream: bool,
        listeners_hidden: bool,
        conference: bool,
        creator: bool,
        messages_enabled: bool,
        can_change_messages_enabled: bool,
        min: bool,
        has_invite_link: bool,
        #[schemars(description = "Exact per-message Stars charge encoded as decimal text.")]
        send_paid_messages_stars: Option<String>,
        default_send_as: Option<String>,
    },
    Discarded {
        #[schemars(description = "Exact Telegram group-call id encoded as decimal text.")]
        call_id: String,
        duration_seconds: i32,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct GroupCallPage {
    pub chat_id: String,
    pub call: GroupCallInfoSummary,
    pub participants: Vec<GroupCallParticipantSummary>,
    pub participants_next_offset: Option<String>,
    pub related_chats: Vec<ChatSummary>,
    pub related_users: Vec<ContactSummary>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct GroupCallParticipantsPage {
    pub chat_id: String,
    pub count: i32,
    pub participants: Vec<GroupCallParticipantSummary>,
    pub next_offset: Option<String>,
    pub version: i32,
    pub related_chats: Vec<ChatSummary>,
    pub related_users: Vec<ContactSummary>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum StarsAmountKind {
    Stars,
    Ton,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct StarsAmountSummary {
    pub kind: StarsAmountKind,
    #[schemars(description = "Exact signed Telegram amount encoded as decimal text.")]
    pub amount: String,
    pub nanos: Option<i32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum StarsTransactionPeerSummary {
    Unsupported,
    AppStore,
    PlayMarket,
    PremiumBot,
    Fragment,
    Ads,
    Api,
    Peer { peer_ref: String },
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum StarsTransactionGiftSummary {
    Regular {
        #[schemars(description = "Exact Telegram Star Gift id encoded as decimal text.")]
        gift_id: String,
        title: Option<String>,
    },
    Unique {
        #[schemars(description = "Exact unique gift id encoded as decimal text.")]
        unique_gift_id: String,
        #[schemars(description = "Exact original Star Gift id encoded as decimal text.")]
        gift_id: String,
        title: String,
        slug: String,
        number: i32,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct StarsTransactionSummary {
    pub transaction_id: String,
    pub amount: StarsAmountSummary,
    pub date_unix: i32,
    pub peer: StarsTransactionPeerSummary,
    pub refund: bool,
    pub pending: bool,
    pub failed: bool,
    pub gift: bool,
    pub reaction: bool,
    pub star_gift_upgrade: bool,
    pub business_transfer: bool,
    pub star_gift_resale: bool,
    pub posts_search: bool,
    pub star_gift_prepaid_upgrade: bool,
    pub star_gift_drop_original_details: bool,
    pub phone_group_message: bool,
    pub star_gift_auction_bid: bool,
    pub offer: bool,
    pub title: Option<String>,
    pub description: Option<String>,
    pub has_photo: bool,
    pub transaction_date_unix: Option<i32>,
    #[schemars(description = "Untrusted provider URL; never open it automatically.")]
    pub transaction_url: Option<String>,
    pub bot_payload_sha256: Option<String>,
    pub bot_payload_length: Option<usize>,
    pub message_id: Option<i32>,
    pub extended_media_types: Option<Vec<String>>,
    pub subscription_period_seconds: Option<i32>,
    pub giveaway_post_id: Option<i32>,
    pub star_gift: Option<StarsTransactionGiftSummary>,
    pub floodskip_number: Option<i32>,
    pub star_ref_commission_permille: Option<i32>,
    pub star_ref_peer_ref: Option<String>,
    pub star_ref_amount: Option<StarsAmountSummary>,
    pub paid_messages: Option<i32>,
    pub premium_gift_months: Option<i32>,
    pub ads_proceeds_from_date_unix: Option<i32>,
    pub ads_proceeds_to_date_unix: Option<i32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct StarsSubscriptionPricingSummary {
    pub period_seconds: i32,
    #[schemars(description = "Exact recurring Stars amount encoded as decimal text.")]
    pub amount_stars: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct StarsSubscriptionSummary {
    pub subscription_id: String,
    pub peer_ref: String,
    pub until_date_unix: i32,
    pub pricing: StarsSubscriptionPricingSummary,
    pub canceled: bool,
    pub can_refulfill: bool,
    pub missing_balance: bool,
    pub bot_canceled: bool,
    pub has_chat_invite_hash: bool,
    pub title: Option<String>,
    pub has_photo: bool,
    pub invoice_slug: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct StarsStatusPage {
    pub peer_ref: String,
    pub balance: StarsAmountSummary,
    pub subscriptions: Vec<StarsSubscriptionSummary>,
    pub subscriptions_next_offset: Option<String>,
    #[schemars(description = "Exact missing subscription balance encoded as decimal text.")]
    pub subscriptions_missing_balance: Option<String>,
    pub history: Vec<StarsTransactionSummary>,
    pub next_offset: Option<String>,
    pub related_chats: Vec<ChatSummary>,
    pub related_users: Vec<ContactSummary>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum StarGiftRaritySummary {
    Exact { permille: i32 },
    Uncommon,
    Rare,
    Epic,
    Legendary,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum StarGiftAttributeSummary {
    Model {
        name: String,
        #[schemars(description = "Exact Telegram document id encoded as decimal text.")]
        document_id: String,
        crafted: bool,
        rarity: StarGiftRaritySummary,
    },
    Pattern {
        name: String,
        #[schemars(description = "Exact Telegram document id encoded as decimal text.")]
        document_id: String,
        rarity: StarGiftRaritySummary,
    },
    Backdrop {
        name: String,
        backdrop_id: i32,
        center_color: i32,
        edge_color: i32,
        pattern_color: i32,
        text_color: i32,
        rarity: StarGiftRaritySummary,
    },
    OriginalDetails {
        sender_peer_ref: Option<String>,
        recipient_peer_ref: String,
        date_unix: i32,
        message: Option<String>,
        message_has_entities: bool,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum StarGiftSummary {
    Regular {
        #[schemars(description = "Exact Telegram Star Gift id encoded as decimal text.")]
        gift_id: String,
        #[schemars(description = "Exact sticker document id encoded as decimal text.")]
        sticker_document_id: String,
        #[schemars(description = "Exact purchase price in Stars encoded as decimal text.")]
        stars: String,
        #[schemars(description = "Exact conversion value in Stars encoded as decimal text.")]
        convert_stars: String,
        limited: bool,
        sold_out: bool,
        birthday: bool,
        require_premium: bool,
        limited_per_user: bool,
        peer_color_available: bool,
        auction: bool,
        availability_remains: Option<i32>,
        availability_total: Option<i32>,
        availability_resale: Option<String>,
        first_sale_date_unix: Option<i32>,
        last_sale_date_unix: Option<i32>,
        upgrade_stars: Option<String>,
        resell_min_stars: Option<String>,
        title: Option<String>,
        released_by_peer_ref: Option<String>,
        per_user_total: Option<i32>,
        per_user_remains: Option<i32>,
        locked_until_date_unix: Option<i32>,
        auction_slug: Option<String>,
        gifts_per_round: Option<i32>,
        auction_start_date_unix: Option<i32>,
        upgrade_variants: Option<i32>,
        has_background: bool,
    },
    Unique {
        #[schemars(description = "Exact unique gift id encoded as decimal text.")]
        unique_gift_id: String,
        #[schemars(description = "Exact original Star Gift id encoded as decimal text.")]
        gift_id: String,
        title: String,
        slug: String,
        number: i32,
        require_premium: bool,
        resale_ton_only: bool,
        theme_available: bool,
        burned: bool,
        crafted: bool,
        owner_peer_ref: Option<String>,
        owner_name: Option<String>,
        has_owner_address: bool,
        attributes: Vec<StarGiftAttributeSummary>,
        availability_issued: i32,
        availability_total: i32,
        has_gift_address: bool,
        resell_amounts: Vec<StarsAmountSummary>,
        released_by_peer_ref: Option<String>,
        value_amount: Option<String>,
        value_currency: Option<String>,
        value_usd_amount: Option<String>,
        theme_peer_ref: Option<String>,
        has_peer_color: bool,
        host_peer_ref: Option<String>,
        offer_min_stars: Option<String>,
        craft_chance_permille: Option<i32>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct AvailableStarGiftsPage {
    pub not_modified: bool,
    pub hash: Option<i32>,
    pub gifts: Vec<StarGiftSummary>,
    pub related_chats: Vec<ChatSummary>,
    pub related_users: Vec<ContactSummary>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct SavedStarGiftSummary {
    pub gift: StarGiftSummary,
    pub name_hidden: bool,
    pub unsaved: bool,
    pub refunded: bool,
    pub can_upgrade: bool,
    pub pinned_to_top: bool,
    pub upgrade_separate: bool,
    pub from_peer_ref: Option<String>,
    pub date_unix: i32,
    pub message: Option<String>,
    pub message_has_entities: bool,
    pub message_id: Option<i32>,
    #[schemars(description = "Exact saved-gift id encoded as decimal text.")]
    pub saved_id: Option<String>,
    pub convert_stars: Option<String>,
    pub upgrade_stars: Option<String>,
    pub can_export_at_unix: Option<i32>,
    pub transfer_stars: Option<String>,
    pub can_transfer_at_unix: Option<i32>,
    pub can_resell_at_unix: Option<i32>,
    pub collection_ids: Vec<i32>,
    pub has_prepaid_upgrade_hash: bool,
    pub prepaid_upgrade_hash_sha256: Option<String>,
    pub prepaid_upgrade_hash_length: Option<usize>,
    pub drop_original_details_stars: Option<String>,
    pub gift_number: Option<i32>,
    pub can_craft_at_unix: Option<i32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SavedStarGiftRef {
    UserMessage {
        message_id: i32,
    },
    ChatSaved {
        chat_id: String,
        #[schemars(description = "Exact saved-gift id encoded as decimal text.")]
        saved_id: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct SavedStarGiftSnapshot {
    pub wallet_peer_ref: String,
    pub reference: SavedStarGiftRef,
    pub gift: SavedStarGiftSummary,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct SavedStarGiftsPage {
    pub peer_ref: String,
    pub count: i32,
    pub chat_notifications_enabled: Option<bool>,
    pub gifts: Vec<SavedStarGiftSummary>,
    pub next_offset: Option<String>,
    pub related_chats: Vec<ChatSummary>,
    pub related_users: Vec<ContactSummary>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct StarsTopupOptionSummary {
    #[schemars(description = "Exact number of Stars encoded as decimal text.")]
    pub stars: String,
    pub store_product: Option<String>,
    pub currency: String,
    #[schemars(
        description = "Exact price in the currency's smallest unit, encoded as decimal text."
    )]
    pub amount: String,
    pub extended: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct StarsTopupOptionsPage {
    pub options: Vec<StarsTopupOptionSummary>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct StarsSubscriptionsPage {
    pub peer_ref: String,
    pub balance: StarsAmountSummary,
    pub subscriptions: Vec<StarsSubscriptionSummary>,
    pub next_offset: Option<String>,
    pub subscriptions_missing_balance: Option<String>,
    pub related_chats: Vec<ChatSummary>,
    pub related_users: Vec<ContactSummary>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct MessageContext {
    pub target: MessageSummary,
    pub newer: Vec<MessageSummary>,
    pub older: Vec<MessageSummary>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct SavedDialogSummary {
    pub peer_ref: String,
    pub title: Option<String>,
    pub pinned: bool,
    pub top_message_id: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct SavedDialogsPage {
    pub dialogs: Vec<SavedDialogSummary>,
    pub total_count: Option<i32>,
    pub next_offset_id: Option<i32>,
    pub completeness: Completeness,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SavedHistoryPage {
    pub peer_ref: String,
    pub messages: Vec<MessageSummary>,
    pub total_count: Option<i32>,
    pub next_offset_id: Option<i32>,
    pub completeness: Completeness,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct SavedReactionTagSummary {
    pub reaction: ReactionIdentity,
    pub title: Option<String>,
    pub count: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct SavedReactionTagsPage {
    pub tags: Vec<SavedReactionTagSummary>,
    pub hash: Option<String>,
    pub not_modified: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct SavedHistoryMutationSnapshot {
    pub peer_ref: String,
    pub max_id: i32,
    pub min_date: Option<i32>,
    pub max_date: Option<i32>,
    pub observed_message_ids: Vec<i32>,
    pub observed_total_count: Option<i32>,
    pub observed_fingerprint: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct MessageTranslationSummary {
    pub message_id: i32,
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct MessageTranslationBatch {
    pub chat_id: String,
    pub to_language: String,
    pub translations: Vec<MessageTranslationSummary>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct AudioTranscriptionSummary {
    pub chat_id: String,
    pub message_id: i32,
    #[schemars(description = "Telegram's exact 64-bit transcription id encoded as decimal text.")]
    pub transcription_id: String,
    pub text: String,
    pub pending: bool,
    pub trial_remains_num: Option<i32>,
    pub trial_remains_until_unix: Option<i32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct FactCheckSummary {
    pub message_id: i32,
    pub need_check: bool,
    pub country: Option<String>,
    pub text: Option<String>,
    #[schemars(description = "Telegram's exact 64-bit fact-check hash encoded as decimal text.")]
    pub hash: String,
}

impl FactCheckSummary {
    pub fn is_empty(&self) -> bool {
        self.country.is_none() && self.text.as_deref().is_none_or(str::is_empty)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct FactCheckPage {
    pub chat_id: String,
    pub returned_count: usize,
    pub fact_checks: Vec<FactCheckSummary>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct ContactSummary {
    pub user_id: i64,
    pub first_name: String,
    pub last_name: Option<String>,
    pub display_name: String,
    pub username: Option<String>,
    pub phone: Option<String>,
    pub is_contact: bool,
    pub mutual_contact: bool,
    pub bot: bool,
    pub verified: bool,
    pub premium: bool,
    pub scam: bool,
    pub restricted: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct ChatSummary {
    pub chat_id: String,
    pub title: String,
    pub kind: String,
    pub username: Option<String>,
    pub participants_count: Option<i32>,
    pub creator: bool,
    pub verified: bool,
    pub restricted: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct ChatDetailsSummary {
    pub chat: ChatSummary,
    pub about: Option<String>,
    #[schemars(
        description = "Opaque chat-scoped photo identity. Raw Telegram photo ids, access hashes, and file references are never exposed."
    )]
    pub photo_handle: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct ChatAutoDeleteSummary {
    pub chat_id: String,
    #[schemars(description = "Current Telegram history TTL in seconds; 0 means disabled.")]
    pub period_seconds: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct ChatLifecycleState {
    pub details: ChatDetailsSummary,
    pub in_dialogs: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
pub struct StatsDateRangeSummary {
    pub min_date_unix: i32,
    pub max_date_unix: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
pub struct StatsValueSummary {
    pub current: f64,
    pub previous: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
pub struct StatsPercentSummary {
    pub part: f64,
    pub total: f64,
    pub percentage: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
pub struct StatsGraphSummary {
    pub name: String,
    pub state: String,
    pub data: Option<serde_json::Value>,
    pub token: Option<String>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum StatsPostInteractionSummary {
    Message { message_id: i32, views: i32, forwards: i32, reactions: i32 },
    Story { story_id: i32, views: i32, forwards: i32, reactions: i32 },
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct StatsTopPosterSummary {
    pub user_id: i64,
    pub messages: i32,
    pub average_characters: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct StatsTopAdminSummary {
    pub user_id: i64,
    pub deleted_messages: i32,
    pub kicked_users: i32,
    pub banned_users: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct StatsTopInviterSummary {
    pub user_id: i64,
    pub invitations: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ChannelStatsSummary {
    Broadcast {
        chat_id: String,
        period: StatsDateRangeSummary,
        followers: StatsValueSummary,
        views_per_post: StatsValueSummary,
        shares_per_post: StatsValueSummary,
        reactions_per_post: StatsValueSummary,
        views_per_story: StatsValueSummary,
        shares_per_story: StatsValueSummary,
        reactions_per_story: StatsValueSummary,
        enabled_notifications: StatsPercentSummary,
        graphs: Vec<StatsGraphSummary>,
        recent_interactions: Vec<StatsPostInteractionSummary>,
    },
    Megagroup {
        chat_id: String,
        period: StatsDateRangeSummary,
        members: StatsValueSummary,
        messages: StatsValueSummary,
        viewers: StatsValueSummary,
        posters: StatsValueSummary,
        graphs: Vec<StatsGraphSummary>,
        top_posters: Vec<StatsTopPosterSummary>,
        top_admins: Vec<StatsTopAdminSummary>,
        top_inviters: Vec<StatsTopInviterSummary>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct BoostSlotSummary {
    pub slot: i32,
    pub chat_id: Option<String>,
    pub applied_at_unix: i32,
    pub expires_at_unix: i32,
    pub cooldown_until_unix: Option<i32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct MyBoostsSummary {
    pub returned_count: usize,
    pub slots: Vec<BoostSlotSummary>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
pub struct BoostStatusSummary {
    pub chat_id: String,
    pub level: i32,
    pub current_level_boosts: i32,
    pub boosts: i32,
    pub gift_boosts: Option<i32>,
    pub next_level_boosts: Option<i32>,
    pub premium_audience: Option<StatsPercentSummary>,
    pub boost_url: String,
    pub prepaid_giveaway_count: usize,
    pub my_boost: bool,
    pub my_boost_slots: Vec<i32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct BoostSummary {
    pub boost_id: String,
    pub user_id: Option<i64>,
    pub applied_at_unix: i32,
    pub expires_at_unix: i32,
    pub gift: bool,
    pub giveaway: bool,
    pub unclaimed: bool,
    pub giveaway_message_id: Option<i32>,
    pub used_gift_slug: Option<String>,
    pub multiplier: Option<i32>,
    pub stars: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct BoostPage {
    pub total_count: i32,
    pub returned_count: usize,
    pub boosts: Vec<BoostSummary>,
    pub next_offset: Option<String>,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(default, deny_unknown_fields)]
pub struct AdminLogEventFilter {
    pub join: bool,
    pub leave: bool,
    pub invite: bool,
    pub ban: bool,
    pub unban: bool,
    pub kick: bool,
    pub unkick: bool,
    pub promote: bool,
    pub demote: bool,
    pub info: bool,
    pub settings: bool,
    pub pinned: bool,
    pub edit: bool,
    pub delete: bool,
    pub group_call: bool,
    pub invites: bool,
    pub send: bool,
    pub forums: bool,
    pub sub_extend: bool,
    pub edit_rank: bool,
}

impl AdminLogEventFilter {
    pub fn is_empty(self) -> bool {
        !self.join
            && !self.leave
            && !self.invite
            && !self.ban
            && !self.unban
            && !self.kick
            && !self.unkick
            && !self.promote
            && !self.demote
            && !self.info
            && !self.settings
            && !self.pinned
            && !self.edit
            && !self.delete
            && !self.group_call
            && !self.invites
            && !self.send
            && !self.forums
            && !self.sub_extend
            && !self.edit_rank
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AdminLogActionKind {
    ChangeTitle,
    ChangeAbout,
    ChangeUsername,
    ChangePhoto,
    ToggleInvites,
    ToggleSignatures,
    UpdatePinned,
    EditMessage,
    DeleteMessage,
    ParticipantJoin,
    ParticipantLeave,
    ParticipantInvite,
    ParticipantToggleBan,
    ParticipantToggleAdmin,
    ChangeStickerSet,
    TogglePreHistoryHidden,
    DefaultBannedRights,
    StopPoll,
    ChangeLinkedChat,
    ChangeLocation,
    ToggleSlowMode,
    StartGroupCall,
    DiscardGroupCall,
    ParticipantMute,
    ParticipantUnmute,
    ToggleGroupCallSetting,
    ParticipantJoinByInvite,
    ExportedInviteDelete,
    ExportedInviteRevoke,
    ExportedInviteEdit,
    ParticipantVolume,
    ChangeHistoryTtl,
    ParticipantJoinByRequest,
    ToggleNoForwards,
    SendMessage,
    ChangeAvailableReactions,
    ChangeUsernames,
    ToggleForum,
    CreateTopic,
    EditTopic,
    DeleteTopic,
    PinTopic,
    ToggleAntiSpam,
    ChangePeerColor,
    ChangeProfilePeerColor,
    ChangeWallpaper,
    ChangeEmojiStatus,
    ChangeEmojiStickerSet,
    ToggleSignatureProfiles,
    ParticipantSubExtend,
    ToggleAutotranslation,
    ParticipantEditRank,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct AdminLogEventSummary {
    pub event_id: i64,
    pub date_unix: i32,
    pub actor_user_id: i64,
    pub actor: Option<ContactSummary>,
    pub action: AdminLogActionKind,
    pub details: Option<String>,
    pub target_user_id: Option<i64>,
    pub target_user: Option<ContactSummary>,
    pub message_id: Option<i32>,
    pub topic_id: Option<i32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct AdminLogPage {
    pub chat_id: String,
    pub returned_count: usize,
    pub events: Vec<AdminLogEventSummary>,
    pub next_max_id: Option<i64>,
    pub completeness: Completeness,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct ChannelSettingsSummary {
    pub details: ChatDetailsSummary,
    pub can_set_username: bool,
    pub can_view_stats: bool,
    pub signatures_enabled: bool,
    pub signature_profiles_enabled: bool,
    pub pre_history_hidden: bool,
    pub join_request_required: bool,
    pub join_to_send_required: bool,
    pub slow_mode_seconds: i32,
    pub no_forwards: bool,
    pub forum_enabled: bool,
    pub history_ttl_seconds: Option<i32>,
    pub linked_chat_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ChatReactionsPolicy {
    None,
    All {
        #[serde(default)]
        allow_custom: bool,
    },
    Some {
        reactions: Vec<ReactionIdentity>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct ChatModerationSettingsSummary {
    pub details: ChatDetailsSummary,
    #[schemars(
        description = "Telegram's exact default restrictions for non-admin members. true means the capability is banned. None means Telegram omitted the field."
    )]
    pub default_banned_rights: Option<ChatBannedRightsSummary>,
    #[schemars(description = "Telegram's exact available-reaction policy, when returned.")]
    pub available_reactions: Option<ChatReactionsPolicy>,
    pub reactions_limit: Option<i32>,
    #[schemars(description = "Present only for supergroups where native anti-spam is meaningful.")]
    pub anti_spam_enabled: Option<bool>,
    #[schemars(description = "Present only for supergroups where forum mode is meaningful.")]
    pub forum_enabled: Option<bool>,
    #[schemars(description = "Present only for supergroups on layers that expose forum tabs.")]
    pub forum_tabs_enabled: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct StorySummary {
    /// active, skipped, or deleted as returned by Telegram's StoryItem union.
    pub state: String,
    pub story_id: i32,
    pub date_unix: i32,
    pub expire_date_unix: i32,
    pub from_ref: Option<String>,
    pub caption: Option<String>,
    pub pinned: bool,
    pub public: bool,
    pub close_friends: bool,
    pub noforwards: bool,
    pub edited: bool,
    pub contacts: bool,
    pub selected_contacts: bool,
    pub outgoing: bool,
    pub live: bool,
    pub media_kind: Option<String>,
    pub views_count: Option<i32>,
    pub forwards_count: Option<i32>,
    pub reactions_count: Option<i32>,
    pub sent_reaction: Option<ReactionSpec>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct StoryPeerSummary {
    pub peer_ref: String,
    pub max_read_id: Option<i32>,
    pub stories: Vec<StorySummary>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct StoryListPage {
    pub state: Option<String>,
    pub has_more: bool,
    pub count: i32,
    pub peers: Vec<StoryPeerSummary>,
    pub stealth_mode_active_until_unix: Option<i32>,
    pub stealth_mode_cooldown_until_unix: Option<i32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct StoryPeerPage {
    pub peer: StoryPeerSummary,
    pub count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct StoryCollectionPage {
    pub peer_ref: String,
    pub count: i32,
    pub stories: Vec<StorySummary>,
    pub pinned_to_top_ids: Vec<i32>,
    pub next_offset_id: Option<i32>,
    pub completeness: Completeness,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct StoryLinkSummary {
    pub peer_ref: String,
    pub story_id: i32,
    pub link: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct StoryReportOption {
    pub text: String,
    #[schemars(description = "Telegram's exact opaque report option bytes as lowercase hex.")]
    pub option_hex: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum StoryReportResult {
    ChooseOption { title: String, options: Vec<StoryReportOption> },
    AddComment { optional: bool, option_hex: String },
    Reported,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct StoryMutationSnapshot {
    pub story_id: i32,
    pub state: String,
    pub date_unix: i32,
    pub expire_date_unix: i32,
    pub media_kind: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct StoryPinSnapshot {
    pub story: StoryMutationSnapshot,
    pub pinned: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct StoryReactionSnapshot {
    pub story: StoryMutationSnapshot,
    pub sent_reaction: Option<ReactionSpec>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct StoryEditSnapshot {
    pub story: StoryMutationSnapshot,
    pub caption: Option<String>,
    pub public: bool,
    pub close_friends: bool,
    pub contacts: bool,
    pub selected_contacts: bool,
    pub edited: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct StoryTopPinSnapshot {
    pub story: StoryMutationSnapshot,
    pub pinned_to_top: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct StoryStealthModeSummary {
    pub active_until_unix: Option<i32>,
    pub cooldown_until_unix: Option<i32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct StoryPrivacySpec {
    /// everyone, contacts, close_friends, or selected_contacts.
    pub kind: String,
    pub selected_user_ids: Vec<i64>,
    pub disallowed_user_ids: Vec<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct StoryViewerSummary {
    pub kind: String,
    pub user_id: Option<i64>,
    pub source_peer_ref: Option<String>,
    pub user: Option<ContactSummary>,
    pub date_unix: Option<i32>,
    pub blocked: bool,
    pub blocked_my_stories_from: bool,
    pub reaction_kind: Option<String>,
    pub reaction_emoji: Option<String>,
    pub reaction_custom_emoji_id: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct StoryViewersPage {
    pub story_id: i32,
    pub count: i32,
    pub views_count: i32,
    pub forwards_count: i32,
    pub reactions_count: i32,
    pub viewers: Vec<StoryViewerSummary>,
    pub next_offset: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct MessageReadReceipt {
    pub user_id: i64,
    pub read_at_unix: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct UserDetails {
    pub user: ContactSummary,
    pub about: Option<String>,
    pub blocked: bool,
    pub common_chats_count: i32,
    pub phone_calls_available: bool,
    pub phone_calls_private: bool,
    pub status: Option<String>,
    pub status_timestamp_unix: Option<i32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct BotCommandSummary {
    pub command: String,
    pub description: String,
    #[serde(default)]
    pub ephemeral: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct BotCommandSetSummary {
    pub bot: ContactSummary,
    pub language_code: String,
    pub commands: Vec<BotCommandSummary>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum EmojiStatusCatalogKind {
    Default,
    Recent,
    ChannelDefault,
    Collectible,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum EmojiStatusKind {
    Empty,
    Default,
    Collectible,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct EmojiStatusSummary {
    pub kind: EmojiStatusKind,
    #[schemars(description = "Exact Telegram document id encoded as decimal text.")]
    pub document_id: Option<String>,
    #[schemars(description = "Exact Telegram collectible id encoded as decimal text.")]
    pub collectible_id: Option<String>,
    pub until_unix: Option<i32>,
    pub title: Option<String>,
    pub slug: Option<String>,
    #[schemars(description = "Exact Telegram pattern document id encoded as decimal text.")]
    pub pattern_document_id: Option<String>,
    pub center_color: Option<i32>,
    pub edge_color: Option<i32>,
    pub pattern_color: Option<i32>,
    pub text_color: Option<i32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AccountEmojiStatusSpec {
    Empty,
    Default {
        #[schemars(description = "Exact positive Telegram document id encoded as decimal text.")]
        document_id: String,
        until_unix: Option<i32>,
    },
    Collectible {
        #[schemars(
            description = "Exact positive Telegram collectible id encoded as decimal text."
        )]
        collectible_id: String,
        until_unix: Option<i32>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct EmojiStatusCatalogPage {
    pub kind: EmojiStatusCatalogKind,
    #[schemars(description = "Exact request hash encoded as signed decimal text.")]
    pub request_hash: String,
    #[schemars(description = "Exact response hash encoded as signed decimal text when returned.")]
    pub response_hash: Option<String>,
    pub not_modified: bool,
    pub available_count: Option<usize>,
    pub returned_count: usize,
    pub truncated: bool,
    pub statuses: Vec<EmojiStatusSummary>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct BirthdaySummary {
    pub day: i32,
    pub month: i32,
    pub year: Option<i32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PeerColorSummary {
    Simple {
        color: Option<i32>,
        #[schemars(
            description = "Exact Telegram custom-emoji document id encoded as decimal text."
        )]
        background_emoji_id: Option<String>,
    },
    Collectible {
        #[schemars(description = "Exact Telegram collectible id encoded as decimal text.")]
        collectible_id: String,
        #[schemars(description = "Exact Telegram gift-emoji document id encoded as decimal text.")]
        gift_emoji_id: String,
        #[schemars(
            description = "Exact Telegram background-emoji document id encoded as decimal text."
        )]
        background_emoji_id: String,
        accent_color: i32,
        colors: Vec<i32>,
        dark_accent_color: Option<i32>,
        dark_colors: Option<Vec<i32>>,
    },
    InputCollectible {
        #[schemars(description = "Exact Telegram collectible id encoded as decimal text.")]
        collectible_id: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct PersonalChannelSummary {
    pub chat: ChatSummary,
    pub message_id: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct AccountProfileSummary {
    pub user_id: i64,
    pub first_name: String,
    pub last_name: Option<String>,
    pub username: Option<String>,
    pub about: Option<String>,
    pub birthday: Option<BirthdaySummary>,
    pub name_color: Option<PeerColorSummary>,
    pub profile_color: Option<PeerColorSummary>,
    pub personal_channel: Option<PersonalChannelSummary>,
    pub business: BusinessAccountSettingsSummary,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct ProfilePhotoSummary {
    #[schemars(
        description = "Opaque account-scoped handle. Raw Telegram photo ids, access hashes, and file references are never exposed."
    )]
    pub photo_handle: String,
    pub date_unix: i32,
    pub has_stickers: bool,
    pub has_video: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct ProfilePhotoPage {
    pub photos: Vec<ProfilePhotoSummary>,
    pub offset: usize,
    pub next_offset: Option<usize>,
    pub total_count: usize,
    pub completeness: Completeness,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct ParticipantSummary {
    pub user: ContactSummary,
    pub role: String,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ParticipantFilter {
    #[default]
    Recent,
    Admins,
    Kicked,
    Banned,
    Bots,
    Contacts,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct ParticipantPage {
    pub chat_id: String,
    pub filter: ParticipantFilter,
    pub query: Option<String>,
    pub offset: usize,
    pub limit: usize,
    pub total_count: usize,
    pub returned_count: usize,
    pub participants: Vec<ChatMemberSummary>,
    pub next_offset: Option<usize>,
    pub has_more: bool,
    pub completeness: Completeness,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct ChatAdminRightsSummary {
    pub change_info: bool,
    pub post_messages: bool,
    pub edit_messages: bool,
    pub delete_messages: bool,
    pub ban_users: bool,
    pub invite_users: bool,
    pub pin_messages: bool,
    pub add_admins: bool,
    pub anonymous: bool,
    pub manage_call: bool,
    pub other: bool,
    pub manage_topics: bool,
    pub post_stories: bool,
    pub edit_stories: bool,
    pub delete_stories: bool,
    pub manage_direct_messages: bool,
    pub manage_ranks: bool,
    pub manage_linked_peers: bool,
}

impl ChatAdminRightsSummary {
    pub fn any(&self) -> bool {
        self.change_info
            || self.post_messages
            || self.edit_messages
            || self.delete_messages
            || self.ban_users
            || self.invite_users
            || self.pin_messages
            || self.add_admins
            || self.anonymous
            || self.manage_call
            || self.other
            || self.manage_topics
            || self.post_stories
            || self.edit_stories
            || self.delete_stories
            || self.manage_direct_messages
            || self.manage_ranks
            || self.manage_linked_peers
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct ChatBannedRightsSummary {
    pub view_messages: bool,
    pub send_messages: bool,
    pub send_media: bool,
    pub send_stickers: bool,
    pub send_gifs: bool,
    pub send_games: bool,
    pub send_inline: bool,
    pub embed_links: bool,
    pub send_polls: bool,
    pub change_info: bool,
    pub invite_users: bool,
    pub pin_messages: bool,
    pub manage_topics: bool,
    pub send_photos: bool,
    pub send_videos: bool,
    pub send_roundvideos: bool,
    pub send_audios: bool,
    pub send_voices: bool,
    pub send_docs: bool,
    pub send_plain: bool,
    pub edit_rank: bool,
    pub send_reactions: bool,
    pub manage_linked_peers: bool,
    #[schemars(description = "Unix timestamp; 0 means no expiry.")]
    pub until_date_unix: i32,
}

impl ChatBannedRightsSummary {
    pub fn any_restriction(&self) -> bool {
        self.view_messages
            || self.send_messages
            || self.send_media
            || self.send_stickers
            || self.send_gifs
            || self.send_games
            || self.send_inline
            || self.embed_links
            || self.send_polls
            || self.change_info
            || self.invite_users
            || self.pin_messages
            || self.manage_topics
            || self.send_photos
            || self.send_videos
            || self.send_roundvideos
            || self.send_audios
            || self.send_voices
            || self.send_docs
            || self.send_plain
            || self.edit_rank
            || self.send_reactions
            || self.manage_linked_peers
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ChatMemberRole {
    Member,
    Creator,
    Admin,
    Restricted,
    Banned,
    Left,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct ChatMemberSummary {
    pub chat_id: String,
    pub user: ContactSummary,
    pub role: ChatMemberRole,
    pub can_edit: bool,
    pub is_self: bool,
    pub left: bool,
    pub joined_date_unix: Option<i32>,
    pub inviter_user_id: Option<i64>,
    pub promoted_by_user_id: Option<i64>,
    pub kicked_by_user_id: Option<i64>,
    pub rank: Option<String>,
    pub subscription_until_date_unix: Option<i32>,
    pub via_request: bool,
    pub admin_rights: Option<ChatAdminRightsSummary>,
    pub banned_rights: Option<ChatBannedRightsSummary>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum InviteChatKind {
    BasicGroup,
    BroadcastChannel,
    Supergroup,
    Gigagroup,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct InviteChatBinding {
    pub chat_id: String,
    pub kind: InviteChatKind,
    pub title_at_prepare: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct InviteUserBinding {
    pub index: usize,
    pub source_ref: String,
    pub user_id: i64,
    pub display_name_at_prepare: String,
    pub username_at_prepare: Option<String>,
    pub before: Option<ChatMemberSummary>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum InviteTargetState {
    Pending,
    Issuing,
    RetryWait,
    Confirmed,
    AlreadyPresent,
    Rejected,
    OutcomeUnknown,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum InviteConfirmation {
    ActiveBeforeCall,
    ProviderReceiptAndReadback,
    ActiveObservedAfterUncertain,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct InviteTargetProgress {
    pub target: InviteUserBinding,
    pub state: InviteTargetState,
    pub attempt_count: u32,
    pub observation_count: u32,
    pub next_action_at_unix: Option<i64>,
    pub last_error_code: Option<String>,
    pub confirmation: Option<InviteConfirmation>,
    pub member: Option<ChatMemberSummary>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct InviteBatchProgress {
    pub chat: InviteChatBinding,
    pub total_count: usize,
    pub pending_count: usize,
    pub issuing_count: usize,
    pub retry_wait_count: usize,
    pub confirmed_count: usize,
    pub already_present_count: usize,
    pub rejected_count: usize,
    pub outcome_unknown_count: usize,
    pub next_action_at_unix: Option<i64>,
    pub targets: Vec<InviteTargetProgress>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum InviteLinkKind {
    Exported,
    PublicJoinRequests,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct InviteSubscriptionPricingSummary {
    pub period_seconds: i32,
    pub amount_stars: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct InviteLinkSummary {
    pub invite_handle: String,
    pub kind: InviteLinkKind,
    pub revoked: bool,
    pub permanent: bool,
    pub request_needed: bool,
    pub admin_user_id: Option<i64>,
    pub date_unix: Option<i32>,
    pub start_date_unix: Option<i32>,
    pub expire_date_unix: Option<i32>,
    pub usage_limit: Option<i32>,
    pub usage_count: Option<i32>,
    pub requested_count: Option<i32>,
    pub subscription_expired_count: Option<i32>,
    pub title: Option<String>,
    pub subscription_pricing: Option<InviteSubscriptionPricingSummary>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct InviteJoinerSummary {
    pub user_id: i64,
    pub user: Option<ContactSummary>,
    pub requested: bool,
    pub via_chatlist: bool,
    pub date_unix: i32,
    pub about: Option<String>,
    pub approved_by_user_id: Option<i64>,
    #[schemars(
        description = "Present only when the caller filtered by one exact opaque invite handle; Telegram does not identify the source link in an unfiltered result."
    )]
    pub source_invite_handle: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct InviteJoinerPage {
    pub total_count: usize,
    pub entries: Vec<InviteJoinerSummary>,
    pub next_offset_date_unix: Option<i32>,
    pub next_offset_user_id: Option<i64>,
    pub completeness: Completeness,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq, Hash)]
pub struct JoinRequestSnapshot {
    pub user_id: i64,
    pub date_unix: i32,
    pub via_chatlist: bool,
    pub source_invite_handle: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct SessionSummary {
    pub authorization_id: i64,
    pub current: bool,
    pub official_app: bool,
    pub password_pending: bool,
    pub encrypted_requests_disabled: bool,
    pub call_requests_disabled: bool,
    pub unconfirmed: bool,
    pub device_model: String,
    pub platform: String,
    pub system_version: String,
    pub api_id: i32,
    pub app_name: String,
    pub app_version: String,
    pub date_created_unix: i32,
    pub date_active_unix: i32,
    pub ip: String,
    pub country: String,
    pub region: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct DraftSummary {
    pub message: String,
    pub date_unix: i32,
    pub no_webpage: bool,
    pub invert_media: bool,
    pub has_reply_to: bool,
    pub has_entities: bool,
    pub has_media: bool,
    pub effect_id: Option<i64>,
    pub has_suggested_post: bool,
    pub has_rich_message: bool,
    #[schemars(
        description = "Lowercase SHA-256 of the complete Telegram DraftMessage TL object, including reply, entities, media, suggested-post, rich-message, and effect state not otherwise expanded here."
    )]
    pub state_sha256: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct DialogDraftSummary {
    pub chat_id: String,
    pub title: String,
    pub draft: DraftSummary,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct DialogStateSnapshot {
    pub pinned: bool,
    pub unread_mark: bool,
    pub top_message_id: i32,
    pub read_inbox_max_id: i32,
    pub unread_count: i32,
    pub unread_reactions_count: i32,
    pub archived: bool,
    pub folder_id: Option<i32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct DiscussionMessageSummary {
    pub source_chat_id: String,
    pub source_message_id: i32,
    pub messages: Vec<MessageSummary>,
    pub max_id: Option<i32>,
    pub read_inbox_max_id: Option<i32>,
    pub read_outbox_max_id: Option<i32>,
    pub unread_count: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct MessageLinkSummary {
    pub chat_id: String,
    pub message_id: i32,
    pub grouped: bool,
    pub thread: bool,
    pub link: String,
    pub html: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct WebPagePreviewSummary {
    pub state: String,
    pub webpage_id: Option<i64>,
    pub pending_until_unix: Option<i32>,
    pub url: Option<String>,
    pub display_url: Option<String>,
    pub page_type: Option<String>,
    pub site_name: Option<String>,
    pub title: Option<String>,
    pub description: Option<String>,
    pub author: Option<String>,
    pub embed_url: Option<String>,
    pub embed_type: Option<String>,
    pub embed_width: Option<i32>,
    pub embed_height: Option<i32>,
    pub duration_seconds: Option<i32>,
    pub has_photo: bool,
    pub has_document: bool,
    pub has_cached_page: bool,
    pub has_large_media: bool,
    pub video_cover_photo: bool,
    pub force_large_media: bool,
    pub force_small_media: bool,
    pub manual: bool,
    pub safe: bool,
    pub related_chats: Vec<ChatSummary>,
    pub related_users: Vec<ContactSummary>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct MessageReactionReadReceipt {
    pub pts: i32,
    pub pts_count: i32,
    pub offset: i32,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DialogFolderKind {
    Default,
    Custom,
    Chatlist,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct DialogFolderSummary {
    pub folder_id: i32,
    pub kind: DialogFolderKind,
    pub title: String,
    pub title_has_entities: bool,
    pub title_noanimate: bool,
    pub emoticon: Option<String>,
    pub color: Option<i32>,
    pub contacts: bool,
    pub non_contacts: bool,
    pub groups: bool,
    pub broadcasts: bool,
    pub bots: bool,
    pub exclude_muted: bool,
    pub exclude_read: bool,
    pub exclude_archived: bool,
    pub has_my_invites: bool,
    pub pinned_chat_ids: Vec<String>,
    pub included_chat_ids: Vec<String>,
    pub excluded_chat_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct DialogFolderList {
    pub tags_enabled: bool,
    pub folders: Vec<DialogFolderSummary>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct SuggestedDialogFolderSummary {
    pub description: String,
    pub folder: DialogFolderSummary,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct SuggestedDialogFolderList {
    pub suggestions: Vec<SuggestedDialogFolderSummary>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct DisallowedGiftsSettingsSummary {
    pub disallow_unlimited_stargifts: bool,
    pub disallow_limited_stargifts: bool,
    pub disallow_unique_stargifts: bool,
    pub disallow_premium_gifts: bool,
    pub disallow_stargifts_from_channels: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct GlobalPrivacySettingsSummary {
    pub archive_and_mute_new_noncontact_peers: bool,
    pub keep_archived_unmuted: bool,
    pub keep_archived_folders: bool,
    pub hide_read_marks: bool,
    pub new_noncontact_peers_require_premium: bool,
    pub display_gifts_button: bool,
    #[schemars(description = "Exact Telegram Stars amount as canonical positive decimal text.")]
    pub noncontact_peers_paid_stars: Option<String>,
    pub disallowed_gifts: Option<DisallowedGiftsSettingsSummary>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct NotificationSoundSummary {
    pub kind: String,
    pub title: Option<String>,
    pub data: Option<String>,
    pub ringtone_id: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct NotificationSettingsSummary {
    pub show_previews: Option<bool>,
    pub silent: Option<bool>,
    pub mute_until_unix: Option<i32>,
    pub ios_sound: Option<NotificationSoundSummary>,
    pub android_sound: Option<NotificationSoundSummary>,
    pub other_sound: Option<NotificationSoundSummary>,
    pub stories_muted: Option<bool>,
    pub stories_hide_sender: Option<bool>,
    pub stories_ios_sound: Option<NotificationSoundSummary>,
    pub stories_android_sound: Option<NotificationSoundSummary>,
    pub stories_other_sound: Option<NotificationSoundSummary>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct NotificationSettingsPatch {
    pub show_previews: Option<bool>,
    pub silent: Option<bool>,
    pub mute_until_unix: Option<i32>,
    pub sound: Option<NotificationSoundSummary>,
    pub stories_muted: Option<bool>,
    pub stories_hide_sender: Option<bool>,
    pub stories_sound: Option<NotificationSoundSummary>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum NotificationSoundPlatform {
    Ios,
    Android,
    Other,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct NotificationSoundTarget {
    pub authorization_id: i64,
    pub platform: NotificationSoundPlatform,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct ForumTopicSummary {
    pub topic_id: i32,
    pub deleted: bool,
    pub title: Option<String>,
    pub date_unix: Option<i32>,
    pub creator_ref: Option<String>,
    pub my: bool,
    pub closed: bool,
    pub pinned: bool,
    pub short: bool,
    pub hidden: bool,
    pub title_missing: bool,
    pub icon_color: Option<i32>,
    pub icon_emoji_id: Option<i64>,
    pub top_message_id: Option<i32>,
    pub read_inbox_max_id: Option<i32>,
    pub read_outbox_max_id: Option<i32>,
    pub unread_count: Option<i32>,
    pub unread_mentions_count: Option<i32>,
    pub unread_reactions_count: Option<i32>,
    pub unread_poll_votes_count: Option<i32>,
    pub notification_settings: Option<NotificationSettingsSummary>,
    pub draft: Option<DraftSummary>,
}

impl ForumTopicSummary {
    pub fn mutation_snapshot(&self) -> ForumTopicMutationSnapshot {
        ForumTopicMutationSnapshot {
            topic_id: self.topic_id,
            deleted: self.deleted,
            title: self.title.clone(),
            creator_ref: self.creator_ref.clone(),
            my: self.my,
            closed: self.closed,
            pinned: self.pinned,
            short: self.short,
            hidden: self.hidden,
            title_missing: self.title_missing,
            icon_color: self.icon_color,
            icon_emoji_id: self.icon_emoji_id,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct ForumTopicMutationSnapshot {
    pub topic_id: i32,
    pub deleted: bool,
    pub title: Option<String>,
    pub creator_ref: Option<String>,
    pub my: bool,
    pub closed: bool,
    pub pinned: bool,
    pub short: bool,
    pub hidden: bool,
    pub title_missing: bool,
    pub icon_color: Option<i32>,
    pub icon_emoji_id: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum PrivacyKey {
    StatusTimestamp,
    ChatInvite,
    PhoneCall,
    PhoneP2p,
    Forwards,
    ProfilePhoto,
    PhoneNumber,
    AddedByPhone,
    VoiceMessages,
    About,
    Birthday,
    StarGiftsAutoSave,
    NoPaidMessages,
    SavedMusic,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct PrivacyRuleSummary {
    pub kind: String,
    pub user_ids: Vec<i64>,
    pub chat_ids: Vec<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct PrivacyUserBinding {
    pub source_ref: String,
    pub user: ContactSummary,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct StickerSetSummary {
    pub sticker_set_id: i64,
    pub title: String,
    pub short_name: String,
    pub installed_date_unix: Option<i32>,
    pub archived: bool,
    pub official: bool,
    pub masks: bool,
    pub emojis: bool,
    pub text_color: bool,
    pub channel_emoji_status: bool,
    pub creator: bool,
    pub thumb_dc_id: Option<i32>,
    pub thumb_version: Option<i32>,
    pub thumb_document_id: Option<i64>,
    pub sticker_count: i32,
    pub content_hash: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct StickerDocumentSummary {
    #[schemars(
        description = "Opaque document-bound handle. Raw Telegram document ids, access hashes, and file references are never exposed."
    )]
    pub sticker_handle: String,
    pub set_short_name: Option<String>,
    pub emoji: Option<String>,
    pub mime_type: String,
    pub size_bytes: u64,
    pub date_unix: i32,
    pub recent_date_unix: Option<i32>,
    pub animated: bool,
    pub video: bool,
    pub custom_emoji: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct CustomEmojiDocumentSummary {
    #[schemars(
        description = "Telegram custom-emoji document id returned by reaction and emoji-status surfaces. It is an identity reference, not a download credential."
    )]
    pub document_id: i64,
    pub sticker: StickerDocumentSummary,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct CustomEmojiDocumentsPage {
    pub requested_count: usize,
    pub returned_count: usize,
    pub missing_document_ids: Vec<i64>,
    pub documents: Vec<CustomEmojiDocumentSummary>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct DownloadedCustomEmoji {
    pub custom_emoji: CustomEmojiDocumentSummary,
    pub local_path: String,
    pub bytes_written: u64,
    pub sha256: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct StickerEmojiPackSummary {
    pub emoji: String,
    pub sticker_handles: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct StickerKeywordSummary {
    pub sticker_handle: String,
    pub keywords: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct StickerCatalogPage {
    pub not_modified: bool,
    pub hash: Option<i64>,
    pub attached: bool,
    pub total_count: usize,
    pub returned_count: usize,
    pub has_more: bool,
    pub stickers: Vec<StickerDocumentSummary>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct StickerSetDetailsPage {
    pub not_modified: bool,
    pub sticker_set: Option<StickerSetSummary>,
    pub packs: Vec<StickerEmojiPackSummary>,
    pub keywords: Vec<StickerKeywordSummary>,
    pub stickers: Vec<StickerDocumentSummary>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct StickerSetSearchPage {
    pub not_modified: bool,
    pub hash: Option<i64>,
    pub total_count: usize,
    pub returned_count: usize,
    pub has_more: bool,
    pub sticker_sets: Vec<StickerSetSummary>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct MessageReactionSummary {
    pub peer_ref: String,
    pub date_unix: i32,
    pub big: bool,
    pub unread: bool,
    pub my: bool,
    pub kind: String,
    pub emoji: Option<String>,
    pub custom_emoji_id: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct MessageReactionPage {
    pub total_count: i32,
    pub reactions: Vec<MessageReactionSummary>,
    pub next_offset: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct OutboxReadDateSummary {
    pub chat_id: String,
    pub message_id: i32,
    pub read_at_unix: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ReactionSpec {
    Remove,
    Emoji {
        emoji: String,
        #[serde(default)]
        big: bool,
        #[serde(default)]
        add_to_recent: bool,
    },
    CustomEmoji {
        document_id: i64,
        #[serde(default)]
        big: bool,
        #[serde(default)]
        add_to_recent: bool,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ReactionIdentity {
    Empty,
    Emoji { emoji: String },
    CustomEmoji { document_id: i64 },
    Paid,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum DefaultReactionSpec {
    Emoji { emoji: String },
    CustomEmoji { document_id: i64 },
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct DefaultReactionSummary {
    pub reaction: Option<ReactionIdentity>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct ReactionCatalogPage {
    pub not_modified: bool,
    pub hash: Option<i64>,
    pub reactions: Vec<ReactionIdentity>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct PollOptionBinding {
    pub index: usize,
    pub text: String,
    #[schemars(
        description = "Telegram's exact opaque poll option bytes encoded as lowercase hex."
    )]
    pub option_hex: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct PollAnswerSummary {
    pub option: PollOptionBinding,
    pub media_type: Option<String>,
    pub added_by_ref: Option<String>,
    pub date_unix: Option<i32>,
    pub chosen: bool,
    pub correct: bool,
    pub voters: Option<i32>,
    pub recent_voter_refs: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct PollSummary {
    pub chat_id: String,
    pub message_id: i32,
    pub poll_id: i64,
    pub closed: bool,
    pub public_voters: bool,
    pub multiple_choice: bool,
    pub quiz: bool,
    pub open_answers: bool,
    pub revoting_disabled: bool,
    pub shuffle_answers: bool,
    pub hide_results_until_close: bool,
    pub creator: bool,
    pub subscribers_only: bool,
    pub question: String,
    pub answers: Vec<PollAnswerSummary>,
    pub unmatched_result_option_hex: Vec<String>,
    pub close_period_seconds: Option<i32>,
    pub close_date_unix: Option<i32>,
    pub countries_iso2: Vec<String>,
    pub poll_hash: i64,
    pub results_minimal: bool,
    pub has_unread_votes: bool,
    pub can_view_stats: bool,
    pub total_voters: Option<i32>,
    pub recent_voter_refs: Vec<String>,
    pub solution: Option<String>,
    pub solution_has_media: bool,
    pub attached_media_type: Option<String>,
}

impl PollSummary {
    pub fn selected_option_indices(&self) -> Vec<usize> {
        self.answers
            .iter()
            .filter(|answer| answer.chosen)
            .map(|answer| answer.option.index)
            .collect()
    }

    pub fn mutation_snapshot(&self) -> PollMutationSnapshot {
        PollMutationSnapshot {
            message_id: self.message_id,
            poll_id: self.poll_id,
            closed: self.closed,
            public_voters: self.public_voters,
            multiple_choice: self.multiple_choice,
            quiz: self.quiz,
            open_answers: self.open_answers,
            revoting_disabled: self.revoting_disabled,
            shuffle_answers: self.shuffle_answers,
            hide_results_until_close: self.hide_results_until_close,
            subscribers_only: self.subscribers_only,
            question: self.question.clone(),
            options: self.answers.iter().map(|answer| answer.option.clone()).collect(),
            close_period_seconds: self.close_period_seconds,
            close_date_unix: self.close_date_unix,
            countries_iso2: self.countries_iso2.clone(),
            selected_option_indices: self.selected_option_indices(),
            attached_media_type: self.attached_media_type.clone(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct PollMutationSnapshot {
    pub message_id: i32,
    pub poll_id: i64,
    pub closed: bool,
    pub public_voters: bool,
    pub multiple_choice: bool,
    pub quiz: bool,
    pub open_answers: bool,
    pub revoting_disabled: bool,
    pub shuffle_answers: bool,
    pub hide_results_until_close: bool,
    pub subscribers_only: bool,
    pub question: String,
    pub options: Vec<PollOptionBinding>,
    pub close_period_seconds: Option<i32>,
    pub close_date_unix: Option<i32>,
    pub countries_iso2: Vec<String>,
    pub selected_option_indices: Vec<usize>,
    pub attached_media_type: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct PollCreateSpec {
    pub question: String,
    pub answers: Vec<String>,
    pub public_voters: bool,
    pub multiple_choice: bool,
    pub quiz: bool,
    pub correct_option_index: Option<usize>,
    pub open_answers: bool,
    pub revoting_disabled: bool,
    pub shuffle_answers: bool,
    pub hide_results_until_close: bool,
    pub subscribers_only: bool,
    pub countries_iso2: Vec<String>,
    pub close_period_seconds: Option<i32>,
    pub close_date_unix: Option<i32>,
    pub solution: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct PollVoterSummary {
    pub peer_ref: String,
    pub option_indices: Vec<usize>,
    #[schemars(
        description = "Opaque option bytes that were not present in the current poll answer list, encoded as lowercase hex."
    )]
    pub unknown_option_hex: Vec<String>,
    pub submitted_open_answer: bool,
    pub date_unix: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct PollVoterPage {
    pub total_count: i32,
    pub voters: Vec<PollVoterSummary>,
    pub next_offset: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum DeletedMessageRestoreMedia {
    File {
        artifact: MutationFileArtifact,
        mode: FileSendMode,
        expected_media_type: String,
        album_media_kind: Option<AlbumMediaKind>,
    },
    Voice {
        artifact: MutationFileArtifact,
    },
    VideoNote {
        artifact: MutationFileArtifact,
        duration_seconds: i32,
        length_pixels: i32,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct DeletedMessageRestoreSnapshot {
    #[schemars(
        description = "Telegram HTML reconstructed from the exact visible text and formatting entities before deletion."
    )]
    pub body_html: String,
    pub topic_id: Option<i32>,
    pub grouped_id: Option<i64>,
    pub media: Option<DeletedMessageRestoreMedia>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct MutationMessageSnapshot {
    pub message_id: i32,
    pub sender_ref: Option<String>,
    pub outgoing: bool,
    pub pinned: bool,
    pub silent: bool,
    pub noforwards: bool,
    pub text: Option<String>,
    pub date_unix: i32,
    pub edit_date_unix: Option<i32>,
    pub reply_to_message_id: Option<i32>,
    pub media_type: Option<String>,
    pub forward_origin: Option<MessageForwardOrigin>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub restore: Option<DeletedMessageRestoreSnapshot>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct RestoredMessageMapping {
    pub original_message_id: i32,
    pub restored_message_id: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct MutationFileArtifact {
    pub artifact_id: String,
    pub file_name: String,
    pub size_bytes: u64,
    pub sha256: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct GeoPointSpec {
    #[schemars(description = "Latitude in signed 1e-7 degree units.")]
    pub latitude_e7: i32,
    #[schemars(description = "Longitude in signed 1e-7 degree units.")]
    pub longitude_e7: i32,
    pub accuracy_radius_m: Option<i32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RichMediaSpec {
    Location {
        point: GeoPointSpec,
        live_period_seconds: Option<i32>,
        heading_degrees: Option<i32>,
        proximity_notification_radius_m: Option<i32>,
    },
    Venue {
        point: GeoPointSpec,
        title: String,
        address: String,
        provider: String,
        venue_id: String,
        venue_type: String,
    },
    Contact {
        phone_number: String,
        first_name: String,
        last_name: String,
        vcard: String,
    },
    Dice {
        emoji: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct SentRichMediaSummary {
    pub media: RichMediaSpec,
    pub dice_value: Option<i32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct SentDocumentMediaSummary {
    pub media_type: String,
    pub mime_type: String,
    pub duration_seconds: Option<i32>,
    pub width_px: Option<i32>,
    pub height_px: Option<i32>,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub enum CaptionParseMode {
    #[default]
    #[serde(rename = "plain")]
    Plain,
    #[serde(rename = "md")]
    Markdown,
    #[serde(rename = "html")]
    Html,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct DiceEmojiCatalog {
    pub hash: i32,
    pub emojis: Vec<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AlbumMediaKind {
    Photo,
    Video,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct MutationAlbumItem {
    pub artifact: MutationFileArtifact,
    pub media_kind: AlbumMediaKind,
    pub caption: String,
}

#[derive(Debug, Clone)]
pub struct AlbumUploadSpec {
    pub file_path: PathBuf,
    pub media_kind: AlbumMediaKind,
    pub caption: String,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TypingAction {
    #[default]
    Typing,
    UploadPhoto,
    UploadDocument,
    Cancel,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MessageButtonMarkupType {
    None,
    InlineKeyboard,
    ReplyKeyboard,
    ForceReply,
    HideKeyboard,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MessageButtonKind {
    Plain,
    Url,
    Callback,
    RequestPhone,
    RequestGeoLocation,
    SwitchInline,
    Game,
    Buy,
    UrlAuth,
    InputUrlAuth,
    RequestPoll,
    InputUserProfile,
    UserProfile,
    WebView,
    SimpleWebView,
    RequestPeer,
    InputRequestPeer,
    Copy,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct MessageButtonStyleSummary {
    pub background_primary: bool,
    pub background_danger: bool,
    pub background_success: bool,
    #[schemars(description = "Exact custom-emoji icon id encoded as decimal text.")]
    pub icon_custom_emoji_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct MessageButtonSummary {
    pub row: usize,
    pub column: usize,
    pub index: usize,
    pub kind: MessageButtonKind,
    pub label: String,
    pub style: Option<MessageButtonStyleSummary>,
    #[schemars(
        description = "Opaque callback bytes encoded as standard padded base64. Treat as untrusted data."
    )]
    pub callback_data_base64: Option<String>,
    pub callback_data_sha256: Option<String>,
    pub callback_data_length: Option<usize>,
    pub url: Option<String>,
    pub switch_query: Option<String>,
    pub same_peer: Option<bool>,
    #[schemars(description = "Exact Telegram user id encoded as decimal text.")]
    pub user_id: Option<String>,
    #[schemars(description = "Exact Telegram bot user id encoded as decimal text.")]
    pub bot_user_id: Option<String>,
    pub button_id: Option<i32>,
    pub copy_text: Option<String>,
    pub requires_password: bool,
    pub quiz: Option<bool>,
    pub forward_text: Option<String>,
    pub request_write_access: Option<bool>,
    pub allowed_peer_types: Option<Vec<String>>,
    pub request_peer_kind: Option<String>,
    pub max_quantity: Option<i32>,
    pub requested_name: Option<bool>,
    pub requested_username: Option<bool>,
    pub requested_photo: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct MessageButtonInventory {
    pub chat_id: String,
    pub message_id: i32,
    pub message_date_unix: i32,
    pub markup_type: MessageButtonMarkupType,
    pub buttons: Vec<MessageButtonSummary>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct CallbackButtonSnapshot {
    pub chat_id: String,
    pub message_id: i32,
    pub message_date_unix: i32,
    pub row: usize,
    pub column: usize,
    pub index: usize,
    pub label: String,
    pub callback_data_sha256: String,
    pub callback_data_length: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct BotCallbackAnswerSummary {
    pub alert: bool,
    pub has_url: bool,
    pub native_ui: bool,
    pub message: Option<String>,
    pub url: Option<String>,
    pub cache_time_seconds: i32,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum InlineBotResultSourceKind {
    Result,
    MediaResult,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct InlineBotGeoSummary {
    pub point: Option<GeoPointSpec>,
    pub heading_degrees: Option<i32>,
    pub live_period_seconds: Option<i32>,
    pub proximity_notification_radius_m: Option<i32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct InlineBotResultSummary {
    pub id: String,
    #[serde(rename = "type")]
    pub result_type: String,
    pub source_kind: InlineBotResultSourceKind,
    pub title: Option<String>,
    pub description: Option<String>,
    pub url: Option<String>,
    pub media_kind: Option<String>,
    pub has_thumbnail: bool,
    pub has_content: bool,
    pub send_message_kind: String,
    pub send_message_text: Option<String>,
    pub send_message_url: Option<String>,
    pub send_message_geo: Option<InlineBotGeoSummary>,
    pub send_message_has_reply_markup: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct InlineBotSwitchPmSummary {
    pub text: String,
    pub start_param: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct InlineBotSwitchWebViewSummary {
    pub text: String,
    pub url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct InlineBotResults {
    pub bot: ContactSummary,
    pub context_chat_id: String,
    pub query: String,
    pub offset: String,
    #[schemars(description = "Telegram's exact signed 64-bit inline query id as decimal text.")]
    pub query_id: String,
    pub next_offset: Option<String>,
    pub cache_time_seconds: i32,
    pub gallery: bool,
    pub switch_pm: Option<InlineBotSwitchPmSummary>,
    pub switch_webview: Option<InlineBotSwitchWebViewSummary>,
    pub results: Vec<InlineBotResultSummary>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct InlineBotResultsPage {
    #[serde(flatten)]
    pub response: InlineBotResults,
    pub observed_at_unix: i64,
    pub expires_at_unix: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct InlineBotQuerySnapshot {
    pub bot: ContactSummary,
    pub context_chat_id: String,
    pub query: String,
    pub offset: String,
    pub query_id: String,
    pub observed_at_unix: i64,
    pub expires_at_unix: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum MutationAction {
    SendTypingAction {
        chat_id: String,
        action: TypingAction,
    },
    PressCallbackButton {
        button: CallbackButtonSnapshot,
    },
    SendInlineBotResult {
        query: InlineBotQuerySnapshot,
        result: InlineBotResultSummary,
        target_chat_id: String,
        reply_to: Option<MutationMessageSnapshot>,
        silent: bool,
        hide_via: bool,
        clear_draft: bool,
        clear_draft_before: Option<DraftSummary>,
        random_id: i64,
    },
    DeleteSavedHistory {
        before: SavedHistoryMutationSnapshot,
    },
    SetSavedDialogPinned {
        before: SavedDialogSummary,
        pinned: bool,
    },
    UpdateSavedReactionTag {
        before: SavedReactionTagSummary,
        title: Option<String>,
    },
    TranslateMessages {
        chat_id: String,
        messages: Vec<MutationMessageSnapshot>,
        to_language: String,
    },
    TranscribeAudio {
        chat_id: String,
        before: MutationMessageSnapshot,
    },
    RateTranscription {
        before: AudioTranscriptionSummary,
        good: bool,
    },
    EditFactCheck {
        chat_id: String,
        before: FactCheckSummary,
        text: String,
    },
    DeleteFactCheck {
        chat_id: String,
        before: FactCheckSummary,
    },
    EditMessage {
        chat_id: String,
        before: MutationMessageSnapshot,
        text: String,
    },
    DeleteMessages {
        chat_id: String,
        messages: Vec<MutationMessageSnapshot>,
        revoke: bool,
    },
    RestoreDeletedMessages {
        source_deletion_approval_id: String,
        chat_id: String,
        messages: Vec<MutationMessageSnapshot>,
        allow_sender_change: bool,
        random_ids: Vec<i64>,
    },
    SetMessagePinned {
        chat_id: String,
        before: MutationMessageSnapshot,
        pinned: bool,
        silent: bool,
        one_sided: bool,
    },
    SetMessageReaction {
        chat_id: String,
        before: MutationMessageSnapshot,
        before_reaction: Option<ReactionSpec>,
        reaction: ReactionSpec,
    },
    SetDefaultReaction {
        before: Option<ReactionIdentity>,
        reaction: DefaultReactionSpec,
    },
    SetDraft {
        chat_id: String,
        before: Option<DraftSummary>,
        text: Option<String>,
    },
    ClearAllDrafts {
        before: Vec<DialogDraftSummary>,
    },
    SetDialogUnread {
        chat_id: String,
        before: DialogStateSnapshot,
        unread: bool,
    },
    MarkMessagesRead {
        chat_id: String,
        before: DialogStateSnapshot,
        max_id: i32,
    },
    MarkReactionsRead {
        chat_id: String,
        before_unread_reactions_count: i32,
    },
    SetDialogArchived {
        chat_id: String,
        before: DialogStateSnapshot,
        archived: bool,
    },
    SetDialogPinned {
        chat_id: String,
        before: DialogStateSnapshot,
        pinned: bool,
    },
    SetUserBlocked {
        user: ContactSummary,
        before_blocked: bool,
        blocked: bool,
    },
    TerminateSession {
        before: SessionSummary,
    },
    TerminateAllOtherSessions {
        before: Vec<SessionSummary>,
    },
    SetChatAutoDelete {
        before: ChatAutoDeleteSummary,
        period_seconds: i32,
    },
    SetContact {
        before: ContactSummary,
        contact: bool,
        first_name: String,
        last_name: String,
        phone: String,
        add_phone_privacy_exception: bool,
    },
    SetPrivacyRules {
        key: PrivacyKey,
        before: Vec<PrivacyRuleSummary>,
        rules: Vec<PrivacyRuleSummary>,
        user_bindings: Vec<PrivacyUserBinding>,
    },
    SetNotificationSettings {
        chat_id: String,
        before: NotificationSettingsSummary,
        patch: NotificationSettingsPatch,
        sound_target: Option<NotificationSoundTarget>,
    },
    ScheduleMessage {
        chat_id: String,
        #[serde(default)]
        target: Option<PreparedSendTarget>,
        #[serde(default)]
        source: Option<PreparedOutboundSource>,
        text: String,
        reply_to: Option<MutationMessageSnapshot>,
        schedule_date_unix: i32,
        silent: bool,
        noforwards: bool,
        random_id: i64,
    },
    ScheduleFile {
        chat_id: String,
        #[serde(default)]
        target: Option<PreparedSendTarget>,
        #[serde(default)]
        source: Option<PreparedOutboundSource>,
        artifact: MutationFileArtifact,
        caption: String,
        caption_parse_mode: CaptionParseMode,
        reply_to: Option<MutationMessageSnapshot>,
        schedule_date_unix: i32,
        silent: bool,
        noforwards: bool,
        mode: FileSendMode,
        random_id: i64,
    },
    EditScheduledMessage {
        chat_id: String,
        before: MutationMessageSnapshot,
        text: String,
        schedule_date_unix: i32,
    },
    DeleteScheduledMessages {
        chat_id: String,
        messages: Vec<MutationMessageSnapshot>,
    },
    SendScheduledMessagesNow {
        chat_id: String,
        messages: Vec<MutationMessageSnapshot>,
    },
    CreatePoll {
        chat_id: String,
        poll: PollCreateSpec,
        reply_to: Option<MutationMessageSnapshot>,
        silent: bool,
        noforwards: bool,
        random_id: i64,
    },
    SetPollVote {
        chat_id: String,
        before: PollMutationSnapshot,
        options: Vec<PollOptionBinding>,
    },
    StopPoll {
        chat_id: String,
        before: PollMutationSnapshot,
    },
    CreateForumTopic {
        chat_id: String,
        title: String,
        icon_color: Option<i32>,
        icon_emoji_id: Option<i64>,
        random_id: i64,
    },
    EditForumTopic {
        chat_id: String,
        before: ForumTopicMutationSnapshot,
        title: Option<String>,
        icon_emoji_id: Option<i64>,
        closed: Option<bool>,
        hidden: Option<bool>,
    },
    SetForumTopicPinned {
        chat_id: String,
        before: ForumTopicMutationSnapshot,
        pinned: bool,
    },
    DeleteForumTopic {
        chat_id: String,
        before: ForumTopicMutationSnapshot,
    },
    AddChatMember {
        chat_id: String,
        user: ContactSummary,
        before: Option<ChatMemberSummary>,
    },
    InviteChatMembers {
        chat: InviteChatBinding,
        targets: Vec<InviteUserBinding>,
    },
    RemoveChatMember {
        chat_id: String,
        before: ChatMemberSummary,
    },
    SetChatMemberAdmin {
        chat_id: String,
        before: ChatMemberSummary,
        rights: ChatAdminRightsSummary,
        rank: Option<String>,
    },
    SetChatMemberRestrictions {
        chat_id: String,
        user: ContactSummary,
        before: Option<ChatMemberSummary>,
        rights: ChatBannedRightsSummary,
    },
    CreateGroup {
        title: String,
        users: Vec<ContactSummary>,
    },
    CreateChannel {
        title: String,
        about: String,
        broadcast: bool,
    },
    JoinChat {
        before: ChatLifecycleState,
    },
    LeaveChat {
        before: ChatLifecycleState,
    },
    DeleteChat {
        before: ChatLifecycleState,
    },
    UpdateChatTitle {
        before: ChatDetailsSummary,
        title: String,
    },
    UpdateChatAbout {
        before: ChatDetailsSummary,
        about: String,
    },
    UpdateChatPhoto {
        before: ChatDetailsSummary,
        artifact: MutationFileArtifact,
    },
    DeleteChatPhoto {
        before: ChatDetailsSummary,
    },
    UpdateChatUsername {
        before: ChannelSettingsSummary,
        username: Option<String>,
    },
    SetChatSlowMode {
        before: ChannelSettingsSummary,
        seconds: i32,
    },
    ToggleChatSignatures {
        before: ChannelSettingsSummary,
        signatures_enabled: bool,
        profiles_enabled: bool,
    },
    ToggleChatPreHistoryHidden {
        before: ChannelSettingsSummary,
        enabled: bool,
    },
    ToggleChatJoinRequest {
        before: ChannelSettingsSummary,
        enabled: bool,
    },
    SetChatPermissions {
        before: ChatModerationSettingsSummary,
        target: ChatBannedRightsSummary,
    },
    SetChatReactions {
        before: ChatModerationSettingsSummary,
        target: ChatReactionsPolicy,
    },
    ToggleChatAntiSpam {
        before: ChatModerationSettingsSummary,
        enabled: bool,
    },
    ToggleChatForum {
        before: ChatModerationSettingsSummary,
        enabled: bool,
        tabs_enabled: bool,
    },
    ViewStories {
        peer_ref: String,
        before: Vec<StoryMutationSnapshot>,
        story_ids: Vec<i32>,
    },
    ReadStories {
        peer_ref: String,
        before_max_read_id: Option<i32>,
        target: StoryMutationSnapshot,
        max_id: i32,
    },
    SetStoryReaction {
        peer_ref: String,
        before: StoryReactionSnapshot,
        reaction: ReactionSpec,
    },
    SetStoriesPinned {
        peer_ref: String,
        before: Vec<StoryPinSnapshot>,
        story_ids: Vec<i32>,
        pinned: bool,
    },
    SetStoriesPinnedToTop {
        peer_ref: String,
        before: Vec<StoryTopPinSnapshot>,
        story_ids: Vec<i32>,
        pinned_to_top: bool,
    },
    ActivateStoryStealthMode {
        before: StoryStealthModeSummary,
        past: bool,
        future: bool,
    },
    SendStory {
        peer_ref: String,
        before_story_ids: Vec<i32>,
        artifact: MutationFileArtifact,
        caption: String,
        privacy: StoryPrivacySpec,
        pinned: bool,
        noforwards: bool,
        period_seconds: Option<i32>,
        random_id: i64,
    },
    EditStory {
        peer_ref: String,
        before: StoryEditSnapshot,
        artifact: Option<MutationFileArtifact>,
        target_media_kind: Option<String>,
        caption: Option<String>,
        privacy: Option<StoryPrivacySpec>,
    },
    ReportStories {
        peer_ref: String,
        before: Vec<StoryMutationSnapshot>,
        story_ids: Vec<i32>,
        option_hex: String,
        message: String,
    },
    DeleteStories {
        peer_ref: String,
        before: Vec<StoryMutationSnapshot>,
        story_ids: Vec<i32>,
    },
    ExportInviteLink {
        chat_id: String,
        before_handles: Vec<String>,
        expire_date_unix: Option<i32>,
        usage_limit: Option<i32>,
        request_needed: bool,
        title: Option<String>,
    },
    RevokeInviteLink {
        chat_id: String,
        before: InviteLinkSummary,
    },
    DeleteRevokedInviteLink {
        chat_id: String,
        before: InviteLinkSummary,
    },
    ProcessJoinRequest {
        chat_id: String,
        before: JoinRequestSnapshot,
        approve: bool,
    },
    ProcessAllJoinRequests {
        chat_id: String,
        invite_handle: Option<String>,
        before: Vec<JoinRequestSnapshot>,
        approve: bool,
    },
    UpdateAccountProfile {
        before: AccountProfileSummary,
        first_name: Option<String>,
        last_name: Option<String>,
        about: Option<String>,
    },
    UpdateAccountUsername {
        before: AccountProfileSummary,
        username: Option<String>,
    },
    UpdateAccountBirthday {
        before: AccountProfileSummary,
        birthday: Option<BirthdaySummary>,
    },
    SetAccountPeerColor {
        before: Option<PeerColorSummary>,
        for_profile: bool,
        color: Option<i32>,
        background_emoji_id: Option<String>,
    },
    SetAccountPersonalChannel {
        before: Option<PersonalChannelSummary>,
        channel: Option<ChatSummary>,
    },
    SetBotCommands {
        before: BotCommandSetSummary,
        commands: Vec<BotCommandSummary>,
    },
    SetAccountEmojiStatus {
        before: EmojiStatusSummary,
        status: AccountEmojiStatusSpec,
    },
    ClearRecentEmojiStatuses {
        before: EmojiStatusCatalogPage,
    },
    SetAccountPresence {
        online: bool,
    },
    UploadAccountProfilePhoto {
        before_handles: Vec<String>,
        artifact: MutationFileArtifact,
        fallback: bool,
    },
    DeleteAccountProfilePhotos {
        before: Vec<ProfilePhotoSummary>,
    },
    CreateDialogFolder {
        before_folder_ids: Vec<i32>,
        target: DialogFolderSummary,
    },
    EditDialogFolder {
        before: DialogFolderSummary,
        target: DialogFolderSummary,
        preserve_title_entities: bool,
    },
    DeleteDialogFolder {
        before: DialogFolderSummary,
    },
    ReorderDialogFolders {
        before_order: Vec<i32>,
        target_order: Vec<i32>,
    },
    SetDialogFolderMembership {
        before: DialogFolderSummary,
        chat_id: String,
        included: bool,
    },
    SetDialogFolderTags {
        before: bool,
        enabled: bool,
    },
    SetGlobalPrivacySettings {
        before: GlobalPrivacySettingsSummary,
        target: GlobalPrivacySettingsSummary,
    },
    ForwardMessages {
        source_chat_id: String,
        messages: Vec<MutationMessageSnapshot>,
        destination_chat_id: String,
        silent: bool,
        drop_author: bool,
        drop_media_captions: bool,
        noforwards: bool,
        random_ids: Vec<i64>,
    },
    SendRichMedia {
        chat_id: String,
        media: RichMediaSpec,
        caption: String,
        reply_to_message_id: Option<i32>,
        #[serde(default)]
        topic_id: Option<i32>,
        silent: bool,
        noforwards: bool,
        random_id: i64,
    },
    SendFile {
        chat_id: String,
        artifact: MutationFileArtifact,
        caption: String,
        reply_to_message_id: Option<i32>,
        mode: FileSendMode,
        random_id: i64,
    },
    SendVoice {
        chat_id: String,
        artifact: MutationFileArtifact,
        caption: String,
        caption_parse_mode: CaptionParseMode,
        reply_to_message_id: Option<i32>,
        topic_id: Option<i32>,
        silent: bool,
        noforwards: bool,
        random_id: i64,
    },
    SendVideoNote {
        chat_id: String,
        artifact: MutationFileArtifact,
        duration_seconds: i32,
        length_pixels: i32,
        reply_to_message_id: Option<i32>,
        topic_id: Option<i32>,
        silent: bool,
        noforwards: bool,
        random_id: i64,
    },
    SendAlbum {
        chat_id: String,
        items: Vec<MutationAlbumItem>,
        caption_parse_mode: CaptionParseMode,
        reply_to_message_id: Option<i32>,
        topic_id: Option<i32>,
        silent: bool,
        noforwards: bool,
        random_ids: Vec<i64>,
    },
    SendSticker {
        chat_id: String,
        sticker: StickerDocumentSummary,
        reply_to_message_id: Option<i32>,
        silent: bool,
        noforwards: bool,
        random_id: i64,
    },
    SetStickerFavorite {
        sticker: StickerDocumentSummary,
        before_favorite: bool,
        favorite: bool,
    },
    SetRecentSticker {
        sticker: StickerDocumentSummary,
        attached: bool,
        before_saved: bool,
        saved: bool,
    },
    CreateBusinessChatLink {
        content: BusinessChatLinkContent,
    },
    EditBusinessChatLink {
        before: BusinessChatLinkSummary,
        content: BusinessChatLinkContent,
    },
    DeleteBusinessChatLink {
        before: BusinessChatLinkSummary,
    },
    SetBusinessWorkHours {
        before: Option<BusinessWorkHoursConfig>,
        target: Option<BusinessWorkHoursConfig>,
    },
    SetBusinessLocation {
        before: Option<BusinessLocationSummary>,
        target: Option<BusinessLocationSummary>,
    },
    SetBusinessGreeting {
        before: Option<BusinessGreetingMessageSummary>,
        target: Option<BusinessGreetingMessageSummary>,
    },
    SetBusinessAway {
        before: Option<BusinessAwayMessageSummary>,
        target: Option<BusinessAwayMessageSummary>,
    },
    SetBusinessIntro {
        before: Option<BusinessIntroSummary>,
        target: Option<BusinessIntroSpec>,
    },
    SetStarGiftSaved {
        before: SavedStarGiftSnapshot,
        unsaved: bool,
    },
    ConvertStarGift {
        before: SavedStarGiftSnapshot,
        before_balance: StarsAmountSummary,
        convert_stars: String,
    },
    SetStarsSubscriptionCanceled {
        wallet_peer_ref: String,
        before: StarsSubscriptionSummary,
        canceled: bool,
    },
    SetStickerSetInstalled {
        sticker_set: StickerSetSummary,
        before_installed: bool,
        installed: bool,
        archived: bool,
    },
    ApplyBoost {
        chat_id: String,
        slots: Vec<i32>,
        before: MyBoostsSummary,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum MutationResult {
    TypingActionSent {
        chat_id: String,
        action: TypingAction,
    },
    CallbackButtonPressed {
        button: CallbackButtonSnapshot,
        answer: BotCallbackAnswerSummary,
    },
    InlineBotResultSent {
        chat_id: String,
        message_id: i32,
        query_id: String,
        result: InlineBotResultSummary,
    },
    SavedHistoryDeleted {
        peer_ref: String,
        remaining_count: usize,
    },
    SavedDialogPinUpdated {
        dialog: SavedDialogSummary,
    },
    SavedReactionTagUpdated {
        tag: SavedReactionTagSummary,
    },
    MessagesTranslated {
        translation: MessageTranslationBatch,
    },
    AudioTranscribed {
        transcription: AudioTranscriptionSummary,
    },
    TranscriptionRated {
        chat_id: String,
        message_id: i32,
        transcription_id: String,
        good: bool,
    },
    FactCheckUpdated {
        chat_id: String,
        fact_check: FactCheckSummary,
    },
    FactCheckDeleted {
        chat_id: String,
        fact_check: FactCheckSummary,
    },
    MessageEdited {
        message_id: i32,
    },
    MessagesDeleted {
        message_ids: Vec<i32>,
    },
    MessagesRestored {
        source_deletion_approval_id: String,
        mappings: Vec<RestoredMessageMapping>,
    },
    MessagePinUpdated {
        message_id: i32,
        pinned: bool,
    },
    MessageReactionUpdated {
        message_id: i32,
        reaction: Option<ReactionSpec>,
    },
    DefaultReactionUpdated {
        reaction: ReactionIdentity,
    },
    DraftUpdated {
        chat_id: String,
        draft: Option<DraftSummary>,
    },
    AllDraftsCleared {
        cleared_chat_ids: Vec<String>,
        remaining: Vec<DialogDraftSummary>,
    },
    DialogStateUpdated {
        chat_id: String,
        state: DialogStateSnapshot,
    },
    MessagesMarkedRead {
        chat_id: String,
        max_id: i32,
        state: DialogStateSnapshot,
    },
    MessageReactionsMarkedRead {
        chat_id: String,
        unread_reactions_count: i32,
        server_receipt: Option<MessageReactionReadReceipt>,
    },
    UserBlocked {
        user_id: i64,
        blocked: bool,
    },
    SessionTerminated {
        authorization_id: i64,
    },
    OtherSessionsTerminated {
        authorization_ids: Vec<i64>,
    },
    ChatAutoDeleteUpdated {
        state: ChatAutoDeleteSummary,
    },
    ContactStateUpdated {
        user_id: i64,
        is_contact: bool,
    },
    PrivacyRulesUpdated {
        key: PrivacyKey,
        rules: Vec<PrivacyRuleSummary>,
    },
    NotificationSettingsUpdated {
        chat_id: String,
        settings: NotificationSettingsSummary,
    },
    MessageScheduled {
        message_id: i32,
        schedule_date_unix: i32,
    },
    FileScheduled {
        message_id: i32,
        schedule_date_unix: i32,
        sha256: String,
    },
    ScheduledMessageEdited {
        message_id: i32,
        schedule_date_unix: i32,
    },
    ScheduledMessagesDeleted {
        message_ids: Vec<i32>,
    },
    ScheduledMessagesSent {
        scheduled_message_ids: Vec<i32>,
        sent_message_ids: Vec<i32>,
    },
    PollCreated {
        message_id: i32,
        poll_id: i64,
    },
    PollVoteUpdated {
        message_id: i32,
        selected_option_indices: Vec<usize>,
    },
    PollStopped {
        message_id: i32,
        poll_id: i64,
    },
    ForumTopicCreated {
        topic_id: i32,
    },
    ForumTopicUpdated {
        topic: ForumTopicSummary,
    },
    ForumTopicPinned {
        topic_id: i32,
        pinned: bool,
    },
    ForumTopicDeleted {
        topic_id: i32,
    },
    ChatMemberAdded {
        member: ChatMemberSummary,
    },
    ChatMembersInvited {
        chat_id: String,
        outcomes: Vec<InviteTargetProgress>,
    },
    ChatMemberRemoved {
        user_id: i64,
    },
    ChatMemberAdminUpdated {
        member: ChatMemberSummary,
    },
    ChatMemberRestrictionsUpdated {
        member: Option<ChatMemberSummary>,
    },
    ChatCreated {
        chat: ChatDetailsSummary,
    },
    ChatJoined {
        state: ChatLifecycleState,
    },
    ChatLeft {
        state: ChatLifecycleState,
    },
    ChatDeleted {
        state: ChatLifecycleState,
    },
    ChatTitleUpdated {
        chat: ChatDetailsSummary,
    },
    ChatAboutUpdated {
        chat: ChatDetailsSummary,
    },
    ChatPhotoUpdated {
        chat: ChatDetailsSummary,
    },
    ChannelSettingsUpdated {
        settings: ChannelSettingsSummary,
    },
    ChatModerationSettingsUpdated {
        settings: ChatModerationSettingsSummary,
    },
    StoriesViewed {
        peer_ref: String,
        story_ids: Vec<i32>,
    },
    StoriesRead {
        peer_ref: String,
        max_id: i32,
    },
    StoryReactionUpdated {
        peer_ref: String,
        story_id: i32,
        reaction: Option<ReactionSpec>,
    },
    StoriesPinnedUpdated {
        peer_ref: String,
        story_ids: Vec<i32>,
        pinned: bool,
    },
    StoriesPinnedToTopUpdated {
        peer_ref: String,
        story_ids: Vec<i32>,
        pinned_to_top: bool,
    },
    StoryStealthModeActivated {
        mode: StoryStealthModeSummary,
        past: bool,
        future: bool,
    },
    StorySent {
        peer_ref: String,
        story: StorySummary,
    },
    StoryEdited {
        peer_ref: String,
        story: StorySummary,
        source_sha256: Option<String>,
    },
    StoryReportStep {
        peer_ref: String,
        story_ids: Vec<i32>,
        report: StoryReportResult,
    },
    StoriesDeleted {
        peer_ref: String,
        story_ids: Vec<i32>,
    },
    InviteLinkExported {
        invite: InviteLinkSummary,
    },
    InviteLinkRevoked {
        invite: InviteLinkSummary,
    },
    RevokedInviteLinkDeleted {
        invite_handle: String,
    },
    JoinRequestProcessed {
        user_id: i64,
        approved: bool,
    },
    JoinRequestsProcessed {
        user_ids: Vec<i64>,
        approved: bool,
    },
    AccountProfileUpdated {
        profile: AccountProfileSummary,
    },
    AccountUsernameUpdated {
        profile: AccountProfileSummary,
    },
    AccountBirthdayUpdated {
        profile: AccountProfileSummary,
    },
    AccountPeerColorUpdated {
        profile: AccountProfileSummary,
    },
    AccountPersonalChannelUpdated {
        profile: AccountProfileSummary,
    },
    BotCommandsSet {
        state: BotCommandSetSummary,
    },
    AccountEmojiStatusSet {
        status: EmojiStatusSummary,
    },
    RecentEmojiStatusesCleared {
        remaining: EmojiStatusCatalogPage,
    },
    AccountPresenceUpdated {
        online: bool,
    },
    AccountProfilePhotoUploaded {
        photo: ProfilePhotoSummary,
    },
    AccountProfilePhotosDeleted {
        photo_handles: Vec<String>,
    },
    DialogFolderCreated {
        folder: DialogFolderSummary,
    },
    DialogFolderUpdated {
        folder: DialogFolderSummary,
    },
    DialogFolderDeleted {
        folder_id: i32,
    },
    DialogFoldersReordered {
        folder_ids: Vec<i32>,
    },
    DialogFolderMembershipUpdated {
        folder: DialogFolderSummary,
        chat_id: String,
        included: bool,
    },
    DialogFolderTagsUpdated {
        tags_enabled: bool,
    },
    GlobalPrivacySettingsUpdated {
        settings: GlobalPrivacySettingsSummary,
    },
    MessagesForwarded {
        source_message_ids: Vec<i32>,
        destination_message_ids: Vec<i32>,
    },
    RichMediaSent {
        message_id: i32,
        media: SentRichMediaSummary,
    },
    FileSent {
        message_id: i32,
        sha256: String,
    },
    VoiceSent {
        message_id: i32,
        sha256: String,
        media: SentDocumentMediaSummary,
    },
    VideoNoteSent {
        message_id: i32,
        sha256: String,
        media: SentDocumentMediaSummary,
    },
    AlbumSent {
        message_ids: Vec<i32>,
        grouped_id: i64,
        sha256s: Vec<String>,
    },
    StickerSent {
        message_id: i32,
        sticker_handle: String,
    },
    StickerFavoriteUpdated {
        sticker_handle: String,
        favorite: bool,
    },
    RecentStickerUpdated {
        sticker_handle: String,
        attached: bool,
        saved: bool,
    },
    BusinessChatLinkCreated {
        link: BusinessChatLinkSummary,
    },
    BusinessChatLinkUpdated {
        link: BusinessChatLinkSummary,
    },
    BusinessChatLinkDeleted {
        slug: String,
    },
    BusinessWorkHoursUpdated {
        work_hours: Option<BusinessWorkHoursSummary>,
    },
    BusinessLocationUpdated {
        location: Option<BusinessLocationSummary>,
    },
    BusinessGreetingUpdated {
        greeting: Option<BusinessGreetingMessageSummary>,
    },
    BusinessAwayUpdated {
        away: Option<BusinessAwayMessageSummary>,
    },
    BusinessIntroUpdated {
        intro: Option<BusinessIntroSummary>,
    },
    StarGiftSavedStateUpdated {
        gift: SavedStarGiftSnapshot,
    },
    StarGiftConverted {
        reference: SavedStarGiftRef,
        convert_stars: String,
        wallet_balance: StarsAmountSummary,
    },
    StarsSubscriptionUpdated {
        wallet_peer_ref: String,
        subscription: StarsSubscriptionSummary,
    },
    StickerSetInstallationUpdated {
        short_name: String,
        installed: bool,
        archived: bool,
    },
    BoostApplied {
        chat_id: String,
        slots: Vec<i32>,
        boosts: MyBoostsSummary,
    },
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PreparedMutationState {
    Prepared,
    Applying,
    Applied,
    OutcomeUnknown,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct PreparedMutation {
    pub mutation_id: String,
    pub schema_version: u32,
    pub account_user_id: i64,
    pub action: MutationAction,
    pub payload_sha256: String,
    pub state: PreparedMutationState,
    pub result: Option<MutationResult>,
    pub created_at_unix: i64,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MutationApprovalState {
    DispatchingCard,
    Pending,
    CardDeliveryUncertain,
    Applying,
    OutcomeUnknown,
    Applied,
    Cancelled,
    Expired,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct MutationApproval {
    pub approval_id: String,
    pub mutation_id: String,
    pub schema_version: u32,
    pub account_user_id: i64,
    pub action: MutationAction,
    pub payload_sha256: String,
    pub state: MutationApprovalState,
    pub approval_chat_id: i64,
    pub approval_user_id: i64,
    pub approval_message_id: Option<i64>,
    pub created_at_unix: i64,
    pub expires_at_unix: i64,
    pub result: Option<MutationResult>,
    pub automated_retry_count: u32,
    pub next_retry_at_unix: Option<i64>,
    pub last_error_code: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct AttachmentDescriptor {
    pub attachment_id: String,
    pub chat_id: String,
    pub message_id: i32,
    pub kind: String,
    pub file_name: Option<String>,
    pub mime_type: Option<String>,
    pub size_bytes: Option<u64>,
    pub downloadable: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct DownloadedAttachment {
    pub attachment: AttachmentDescriptor,
    pub local_path: String,
    pub bytes_written: u64,
    pub sha256: String,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum FileSendMode {
    #[default]
    Auto,
    Document,
    Photo,
    Gif,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PreparedSendState {
    Prepared,
    Dispatching,
    Sent,
    DeliveryUncertain,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PreparedSendKind {
    #[default]
    PlainText,
    RichMarkdown,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct PreparedSendTarget {
    pub display_name: String,
    pub username: Option<String>,
    pub kind: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct PreparedMessageSource {
    pub chat_id: String,
    pub target: PreparedSendTarget,
    pub message_id: i32,
    pub message_date_unix: i32,
    pub text: Option<String>,
    #[serde(default)]
    pub relation: Option<OutboundSourceRelation>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum OutboundSourceRelation {
    TriggeredOutreach,
    MateriallyInformed,
    ReplyContext,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PreparedNonTelegramSource {
    PublicWeb {
        relation: OutboundSourceRelation,
        publisher: String,
        title: String,
        url: String,
        published_at_unix: Option<i64>,
        observed_at_unix: i64,
    },
    SelfOriginated,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(untagged)]
pub enum PreparedOutboundSource {
    TelegramMessage(PreparedMessageSource),
    NonTelegram(PreparedNonTelegramSource),
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct PreparedSend {
    pub draft_id: String,
    pub account_user_id: i64,
    pub chat_id: String,
    pub target: PreparedSendTarget,
    #[serde(default)]
    pub source: Option<PreparedOutboundSource>,
    pub kind: PreparedSendKind,
    pub text: String,
    pub reply_to_message_id: Option<i32>,
    pub payload_sha256: String,
    pub state: PreparedSendState,
    pub message_id: Option<i32>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SendApprovalState {
    DispatchingCard,
    Pending,
    CardDeliveryUncertain,
    Sending,
    DeliveryUncertain,
    Sent,
    Cancelled,
    Expired,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SendApproval {
    pub approval_id: String,
    pub draft_id: String,
    pub account_user_id: i64,
    pub chat_id: String,
    pub target: PreparedSendTarget,
    #[serde(default)]
    pub source: Option<PreparedOutboundSource>,
    pub kind: PreparedSendKind,
    pub text: String,
    pub reply_to_message_id: Option<i32>,
    pub payload_sha256: String,
    pub state: SendApprovalState,
    pub approval_chat_id: i64,
    pub approval_user_id: i64,
    pub approval_message_id: Option<i64>,
    pub created_at_unix: i64,
    pub expires_at_unix: i64,
    pub target_message_id: Option<i32>,
    pub automated_retry_count: u32,
    pub next_retry_at_unix: Option<i64>,
    pub last_error_code: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct RichMessageSnapshot {
    pub chat_id: String,
    pub message_id: i32,
    pub rtl: bool,
    pub part: bool,
    pub block_count: usize,
    pub photo_count: usize,
    pub document_count: usize,
    #[schemars(
        description = "Lowercase SHA-256 of the complete Telegram RichMessage TL object returned by messages.getRichMessage."
    )]
    pub state_sha256: String,
}
