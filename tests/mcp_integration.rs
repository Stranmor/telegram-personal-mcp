#![allow(
    clippy::arithmetic_side_effects,
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::unwrap_used
)]

use rmcp::{ServiceExt, model::CallToolRequestParams};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::sync::Arc;

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use telegram_personal_mcp::{FakeTelegramBackend, McpServer, McpServerSettings};

#[tokio::test]
async fn official_mcp_transport_exposes_structured_tools_and_typed_errors() -> anyhow::Result<()> {
    let backend = Arc::new(FakeTelegramBackend::fixture());
    let cache = tempfile::tempdir()?;
    let server = McpServer::with_settings(
        backend.clone(),
        McpServerSettings {
            attachment_cache_dir: cache.path().to_path_buf(),
            max_download_bytes: 1024 * 1024,
            max_upload_bytes: 1024 * 1024,
            state_db_path: cache.path().join("state.sqlite"),
        },
    );
    let (server_transport, client_transport) = tokio::io::duplex(64 * 1024);

    let server_task = tokio::spawn(async move {
        server.serve(server_transport).await?.waiting().await?;
        anyhow::Ok(())
    });
    let client = ().serve(client_transport).await?;

    let tools = client.list_all_tools().await?;
    assert_eq!(tools.len(), 309);
    assert!(
        tools
            .iter()
            .filter(|tool| !tool.name.starts_with("invoke_"))
            .all(|tool| tool.output_schema.is_some())
    );
    assert!(
        tools
            .iter()
            .all(|tool| tool.input_schema.get("additionalProperties") == Some(&json!(false)))
    );
    assert!(tools.iter().any(|tool| tool.name == "search_dialogs"));
    assert!(tools.iter().any(|tool| tool.name == "search_tools"));
    assert!(tools.iter().any(|tool| tool.name == "describe_tool"));
    assert!(tools.iter().any(|tool| tool.name == "invoke_read_only_tool"));
    assert!(tools.iter().any(|tool| tool.name == "invoke_tool"));
    assert!(tools.iter().any(|tool| tool.name == "get_unread"));
    assert!(tools.iter().any(|tool| tool.name == "get_unread_mentions"));
    assert!(tools.iter().any(|tool| tool.name == "get_unread_reactions"));
    assert!(tools.iter().any(|tool| tool.name == "get_unread_poll_votes"));
    assert!(tools.iter().any(|tool| tool.name == "search_messages"));
    assert!(tools.iter().any(|tool| tool.name == "prepare_rich_message"));
    assert!(tools.iter().any(|tool| tool.name == "send_operator_bot_rich_message"));
    assert!(tools.iter().any(|tool| tool.name == "send_saved_rich_message"));
    assert!(tools.iter().any(|tool| tool.name == "cancel_send_approval"));
    assert!(tools.iter().any(|tool| tool.name == "cancel_mutation_approval"));
    assert!(tools.iter().any(|tool| tool.name == "get_rich_message"));
    assert!(tools.iter().any(|tool| tool.name == "search_regex"));
    assert!(tools.iter().any(|tool| tool.name == "export_chat"));
    assert!(tools.iter().any(|tool| tool.name == "clear_cache"));
    assert!(tools.iter().any(|tool| tool.name == "sync_messages"));
    assert!(tools.iter().any(|tool| tool.name == "chat_analytics"));
    assert!(tools.iter().any(|tool| tool.name == "message_timeline"));
    assert!(tools.iter().any(|tool| tool.name == "today_messages"));
    assert!(tools.iter().any(|tool| tool.name == "export_cached_messages"));
    assert!(tools.iter().any(|tool| tool.name == "wait_for_new_message"));
    assert!(tools.iter().any(|tool| tool.name == "wait_for_settled_message"));
    for name in ["wait_for_new_message", "wait_for_settled_message"] {
        let tool = tools.iter().find(|tool| tool.name == name).expect("event wait tool");
        let annotations = tool.annotations.as_ref().expect("event wait annotations");
        assert_eq!(annotations.read_only_hint, Some(true));
        assert_eq!(annotations.destructive_hint, Some(false));
        assert_eq!(annotations.idempotent_hint, Some(true));
        assert!(tool.output_schema.is_some());
    }
    assert!(tools.iter().any(|tool| tool.name == "get_updates_state"));
    assert!(tools.iter().any(|tool| tool.name == "get_updates"));
    assert!(tools.iter().any(|tool| tool.name == "get_chat_moderation_settings"));
    assert!(tools.iter().any(|tool| tool.name == "set_chat_permissions"));
    assert!(tools.iter().any(|tool| tool.name == "set_chat_reactions"));
    assert!(tools.iter().any(|tool| tool.name == "toggle_anti_spam"));
    assert!(tools.iter().any(|tool| tool.name == "toggle_forum_mode"));
    assert!(tools.iter().any(|tool| tool.name == "get_channel_updates"));
    assert!(tools.iter().any(|tool| tool.name == "get_quick_replies"));
    assert!(tools.iter().any(|tool| tool.name == "get_quick_reply_messages"));
    assert!(tools.iter().any(|tool| tool.name == "get_group_call"));
    assert!(tools.iter().any(|tool| tool.name == "get_group_call_participants"));
    assert!(tools.iter().any(|tool| tool.name == "get_business_chat_links"));
    assert!(tools.iter().any(|tool| tool.name == "resolve_business_chat_link"));
    assert!(tools.iter().any(|tool| tool.name == "create_business_chat_link"));
    assert!(tools.iter().any(|tool| tool.name == "edit_business_chat_link"));
    assert!(tools.iter().any(|tool| tool.name == "delete_business_chat_link"));
    assert!(tools.iter().any(|tool| tool.name == "set_business_hours"));
    assert!(tools.iter().any(|tool| tool.name == "set_business_location"));
    assert!(tools.iter().any(|tool| tool.name == "set_business_greeting"));
    assert!(tools.iter().any(|tool| tool.name == "set_business_away"));
    assert!(tools.iter().any(|tool| tool.name == "set_business_intro"));
    assert!(tools.iter().any(|tool| tool.name == "get_stars_status"));
    assert!(tools.iter().any(|tool| tool.name == "get_stars_transactions"));
    assert!(tools.iter().any(|tool| tool.name == "get_available_star_gifts"));
    assert!(tools.iter().any(|tool| tool.name == "get_saved_star_gifts"));
    assert!(tools.iter().any(|tool| tool.name == "get_stars_topup_options"));
    assert!(tools.iter().any(|tool| tool.name == "get_stars_subscriptions"));
    assert!(tools.iter().any(|tool| tool.name == "save_star_gift"));
    assert!(tools.iter().any(|tool| tool.name == "convert_star_gift"));
    assert!(tools.iter().any(|tool| tool.name == "change_stars_subscription"));
    assert!(tools.iter().any(|tool| tool.name == "send_typing"));
    assert!(tools.iter().any(|tool| tool.name == "get_message_buttons"));
    assert!(tools.iter().any(|tool| tool.name == "press_button"));
    assert!(tools.iter().any(|tool| tool.name == "get_inline_bot_results"));
    assert!(tools.iter().any(|tool| tool.name == "send_inline_bot_result"));
    assert!(tools.iter().any(|tool| tool.name == "get_bot_commands"));
    assert!(tools.iter().any(|tool| tool.name == "set_bot_commands"));
    assert!(tools.iter().any(|tool| tool.name == "get_emoji_status"));
    assert!(tools.iter().any(|tool| tool.name == "list_emoji_statuses"));
    assert!(tools.iter().any(|tool| tool.name == "set_emoji_status"));
    assert!(tools.iter().any(|tool| tool.name == "clear_recent_emoji_statuses"));
    assert!(tools.iter().any(|tool| tool.name == "set_profile_color"));
    assert!(tools.iter().any(|tool| tool.name == "set_personal_channel"));
    assert!(tools.iter().any(|tool| tool.name == "get_suggested_folders"));
    assert!(tools.iter().any(|tool| tool.name == "toggle_folder_tags"));
    assert!(tools.iter().any(|tool| tool.name == "get_global_privacy_settings"));
    assert!(tools.iter().any(|tool| tool.name == "set_global_privacy_settings"));
    assert!(tools.iter().any(|tool| tool.name == "prepare_terminate_all_other_sessions"));
    assert!(tools.iter().any(|tool| tool.name == "get_auto_delete"));
    assert!(tools.iter().any(|tool| tool.name == "set_auto_delete"));
    assert!(tools.iter().any(|tool| tool.name == "clear_all_drafts"));
    assert!(tools.iter().any(|tool| tool.name == "get_message_link"));
    assert!(tools.iter().any(|tool| tool.name == "translate_message"));
    assert!(tools.iter().any(|tool| tool.name == "transcribe_audio"));
    assert!(tools.iter().any(|tool| tool.name == "get_transcription"));
    assert!(tools.iter().any(|tool| tool.name == "rate_transcription"));
    assert!(tools.iter().any(|tool| tool.name == "get_fact_check"));
    assert!(tools.iter().any(|tool| tool.name == "edit_fact_check"));
    assert!(tools.iter().any(|tool| tool.name == "delete_fact_check"));
    assert!(tools.iter().any(|tool| tool.name == "search_saved_messages"));
    assert!(tools.iter().any(|tool| tool.name == "get_saved_dialogs"));
    assert!(tools.iter().any(|tool| tool.name == "get_saved_history"));
    assert!(tools.iter().any(|tool| tool.name == "delete_saved_history"));
    assert!(tools.iter().any(|tool| tool.name == "toggle_saved_dialog_pin"));
    assert!(tools.iter().any(|tool| tool.name == "get_saved_reaction_tags"));
    assert!(tools.iter().any(|tool| tool.name == "update_saved_reaction_tag"));
    assert!(tools.iter().any(|tool| tool.name == "get_default_tag_reactions"));
    assert!(tools.iter().any(|tool| tool.name == "get_web_preview"));
    assert!(tools.iter().any(|tool| tool.name == "get_groups_for_discussion"));
    assert!(tools.iter().any(|tool| tool.name == "list_contacts"));
    assert!(tools.iter().any(|tool| tool.name == "search_contacts"));
    assert!(tools.iter().any(|tool| tool.name == "get_pinned_message"));
    assert!(tools.iter().any(|tool| tool.name == "get_scheduled_messages"));
    assert!(tools.iter().any(|tool| tool.name == "get_poll"));
    assert!(tools.iter().any(|tool| tool.name == "list_poll_voters"));
    assert!(tools.iter().any(|tool| tool.name == "list_drafts"));
    assert!(tools.iter().any(|tool| tool.name == "get_replies"));
    assert!(tools.iter().any(|tool| tool.name == "get_discussion_message"));
    assert!(tools.iter().any(|tool| tool.name == "get_chat_details"));
    assert!(tools.iter().any(|tool| tool.name == "create_group"));
    assert!(tools.iter().any(|tool| tool.name == "create_channel"));
    assert!(tools.iter().any(|tool| tool.name == "join_chat"));
    assert!(tools.iter().any(|tool| tool.name == "leave_chat"));
    assert!(tools.iter().any(|tool| tool.name == "delete_chat"));
    assert!(tools.iter().any(|tool| tool.name == "update_chat_title"));
    assert!(tools.iter().any(|tool| tool.name == "update_chat_about"));
    assert!(tools.iter().any(|tool| tool.name == "update_chat_photo"));
    assert!(tools.iter().any(|tool| tool.name == "delete_chat_photo"));
    assert!(tools.iter().any(|tool| tool.name == "get_channel_info"));
    assert!(tools.iter().any(|tool| tool.name == "get_channel_stats"));
    assert!(tools.iter().any(|tool| tool.name == "get_admin_log"));
    assert!(tools.iter().any(|tool| tool.name == "get_my_boosts"));
    assert!(tools.iter().any(|tool| tool.name == "get_boost_status"));
    assert!(tools.iter().any(|tool| tool.name == "list_boosts"));
    assert!(tools.iter().any(|tool| tool.name == "apply_boost"));
    assert!(tools.iter().any(|tool| tool.name == "check_username"));
    assert!(tools.iter().any(|tool| tool.name == "update_chat_username"));
    assert!(tools.iter().any(|tool| tool.name == "set_slow_mode"));
    assert!(tools.iter().any(|tool| tool.name == "toggle_signatures"));
    assert!(tools.iter().any(|tool| tool.name == "toggle_pre_history_hidden"));
    assert!(tools.iter().any(|tool| tool.name == "toggle_join_request"));
    assert!(tools.iter().any(|tool| tool.name == "list_stories"));
    assert!(tools.iter().any(|tool| tool.name == "get_peer_stories"));
    assert!(tools.iter().any(|tool| tool.name == "get_stories_by_id"));
    assert!(tools.iter().any(|tool| tool.name == "get_stories_archive"));
    assert!(tools.iter().any(|tool| tool.name == "export_story_link"));
    assert!(tools.iter().any(|tool| tool.name == "get_story_viewers"));
    assert!(tools.iter().any(|tool| tool.name == "view_story"));
    assert!(tools.iter().any(|tool| tool.name == "read_stories"));
    assert!(tools.iter().any(|tool| tool.name == "react_to_story"));
    assert!(tools.iter().any(|tool| tool.name == "toggle_story_pinned"));
    assert!(tools.iter().any(|tool| tool.name == "toggle_story_pinned_to_top"));
    assert!(tools.iter().any(|tool| tool.name == "activate_stealth_mode"));
    assert!(tools.iter().any(|tool| tool.name == "send_story"));
    assert!(tools.iter().any(|tool| tool.name == "edit_story"));
    assert!(tools.iter().any(|tool| tool.name == "report_story"));
    assert!(tools.iter().any(|tool| tool.name == "delete_story"));
    assert!(tools.iter().any(|tool| tool.name == "mark_as_read"));
    assert!(tools.iter().any(|tool| tool.name == "mark_reactions_read"));
    assert!(tools.iter().any(|tool| tool.name == "get_message_read_receipts"));
    assert!(tools.iter().any(|tool| tool.name == "get_outbox_read_date"));
    assert!(tools.iter().any(|tool| tool.name == "get_user_info"));
    assert!(tools.iter().any(|tool| tool.name == "list_participants"));
    assert!(tools.iter().any(|tool| tool.name == "get_chat_member"));
    assert!(tools.iter().any(|tool| tool.name == "get_my_role"));
    assert!(tools.iter().any(|tool| tool.name == "prepare_add_chat_member"));
    assert!(tools.iter().any(|tool| tool.name == "prepare_remove_chat_member"));
    assert!(tools.iter().any(|tool| tool.name == "prepare_set_chat_member_admin"));
    assert!(tools.iter().any(|tool| tool.name == "prepare_set_chat_member_restrictions"));
    assert!(tools.iter().any(|tool| tool.name == "list_invite_links"));
    assert!(tools.iter().any(|tool| tool.name == "get_invite_link"));
    assert!(tools.iter().any(|tool| tool.name == "export_invite_link"));
    assert!(tools.iter().any(|tool| tool.name == "revoke_invite_link"));
    assert!(tools.iter().any(|tool| tool.name == "delete_revoked_invite_link"));
    assert!(tools.iter().any(|tool| tool.name == "list_invite_joiners"));
    assert!(tools.iter().any(|tool| tool.name == "list_join_requests"));
    assert!(tools.iter().any(|tool| tool.name == "approve_join_request"));
    assert!(tools.iter().any(|tool| tool.name == "hide_join_request"));
    assert!(tools.iter().any(|tool| tool.name == "approve_all_join_requests"));
    assert!(tools.iter().any(|tool| tool.name == "hide_all_join_requests"));
    assert!(tools.iter().any(|tool| tool.name == "get_account_profile"));
    assert!(tools.iter().any(|tool| tool.name == "update_profile"));
    assert!(tools.iter().any(|tool| tool.name == "update_username"));
    assert!(tools.iter().any(|tool| tool.name == "set_birthday"));
    assert!(tools.iter().any(|tool| tool.name == "set_presence"));
    assert!(tools.iter().any(|tool| tool.name == "list_profile_photos"));
    assert!(tools.iter().any(|tool| tool.name == "get_profile_photo"));
    assert!(tools.iter().any(|tool| tool.name == "upload_profile_photo"));
    assert!(tools.iter().any(|tool| tool.name == "delete_profile_photos"));
    assert!(tools.iter().any(|tool| tool.name == "list_folders"));
    assert!(tools.iter().any(|tool| tool.name == "create_folder"));
    assert!(tools.iter().any(|tool| tool.name == "edit_folder"));
    assert!(tools.iter().any(|tool| tool.name == "delete_folder"));
    assert!(tools.iter().any(|tool| tool.name == "reorder_folders"));
    assert!(tools.iter().any(|tool| tool.name == "add_chat_to_folder"));
    assert!(tools.iter().any(|tool| tool.name == "remove_chat_from_folder"));
    for alias in [
        "get_me",
        "resolve_username",
        "search_global",
        "list_messages",
        "list_scheduled_messages",
        "get_message_reactions",
        "get_poll_results",
        "get_chat_admins",
        "list_topics",
        "get_topic",
        "get_notify_settings",
        "list_blocked",
        "get_privacy",
        "edit_message",
        "delete_messages",
        "forward_messages",
        "add_reaction",
        "remove_reaction",
        "pin_message",
        "unpin_message",
        "create_poll",
        "vote_poll",
        "close_poll",
        "create_topic",
        "edit_topic",
        "delete_topic",
        "pin_topic",
        "save_draft",
        "clear_draft",
        "update_notify_settings",
        "set_privacy",
        "create_invite_link",
        "add_chat_member",
        "remove_chat_member",
        "promote_member",
        "restrict_member",
        "ban_member",
        "unban_member",
        "block_user",
        "unblock_user",
    ] {
        assert!(tools.iter().any(|tool| tool.name == alias), "missing alias {alias}");
    }
    assert!(tools.iter().any(|tool| tool.name == "get_sessions"));
    assert!(tools.iter().any(|tool| tool.name == "get_common_chats"));
    assert!(tools.iter().any(|tool| tool.name == "get_chat_administrators"));
    assert!(tools.iter().any(|tool| tool.name == "get_online_count"));
    assert!(tools.iter().any(|tool| tool.name == "get_blocked_users"));
    assert!(tools.iter().any(|tool| tool.name == "list_forum_topics"));
    assert!(tools.iter().any(|tool| tool.name == "get_forum_topic"));
    assert!(tools.iter().any(|tool| tool.name == "read_topic_messages"));
    assert!(tools.iter().any(|tool| tool.name == "get_privacy_rules"));
    assert!(tools.iter().any(|tool| tool.name == "get_notification_settings"));
    assert!(tools.iter().any(|tool| tool.name == "list_sticker_sets"));
    assert!(tools.iter().any(|tool| tool.name == "get_recent_stickers"));
    assert!(tools.iter().any(|tool| tool.name == "get_sticker_set"));
    assert!(tools.iter().any(|tool| tool.name == "search_sticker_sets"));
    assert!(tools.iter().any(|tool| tool.name == "send_sticker"));
    assert!(tools.iter().any(|tool| tool.name == "get_favorite_stickers"));
    assert!(tools.iter().any(|tool| tool.name == "set_sticker_favorite"));
    assert!(tools.iter().any(|tool| tool.name == "add_recent_sticker"));
    assert!(tools.iter().any(|tool| tool.name == "install_sticker_set"));
    assert!(tools.iter().any(|tool| tool.name == "uninstall_sticker_set"));
    assert!(tools.iter().any(|tool| tool.name == "get_custom_emoji_documents"));
    assert!(tools.iter().any(|tool| tool.name == "download_custom_emoji"));
    assert!(tools.iter().any(|tool| tool.name == "send_gif"));
    assert!(tools.iter().any(|tool| tool.name == "get_dice_emojis"));
    assert!(tools.iter().any(|tool| tool.name == "send_location"));
    assert!(tools.iter().any(|tool| tool.name == "send_venue"));
    assert!(tools.iter().any(|tool| tool.name == "send_contact"));
    assert!(tools.iter().any(|tool| tool.name == "send_dice"));
    assert!(tools.iter().any(|tool| tool.name == "list_message_reactions"));
    assert!(tools.iter().any(|tool| tool.name == "get_default_reaction"));
    assert!(tools.iter().any(|tool| tool.name == "get_top_reactions"));
    assert!(tools.iter().any(|tool| tool.name == "get_recent_reactions"));
    assert!(tools.iter().any(|tool| tool.name == "set_default_reaction"));
    assert!(tools.iter().any(|tool| tool.name == "request_send_approval"));
    assert!(tools.iter().any(|tool| tool.name == "get_send_approval"));
    assert!(tools.iter().any(|tool| tool.name == "prepare_edit_message"));
    assert!(tools.iter().any(|tool| tool.name == "prepare_delete_messages"));
    assert!(tools.iter().any(|tool| tool.name == "prepare_restore_deleted_messages"));
    assert!(tools.iter().any(|tool| tool.name == "prepare_set_message_pinned"));
    assert!(tools.iter().any(|tool| tool.name == "prepare_set_message_reaction"));
    assert!(tools.iter().any(|tool| tool.name == "prepare_set_draft"));
    assert!(tools.iter().any(|tool| tool.name == "prepare_set_dialog_unread"));
    assert!(tools.iter().any(|tool| tool.name == "prepare_set_dialog_archived"));
    assert!(tools.iter().any(|tool| tool.name == "prepare_set_dialog_pinned"));
    assert!(tools.iter().any(|tool| tool.name == "prepare_set_user_blocked"));
    assert!(tools.iter().any(|tool| tool.name == "prepare_set_contact"));
    assert!(tools.iter().any(|tool| tool.name == "prepare_set_privacy_rules"));
    assert!(tools.iter().any(|tool| tool.name == "prepare_set_notification_settings"));
    assert!(tools.iter().any(|tool| tool.name == "prepare_schedule_message"));
    assert!(tools.iter().any(|tool| tool.name == "prepare_schedule_file_message"));
    assert!(tools.iter().any(|tool| tool.name == "prepare_edit_scheduled_message"));
    assert!(tools.iter().any(|tool| tool.name == "prepare_delete_scheduled_messages"));
    assert!(tools.iter().any(|tool| tool.name == "prepare_send_scheduled_messages_now"));
    assert!(tools.iter().any(|tool| tool.name == "prepare_create_poll"));
    assert!(tools.iter().any(|tool| tool.name == "prepare_set_poll_vote"));
    assert!(tools.iter().any(|tool| tool.name == "prepare_stop_poll"));
    assert!(tools.iter().any(|tool| tool.name == "prepare_create_forum_topic"));
    assert!(tools.iter().any(|tool| tool.name == "prepare_edit_forum_topic"));
    assert!(tools.iter().any(|tool| tool.name == "prepare_set_forum_topic_pinned"));
    assert!(tools.iter().any(|tool| tool.name == "prepare_delete_forum_topic"));
    assert!(tools.iter().any(|tool| tool.name == "prepare_terminate_session"));
    assert!(tools.iter().any(|tool| tool.name == "prepare_forward_messages"));
    assert!(tools.iter().any(|tool| tool.name == "prepare_file_message"));
    assert!(tools.iter().any(|tool| tool.name == "get_prepared_mutation"));
    assert!(tools.iter().any(|tool| tool.name == "request_mutation_approval"));
    assert!(tools.iter().any(|tool| tool.name == "get_mutation_approval"));

    let health = client.call_tool(CallToolRequestParams::new("health")).await?;
    assert_eq!(health.is_error, Some(false));
    assert_eq!(
        health
            .structured_content
            .as_ref()
            .and_then(|value| value.pointer("/data/username"))
            .and_then(|value| value.as_str()),
        Some("fixture_account")
    );
    assert_eq!(
        health
            .structured_content
            .as_ref()
            .and_then(|value| value.pointer("/data/premium"))
            .and_then(|value| value.as_bool()),
        Some(false)
    );
    assert_eq!(
        health
            .structured_content
            .as_ref()
            .and_then(|value| value.pointer("/data/max_upload_bytes"))
            .and_then(|value| value.as_u64()),
        Some(1024 * 1024)
    );
    assert!(
        health
            .structured_content
            .as_ref()
            .and_then(|value| value.pointer("/data/session_path"))
            .is_none()
    );

    let unread = client
        .call_tool(
            CallToolRequestParams::new("get_unread")
                .with_arguments(json!({ "limit": 10 }).as_object().expect("object").clone()),
        )
        .await?;
    assert_eq!(unread.is_error, Some(false));
    assert_eq!(
        unread
            .structured_content
            .as_ref()
            .and_then(|value| value.pointer("/data/dialogs/0/chat_ref"))
            .and_then(Value::as_str),
        Some("@alex")
    );
    assert_eq!(
        unread
            .structured_content
            .as_ref()
            .and_then(|value| value.pointer("/data/dialogs/0/unread_count"))
            .and_then(Value::as_i64),
        Some(2)
    );
    assert_eq!(
        unread
            .structured_content
            .as_ref()
            .and_then(|value| value.pointer("/data/completeness"))
            .and_then(Value::as_str),
        Some("complete")
    );

    let message_link = client
        .call_tool(
            CallToolRequestParams::new("get_message_link").with_arguments(
                json!({
                    "chat_id": "-100505",
                    "message_id": 50,
                    "grouped": true,
                    "thread": true
                })
                .as_object()
                .expect("object")
                .clone(),
            ),
        )
        .await?;
    assert_eq!(message_link.is_error, Some(false));
    assert_eq!(
        message_link
            .structured_content
            .as_ref()
            .and_then(|value| value.pointer("/data/message_link/chat_id"))
            .and_then(Value::as_str),
        Some("-100505")
    );
    assert!(
        message_link
            .structured_content
            .as_ref()
            .and_then(|value| value.pointer("/data/message_link/link"))
            .and_then(Value::as_str)
            .is_some_and(|link| link.ends_with("/50"))
    );

    let invalid_message_link = client
        .call_tool(CallToolRequestParams::new("get_message_link").with_arguments(
            json!({ "chat_id": "@alex", "message_id": 11 }).as_object().expect("object").clone(),
        ))
        .await?;
    assert_eq!(invalid_message_link.is_error, Some(true));

    let web_preview = client
        .call_tool(
            CallToolRequestParams::new("get_web_preview").with_arguments(
                json!({ "message": "See https://example.com/release." })
                    .as_object()
                    .expect("object")
                    .clone(),
            ),
        )
        .await?;
    assert_eq!(web_preview.is_error, Some(false));
    assert_eq!(
        web_preview
            .structured_content
            .as_ref()
            .and_then(|value| value.pointer("/data/preview/state"))
            .and_then(Value::as_str),
        Some("loaded")
    );
    assert_eq!(
        web_preview
            .structured_content
            .as_ref()
            .and_then(|value| value.pointer("/data/preview/url"))
            .and_then(Value::as_str),
        Some("https://example.com/release")
    );

    let discussion_groups =
        client.call_tool(CallToolRequestParams::new("get_groups_for_discussion")).await?;
    assert_eq!(discussion_groups.is_error, Some(false));
    assert_eq!(
        discussion_groups
            .structured_content
            .as_ref()
            .and_then(|value| value.pointer("/data/chats/0/chat_id"))
            .and_then(Value::as_str),
        Some("-202")
    );

    let prepared_privacy = client
        .call_tool(
            CallToolRequestParams::new("prepare_set_privacy_rules").with_arguments(
                json!({
                    "key": "phone_number",
                    "rules": [
                        { "kind": "allow_users", "user_refs": ["@dana_candidate"] },
                        { "kind": "allow_chat_participants", "chat_refs": ["-202"] },
                        { "kind": "disallow_all" }
                    ]
                })
                .as_object()
                .expect("object")
                .clone(),
            ),
        )
        .await?;
    assert_eq!(prepared_privacy.is_error, Some(false));
    assert_eq!(
        prepared_privacy
            .structured_content
            .as_ref()
            .and_then(|value| value.pointer("/data/action/rules/0/user_ids/0"))
            .and_then(|value| value.as_i64()),
        Some(404)
    );
    assert_eq!(
        prepared_privacy
            .structured_content
            .as_ref()
            .and_then(|value| value.pointer("/data/action/rules/1/chat_ids/0"))
            .and_then(|value| value.as_i64()),
        Some(202)
    );
    assert_eq!(
        prepared_privacy
            .structured_content
            .as_ref()
            .and_then(|value| value.pointer("/data/action/user_bindings/0/user/user_id"))
            .and_then(|value| value.as_i64()),
        Some(404)
    );

    let prepared_notifications = client
        .call_tool(
            CallToolRequestParams::new("prepare_set_notification_settings").with_arguments(
                json!({
                    "chat_id": "@alex",
                    "show_previews": false,
                    "silent": true,
                    "mute_until_unix": 2_000_000_000,
                    "sound": { "kind": "default" },
                    "stories_muted": true,
                    "stories_hide_sender": true,
                    "stories_sound": { "kind": "none" }
                })
                .as_object()
                .expect("object")
                .clone(),
            ),
        )
        .await?;
    assert_eq!(prepared_notifications.is_error, Some(false));
    assert_eq!(
        prepared_notifications
            .structured_content
            .as_ref()
            .and_then(|value| value.pointer("/data/action/chat_id"))
            .and_then(|value| value.as_str()),
        Some("101")
    );
    assert_eq!(
        prepared_notifications
            .structured_content
            .as_ref()
            .and_then(|value| value.pointer("/data/action/sound_target/platform"))
            .and_then(|value| value.as_str()),
        Some("other")
    );
    assert_eq!(
        prepared_notifications
            .structured_content
            .as_ref()
            .and_then(|value| value.pointer("/data/action/sound_target/authorization_id"))
            .and_then(|value| value.as_i64()),
        Some(1)
    );

    let prepared_schedule = client
        .call_tool(
            CallToolRequestParams::new("prepare_schedule_message").with_arguments(
                json!({
                    "chat_id": "@alex",
                    "text": "Scheduled through the official MCP transport",
                    "source": {
                        "kind": "telegram_message",
                        "relation": "triggered_outreach",
                        "chat_id": "-100505",
                        "message_id": 50
                    },
                    "reply_to_message_id": 11,
                    "schedule_at_unix": 2_000_000_100,
                    "silent": true,
                    "noforwards": true
                })
                .as_object()
                .expect("object")
                .clone(),
            ),
        )
        .await?;
    assert_eq!(prepared_schedule.is_error, Some(false));
    assert_eq!(
        prepared_schedule
            .structured_content
            .as_ref()
            .and_then(|value| value.pointer("/data/action/reply_to/message_id"))
            .and_then(|value| value.as_i64()),
        Some(11)
    );
    assert_eq!(
        prepared_schedule
            .structured_content
            .as_ref()
            .and_then(|value| value.pointer("/data/action/source/message_id"))
            .and_then(|value| value.as_i64()),
        Some(50)
    );
    assert_eq!(
        prepared_schedule
            .structured_content
            .as_ref()
            .and_then(|value| value.pointer("/data/action/source/target/display_name"))
            .and_then(|value| value.as_str()),
        Some("Ops Supergroup")
    );
    assert_eq!(
        prepared_schedule
            .structured_content
            .as_ref()
            .and_then(|value| value.pointer("/data/action/source/message_date_unix"))
            .and_then(|value| value.as_i64()),
        Some(1_700_000_050)
    );
    assert_eq!(
        prepared_schedule
            .structured_content
            .as_ref()
            .and_then(|value| value.pointer("/data/action/silent"))
            .and_then(|value| value.as_bool()),
        Some(true)
    );

    let prepared_public_source_schedule = client
        .call_tool(
            CallToolRequestParams::new("prepare_schedule_message").with_arguments(
                json!({
                    "chat_id": "@alex",
                    "text": "Scheduled from a public opportunity",
                    "source": {
                        "kind": "public_web",
                        "relation": "triggered_outreach",
                        "publisher": "Example Marketplace · fixture_workspace",
                        "title": "Synthetic content automation brief",
                        "url": "https://example.test/projects/fixture-3228097",
                        "published_at_unix": 1_785_509_667_i64,
                        "observed_at_unix": 1_785_540_576_i64
                    },
                    "schedule_at_unix": 2_000_000_101
                })
                .as_object()
                .expect("object")
                .clone(),
            ),
        )
        .await?;
    assert_eq!(prepared_public_source_schedule.is_error, Some(false));
    assert_eq!(
        prepared_public_source_schedule
            .structured_content
            .as_ref()
            .and_then(|value| value.pointer("/data/action/source/kind"))
            .and_then(|value| value.as_str()),
        Some("public_web")
    );
    assert_eq!(
        prepared_public_source_schedule
            .structured_content
            .as_ref()
            .and_then(|value| value.pointer("/data/action/source/url"))
            .and_then(|value| value.as_str()),
        Some("https://example.test/projects/fixture-3228097")
    );

    let poll = client
        .call_tool(CallToolRequestParams::new("get_poll").with_arguments(
            json!({ "chat_id": "-202", "message_id": 20 }).as_object().expect("object").clone(),
        ))
        .await?;
    assert_eq!(poll.is_error, Some(false));
    assert_eq!(
        poll.structured_content
            .as_ref()
            .and_then(|value| value.pointer("/data/poll/question"))
            .and_then(|value| value.as_str()),
        Some("Which release path should we use?")
    );
    assert_eq!(
        poll.structured_content
            .as_ref()
            .and_then(|value| value.pointer("/data/poll/answers/2/option/option_hex"))
            .and_then(|value| value.as_str()),
        Some("02")
    );

    let voters = client
        .call_tool(
            CallToolRequestParams::new("list_poll_voters").with_arguments(
                json!({
                    "chat_id": "-202",
                    "message_id": 20,
                    "option_index": 1,
                    "limit": 10
                })
                .as_object()
                .expect("object")
                .clone(),
            ),
        )
        .await?;
    assert_eq!(voters.is_error, Some(false));
    assert_eq!(
        voters
            .structured_content
            .as_ref()
            .and_then(|value| value.pointer("/data/voters/0/peer_ref"))
            .and_then(|value| value.as_str()),
        Some("303")
    );

    let prepared_poll = client
        .call_tool(
            CallToolRequestParams::new("prepare_create_poll").with_arguments(
                json!({
                    "chat_id": "-202",
                    "question": "Which invariant must a release preserve?",
                    "answers": ["Continuity", "Convenience", "Novelty"],
                    "public_voters": true,
                    "quiz": true,
                    "correct_option_index": 0,
                    "shuffle_answers": true,
                    "countries_iso2": ["us", "de"],
                    "close_period_seconds": 60,
                    "solution": "Continuity keeps the live consumer path usable.",
                    "reply_to_message_id": 21,
                    "silent": true,
                    "noforwards": true
                })
                .as_object()
                .expect("object")
                .clone(),
            ),
        )
        .await?;
    assert_eq!(prepared_poll.is_error, Some(false));
    assert_eq!(
        prepared_poll
            .structured_content
            .as_ref()
            .and_then(|value| value.pointer("/data/action/poll/countries_iso2/0"))
            .and_then(|value| value.as_str()),
        Some("US")
    );
    assert_eq!(
        prepared_poll
            .structured_content
            .as_ref()
            .and_then(|value| value.pointer("/data/action/reply_to/message_id"))
            .and_then(|value| value.as_i64()),
        Some(21)
    );

    let prepared_vote = client
        .call_tool(
            CallToolRequestParams::new("prepare_set_poll_vote").with_arguments(
                json!({ "chat_id": "-202", "message_id": 20, "option_indices": [2] })
                    .as_object()
                    .expect("object")
                    .clone(),
            ),
        )
        .await?;
    assert_eq!(prepared_vote.is_error, Some(false));
    assert_eq!(
        prepared_vote
            .structured_content
            .as_ref()
            .and_then(|value| value.pointer("/data/action/options/0/option_hex"))
            .and_then(|value| value.as_str()),
        Some("02")
    );

    let prepared_stop = client
        .call_tool(CallToolRequestParams::new("prepare_stop_poll").with_arguments(
            json!({ "chat_id": "-202", "message_id": 20 }).as_object().expect("object").clone(),
        ))
        .await?;
    assert_eq!(prepared_stop.is_error, Some(false));
    assert_eq!(
        prepared_stop
            .structured_content
            .as_ref()
            .and_then(|value| value.pointer("/data/action/before/poll_id"))
            .and_then(|value| value.as_i64()),
        Some(9_001)
    );

    let forum_topic = client
        .call_tool(CallToolRequestParams::new("get_forum_topic").with_arguments(
            json!({ "chat_id": "-202", "topic_id": 44 }).as_object().expect("object").clone(),
        ))
        .await?;
    assert_eq!(forum_topic.is_error, Some(false));
    assert_eq!(
        forum_topic
            .structured_content
            .as_ref()
            .and_then(|value| value.pointer("/data/topic/title"))
            .and_then(|value| value.as_str()),
        Some("Release coordination")
    );

    let prepared_topic_create = client
        .call_tool(
            CallToolRequestParams::new("prepare_create_forum_topic").with_arguments(
                json!({
                    "chat_id": "-202",
                    "title": "Post-release review",
                    "icon_color": 7_322_320
                })
                .as_object()
                .expect("object")
                .clone(),
            ),
        )
        .await?;
    assert_eq!(prepared_topic_create.is_error, Some(false));
    assert_eq!(
        prepared_topic_create
            .structured_content
            .as_ref()
            .and_then(|value| value.pointer("/data/action/chat_id"))
            .and_then(|value| value.as_str()),
        Some("-202")
    );

    let prepared_topic_edit = client
        .call_tool(
            CallToolRequestParams::new("prepare_edit_forum_topic").with_arguments(
                json!({
                    "chat_id": "-202",
                    "topic_id": 44,
                    "title": "Release execution",
                    "icon_emoji_id": 123_456,
                    "closed": true,
                    "hidden": true
                })
                .as_object()
                .expect("object")
                .clone(),
            ),
        )
        .await?;
    assert_eq!(prepared_topic_edit.is_error, Some(false));
    assert_eq!(
        prepared_topic_edit
            .structured_content
            .as_ref()
            .and_then(|value| value.pointer("/data/action/before/pinned"))
            .and_then(|value| value.as_bool()),
        Some(true)
    );

    let prepared_topic_pin = client
        .call_tool(
            CallToolRequestParams::new("prepare_set_forum_topic_pinned").with_arguments(
                json!({ "chat_id": "-202", "topic_id": 44, "pinned": false })
                    .as_object()
                    .expect("object")
                    .clone(),
            ),
        )
        .await?;
    assert_eq!(prepared_topic_pin.is_error, Some(false));
    assert_eq!(
        prepared_topic_pin
            .structured_content
            .as_ref()
            .and_then(|value| value.pointer("/data/action/pinned"))
            .and_then(|value| value.as_bool()),
        Some(false)
    );

    let prepared_topic_delete = client
        .call_tool(CallToolRequestParams::new("prepare_delete_forum_topic").with_arguments(
            json!({ "chat_id": "-202", "topic_id": 44 }).as_object().expect("object").clone(),
        ))
        .await?;
    assert_eq!(prepared_topic_delete.is_error, Some(false));
    assert_eq!(
        prepared_topic_delete
            .structured_content
            .as_ref()
            .and_then(|value| value.pointer("/data/action/before/topic_id"))
            .and_then(|value| value.as_i64()),
        Some(44)
    );

    let prepared_scheduled_edit = client
        .call_tool(
            CallToolRequestParams::new("prepare_edit_scheduled_message").with_arguments(
                json!({
                    "chat_id": "@alex",
                    "message_id": 301,
                    "text": "Edited scheduled fixture",
                    "schedule_at_unix": 2_000_000_200
                })
                .as_object()
                .expect("object")
                .clone(),
            ),
        )
        .await?;
    assert_eq!(prepared_scheduled_edit.is_error, Some(false));
    assert_eq!(
        prepared_scheduled_edit
            .structured_content
            .as_ref()
            .and_then(|value| value.pointer("/data/action/before/message_id"))
            .and_then(|value| value.as_i64()),
        Some(301)
    );

    for tool in ["prepare_delete_scheduled_messages", "prepare_send_scheduled_messages_now"] {
        let prepared = client
            .call_tool(
                CallToolRequestParams::new(tool).with_arguments(
                    json!({ "chat_id": "@alex", "message_ids": [301] })
                        .as_object()
                        .expect("object")
                        .clone(),
                ),
            )
            .await?;
        assert_eq!(prepared.is_error, Some(false), "{tool}");
        assert_eq!(
            prepared
                .structured_content
                .as_ref()
                .and_then(|value| value.pointer("/data/action/messages/0/message_id"))
                .and_then(|value| value.as_i64()),
            Some(301),
            "{tool}"
        );
    }

    let prepared_edit = client
        .call_tool(
            CallToolRequestParams::new("prepare_edit_message").with_arguments(
                json!({
                    "chat_id": "-202",
                    "message_id": 21,
                    "text": "Release passed with typed mutation approval"
                })
                .as_object()
                .expect("object")
                .clone(),
            ),
        )
        .await?;
    assert_eq!(prepared_edit.is_error, Some(false));
    assert_eq!(
        prepared_edit
            .structured_content
            .as_ref()
            .and_then(|value| value.pointer("/data/action/before/text"))
            .and_then(|value| value.as_str()),
        Some("Release passed")
    );
    let mutation_id = prepared_edit
        .structured_content
        .as_ref()
        .and_then(|value| value.pointer("/data/mutation_id"))
        .and_then(|value| value.as_str())
        .expect("mutation id");
    let prepared_readback = client
        .call_tool(CallToolRequestParams::new("get_prepared_mutation").with_arguments(
            json!({ "mutation_id": mutation_id }).as_object().expect("object").clone(),
        ))
        .await?;
    assert_eq!(prepared_readback.is_error, Some(false));
    assert_eq!(
        prepared_readback
            .structured_content
            .as_ref()
            .and_then(|value| value.pointer("/data/state"))
            .and_then(|value| value.as_str()),
        Some("prepared")
    );

    let contacts = client
        .call_tool(
            CallToolRequestParams::new("list_contacts")
                .with_arguments(json!({ "limit": 10 }).as_object().expect("object").clone()),
        )
        .await?;
    assert_eq!(contacts.is_error, Some(false));
    assert_eq!(
        contacts
            .structured_content
            .as_ref()
            .and_then(|value| value.pointer("/data/contacts/0/user_id"))
            .and_then(|value| value.as_i64()),
        Some(101)
    );

    let contact_search = client
        .call_tool(CallToolRequestParams::new("search_contacts").with_arguments(
            json!({ "query": "mira", "limit": 10 }).as_object().expect("object").clone(),
        ))
        .await?;
    assert_eq!(contact_search.is_error, Some(false));
    assert_eq!(
        contact_search
            .structured_content
            .as_ref()
            .and_then(|value| value.pointer("/data/contacts/0/username"))
            .and_then(|value| value.as_str()),
        Some("mira_ops")
    );

    let pinned = client
        .call_tool(
            CallToolRequestParams::new("get_pinned_message")
                .with_arguments(json!({ "chat_id": "@alex" }).as_object().expect("object").clone()),
        )
        .await?;
    assert_eq!(pinned.is_error, Some(false));
    assert_eq!(
        pinned
            .structured_content
            .as_ref()
            .and_then(|value| value.pointer("/data/message/message_id"))
            .and_then(|value| value.as_i64()),
        Some(10)
    );

    let scheduled = client
        .call_tool(CallToolRequestParams::new("get_scheduled_messages").with_arguments(
            json!({ "chat_id": "@alex", "limit": 10 }).as_object().expect("object").clone(),
        ))
        .await?;
    assert_eq!(scheduled.is_error, Some(false));
    assert_eq!(
        scheduled
            .structured_content
            .as_ref()
            .and_then(|value| value.pointer("/data/messages/0/message_id"))
            .and_then(|value| value.as_i64()),
        Some(301)
    );

    let replies = client
        .call_tool(
            CallToolRequestParams::new("get_replies").with_arguments(
                json!({ "chat_id": "@alex", "message_id": 10, "limit": 10 })
                    .as_object()
                    .expect("object")
                    .clone(),
            ),
        )
        .await?;
    assert_eq!(replies.is_error, Some(false));
    assert_eq!(
        replies
            .structured_content
            .as_ref()
            .and_then(|value| value.pointer("/data/messages/0/message_id"))
            .and_then(|value| value.as_i64()),
        Some(11)
    );

    let read_receipts = client
        .call_tool(CallToolRequestParams::new("get_message_read_receipts").with_arguments(
            json!({ "chat_id": "-202", "message_id": 21 }).as_object().expect("object").clone(),
        ))
        .await?;
    assert_eq!(read_receipts.is_error, Some(false));
    assert_eq!(
        read_receipts
            .structured_content
            .as_ref()
            .and_then(|value| value.pointer("/data/receipts/0/user_id"))
            .and_then(|value| value.as_i64()),
        Some(101)
    );

    let user_info =
        client
            .call_tool(CallToolRequestParams::new("get_user_info").with_arguments(
                json!({ "user_ref": "@alex" }).as_object().expect("object").clone(),
            ))
            .await?;
    assert_eq!(user_info.is_error, Some(false));
    assert_eq!(
        user_info
            .structured_content
            .as_ref()
            .and_then(|value| value.pointer("/data/user/user_id"))
            .and_then(|value| value.as_i64()),
        Some(101)
    );
    assert_eq!(
        user_info
            .structured_content
            .as_ref()
            .and_then(|value| value.pointer("/data/status"))
            .and_then(|value| value.as_str()),
        Some("recently")
    );

    let participants = client
        .call_tool(CallToolRequestParams::new("list_participants").with_arguments(
            json!({ "chat_id": "-202", "limit": 10 }).as_object().expect("object").clone(),
        ))
        .await?;
    assert_eq!(participants.is_error, Some(false));
    assert_eq!(
        participants
            .structured_content
            .as_ref()
            .and_then(|value| value.pointer("/data/participants/0/role"))
            .and_then(|value| value.as_str()),
        Some("creator")
    );
    assert_eq!(
        participants
            .structured_content
            .as_ref()
            .and_then(|value| value.pointer("/data/participants/1/user/user_id"))
            .and_then(|value| value.as_i64()),
        Some(101)
    );

    let sessions = client.call_tool(CallToolRequestParams::new("get_sessions")).await?;
    assert_eq!(sessions.is_error, Some(false));
    assert_eq!(
        sessions
            .structured_content
            .as_ref()
            .and_then(|value| value.pointer("/data/sessions/0/current"))
            .and_then(|value| value.as_bool()),
        Some(true)
    );

    let common_chats = client
        .call_tool(CallToolRequestParams::new("get_common_chats").with_arguments(
            json!({ "user_ref": "@alex", "limit": 10 }).as_object().expect("object").clone(),
        ))
        .await?;
    assert_eq!(common_chats.is_error, Some(false));
    assert_eq!(
        common_chats
            .structured_content
            .as_ref()
            .and_then(|value| value.pointer("/data/chats/0/chat_id"))
            .and_then(|value| value.as_str()),
        Some("-202")
    );

    let administrators = client
        .call_tool(
            CallToolRequestParams::new("get_chat_administrators")
                .with_arguments(json!({ "chat_id": "-202" }).as_object().expect("object").clone()),
        )
        .await?;
    assert_eq!(administrators.is_error, Some(false));
    assert_eq!(
        administrators
            .structured_content
            .as_ref()
            .and_then(|value| value.pointer("/data/participants/0/role"))
            .and_then(|value| value.as_str()),
        Some("creator")
    );

    let online_count = client
        .call_tool(
            CallToolRequestParams::new("get_online_count")
                .with_arguments(json!({ "chat_id": "-202" }).as_object().expect("object").clone()),
        )
        .await?;
    assert_eq!(online_count.is_error, Some(false));
    assert_eq!(
        online_count
            .structured_content
            .as_ref()
            .and_then(|value| value.pointer("/data/online_count"))
            .and_then(|value| value.as_i64()),
        Some(2)
    );

    let blocked_users = client
        .call_tool(
            CallToolRequestParams::new("get_blocked_users")
                .with_arguments(json!({ "limit": 10 }).as_object().expect("object").clone()),
        )
        .await?;
    assert_eq!(blocked_users.is_error, Some(false));
    assert_eq!(
        blocked_users
            .structured_content
            .as_ref()
            .and_then(|value| value.pointer("/data/contacts/0/user_id"))
            .and_then(|value| value.as_i64()),
        Some(303)
    );

    let forum_topics = client
        .call_tool(
            CallToolRequestParams::new("list_forum_topics").with_arguments(
                json!({ "chat_id": "-202", "query": "release", "limit": 10 })
                    .as_object()
                    .expect("object")
                    .clone(),
            ),
        )
        .await?;
    assert_eq!(forum_topics.is_error, Some(false));
    assert_eq!(
        forum_topics
            .structured_content
            .as_ref()
            .and_then(|value| value.pointer("/data/topics/0/topic_id"))
            .and_then(|value| value.as_i64()),
        Some(44)
    );

    let privacy =
        client
            .call_tool(CallToolRequestParams::new("get_privacy_rules").with_arguments(
                json!({ "key": "phone_number" }).as_object().expect("object").clone(),
            ))
            .await?;
    assert_eq!(privacy.is_error, Some(false));
    assert_eq!(
        privacy
            .structured_content
            .as_ref()
            .and_then(|value| value.pointer("/data/rules/0/kind"))
            .and_then(|value| value.as_str()),
        Some("allow_contacts")
    );

    let notifications = client
        .call_tool(
            CallToolRequestParams::new("get_notification_settings")
                .with_arguments(json!({ "chat_id": "@alex" }).as_object().expect("object").clone()),
        )
        .await?;
    assert_eq!(notifications.is_error, Some(false));
    assert_eq!(
        notifications
            .structured_content
            .as_ref()
            .and_then(|value| value.pointer("/data/settings/show_previews"))
            .and_then(|value| value.as_bool()),
        Some(true)
    );

    let sticker_sets = client.call_tool(CallToolRequestParams::new("list_sticker_sets")).await?;
    assert_eq!(sticker_sets.is_error, Some(false));
    assert_eq!(
        sticker_sets
            .structured_content
            .as_ref()
            .and_then(|value| value.pointer("/data/sticker_sets/0/content_hash"))
            .and_then(|value| value.as_i64()),
        Some(4242)
    );

    let reactions = client
        .call_tool(
            CallToolRequestParams::new("list_message_reactions").with_arguments(
                json!({ "chat_id": "-202", "message_id": 21, "limit": 10 })
                    .as_object()
                    .expect("object")
                    .clone(),
            ),
        )
        .await?;
    assert_eq!(reactions.is_error, Some(false));
    assert_eq!(
        reactions
            .structured_content
            .as_ref()
            .and_then(|value| value.pointer("/data/reactions/0/emoji"))
            .and_then(|value| value.as_str()),
        Some("👍")
    );

    let unknown_send_argument = client
        .call_tool(
            CallToolRequestParams::new("send_message").with_arguments(
                json!({
                    "chat_id": "@alex",
                    "text": "Must not be sent",
                    "reply_to_message_id": 11
                })
                .as_object()
                .expect("object")
                .clone(),
            ),
        )
        .await
        .expect_err("unknown send arguments must be rejected at the MCP boundary");
    assert!(unknown_send_argument.to_string().contains("unknown field"));
    assert!(backend.sent_messages().await.is_empty());

    let search = client
        .call_tool(
            CallToolRequestParams::new("search_messages").with_arguments(
                json!({
                    "chat_id": "@alex",
                    "query": "contract",
                    "limit": 10
                })
                .as_object()
                .expect("object")
                .clone(),
            ),
        )
        .await?;
    assert_eq!(search.is_error, Some(false));
    assert_eq!(
        search
            .structured_content
            .as_ref()
            .and_then(|value| value.pointer("/data/messages/0/message_id"))
            .and_then(|value| value.as_i64()),
        Some(11)
    );

    let unsupported_global_page = client
        .call_tool(
            CallToolRequestParams::new("search_messages").with_arguments(
                json!({
                    "query": "contract",
                    "limit": 10,
                    "offset_id": 11
                })
                .as_object()
                .expect("object")
                .clone(),
            ),
        )
        .await?;
    assert_eq!(unsupported_global_page.is_error, Some(true));
    assert_eq!(
        unsupported_global_page
            .structured_content
            .as_ref()
            .and_then(|value| value.pointer("/error/code"))
            .and_then(|value| value.as_str()),
        Some("INVALID_ARGUMENT")
    );

    let global_group_match = client
        .call_tool(
            CallToolRequestParams::new("search_messages").with_arguments(
                json!({
                    "query": "Release",
                    "limit": 10
                })
                .as_object()
                .expect("object")
                .clone(),
            ),
        )
        .await?;
    assert_eq!(
        global_group_match
            .structured_content
            .as_ref()
            .and_then(|value| value.pointer("/data/messages/0/chat_ref"))
            .and_then(|value| value.as_str()),
        Some("-202")
    );
    let round_trip_group_message = client
        .call_tool(
            CallToolRequestParams::new("get_message").with_arguments(
                json!({
                    "chat_id": "-202",
                    "message_id": 21
                })
                .as_object()
                .expect("object")
                .clone(),
            ),
        )
        .await?;
    assert_eq!(round_trip_group_message.is_error, Some(false));

    let context = client
        .call_tool(
            CallToolRequestParams::new("read_context").with_arguments(
                json!({
                    "chat_id": "@alex",
                    "message_id": 11,
                    "before": 5,
                    "after": 5
                })
                .as_object()
                .expect("object")
                .clone(),
            ),
        )
        .await?;
    assert_eq!(context.is_error, Some(false));
    assert_eq!(
        context
            .structured_content
            .as_ref()
            .and_then(|value| value.pointer("/data/target/message_id"))
            .and_then(|value| value.as_i64()),
        Some(11)
    );
    assert_eq!(
        context
            .structured_content
            .as_ref()
            .and_then(|value| value.pointer("/data/older/0/message_id"))
            .and_then(|value| value.as_i64()),
        Some(10)
    );

    let prepared = client
        .call_tool(
            CallToolRequestParams::new("prepare_message").with_arguments(
                json!({
                    "chat_id": "@alex",
                    "text": "Prepared exact reply",
                    "source": {
                        "kind": "telegram_message",
                        "relation": "reply_context",
                        "chat_id": "@alex",
                        "message_id": 11
                    },
                    "reply_to_message_id": 11
                })
                .as_object()
                .expect("object")
                .clone(),
            ),
        )
        .await?;
    assert_eq!(prepared.is_error, Some(false));
    let draft_id = prepared
        .structured_content
        .as_ref()
        .and_then(|value| value.pointer("/data/draft_id"))
        .and_then(|value| value.as_str())
        .expect("draft id")
        .to_string();
    let payload_sha256 = prepared
        .structured_content
        .as_ref()
        .and_then(|value| value.pointer("/data/payload_sha256"))
        .and_then(|value| value.as_str())
        .expect("payload sha256")
        .to_string();

    let prepared_readback =
        client
            .call_tool(CallToolRequestParams::new("get_prepared").with_arguments(
                json!({ "draft_id": draft_id }).as_object().expect("object").clone(),
            ))
            .await?;
    assert_eq!(prepared_readback.is_error, Some(false));
    assert_eq!(
        prepared_readback
            .structured_content
            .as_ref()
            .and_then(|value| value.pointer("/data/payload_sha256"))
            .and_then(|value| value.as_str()),
        Some(payload_sha256.as_str())
    );

    let mismatched_approval = client
        .call_tool(
            CallToolRequestParams::new("send_prepared").with_arguments(
                json!({
                    "draft_id": draft_id,
                    "approved_payload_sha256": "not-the-approved-hash"
                })
                .as_object()
                .expect("object")
                .clone(),
            ),
        )
        .await?;
    assert_eq!(mismatched_approval.is_error, Some(true));
    assert_eq!(
        mismatched_approval
            .structured_content
            .as_ref()
            .and_then(|value| value.pointer("/error/code"))
            .and_then(|value| value.as_str()),
        Some("PAYLOAD_HASH_MISMATCH")
    );
    assert!(backend.sent_messages().await.is_empty());

    backend.set_fixture_account_user_id(8).await;
    let wrong_account = client
        .call_tool(
            CallToolRequestParams::new("send_prepared").with_arguments(
                json!({
                    "draft_id": draft_id,
                    "approved_payload_sha256": payload_sha256
                })
                .as_object()
                .expect("object")
                .clone(),
            ),
        )
        .await?;
    assert_eq!(wrong_account.is_error, Some(true));
    assert_eq!(
        wrong_account
            .structured_content
            .as_ref()
            .and_then(|value| value.pointer("/error/code"))
            .and_then(|value| value.as_str()),
        Some("ACCOUNT_MISMATCH")
    );
    assert!(backend.sent_messages().await.is_empty());
    backend.set_fixture_account_user_id(7).await;

    let send_prepared_args = json!({
        "draft_id": draft_id,
        "approved_payload_sha256": payload_sha256
    })
    .as_object()
    .expect("object")
    .clone();
    let prepared_sent = client
        .call_tool(
            CallToolRequestParams::new("send_prepared").with_arguments(send_prepared_args.clone()),
        )
        .await?;
    assert_eq!(prepared_sent.is_error, Some(false));
    let first_prepared_message_id = prepared_sent
        .structured_content
        .as_ref()
        .and_then(|value| value.pointer("/data/message_id"))
        .and_then(|value| value.as_i64());
    let prepared_replayed = client
        .call_tool(CallToolRequestParams::new("send_prepared").with_arguments(send_prepared_args))
        .await?;
    assert_eq!(prepared_replayed.is_error, Some(false));
    assert_eq!(
        prepared_replayed
            .structured_content
            .as_ref()
            .and_then(|value| value.pointer("/data/message_id"))
            .and_then(|value| value.as_i64()),
        first_prepared_message_id
    );
    assert_eq!(backend.sent_messages().await.len(), 1);

    let downloaded = client
        .call_tool(
            CallToolRequestParams::new("download_attachment").with_arguments(
                json!({
                    "chat_id": "@alex",
                    "message_id": 11
                })
                .as_object()
                .expect("object")
                .clone(),
            ),
        )
        .await?;
    assert_eq!(downloaded.is_error, Some(false));
    let downloaded_path = downloaded
        .structured_content
        .as_ref()
        .and_then(|value| value.pointer("/data/local_path"))
        .and_then(|value| value.as_str())
        .expect("downloaded path");
    let downloaded_sha256 = downloaded
        .structured_content
        .as_ref()
        .and_then(|value| value.pointer("/data/sha256"))
        .and_then(|value| value.as_str())
        .expect("downloaded sha256")
        .to_string();
    assert!(std::path::Path::new(downloaded_path).is_file());
    #[cfg(unix)]
    {
        let downloaded_path = std::path::Path::new(downloaded_path);
        assert_eq!(std::fs::metadata(downloaded_path)?.permissions().mode() & 0o777, 0o400);
        let attachment_dir = downloaded_path.parent().expect("attachment directory");
        let account_dir = attachment_dir.parent().expect("account directory");
        assert_eq!(std::fs::metadata(attachment_dir)?.permissions().mode() & 0o777, 0o700);
        assert_eq!(std::fs::metadata(account_dir)?.permissions().mode() & 0o777, 0o700);
    }
    assert_eq!(
        downloaded
            .structured_content
            .as_ref()
            .and_then(|value| value.pointer("/data/bytes_written"))
            .and_then(|value| value.as_u64()),
        Some(26)
    );

    let downloaded_again = client
        .call_tool(
            CallToolRequestParams::new("download_attachment").with_arguments(
                json!({
                    "chat_id": "@alex",
                    "message_id": 11
                })
                .as_object()
                .expect("object")
                .clone(),
            ),
        )
        .await?;
    assert_eq!(downloaded_again.is_error, Some(false));
    assert_eq!(
        downloaded_again
            .structured_content
            .as_ref()
            .and_then(|value| value.pointer("/data/local_path"))
            .and_then(|value| value.as_str()),
        Some(downloaded_path)
    );
    assert_eq!(
        downloaded_again
            .structured_content
            .as_ref()
            .and_then(|value| value.pointer("/data/sha256"))
            .and_then(|value| value.as_str()),
        Some(downloaded_sha256.as_str())
    );

    let mut updated_bytes = std::fs::read(downloaded_path)?;
    updated_bytes.extend_from_slice(b"% deterministic portfolio edit\n");
    let updated_path = std::path::Path::new(downloaded_path)
        .parent()
        .expect("attachment directory")
        .join("contract-updated.pdf");
    std::fs::write(&updated_path, &updated_bytes)?;
    let updated_sha256 = format!("{:x}", Sha256::digest(&updated_bytes));

    backend.set_fixture_account_user_id(8).await;
    let cross_account_file = client
        .call_tool(
            CallToolRequestParams::new("send_file").with_arguments(
                json!({
                    "chat_id": "@alex",
                    "file_path": updated_path,
                    "caption": "Must stay bound to the account that downloaded it",
                    "reply_to_message_id": 11,
                    "mode": "document",
                    "approved_sha256": updated_sha256
                })
                .as_object()
                .expect("object")
                .clone(),
            ),
        )
        .await?;
    assert_eq!(cross_account_file.is_error, Some(true));
    assert_eq!(
        cross_account_file
            .structured_content
            .as_ref()
            .and_then(|value| value.pointer("/error/code"))
            .and_then(|value| value.as_str()),
        Some("FILE_OUTSIDE_UPLOAD_ROOT")
    );
    assert_eq!(backend.sent_messages().await.len(), 1);
    backend.set_fixture_account_user_id(7).await;

    let wrong_file_hash = client
        .call_tool(
            CallToolRequestParams::new("send_file").with_arguments(
                json!({
                    "chat_id": "@alex",
                    "file_path": updated_path,
                    "caption": "Updated contract",
                    "reply_to_message_id": 11,
                    "mode": "document",
                    "approved_sha256": downloaded_sha256
                })
                .as_object()
                .expect("object")
                .clone(),
            ),
        )
        .await?;
    assert_eq!(wrong_file_hash.is_error, Some(true));
    assert_eq!(
        wrong_file_hash
            .structured_content
            .as_ref()
            .and_then(|value| value.pointer("/error/code"))
            .and_then(|value| value.as_str()),
        Some("FILE_HASH_MISMATCH")
    );
    assert_eq!(backend.sent_messages().await.len(), 1);

    let sent_file = client
        .call_tool(
            CallToolRequestParams::new("send_file").with_arguments(
                json!({
                    "chat_id": "@alex",
                    "file_path": updated_path,
                    "caption": "Updated contract",
                    "reply_to_message_id": 11,
                    "mode": "document",
                    "approved_sha256": updated_sha256
                })
                .as_object()
                .expect("object")
                .clone(),
            ),
        )
        .await?;
    assert_eq!(sent_file.is_error, Some(false));
    assert_eq!(
        sent_file
            .structured_content
            .as_ref()
            .and_then(|value| value.pointer("/data/sha256"))
            .and_then(|value| value.as_str()),
        Some(updated_sha256.as_str())
    );
    let sent_messages = backend.sent_messages().await;
    assert_eq!(sent_messages.len(), 2);
    let file_message = sent_messages.last().expect("file send receipt");
    assert_eq!(file_message.reply_to_message_id, Some(11));
    assert_eq!(file_message.file_mode, Some(telegram_personal_mcp::FileSendMode::Document));
    assert!(
        file_message
            .file_name
            .as_deref()
            .is_some_and(|name| name.ends_with("contract-updated.pdf"))
    );

    let outside_upload_root = tempfile::tempdir()?;
    let outside_file = outside_upload_root.path().join("not-managed.txt");
    std::fs::write(&outside_file, b"must not be exfiltrated")?;
    let outside_sha256 = format!("{:x}", Sha256::digest(b"must not be exfiltrated"));
    let rejected_file = client
        .call_tool(
            CallToolRequestParams::new("send_file").with_arguments(
                json!({
                    "chat_id": "@alex",
                    "file_path": outside_file,
                    "caption": "Should not leave the machine",
                    "mode": "document",
                    "approved_sha256": outside_sha256
                })
                .as_object()
                .expect("object")
                .clone(),
            ),
        )
        .await?;
    assert_eq!(rejected_file.is_error, Some(true));
    assert_eq!(
        rejected_file
            .structured_content
            .as_ref()
            .and_then(|value| value.pointer("/error/code"))
            .and_then(|value| value.as_str()),
        Some("FILE_OUTSIDE_UPLOAD_ROOT")
    );
    assert_eq!(backend.sent_messages().await.len(), 2);

    let sent = client
        .call_tool(
            CallToolRequestParams::new("send_message").with_arguments(
                json!({
                    "chat_id": "@alex",
                    "text": "Updated contract attached"
                })
                .as_object()
                .expect("object")
                .clone(),
            ),
        )
        .await?;
    assert_eq!(sent.is_error, Some(false));
    assert_eq!(backend.sent_messages().await.len(), 3);

    let rejected = client
        .call_tool(
            CallToolRequestParams::new("send_message").with_arguments(
                json!({
                    "chat_id": "@alex",
                    "text": "   "
                })
                .as_object()
                .expect("object")
                .clone(),
            ),
        )
        .await?;
    assert_eq!(rejected.is_error, Some(true));
    assert_eq!(
        rejected
            .structured_content
            .as_ref()
            .and_then(|value| value.pointer("/error/code"))
            .and_then(|value| value.as_str()),
        Some("INVALID_ARGUMENT")
    );
    assert_eq!(backend.sent_messages().await.len(), 3);

    client.cancel().await?;
    server_task.await??;
    Ok(())
}

#[tokio::test]
async fn rich_messages_use_structured_dispatch_and_saved_messages_are_self_only()
-> anyhow::Result<()> {
    let backend = Arc::new(FakeTelegramBackend::fixture());
    let temp = tempfile::tempdir()?;
    let server = McpServer::with_settings(
        backend.clone(),
        McpServerSettings {
            attachment_cache_dir: temp.path().join("attachments"),
            max_download_bytes: 1024 * 1024,
            max_upload_bytes: 1024 * 1024,
            state_db_path: temp.path().join("state.sqlite"),
        },
    );
    let (server_transport, client_transport) = tokio::io::duplex(64 * 1024);
    let server_task = tokio::spawn(async move {
        server.serve(server_transport).await?.waiting().await?;
        anyhow::Ok(())
    });
    let client = ().serve(client_transport).await?;

    let prepared = client
        .call_tool(
            CallToolRequestParams::new("prepare_rich_message").with_arguments(
                json!({
                    "chat_id": "@alex",
                    "markdown": "# Structured reply\n\n- **One**\n- _Two_",
                    "source": {
                        "kind": "telegram_message",
                        "relation": "reply_context",
                        "chat_id": "@alex",
                        "message_id": 11
                    },
                    "reply_to_message_id": 11
                })
                .as_object()
                .expect("object")
                .clone(),
            ),
        )
        .await?;
    assert_eq!(prepared.is_error, Some(false));
    assert_eq!(
        prepared
            .structured_content
            .as_ref()
            .and_then(|value| value.pointer("/data/kind"))
            .and_then(|value| value.as_str()),
        Some("rich_markdown")
    );
    let draft_id = prepared
        .structured_content
        .as_ref()
        .and_then(|value| value.pointer("/data/draft_id"))
        .and_then(|value| value.as_str())
        .expect("draft id");
    let payload_sha256 = prepared
        .structured_content
        .as_ref()
        .and_then(|value| value.pointer("/data/payload_sha256"))
        .and_then(|value| value.as_str())
        .expect("payload hash");
    let dispatched = client
        .call_tool(
            CallToolRequestParams::new("send_prepared").with_arguments(
                json!({
                    "draft_id": draft_id,
                    "approved_payload_sha256": payload_sha256
                })
                .as_object()
                .expect("object")
                .clone(),
            ),
        )
        .await?;
    assert_eq!(dispatched.is_error, Some(false));
    let prepared_message_id = dispatched
        .structured_content
        .as_ref()
        .and_then(|value| value.pointer("/data/message_id"))
        .and_then(|value| value.as_i64())
        .expect("prepared rich message id");
    let prepared_readback = client
        .call_tool(
            CallToolRequestParams::new("get_rich_message").with_arguments(
                json!({ "chat_id": "@alex", "message_id": prepared_message_id })
                    .as_object()
                    .expect("object")
                    .clone(),
            ),
        )
        .await?;
    assert_eq!(prepared_readback.is_error, Some(false));
    assert!(
        prepared_readback
            .structured_content
            .as_ref()
            .and_then(|value| value.pointer("/data/block_count"))
            .and_then(|value| value.as_u64())
            .is_some_and(|count| count > 0)
    );

    let saved = client
        .call_tool(CallToolRequestParams::new("send_saved_rich_message").with_arguments(
            json!({
                "markdown": "# Saved Rich Text\n\n**Bold** and _italic_.\n\n> Exact structured readback."
            })
            .as_object()
            .expect("object")
            .clone(),
        ))
        .await?;
    assert_eq!(saved.is_error, Some(false));
    assert_eq!(
        saved
            .structured_content
            .as_ref()
            .and_then(|value| value.pointer("/data/status"))
            .and_then(|value| value.as_str()),
        Some("sent_and_verified")
    );
    assert_eq!(
        saved
            .structured_content
            .as_ref()
            .and_then(|value| value.pointer("/data/chat_id"))
            .and_then(|value| value.as_str()),
        Some("7")
    );
    let saved_message_id = saved
        .structured_content
        .as_ref()
        .and_then(|value| value.pointer("/data/message_id"))
        .and_then(|value| value.as_i64())
        .expect("saved rich message id");
    let saved_readback = client
        .call_tool(
            CallToolRequestParams::new("get_rich_message").with_arguments(
                json!({ "chat_id": "7", "message_id": saved_message_id })
                    .as_object()
                    .expect("object")
                    .clone(),
            ),
        )
        .await?;
    assert_eq!(saved_readback.is_error, Some(false));
    assert_eq!(
        saved_readback
            .structured_content
            .as_ref()
            .and_then(|value| value.pointer("/data/state_sha256"))
            .and_then(|value| value.as_str())
            .map(str::len),
        Some(64)
    );
    assert_eq!(backend.sent_messages().await[1].chat_id, "7");

    client.cancel().await?;
    server_task.await??;
    Ok(())
}
