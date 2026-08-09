use rmcp::{
    ServiceExt,
    model::CallToolRequestParams,
    transport::{ConfigureCommandExt, TokioChildProcess},
};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::process::Stdio;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

const ONE_PIXEL_GIF: &[u8] = b"GIF89a\x01\x00\x01\x00\x80\x00\x00\x00\x00\x00\xff\xff\xff!\xf9\x04\x01\x00\x00\x00\x00,\x00\x00\x00\x00\x01\x00\x01\x00\x00\x02\x02D\x01\x00;";

#[tokio::test]
async fn binary_demo_mode_serves_mcp_over_real_stdio() -> anyhow::Result<()> {
    let temp = tempfile::tempdir()?;
    let binary = env!("CARGO_BIN_EXE_telegram-personal-mcp");
    let transport =
        TokioChildProcess::new(tokio::process::Command::new(binary).configure(|command| {
            command
                .arg("--demo")
                .arg("--attachment-cache-dir")
                .arg(temp.path().join("attachments"))
                .arg("--state-db-path")
                .arg(temp.path().join("state.sqlite"))
                .arg("direct")
                .env_remove("TELEGRAM_API_ID")
                .env_remove("TELEGRAM_API_HASH")
                .env_remove("TELEGRAM_MCP_SESSION_PATH")
                .env_remove("TELEGRAM_MCP_SESSION_CACHE_DIR");
        }))?;

    let client = ().serve(transport).await?;
    let tools = client.list_all_tools().await?;
    assert_eq!(tools.len(), 22);
    assert!(
        tools
            .iter()
            .filter(|tool| !tool.name.starts_with("invoke_"))
            .all(|tool| tool.output_schema.is_some())
    );
    for expected in [
        "health",
        "read_chat",
        "search_messages",
        "prepare_message",
        "prepare_rich_message",
        "send_saved_rich_message",
        "get_rich_message",
        "request_send_approval",
        "search_tools",
        "describe_tool",
        "invoke_read_only_tool",
        "invoke_tool",
    ] {
        assert!(tools.iter().any(|tool| tool.name == expected), "missing compact tool {expected}");
    }
    assert!(!tools.iter().any(|tool| tool.name == "list_drafts"));
    assert_eq!(
        tools
            .iter()
            .find(|tool| tool.name == "invoke_read_only_tool")
            .and_then(|tool| tool.annotations.as_ref())
            .and_then(|annotations| annotations.read_only_hint),
        Some(true)
    );
    assert_eq!(
        tools
            .iter()
            .find(|tool| tool.name == "invoke_tool")
            .and_then(|tool| tool.annotations.as_ref())
            .and_then(|annotations| annotations.read_only_hint),
        Some(false)
    );

    let search = client
        .call_tool(CallToolRequestParams::new("search_tools").with_arguments(
            json!({ "query": "story", "limit": 100 }).as_object().expect("object").clone(),
        ))
        .await?;
    assert_eq!(search.is_error, Some(false));
    assert!(
        search
            .structured_content
            .as_ref()
            .and_then(|value| value.pointer("/data/tools"))
            .and_then(|value| value.as_array())
            .is_some_and(|tools| tools.iter().any(|tool| tool["name"] == "send_story"))
    );

    let described =
        client
            .call_tool(CallToolRequestParams::new("describe_tool").with_arguments(
                json!({ "name": "list_drafts" }).as_object().expect("object").clone(),
            ))
            .await?;
    assert_eq!(described.is_error, Some(false));
    assert_eq!(
        described
            .structured_content
            .as_ref()
            .and_then(|value| value.pointer("/data/tool/name"))
            .and_then(|value| value.as_str()),
        Some("list_drafts")
    );

    let invoked_read = client
        .call_tool(CallToolRequestParams::new("invoke_read_only_tool").with_arguments(
            json!({ "name": "list_drafts", "arguments": {} }).as_object().expect("object").clone(),
        ))
        .await?;
    assert_eq!(invoked_read.is_error, Some(false));
    assert_eq!(
        invoked_read
            .structured_content
            .as_ref()
            .and_then(|value| value.pointer("/data/drafts/0/draft/message"))
            .and_then(|value| value.as_str()),
        Some("Review the contract before replying")
    );

    let invoked_prepare = client
        .call_tool(
            CallToolRequestParams::new("invoke_tool").with_arguments(
                json!({
                    "name": "prepare_set_dialog_unread",
                    "arguments": { "chat_id": "@alex", "unread": false }
                })
                .as_object()
                .expect("object")
                .clone(),
            ),
        )
        .await?;
    assert_eq!(invoked_prepare.is_error, Some(false));

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

    let prepared_edit = client
        .call_tool(
            CallToolRequestParams::new("prepare_edit_message").with_arguments(
                json!({
                    "chat_id": "-202",
                    "message_id": 21,
                    "text": "Release passed via real stdio mutation preparation"
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

    let prepared_unpin = client
        .call_tool(
            CallToolRequestParams::new("prepare_set_message_pinned").with_arguments(
                json!({
                    "chat_id": "@alex",
                    "message_id": 10,
                    "pinned": false,
                    "silent": true,
                    "one_sided": false
                })
                .as_object()
                .expect("object")
                .clone(),
            ),
        )
        .await?;
    assert_eq!(prepared_unpin.is_error, Some(false));
    assert_eq!(
        prepared_unpin
            .structured_content
            .as_ref()
            .and_then(|value| value.pointer("/data/action/before/pinned"))
            .and_then(|value| value.as_bool()),
        Some(true)
    );

    let prepared_reaction = client
        .call_tool(
            CallToolRequestParams::new("prepare_set_message_reaction").with_arguments(
                json!({
                    "chat_id": "@alex",
                    "message_id": 10,
                    "reaction": {
                        "kind": "emoji",
                        "emoji": "🔥",
                        "big": false,
                        "add_to_recent": true
                    }
                })
                .as_object()
                .expect("object")
                .clone(),
            ),
        )
        .await?;
    assert_eq!(prepared_reaction.is_error, Some(false));
    assert_eq!(
        prepared_reaction
            .structured_content
            .as_ref()
            .and_then(|value| value.pointer("/data/action/reaction/emoji"))
            .and_then(|value| value.as_str()),
        Some("🔥")
    );

    let prepared_forward = client
        .call_tool(
            CallToolRequestParams::new("prepare_forward_messages").with_arguments(
                json!({
                    "source_chat_id": "@alex",
                    "destination_chat_id": "-202",
                    "message_ids": [11],
                    "silent": true,
                    "drop_author": true,
                    "drop_media_captions": false,
                    "noforwards": false
                })
                .as_object()
                .expect("object")
                .clone(),
            ),
        )
        .await?;
    assert_eq!(prepared_forward.is_error, Some(false));
    assert_eq!(
        prepared_forward
            .structured_content
            .as_ref()
            .and_then(|value| value.pointer("/data/action/messages/0/message_id"))
            .and_then(|value| value.as_i64()),
        Some(11)
    );

    let drafts = client.call_tool(CallToolRequestParams::new("list_drafts")).await?;
    assert_eq!(drafts.is_error, Some(false));
    assert_eq!(
        drafts
            .structured_content
            .as_ref()
            .and_then(|value| value.pointer("/data/drafts/0/draft/message"))
            .and_then(|value| value.as_str()),
        Some("Review the contract before replying")
    );
    let prepared_draft = client
        .call_tool(
            CallToolRequestParams::new("prepare_set_draft").with_arguments(
                json!({
                    "chat_id": "@alex",
                    "text": "Draft saved through real stdio"
                })
                .as_object()
                .expect("object")
                .clone(),
            ),
        )
        .await?;
    assert_eq!(prepared_draft.is_error, Some(false));
    assert_eq!(
        prepared_draft
            .structured_content
            .as_ref()
            .and_then(|value| value.pointer("/data/action/text"))
            .and_then(|value| value.as_str()),
        Some("Draft saved through real stdio")
    );

    for (tool, field, value) in [
        ("prepare_set_dialog_unread", "unread", false),
        ("prepare_set_dialog_archived", "archived", true),
        ("prepare_set_dialog_pinned", "pinned", true),
    ] {
        let prepared_dialog = client
            .call_tool(CallToolRequestParams::new(tool).with_arguments(
                json!({ "chat_id": "@alex", field: value }).as_object().expect("object").clone(),
            ))
            .await?;
        assert_eq!(prepared_dialog.is_error, Some(false), "{tool}");
    }

    let prepared_block = client
        .call_tool(CallToolRequestParams::new("prepare_set_user_blocked").with_arguments(
            json!({ "user_ref": "@alex", "blocked": true }).as_object().expect("object").clone(),
        ))
        .await?;
    assert_eq!(prepared_block.is_error, Some(false));
    assert_eq!(
        prepared_block
            .structured_content
            .as_ref()
            .and_then(|value| value.pointer("/data/action/user/user_id"))
            .and_then(|value| value.as_i64()),
        Some(101)
    );

    let prepared_contact = client
        .call_tool(
            CallToolRequestParams::new("prepare_set_contact").with_arguments(
                json!({
                    "user_ref": "@dana_candidate",
                    "contact": true,
                    "first_name": "Dana",
                    "last_name": "Candidate",
                    "phone": "+15550000404",
                    "add_phone_privacy_exception": false
                })
                .as_object()
                .expect("object")
                .clone(),
            ),
        )
        .await?;
    assert_eq!(prepared_contact.is_error, Some(false));
    assert_eq!(
        prepared_contact
            .structured_content
            .as_ref()
            .and_then(|value| value.pointer("/data/action/before/user_id"))
            .and_then(|value| value.as_i64()),
        Some(404)
    );
    assert_eq!(
        prepared_contact
            .structured_content
            .as_ref()
            .and_then(|value| value.pointer("/data/action/before/is_contact"))
            .and_then(|value| value.as_bool()),
        Some(false)
    );

    let prepared_privacy = client
        .call_tool(
            CallToolRequestParams::new("prepare_set_privacy_rules").with_arguments(
                json!({
                    "key": "status_timestamp",
                    "rules": [
                        { "kind": "allow_users", "user_refs": ["@alex"] },
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
        Some(101)
    );

    let prepared_notifications = client
        .call_tool(
            CallToolRequestParams::new("prepare_set_notification_settings").with_arguments(
                json!({
                    "chat_id": "@alex",
                    "silent": true,
                    "sound": { "kind": "default" }
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
            .and_then(|value| value.pointer("/data/action/patch/silent"))
            .and_then(|value| value.as_bool()),
        Some(true)
    );
    assert_eq!(
        prepared_notifications
            .structured_content
            .as_ref()
            .and_then(|value| value.pointer("/data/action/sound_target/platform"))
            .and_then(|value| value.as_str()),
        Some("other")
    );

    let prepared_schedule = client
        .call_tool(
            CallToolRequestParams::new("prepare_schedule_message").with_arguments(
                json!({
                    "chat_id": "@alex",
                    "text": "Scheduled via the real stdio child",
                    "source": { "kind": "self_originated" },
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
            .and_then(|value| value.pointer("/data/action/schedule_date_unix"))
            .and_then(|value| value.as_i64()),
        Some(2_000_000_100)
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
            .and_then(|value| value.pointer("/data/poll/poll_id"))
            .and_then(|value| value.as_i64()),
        Some(9_001)
    );

    let prepared_poll = client
        .call_tool(
            CallToolRequestParams::new("prepare_create_poll").with_arguments(
                json!({
                    "chat_id": "-202",
                    "question": "Which stdio path is verified?",
                    "answers": ["Exact MCP child", "Adjacent process"],
                    "quiz": true,
                    "correct_option_index": 0,
                    "solution": "The exact MCP child is the consumer boundary.",
                    "reply_to_message_id": 21,
                    "silent": true
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
            .and_then(|value| value.pointer("/data/action/poll/correct_option_index"))
            .and_then(|value| value.as_u64()),
        Some(0)
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
            .and_then(|value| value.pointer("/data/action/options/0/text"))
            .and_then(|value| value.as_str()),
        Some("Postpone")
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
            .and_then(|value| value.pointer("/data/action/before/question"))
            .and_then(|value| value.as_str()),
        Some("Which release path should we use?")
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
            .and_then(|value| value.pointer("/data/topic/topic_id"))
            .and_then(|value| value.as_i64()),
        Some(44)
    );

    for (tool, arguments) in [
        (
            "prepare_create_forum_topic",
            json!({ "chat_id": "-202", "title": "Stdio topic", "icon_color": 7_322_320 }),
        ),
        (
            "prepare_edit_forum_topic",
            json!({
                "chat_id": "-202",
                "topic_id": 44,
                "title": "Stdio release topic",
                "closed": true
            }),
        ),
        (
            "prepare_set_forum_topic_pinned",
            json!({ "chat_id": "-202", "topic_id": 44, "pinned": false }),
        ),
        ("prepare_delete_forum_topic", json!({ "chat_id": "-202", "topic_id": 44 })),
    ] {
        let prepared = client
            .call_tool(
                CallToolRequestParams::new(tool)
                    .with_arguments(arguments.as_object().expect("object").clone()),
            )
            .await?;
        assert_eq!(prepared.is_error, Some(false), "{tool}");
    }

    let prepared_scheduled_edit = client
        .call_tool(
            CallToolRequestParams::new("prepare_edit_scheduled_message").with_arguments(
                json!({
                    "chat_id": "@alex",
                    "message_id": 301,
                    "text": "Edited via real stdio",
                    "schedule_at_unix": 2_000_000_200
                })
                .as_object()
                .expect("object")
                .clone(),
            ),
        )
        .await?;
    assert_eq!(prepared_scheduled_edit.is_error, Some(false));

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
    }

    let prepared_termination =
        client
            .call_tool(CallToolRequestParams::new("prepare_terminate_session").with_arguments(
                json!({ "authorization_id": 2 }).as_object().expect("object").clone(),
            ))
            .await?;
    assert_eq!(prepared_termination.is_error, Some(false));
    assert_eq!(
        prepared_termination
            .structured_content
            .as_ref()
            .and_then(|value| value.pointer("/data/action/before/current"))
            .and_then(|value| value.as_bool()),
        Some(false)
    );
    let current_termination =
        client
            .call_tool(CallToolRequestParams::new("prepare_terminate_session").with_arguments(
                json!({ "authorization_id": 1 }).as_object().expect("object").clone(),
            ))
            .await?;
    assert_eq!(current_termination.is_error, Some(true));
    assert_eq!(
        current_termination
            .structured_content
            .as_ref()
            .and_then(|value| value.pointer("/error/code"))
            .and_then(|value| value.as_str()),
        Some("CURRENT_SESSION_PROTECTED")
    );

    let contacts = client
        .call_tool(CallToolRequestParams::new("search_contacts").with_arguments(
            json!({ "query": "alex", "limit": 10 }).as_object().expect("object").clone(),
        ))
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

    let attachment = client
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
    assert_eq!(attachment.is_error, Some(false));
    let local_path = attachment
        .structured_content
        .as_ref()
        .and_then(|value| value.pointer("/data/local_path"))
        .and_then(|value| value.as_str())
        .expect("downloaded path");
    let downloaded_sha256 = attachment
        .structured_content
        .as_ref()
        .and_then(|value| value.pointer("/data/sha256"))
        .and_then(|value| value.as_str())
        .expect("downloaded sha256");
    assert!(std::path::Path::new(local_path).is_file());

    let custom_emoji_lookup = client
        .call_tool(CallToolRequestParams::new("get_custom_emoji_documents").with_arguments(
            json!({ "document_ids": [8_001] }).as_object().expect("object").clone(),
        ))
        .await?;
    assert_eq!(custom_emoji_lookup.is_error, Some(false));
    assert_eq!(
        custom_emoji_lookup
            .structured_content
            .as_ref()
            .and_then(|value| value.pointer("/data/documents/0/sticker/custom_emoji"))
            .and_then(|value| value.as_bool()),
        Some(true)
    );
    let custom_emoji_download =
        client
            .call_tool(CallToolRequestParams::new("download_custom_emoji").with_arguments(
                json!({ "document_id": 8_001 }).as_object().expect("object").clone(),
            ))
            .await?;
    assert_eq!(custom_emoji_download.is_error, Some(false));
    let custom_emoji_path = custom_emoji_download
        .structured_content
        .as_ref()
        .and_then(|value| value.pointer("/data/local_path"))
        .and_then(|value| value.as_str())
        .expect("custom emoji download path");
    let custom_emoji_sha256 = custom_emoji_download
        .structured_content
        .as_ref()
        .and_then(|value| value.pointer("/data/sha256"))
        .and_then(|value| value.as_str())
        .expect("custom emoji sha256");
    assert!(std::path::Path::new(custom_emoji_path).is_file());

    let dice_catalog = client.call_tool(CallToolRequestParams::new("get_dice_emojis")).await?;
    assert_eq!(dice_catalog.is_error, Some(false));
    assert_eq!(
        dice_catalog
            .structured_content
            .as_ref()
            .and_then(|value| value.pointer("/data/emojis/0"))
            .and_then(|value| value.as_str()),
        Some("🎲")
    );
    let prepared_location = client
        .call_tool(
            CallToolRequestParams::new("send_location").with_arguments(
                json!({
                    "chat_id": "@alex",
                    "latitude": 55.7558264,
                    "longitude": 37.6172999,
                    "accuracy_radius_m": 25,
                    "caption": "stdio location",
                    "reply_to_message_id": 11,
                    "silent": true,
                    "noforwards": true
                })
                .as_object()
                .expect("object")
                .clone(),
            ),
        )
        .await?;
    assert_eq!(prepared_location.is_error, Some(false));
    assert_eq!(
        prepared_location
            .structured_content
            .as_ref()
            .and_then(|value| value.pointer("/data/action/media/point/latitude_e7"))
            .and_then(|value| value.as_i64()),
        Some(557_558_264)
    );

    let gif_path = std::path::Path::new(local_path)
        .parent()
        .expect("managed attachment directory")
        .join("stdio-animation.gif");
    std::fs::write(&gif_path, ONE_PIXEL_GIF)?;
    let gif_sha256 = format!("{:x}", Sha256::digest(ONE_PIXEL_GIF));
    let prepared_gif = client
        .call_tool(
            CallToolRequestParams::new("send_gif").with_arguments(
                json!({
                    "chat_id": "@alex",
                    "file_path": gif_path,
                    "caption": "stdio GIF",
                    "reply_to_message_id": 11,
                    "approved_sha256": gif_sha256
                })
                .as_object()
                .expect("object")
                .clone(),
            ),
        )
        .await?;
    assert_eq!(prepared_gif.is_error, Some(false));
    assert_eq!(
        prepared_gif
            .structured_content
            .as_ref()
            .and_then(|value| value.pointer("/data/action/mode"))
            .and_then(|value| value.as_str()),
        Some("gif")
    );

    let mut modified_bytes = std::fs::read(local_path)?;
    modified_bytes.extend_from_slice(b"% modified by stdio demo\n");
    let modified_path = std::path::Path::new(local_path)
        .parent()
        .expect("attachment directory")
        .join("contract-updated.pdf");
    std::fs::write(&modified_path, &modified_bytes)?;
    let modified_sha256 = format!("{:x}", Sha256::digest(&modified_bytes));

    let prepared_file = client
        .call_tool(
            CallToolRequestParams::new("prepare_file_message").with_arguments(
                json!({
                    "chat_id": "@alex",
                    "file_path": modified_path.clone(),
                    "caption": "Updated contract",
                    "reply_to_message_id": 11,
                    "mode": "document",
                    "approved_sha256": modified_sha256.clone()
                })
                .as_object()
                .expect("object")
                .clone(),
            ),
        )
        .await?;
    assert_eq!(prepared_file.is_error, Some(false));
    assert_eq!(
        prepared_file
            .structured_content
            .as_ref()
            .and_then(|value| value.pointer("/data/action/artifact/sha256"))
            .and_then(|value| value.as_str()),
        Some(modified_sha256.as_str())
    );

    let sent = client
        .call_tool(
            CallToolRequestParams::new("send_file").with_arguments(
                json!({
                    "chat_id": "@alex",
                    "file_path": modified_path,
                    "caption": "Updated contract",
                    "reply_to_message_id": 11,
                    "mode": "document",
                    "approved_sha256": modified_sha256
                })
                .as_object()
                .expect("object")
                .clone(),
            ),
        )
        .await?;
    assert_eq!(sent.is_error, Some(false));
    assert_eq!(
        sent.structured_content
            .as_ref()
            .and_then(|value| value.pointer("/data/sha256"))
            .and_then(|value| value.as_str()),
        Some(modified_sha256.as_str())
    );
    println!(
        "demo-proof tools={} message_id=11 downloaded_sha256={} custom_emoji_sha256={} file_reply_sha256={}",
        tools.len(),
        downloaded_sha256,
        custom_emoji_sha256,
        modified_sha256
    );

    client.cancel().await?;
    Ok(())
}

#[tokio::test]
async fn binary_does_not_negotiate_unimplemented_2026_07_28_protocol() -> anyhow::Result<()> {
    let binary = env!("CARGO_BIN_EXE_telegram-personal-mcp");
    let mut child = tokio::process::Command::new(binary)
        .arg("--demo")
        .arg("direct")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .env_remove("TELEGRAM_API_ID")
        .env_remove("TELEGRAM_API_HASH")
        .env_remove("TELEGRAM_MCP_SESSION_PATH")
        .env_remove("TELEGRAM_MCP_SESSION_CACHE_DIR")
        .spawn()?;
    let mut stdin = child.stdin.take().expect("child stdin");
    let stdout = child.stdout.take().expect("child stdout");
    let request = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": {
            "protocolVersion": "2026-07-28",
            "capabilities": {},
            "clientInfo": {
                "name": "protocol-regression-test",
                "version": "1.0.0"
            }
        }
    });
    stdin.write_all(format!("{request}\n").as_bytes()).await?;
    stdin.flush().await?;

    let mut response_line = String::new();
    BufReader::new(stdout).read_line(&mut response_line).await?;
    let response: serde_json::Value = serde_json::from_str(&response_line)?;
    let negotiated = response
        .pointer("/result/protocolVersion")
        .and_then(serde_json::Value::as_str)
        .expect("negotiated protocol version");
    assert_eq!(negotiated, "2025-11-25");
    println!("protocol-proof requested=2026-07-28 negotiated={negotiated}");

    child.kill().await?;
    child.wait().await?;
    Ok(())
}
