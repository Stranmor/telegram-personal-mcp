use crate::domain::{
    AccountEmojiStatusSpec, AccountProfileSummary, AdminLogEventFilter, AdminLogPage,
    AlbumUploadSpec, AttachmentDescriptor, AudioTranscriptionSummary, AvailableStarGiftsPage,
    BirthdaySummary, BoostPage, BoostStatusSummary, BotCallbackAnswerSummary, BotCommandSummary,
    BusinessAwayMessageSummary, BusinessChatLinkContent, BusinessChatLinkSummary,
    BusinessChatLinksPage, BusinessGreetingMessageSummary, BusinessIntroSpec,
    BusinessLocationSummary, BusinessWorkHoursConfig, CallbackButtonSnapshot, CaptionParseMode,
    ChannelSettingsSummary, ChannelStatsSummary, ChannelUpdatesDifferenceSummary,
    ChannelUpdatesRequest, ChatAdminRightsSummary, ChatAutoDeleteSummary, ChatBannedRightsSummary,
    ChatDetailsSummary, ChatLifecycleState, ChatMemberSummary, ChatModerationSettingsSummary,
    ChatReactionsPolicy, ChatSummary, ContactSummary, CustomEmojiDocumentSummary,
    DefaultReactionSpec, DefaultReactionSummary, DialogDraftSummary, DialogFolderList,
    DialogFolderSummary, DialogStateSnapshot, DiceEmojiCatalog, DiscussionMessageSummary,
    DraftSummary, EmojiStatusCatalogKind, EmojiStatusCatalogPage, EmojiStatusSummary,
    FactCheckPage, FileSendMode, ForumTopicSummary, GlobalPrivacySettingsSummary, GroupCallPage,
    GroupCallParticipantsPage, InlineBotResults, InviteJoinerPage, InviteLinkSummary,
    MessageButtonInventory, MessageLinkSummary, MessagePage, MessageReactionPage,
    MessageReactionReadReceipt, MessageReadReceipt, MessageTranslationBatch, MyBoostsSummary,
    NotificationSettingsPatch, NotificationSettingsSummary, OutboxReadDateSummary,
    ParticipantFilter, ParticipantPage, ParticipantSummary, PollCreateSpec, PollSummary,
    PollVoterPage, PrivacyKey, PrivacyRuleSummary, ProfilePhotoPage, ProfilePhotoSummary,
    QuickReplyCatalogPage, QuickReplyMessagesPage, ReactionCatalogPage, ReactionIdentity,
    ReactionSpec, ResolvedBusinessChatLinkSummary, RichMediaSpec, RichMessageSnapshot,
    SavedDialogsPage, SavedHistoryPage, SavedMessagesSearchRequest, SavedReactionTagsPage,
    SavedStarGiftRef, SavedStarGiftSummary, SavedStarGiftsPage, SavedStarGiftsRequest,
    SearchMessagesRequest, SentDocumentMediaSummary, SentRichMediaSummary, SessionSummary,
    StarsStatusPage, StarsSubscriptionSummary, StarsSubscriptionsPage, StarsTopupOptionsPage,
    StickerCatalogPage, StickerSetDetailsPage, StickerSetSearchPage, StickerSetSummary,
    StoryCollectionPage, StoryLinkSummary, StoryListPage, StoryPeerPage, StoryPrivacySpec,
    StoryReportResult, StorySummary, StoryViewersPage, SuggestedDialogFolderList, TypingAction,
    UnreadMessagesRequest, UpdatesDifferenceRequest, UpdatesDifferenceSummary, UpdatesStateSummary,
    UserDetails, WebPagePreviewSummary,
};
use crate::telegram::{DialogSummary, MessageSummary, TelegramError, UserInfo};
use async_trait::async_trait;
use std::path::PathBuf;

#[async_trait]
#[allow(clippy::too_many_arguments)]
// The backend boundary mirrors typed Telegram operations. Collapsing unrelated
// protocol fields into bags here would weaken call-site clarity and hash binding.
pub trait TelegramBackend: Send + Sync + 'static {
    async fn get_me(
        &self,
        session_path: Option<PathBuf>,
    ) -> Result<(UserInfo, PathBuf), TelegramError>;

    async fn resolve_chat_id(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
    ) -> Result<String, TelegramError>;

    async fn send_typing_action(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        action: TypingAction,
    ) -> Result<(), TelegramError>;

    async fn list_dialogs(
        &self,
        session_path: Option<PathBuf>,
        limit: usize,
    ) -> Result<Vec<DialogSummary>, TelegramError>;

    async fn history(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        limit: usize,
        offset_id: Option<i32>,
        add_offset: i32,
    ) -> Result<Vec<MessageSummary>, TelegramError>;

    async fn history_newer_than(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        min_message_id: i32,
        limit: usize,
        offset_id: Option<i32>,
    ) -> Result<Vec<MessageSummary>, TelegramError>;

    async fn get_updates_state(
        &self,
        session_path: Option<PathBuf>,
    ) -> Result<UpdatesStateSummary, TelegramError>;

    async fn get_updates_difference(
        &self,
        request: UpdatesDifferenceRequest,
    ) -> Result<UpdatesDifferenceSummary, TelegramError>;

    async fn get_channel_updates(
        &self,
        request: ChannelUpdatesRequest,
    ) -> Result<ChannelUpdatesDifferenceSummary, TelegramError>;

    async fn get_quick_replies(
        &self,
        session_path: Option<PathBuf>,
        hash: i64,
    ) -> Result<QuickReplyCatalogPage, TelegramError>;

    async fn get_quick_reply_messages(
        &self,
        session_path: Option<PathBuf>,
        shortcut_id: i32,
        message_ids: Option<&[i32]>,
        hash: i64,
    ) -> Result<QuickReplyMessagesPage, TelegramError>;

    async fn get_group_call(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        participant_limit: i32,
    ) -> Result<GroupCallPage, TelegramError>;

    async fn get_group_call_participants(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        peer_refs: &[String],
        sources: &[i32],
        offset: &str,
        limit: i32,
    ) -> Result<GroupCallParticipantsPage, TelegramError>;

    async fn get_business_chat_links(
        &self,
        session_path: Option<PathBuf>,
    ) -> Result<BusinessChatLinksPage, TelegramError>;

    async fn resolve_business_chat_link(
        &self,
        session_path: Option<PathBuf>,
        slug: &str,
    ) -> Result<ResolvedBusinessChatLinkSummary, TelegramError>;

    async fn create_business_chat_link(
        &self,
        session_path: Option<PathBuf>,
        content: &BusinessChatLinkContent,
    ) -> Result<BusinessChatLinkSummary, TelegramError>;

    async fn edit_business_chat_link(
        &self,
        session_path: Option<PathBuf>,
        slug: &str,
        content: &BusinessChatLinkContent,
    ) -> Result<BusinessChatLinkSummary, TelegramError>;

    async fn delete_business_chat_link(
        &self,
        session_path: Option<PathBuf>,
        slug: &str,
    ) -> Result<(), TelegramError>;

    async fn set_business_work_hours(
        &self,
        session_path: Option<PathBuf>,
        target: Option<&BusinessWorkHoursConfig>,
    ) -> Result<(), TelegramError>;

    async fn set_business_location(
        &self,
        session_path: Option<PathBuf>,
        target: Option<&BusinessLocationSummary>,
    ) -> Result<(), TelegramError>;

    async fn set_business_greeting(
        &self,
        session_path: Option<PathBuf>,
        target: Option<&BusinessGreetingMessageSummary>,
    ) -> Result<(), TelegramError>;

    async fn set_business_away(
        &self,
        session_path: Option<PathBuf>,
        target: Option<&BusinessAwayMessageSummary>,
    ) -> Result<(), TelegramError>;

    async fn set_business_intro(
        &self,
        session_path: Option<PathBuf>,
        target: Option<&BusinessIntroSpec>,
    ) -> Result<(), TelegramError>;

    async fn get_stars_status(
        &self,
        session_path: Option<PathBuf>,
        peer_ref: &str,
    ) -> Result<StarsStatusPage, TelegramError>;

    #[allow(clippy::too_many_arguments)]
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
    ) -> Result<StarsStatusPage, TelegramError>;

    async fn get_available_star_gifts(
        &self,
        session_path: Option<PathBuf>,
    ) -> Result<AvailableStarGiftsPage, TelegramError>;

    async fn get_saved_star_gifts(
        &self,
        request: SavedStarGiftsRequest,
    ) -> Result<SavedStarGiftsPage, TelegramError>;

    async fn get_stars_topup_options(
        &self,
        session_path: Option<PathBuf>,
    ) -> Result<StarsTopupOptionsPage, TelegramError>;

    async fn get_stars_subscriptions(
        &self,
        session_path: Option<PathBuf>,
        peer_ref: &str,
        missing_balance: bool,
        offset: &str,
    ) -> Result<StarsSubscriptionsPage, TelegramError>;

    async fn get_saved_star_gift(
        &self,
        session_path: Option<PathBuf>,
        reference: &SavedStarGiftRef,
    ) -> Result<Option<SavedStarGiftSummary>, TelegramError>;

    async fn set_star_gift_unsaved(
        &self,
        session_path: Option<PathBuf>,
        reference: &SavedStarGiftRef,
        unsaved: bool,
    ) -> Result<(), TelegramError>;

    async fn convert_star_gift(
        &self,
        session_path: Option<PathBuf>,
        reference: &SavedStarGiftRef,
    ) -> Result<(), TelegramError>;

    async fn get_stars_subscription(
        &self,
        session_path: Option<PathBuf>,
        peer_ref: &str,
        subscription_id: &str,
    ) -> Result<Option<StarsSubscriptionSummary>, TelegramError>;

    async fn change_stars_subscription(
        &self,
        session_path: Option<PathBuf>,
        peer_ref: &str,
        subscription_id: &str,
        canceled: bool,
    ) -> Result<(), TelegramError>;

    async fn search_messages(
        &self,
        request: SearchMessagesRequest,
    ) -> Result<MessagePage, TelegramError>;

    async fn search_saved_messages(
        &self,
        request: SavedMessagesSearchRequest,
    ) -> Result<MessagePage, TelegramError>;

    async fn get_saved_dialogs(
        &self,
        session_path: Option<PathBuf>,
        limit: usize,
        exclude_pinned: bool,
        offset_id: Option<i32>,
    ) -> Result<SavedDialogsPage, TelegramError>;

    async fn get_saved_dialog(
        &self,
        session_path: Option<PathBuf>,
        peer_ref: &str,
    ) -> Result<Option<crate::domain::SavedDialogSummary>, TelegramError>;

    async fn get_saved_history(
        &self,
        session_path: Option<PathBuf>,
        peer_ref: &str,
        limit: usize,
        offset_id: Option<i32>,
    ) -> Result<SavedHistoryPage, TelegramError>;

    async fn delete_saved_history(
        &self,
        session_path: Option<PathBuf>,
        peer_ref: &str,
        max_id: i32,
        min_date: Option<i32>,
        max_date: Option<i32>,
    ) -> Result<(), TelegramError>;

    async fn toggle_saved_dialog_pin(
        &self,
        session_path: Option<PathBuf>,
        peer_ref: &str,
        pinned: bool,
    ) -> Result<(), TelegramError>;

    async fn get_saved_reaction_tags(
        &self,
        session_path: Option<PathBuf>,
        peer_ref: Option<&str>,
        hash: i64,
    ) -> Result<SavedReactionTagsPage, TelegramError>;

    async fn update_saved_reaction_tag(
        &self,
        session_path: Option<PathBuf>,
        reaction: &ReactionIdentity,
        title: Option<&str>,
    ) -> Result<(), TelegramError>;

    async fn get_default_tag_reactions(
        &self,
        session_path: Option<PathBuf>,
        hash: i64,
    ) -> Result<ReactionCatalogPage, TelegramError>;

    async fn get_unread_mentions(
        &self,
        request: UnreadMessagesRequest,
    ) -> Result<MessagePage, TelegramError>;

    async fn get_unread_reactions(
        &self,
        request: UnreadMessagesRequest,
    ) -> Result<MessagePage, TelegramError>;

    async fn get_unread_poll_votes(
        &self,
        request: UnreadMessagesRequest,
    ) -> Result<MessagePage, TelegramError>;

    async fn get_message(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        message_id: i32,
    ) -> Result<Option<MessageSummary>, TelegramError>;

    async fn get_message_html(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        message_id: i32,
    ) -> Result<String, TelegramError>;

    async fn get_message_buttons(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        message_id: Option<i32>,
        limit: usize,
    ) -> Result<MessageButtonInventory, TelegramError>;

    async fn press_callback_button(
        &self,
        session_path: Option<PathBuf>,
        button: &CallbackButtonSnapshot,
    ) -> Result<BotCallbackAnswerSummary, TelegramError>;

    async fn get_inline_bot_results(
        &self,
        session_path: Option<PathBuf>,
        bot_ref: &str,
        context_chat_id: &str,
        query: &str,
        offset: &str,
    ) -> Result<InlineBotResults, TelegramError>;

    #[allow(clippy::too_many_arguments)]
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
    ) -> Result<i32, TelegramError>;

    async fn translate_messages(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        message_ids: &[i32],
        to_language: &str,
    ) -> Result<MessageTranslationBatch, TelegramError>;

    async fn transcribe_audio(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        message_id: i32,
    ) -> Result<AudioTranscriptionSummary, TelegramError>;

    async fn rate_transcription(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        message_id: i32,
        transcription_id: i64,
        good: bool,
    ) -> Result<(), TelegramError>;

    async fn get_fact_checks(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        message_ids: &[i32],
    ) -> Result<FactCheckPage, TelegramError>;

    async fn edit_fact_check(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        message_id: i32,
        text: &str,
    ) -> Result<(), TelegramError>;

    async fn delete_fact_check(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        message_id: i32,
    ) -> Result<(), TelegramError>;

    async fn get_discussion_message(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        message_id: i32,
    ) -> Result<DiscussionMessageSummary, TelegramError>;

    async fn export_message_link(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        message_id: i32,
        grouped: bool,
        thread: bool,
    ) -> Result<MessageLinkSummary, TelegramError>;

    async fn get_web_page_preview(
        &self,
        session_path: Option<PathBuf>,
        message: &str,
    ) -> Result<WebPagePreviewSummary, TelegramError>;

    async fn get_groups_for_discussion(
        &self,
        session_path: Option<PathBuf>,
    ) -> Result<Vec<ChatSummary>, TelegramError>;

    async fn list_contacts(
        &self,
        session_path: Option<PathBuf>,
        limit: usize,
    ) -> Result<Vec<ContactSummary>, TelegramError>;

    async fn search_contacts(
        &self,
        session_path: Option<PathBuf>,
        query: &str,
        limit: usize,
    ) -> Result<Vec<ContactSummary>, TelegramError>;

    async fn get_pinned_message(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
    ) -> Result<Option<MessageSummary>, TelegramError>;

    async fn get_scheduled_messages(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
    ) -> Result<Vec<MessageSummary>, TelegramError>;

    async fn get_poll(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        message_id: i32,
    ) -> Result<Option<PollSummary>, TelegramError>;

    async fn get_poll_votes(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        message_id: i32,
        option_index: Option<usize>,
        limit: usize,
        offset: Option<&str>,
    ) -> Result<PollVoterPage, TelegramError>;

    #[allow(clippy::too_many_arguments)]
    async fn create_poll_with_random_id(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        poll: &PollCreateSpec,
        reply_to_message_id: Option<i32>,
        silent: bool,
        noforwards: bool,
        random_id: i64,
    ) -> Result<i32, TelegramError>;

    async fn set_poll_vote(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        message_id: i32,
        options: &[Vec<u8>],
    ) -> Result<(), TelegramError>;

    async fn stop_poll(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        message_id: i32,
    ) -> Result<(), TelegramError>;

    #[allow(clippy::too_many_arguments)]
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
    ) -> Result<i32, TelegramError>;

    #[allow(clippy::too_many_arguments)]
    async fn schedule_file_with_random_id(
        &self,
        session_path: Option<PathBuf>,
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
    ) -> Result<i32, TelegramError>;

    async fn edit_scheduled_message(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        message_id: i32,
        text: &str,
        parse_mode: CaptionParseMode,
        schedule_date_unix: i32,
    ) -> Result<(), TelegramError>;

    async fn delete_scheduled_messages(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        message_ids: &[i32],
    ) -> Result<(), TelegramError>;

    async fn send_scheduled_messages_now(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        message_ids: &[i32],
    ) -> Result<Vec<i32>, TelegramError>;

    async fn list_drafts(
        &self,
        session_path: Option<PathBuf>,
        limit: usize,
    ) -> Result<Vec<DialogDraftSummary>, TelegramError>;

    async fn get_draft(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
    ) -> Result<Option<DraftSummary>, TelegramError>;

    async fn save_draft(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        text: Option<&str>,
    ) -> Result<(), TelegramError>;

    async fn clear_all_drafts(&self, session_path: Option<PathBuf>) -> Result<(), TelegramError>;

    async fn get_dialog_state(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
    ) -> Result<DialogStateSnapshot, TelegramError>;

    async fn set_dialog_unread(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        unread: bool,
    ) -> Result<(), TelegramError>;

    async fn mark_messages_read(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        max_id: i32,
    ) -> Result<(), TelegramError>;

    async fn mark_reactions_read(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
    ) -> Result<MessageReactionReadReceipt, TelegramError>;

    async fn set_dialog_archived(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        archived: bool,
    ) -> Result<(), TelegramError>;

    async fn set_dialog_pinned(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        pinned: bool,
    ) -> Result<(), TelegramError>;

    async fn list_dialog_folders(
        &self,
        session_path: Option<PathBuf>,
    ) -> Result<DialogFolderList, TelegramError>;

    async fn get_suggested_dialog_folders(
        &self,
        session_path: Option<PathBuf>,
    ) -> Result<SuggestedDialogFolderList, TelegramError>;

    async fn set_dialog_folder_tags(
        &self,
        session_path: Option<PathBuf>,
        enabled: bool,
    ) -> Result<(), TelegramError>;

    async fn get_global_privacy_settings(
        &self,
        session_path: Option<PathBuf>,
    ) -> Result<GlobalPrivacySettingsSummary, TelegramError>;

    async fn set_global_privacy_settings(
        &self,
        session_path: Option<PathBuf>,
        settings: &GlobalPrivacySettingsSummary,
    ) -> Result<(), TelegramError>;

    async fn upsert_dialog_folder(
        &self,
        session_path: Option<PathBuf>,
        folder: &DialogFolderSummary,
        preserve_title_entities: bool,
    ) -> Result<(), TelegramError>;

    async fn delete_dialog_folder(
        &self,
        session_path: Option<PathBuf>,
        folder_id: i32,
    ) -> Result<(), TelegramError>;

    async fn reorder_dialog_folders(
        &self,
        session_path: Option<PathBuf>,
        folder_ids: &[i32],
    ) -> Result<(), TelegramError>;

    async fn set_dialog_folder_membership(
        &self,
        session_path: Option<PathBuf>,
        folder_id: i32,
        chat_id: &str,
        included: bool,
    ) -> Result<(), TelegramError>;

    async fn set_user_blocked(
        &self,
        session_path: Option<PathBuf>,
        user_ref: &str,
        blocked: bool,
    ) -> Result<(), TelegramError>;

    async fn terminate_session(
        &self,
        session_path: Option<PathBuf>,
        authorization_id: i64,
    ) -> Result<(), TelegramError>;

    async fn terminate_all_other_sessions(
        &self,
        session_path: Option<PathBuf>,
    ) -> Result<(), TelegramError>;

    async fn get_chat_auto_delete(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
    ) -> Result<ChatAutoDeleteSummary, TelegramError>;

    async fn set_chat_auto_delete(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        period_seconds: i32,
    ) -> Result<(), TelegramError>;

    async fn set_contact(
        &self,
        session_path: Option<PathBuf>,
        user_id: i64,
        contact: bool,
        first_name: &str,
        last_name: &str,
        phone: &str,
        add_phone_privacy_exception: bool,
    ) -> Result<(), TelegramError>;

    async fn get_replies(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        message_id: i32,
        limit: usize,
        offset_id: Option<i32>,
    ) -> Result<MessagePage, TelegramError>;

    async fn get_message_read_receipts(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        message_id: i32,
    ) -> Result<Vec<MessageReadReceipt>, TelegramError>;

    async fn get_outbox_read_date(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        message_id: i32,
    ) -> Result<OutboxReadDateSummary, TelegramError>;

    async fn get_user_info(
        &self,
        session_path: Option<PathBuf>,
        user_ref: &str,
    ) -> Result<UserDetails, TelegramError>;

    async fn list_participants(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        limit: usize,
    ) -> Result<Vec<ParticipantSummary>, TelegramError>;

    async fn list_chat_members(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        filter: ParticipantFilter,
        query: Option<&str>,
        offset: usize,
        limit: usize,
    ) -> Result<ParticipantPage, TelegramError>;

    async fn get_chat_member(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        user_ref: &str,
    ) -> Result<Option<ChatMemberSummary>, TelegramError>;

    async fn add_chat_member(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        user_ref: &str,
    ) -> Result<(), TelegramError>;

    async fn remove_chat_member(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        user_ref: &str,
    ) -> Result<(), TelegramError>;

    async fn set_chat_member_admin(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        user_ref: &str,
        rights: &ChatAdminRightsSummary,
        rank: Option<&str>,
    ) -> Result<(), TelegramError>;

    async fn set_chat_member_restrictions(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        user_ref: &str,
        rights: &ChatBannedRightsSummary,
    ) -> Result<(), TelegramError>;

    async fn list_invite_links(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        admin_user_id: i64,
        revoked: bool,
        limit: usize,
    ) -> Result<Vec<InviteLinkSummary>, TelegramError>;

    async fn get_invite_link(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        admin_user_id: i64,
        invite_handle: &str,
    ) -> Result<Option<InviteLinkSummary>, TelegramError>;

    async fn export_invite_link(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        expire_date_unix: Option<i32>,
        usage_limit: Option<i32>,
        request_needed: bool,
        title: Option<&str>,
    ) -> Result<InviteLinkSummary, TelegramError>;

    async fn revoke_invite_link(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        admin_user_id: i64,
        invite_handle: &str,
    ) -> Result<InviteLinkSummary, TelegramError>;

    async fn delete_revoked_invite_link(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        admin_user_id: i64,
        invite_handle: &str,
    ) -> Result<(), TelegramError>;

    #[allow(clippy::too_many_arguments)]
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
    ) -> Result<InviteJoinerPage, TelegramError>;

    async fn process_join_request(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        user_id: i64,
        approve: bool,
    ) -> Result<(), TelegramError>;

    async fn get_account_profile(
        &self,
        session_path: Option<PathBuf>,
    ) -> Result<AccountProfileSummary, TelegramError>;

    async fn get_bot_commands(
        &self,
        session_path: Option<PathBuf>,
        language_code: &str,
    ) -> Result<Vec<BotCommandSummary>, TelegramError>;

    async fn set_bot_commands(
        &self,
        session_path: Option<PathBuf>,
        language_code: &str,
        commands: &[BotCommandSummary],
    ) -> Result<(), TelegramError>;

    async fn get_account_emoji_status(
        &self,
        session_path: Option<PathBuf>,
    ) -> Result<EmojiStatusSummary, TelegramError>;

    async fn list_emoji_statuses(
        &self,
        session_path: Option<PathBuf>,
        kind: EmojiStatusCatalogKind,
        hash: i64,
    ) -> Result<EmojiStatusCatalogPage, TelegramError>;

    async fn set_account_emoji_status(
        &self,
        session_path: Option<PathBuf>,
        status: &AccountEmojiStatusSpec,
    ) -> Result<(), TelegramError>;

    async fn clear_recent_emoji_statuses(
        &self,
        session_path: Option<PathBuf>,
    ) -> Result<(), TelegramError>;

    async fn update_account_profile(
        &self,
        session_path: Option<PathBuf>,
        first_name: Option<&str>,
        last_name: Option<&str>,
        about: Option<&str>,
    ) -> Result<(), TelegramError>;

    async fn update_account_username(
        &self,
        session_path: Option<PathBuf>,
        username: Option<&str>,
    ) -> Result<(), TelegramError>;

    async fn update_account_birthday(
        &self,
        session_path: Option<PathBuf>,
        birthday: Option<&BirthdaySummary>,
    ) -> Result<(), TelegramError>;

    async fn update_account_color(
        &self,
        session_path: Option<PathBuf>,
        for_profile: bool,
        color: Option<i32>,
        background_emoji_id: Option<&str>,
    ) -> Result<(), TelegramError>;

    async fn update_personal_channel(
        &self,
        session_path: Option<PathBuf>,
        channel_id: Option<&str>,
    ) -> Result<(), TelegramError>;

    async fn set_account_presence(
        &self,
        session_path: Option<PathBuf>,
        online: bool,
    ) -> Result<(), TelegramError>;

    async fn list_profile_photos(
        &self,
        session_path: Option<PathBuf>,
        offset: usize,
        limit: usize,
    ) -> Result<ProfilePhotoPage, TelegramError>;

    async fn get_profile_photo(
        &self,
        session_path: Option<PathBuf>,
        photo_handle: &str,
    ) -> Result<Option<ProfilePhotoSummary>, TelegramError>;

    async fn upload_profile_photo(
        &self,
        session_path: Option<PathBuf>,
        file_path: &std::path::Path,
        fallback: bool,
    ) -> Result<ProfilePhotoSummary, TelegramError>;

    async fn delete_profile_photos(
        &self,
        session_path: Option<PathBuf>,
        photo_handles: &[String],
    ) -> Result<(), TelegramError>;

    async fn get_sessions(
        &self,
        session_path: Option<PathBuf>,
    ) -> Result<Vec<SessionSummary>, TelegramError>;

    async fn get_common_chats(
        &self,
        session_path: Option<PathBuf>,
        user_ref: &str,
        max_id: i64,
        limit: usize,
    ) -> Result<Vec<ChatSummary>, TelegramError>;

    async fn get_chat_lifecycle_state(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
    ) -> Result<ChatLifecycleState, TelegramError>;

    async fn create_group(
        &self,
        session_path: Option<PathBuf>,
        title: &str,
        user_ids: &[i64],
    ) -> Result<ChatDetailsSummary, TelegramError>;

    async fn create_channel(
        &self,
        session_path: Option<PathBuf>,
        title: &str,
        about: &str,
        broadcast: bool,
    ) -> Result<ChatDetailsSummary, TelegramError>;

    async fn join_chat(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
    ) -> Result<ChatLifecycleState, TelegramError>;

    async fn leave_chat(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
    ) -> Result<ChatLifecycleState, TelegramError>;

    async fn delete_chat(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
    ) -> Result<ChatLifecycleState, TelegramError>;

    async fn update_chat_title(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        title: &str,
    ) -> Result<ChatDetailsSummary, TelegramError>;

    async fn update_chat_about(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        about: &str,
    ) -> Result<ChatDetailsSummary, TelegramError>;

    async fn update_chat_photo(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        file_path: &std::path::Path,
    ) -> Result<ChatDetailsSummary, TelegramError>;

    async fn delete_chat_photo(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
    ) -> Result<ChatDetailsSummary, TelegramError>;

    async fn get_channel_settings(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
    ) -> Result<ChannelSettingsSummary, TelegramError>;

    async fn get_chat_moderation_settings(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
    ) -> Result<ChatModerationSettingsSummary, TelegramError>;

    async fn get_channel_stats(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
    ) -> Result<ChannelStatsSummary, TelegramError>;

    #[allow(clippy::too_many_arguments)]
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
    ) -> Result<AdminLogPage, TelegramError>;

    async fn get_my_boosts(
        &self,
        session_path: Option<PathBuf>,
    ) -> Result<MyBoostsSummary, TelegramError>;

    async fn get_boost_status(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
    ) -> Result<BoostStatusSummary, TelegramError>;

    async fn list_boosts(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        gifts_only: bool,
        offset: &str,
        limit: usize,
    ) -> Result<BoostPage, TelegramError>;

    async fn apply_boosts(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        slots: &[i32],
    ) -> Result<MyBoostsSummary, TelegramError>;

    async fn check_chat_username(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        username: &str,
    ) -> Result<bool, TelegramError>;

    async fn update_chat_username(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        username: Option<&str>,
    ) -> Result<ChannelSettingsSummary, TelegramError>;

    async fn set_chat_slow_mode(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        seconds: i32,
    ) -> Result<ChannelSettingsSummary, TelegramError>;

    async fn toggle_chat_signatures(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        signatures_enabled: bool,
        profiles_enabled: bool,
    ) -> Result<ChannelSettingsSummary, TelegramError>;

    async fn toggle_chat_pre_history_hidden(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        enabled: bool,
    ) -> Result<ChannelSettingsSummary, TelegramError>;

    async fn toggle_chat_join_request(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        enabled: bool,
    ) -> Result<ChannelSettingsSummary, TelegramError>;

    async fn set_chat_default_permissions(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        target: &ChatBannedRightsSummary,
    ) -> Result<ChatModerationSettingsSummary, TelegramError>;

    async fn set_chat_available_reactions(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        target: &ChatReactionsPolicy,
    ) -> Result<ChatModerationSettingsSummary, TelegramError>;

    async fn toggle_chat_anti_spam(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        enabled: bool,
    ) -> Result<ChatModerationSettingsSummary, TelegramError>;

    async fn toggle_chat_forum_mode(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        enabled: bool,
        tabs_enabled: bool,
    ) -> Result<ChatModerationSettingsSummary, TelegramError>;

    async fn list_stories(
        &self,
        session_path: Option<PathBuf>,
        next: bool,
        hidden: bool,
        state: Option<&str>,
    ) -> Result<StoryListPage, TelegramError>;

    async fn get_peer_stories(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
    ) -> Result<StoryPeerPage, TelegramError>;

    async fn get_stories_by_ids(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        story_ids: &[i32],
    ) -> Result<StoryCollectionPage, TelegramError>;

    async fn get_stories_archive(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        offset_id: i32,
        limit: usize,
    ) -> Result<StoryCollectionPage, TelegramError>;

    async fn export_story_link(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        story_id: i32,
    ) -> Result<StoryLinkSummary, TelegramError>;

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
    ) -> Result<StoryViewersPage, TelegramError>;

    async fn view_story(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        story_ids: &[i32],
    ) -> Result<(), TelegramError>;

    async fn read_stories(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        max_id: i32,
    ) -> Result<Vec<i32>, TelegramError>;

    async fn set_story_reaction(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        story_id: i32,
        reaction: &ReactionSpec,
    ) -> Result<(), TelegramError>;

    async fn set_stories_pinned(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        story_ids: &[i32],
        pinned: bool,
    ) -> Result<Vec<i32>, TelegramError>;

    async fn toggle_stories_pinned_to_top(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        story_ids: &[i32],
    ) -> Result<bool, TelegramError>;

    async fn activate_story_stealth_mode(
        &self,
        session_path: Option<PathBuf>,
        past: bool,
        future: bool,
    ) -> Result<(), TelegramError>;

    #[allow(clippy::too_many_arguments)]
    async fn send_story_with_random_id(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        file_path: &std::path::Path,
        caption: &str,
        privacy: &StoryPrivacySpec,
        pinned: bool,
        noforwards: bool,
        period_seconds: Option<i32>,
        random_id: i64,
    ) -> Result<i32, TelegramError>;

    #[allow(clippy::too_many_arguments)]
    async fn edit_story(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        story_id: i32,
        file_path: Option<&std::path::Path>,
        caption: Option<&str>,
        privacy: Option<&StoryPrivacySpec>,
    ) -> Result<(), TelegramError>;

    async fn report_stories(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        story_ids: &[i32],
        option_hex: &str,
        message: &str,
    ) -> Result<StoryReportResult, TelegramError>;

    async fn delete_stories(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        story_ids: &[i32],
    ) -> Result<Vec<i32>, TelegramError>;

    async fn get_story(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        story_id: i32,
    ) -> Result<Option<StorySummary>, TelegramError>;

    async fn get_chat_administrators(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
    ) -> Result<Vec<ParticipantSummary>, TelegramError>;

    async fn get_online_count(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
    ) -> Result<i32, TelegramError>;

    async fn get_blocked_users(
        &self,
        session_path: Option<PathBuf>,
        offset: usize,
        limit: usize,
    ) -> Result<Vec<ContactSummary>, TelegramError>;

    async fn list_forum_topics(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        query: Option<&str>,
        limit: usize,
        offset_date: i32,
        offset_id: i32,
        offset_topic: i32,
    ) -> Result<Vec<ForumTopicSummary>, TelegramError>;

    async fn get_forum_topic(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        topic_id: i32,
    ) -> Result<Option<ForumTopicSummary>, TelegramError>;

    async fn create_forum_topic_with_random_id(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        title: &str,
        icon_color: Option<i32>,
        icon_emoji_id: Option<i64>,
        random_id: i64,
    ) -> Result<i32, TelegramError>;

    #[allow(clippy::too_many_arguments)]
    async fn edit_forum_topic(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        topic_id: i32,
        title: Option<&str>,
        icon_emoji_id: Option<i64>,
        closed: Option<bool>,
        hidden: Option<bool>,
    ) -> Result<(), TelegramError>;

    async fn set_forum_topic_pinned(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        topic_id: i32,
        pinned: bool,
    ) -> Result<(), TelegramError>;

    async fn delete_forum_topic(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        topic_id: i32,
    ) -> Result<(), TelegramError>;

    async fn get_privacy_rules(
        &self,
        session_path: Option<PathBuf>,
        key: PrivacyKey,
    ) -> Result<Vec<PrivacyRuleSummary>, TelegramError>;

    async fn set_privacy_rules(
        &self,
        session_path: Option<PathBuf>,
        key: PrivacyKey,
        rules: &[PrivacyRuleSummary],
    ) -> Result<Vec<PrivacyRuleSummary>, TelegramError>;

    async fn get_notification_settings(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
    ) -> Result<NotificationSettingsSummary, TelegramError>;

    async fn set_notification_settings(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        patch: &NotificationSettingsPatch,
    ) -> Result<(), TelegramError>;

    async fn list_sticker_sets(
        &self,
        session_path: Option<PathBuf>,
        hash: i64,
    ) -> Result<Option<Vec<StickerSetSummary>>, TelegramError>;

    async fn get_recent_stickers(
        &self,
        session_path: Option<PathBuf>,
        attached: bool,
        limit: usize,
        hash: i64,
    ) -> Result<StickerCatalogPage, TelegramError>;

    async fn get_sticker_set(
        &self,
        session_path: Option<PathBuf>,
        short_name: &str,
        hash: i32,
    ) -> Result<StickerSetDetailsPage, TelegramError>;

    async fn search_sticker_sets(
        &self,
        session_path: Option<PathBuf>,
        query: &str,
        exclude_featured: bool,
        limit: usize,
        hash: i64,
    ) -> Result<StickerSetSearchPage, TelegramError>;

    async fn get_favorite_stickers(
        &self,
        session_path: Option<PathBuf>,
        limit: usize,
        hash: i64,
    ) -> Result<StickerCatalogPage, TelegramError>;

    async fn set_sticker_favorite(
        &self,
        session_path: Option<PathBuf>,
        set_short_name: &str,
        sticker_handle: &str,
        favorite: bool,
    ) -> Result<(), TelegramError>;

    async fn set_recent_sticker(
        &self,
        session_path: Option<PathBuf>,
        set_short_name: &str,
        sticker_handle: &str,
        attached: bool,
        saved: bool,
    ) -> Result<(), TelegramError>;

    async fn install_sticker_set(
        &self,
        session_path: Option<PathBuf>,
        short_name: &str,
        archived: bool,
    ) -> Result<(), TelegramError>;

    async fn uninstall_sticker_set(
        &self,
        session_path: Option<PathBuf>,
        short_name: &str,
    ) -> Result<(), TelegramError>;

    async fn get_custom_emoji_documents(
        &self,
        session_path: Option<PathBuf>,
        document_ids: &[i64],
    ) -> Result<Vec<CustomEmojiDocumentSummary>, TelegramError>;

    async fn download_custom_emoji_to(
        &self,
        session_path: Option<PathBuf>,
        document_id: i64,
        destination: &std::path::Path,
        max_bytes: u64,
    ) -> Result<u64, TelegramError>;

    async fn get_dice_emojis(
        &self,
        session_path: Option<PathBuf>,
        hash: i32,
    ) -> Result<Option<DiceEmojiCatalog>, TelegramError>;

    #[allow(clippy::too_many_arguments)]
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
    ) -> Result<i32, TelegramError>;

    async fn get_message_rich_media(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        message_id: i32,
    ) -> Result<Option<SentRichMediaSummary>, TelegramError>;

    async fn list_message_reactions(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        message_id: i32,
        limit: usize,
        offset: Option<&str>,
    ) -> Result<MessageReactionPage, TelegramError>;

    async fn get_default_reaction(
        &self,
        session_path: Option<PathBuf>,
    ) -> Result<DefaultReactionSummary, TelegramError>;

    async fn get_top_reactions(
        &self,
        session_path: Option<PathBuf>,
        limit: usize,
        hash: i64,
    ) -> Result<ReactionCatalogPage, TelegramError>;

    async fn get_recent_reactions(
        &self,
        session_path: Option<PathBuf>,
        limit: usize,
        hash: i64,
    ) -> Result<ReactionCatalogPage, TelegramError>;

    async fn set_default_reaction(
        &self,
        session_path: Option<PathBuf>,
        reaction: &DefaultReactionSpec,
    ) -> Result<(), TelegramError>;

    async fn get_my_reaction(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        message_id: i32,
    ) -> Result<Option<ReactionSpec>, TelegramError>;

    async fn set_message_reaction(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        message_id: i32,
        reaction: &ReactionSpec,
    ) -> Result<(), TelegramError>;

    async fn edit_message(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        message_id: i32,
        text: &str,
        parse_mode: CaptionParseMode,
    ) -> Result<i32, TelegramError>;

    async fn delete_messages(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        message_ids: &[i32],
        revoke: bool,
    ) -> Result<(), TelegramError>;

    async fn set_message_pinned(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        message_id: i32,
        pinned: bool,
        silent: bool,
        one_sided: bool,
    ) -> Result<(), TelegramError>;

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
    ) -> Result<Vec<i32>, TelegramError>;

    async fn list_attachments(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        message_id: i32,
    ) -> Result<Vec<AttachmentDescriptor>, TelegramError>;

    async fn download_attachment_to(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        message_id: i32,
        destination: &std::path::Path,
        max_bytes: u64,
    ) -> Result<u64, TelegramError>;

    async fn send_file(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        file_path: &std::path::Path,
        caption: &str,
        reply_to_message_id: Option<i32>,
        mode: FileSendMode,
    ) -> Result<i32, TelegramError>;

    async fn send_file_with_random_id(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        file_path: &std::path::Path,
        caption: &str,
        reply_to_message_id: Option<i32>,
        mode: FileSendMode,
        random_id: i64,
    ) -> Result<i32, TelegramError>;

    #[allow(clippy::too_many_arguments)]
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
    ) -> Result<i32, TelegramError>;

    #[allow(clippy::too_many_arguments)]
    async fn send_voice_with_random_id(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        file_path: &std::path::Path,
        caption: &str,
        caption_parse_mode: CaptionParseMode,
        reply_to_message_id: Option<i32>,
        topic_id: Option<i32>,
        silent: bool,
        noforwards: bool,
        random_id: i64,
    ) -> Result<i32, TelegramError>;

    #[allow(clippy::too_many_arguments)]
    async fn send_video_note_with_random_id(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        file_path: &std::path::Path,
        duration_seconds: i32,
        length_pixels: i32,
        reply_to_message_id: Option<i32>,
        topic_id: Option<i32>,
        silent: bool,
        noforwards: bool,
        random_id: i64,
    ) -> Result<i32, TelegramError>;

    async fn get_message_document_media(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        message_id: i32,
    ) -> Result<Option<SentDocumentMediaSummary>, TelegramError>;

    async fn get_message_topic_id(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        message_id: i32,
    ) -> Result<Option<i32>, TelegramError>;

    #[allow(clippy::too_many_arguments)]
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
    ) -> Result<Vec<i32>, TelegramError>;

    async fn get_message_grouped_id(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        message_id: i32,
    ) -> Result<Option<i64>, TelegramError>;

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
    ) -> Result<i32, TelegramError>;

    async fn send_text_with_random_id(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        text: &str,
        reply_to_message_id: Option<i32>,
        random_id: i64,
    ) -> Result<i32, TelegramError>;

    #[allow(clippy::too_many_arguments)]
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
    ) -> Result<i32, TelegramError>;

    async fn send_rich_markdown_with_random_id(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        markdown: &str,
        reply_to_message_id: Option<i32>,
        random_id: i64,
    ) -> Result<i32, TelegramError>;

    async fn get_rich_message_snapshot(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        message_id: i32,
    ) -> Result<Option<RichMessageSnapshot>, TelegramError>;

    async fn send_message(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        text: &str,
    ) -> Result<i32, TelegramError>;
}
