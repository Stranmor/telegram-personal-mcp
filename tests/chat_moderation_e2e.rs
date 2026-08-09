use rmcp::{ServiceExt, model::CallToolRequestParams};
use serde_json::{Value, json};
use std::sync::Arc;
use std::time::Duration;
use telegram_personal_mcp::{
    ApprovalAction, ApprovalPolicy, FakeApprovalBot, FakeTelegramBackend, McpServer,
    McpServerSettings, MutationApprovalState, MutationLedger,
};

async fn wait_for_mutation_state(
    state_db_path: &std::path::Path,
    approval_id: &str,
    wanted: MutationApprovalState,
) -> anyhow::Result<()> {
    let mut last = None;
    for _ in 0..1_200 {
        let approval =
            MutationLedger::new(state_db_path.to_path_buf()).get_approval(approval_id)?;
        let state = approval.as_ref().map(|approval| approval.state);
        if state == Some(wanted) {
            return Ok(());
        }
        last = approval.map(|approval| (approval.state, approval.last_error_code));
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    anyhow::bail!("mutation approval did not reach {wanted:?}; last={last:?}")
}

macro_rules! approve_mutation {
    ($client:expr, $bot:expr, $state_db_path:expr, $tool:expr, $arguments:expr) => {{
        let prepared = $client
            .call_tool(
                CallToolRequestParams::new($tool)
                    .with_arguments($arguments.as_object().expect("object").clone()),
            )
            .await?;
        assert_eq!(prepared.is_error, Some(false), "prepare {}", $tool);
        let data = prepared.structured_content.clone().expect("prepared data");
        let mutation_id = data
            .pointer("/data/mutation_id")
            .and_then(Value::as_str)
            .expect("mutation id")
            .to_string();
        let payload_sha256 = data
            .pointer("/data/payload_sha256")
            .and_then(Value::as_str)
            .expect("payload hash")
            .to_string();
        let requested = $client
            .call_tool(
                CallToolRequestParams::new("request_mutation_approval").with_arguments(
                    json!({
                        "mutation_id": mutation_id,
                        "approved_payload_sha256": payload_sha256
                    })
                    .as_object()
                    .expect("object")
                    .clone(),
                ),
            )
            .await?;
        assert_eq!(requested.is_error, Some(false), "request {}", $tool);
        let requested_data = requested.structured_content.clone().expect("approval data");
        let approval_id = requested_data
            .pointer("/data/approval_id")
            .and_then(Value::as_str)
            .expect("approval id")
            .to_string();
        let message_id = requested_data
            .pointer("/data/approval_message_id")
            .and_then(Value::as_i64)
            .expect("approval message id");
        $bot
            .push_mutation_decision(
                &approval_id,
                ApprovalAction::Approve,
                700,
                700,
                message_id,
            )
            .await;
        wait_for_mutation_state(
            &$state_db_path,
            &approval_id,
            MutationApprovalState::Applied,
        )
        .await?;
        let approval = MutationLedger::new($state_db_path.to_path_buf())
            .get_approval(&approval_id)?
            .expect("applied mutation");
        let result = approval
            .result
            .map(|result| serde_json::to_value(result).expect("result json"))
            .expect("mutation result");
        (result, approval.automated_retry_count)
    }};
}

#[tokio::test]
async fn chat_moderation_policies_are_exact_approved_and_recover_without_replay()
-> anyhow::Result<()> {
    let backend = Arc::new(FakeTelegramBackend::fixture());
    let bot = Arc::new(FakeApprovalBot::default());
    let temp = tempfile::tempdir()?;
    let state_db_path = temp.path().join("state.sqlite");
    let server = McpServer::with_approval(
        backend.clone(),
        McpServerSettings {
            attachment_cache_dir: temp.path().join("attachments"),
            max_download_bytes: 1024 * 1024,
            max_upload_bytes: 1024 * 1024,
            state_db_path: state_db_path.clone(),
        },
        ApprovalPolicy {
            approval_chat_id: 700,
            approval_user_id: 700,
            ttl_seconds: 3_600,
            poll_timeout_seconds: 1,
        },
        bot.clone(),
    )
    .map_err(anyhow::Error::msg)?;
    let worker = server.start_approval_worker()?.expect("approval worker");
    let (server_transport, client_transport) = tokio::io::duplex(64 * 1024);
    let server_task = tokio::spawn(async move {
        server.serve(server_transport).await?.waiting().await?;
        anyhow::Ok(())
    });
    let client = ().serve(client_transport).await?;

    for (chat_id, anti_spam, forum) in [("-202", None, None), ("-100505", Some(false), Some(true))]
    {
        let readback = client
            .call_tool(
                CallToolRequestParams::new("get_chat_moderation_settings").with_arguments(
                    json!({ "chatId": chat_id }).as_object().expect("object").clone(),
                ),
            )
            .await?;
        assert_eq!(readback.is_error, Some(false));
        let data = readback.structured_content.expect("moderation readback");
        assert_eq!(
            data.pointer("/data/details/chat/chat_id").and_then(Value::as_str),
            Some(chat_id)
        );
        assert_eq!(
            data.pointer("/data/available_reactions/kind").and_then(Value::as_str),
            Some("all")
        );
        assert_eq!(data.pointer("/data/anti_spam_enabled").and_then(Value::as_bool), anti_spam);
        assert_eq!(data.pointer("/data/forum_enabled").and_then(Value::as_bool), forum);
    }

    let empty_patch = client
        .call_tool(
            CallToolRequestParams::new("set_chat_permissions")
                .with_arguments(json!({ "chatId": "-202" }).as_object().expect("object").clone()),
        )
        .await?;
    assert_eq!(empty_patch.is_error, Some(true));

    let (permissions, retry_count) = approve_mutation!(
        client,
        bot,
        state_db_path,
        "set_chat_permissions",
        json!({
            "chatId": "-202",
            "sendMessages": false,
            "sendMedia": false,
            "inviteUsers": true
        })
    );
    assert_eq!(retry_count, 0);
    assert_eq!(
        permissions.pointer("/kind").and_then(Value::as_str),
        Some("chat_moderation_settings_updated")
    );
    assert_eq!(
        permissions
            .pointer("/settings/default_banned_rights/send_messages")
            .and_then(Value::as_bool),
        Some(true)
    );
    assert_eq!(
        permissions.pointer("/settings/default_banned_rights/send_media").and_then(Value::as_bool),
        Some(true)
    );
    assert_eq!(
        permissions
            .pointer("/settings/default_banned_rights/invite_users")
            .and_then(Value::as_bool),
        Some(false)
    );

    let duplicate_reactions = client
        .call_tool(
            CallToolRequestParams::new("set_chat_reactions").with_arguments(
                json!({
                    "chatId": "-202",
                    "reactions": {
                        "kind": "some",
                        "reactions": [
                            { "kind": "emoji", "emoji": "👍" },
                            { "kind": "emoji", "emoji": "👍" }
                        ]
                    }
                })
                .as_object()
                .expect("object")
                .clone(),
            ),
        )
        .await?;
    assert_eq!(duplicate_reactions.is_error, Some(true));

    let (reactions, _) = approve_mutation!(
        client,
        bot,
        state_db_path,
        "set_chat_reactions",
        json!({
            "chatId": "-202",
            "reactions": {
                "kind": "some",
                "reactions": [
                    { "kind": "emoji", "emoji": "👍" },
                    { "kind": "custom_emoji", "document_id": 7001 }
                ]
            }
        })
    );
    assert_eq!(
        reactions.pointer("/settings/available_reactions/kind").and_then(Value::as_str),
        Some("some")
    );
    assert_eq!(
        reactions
            .pointer("/settings/available_reactions/reactions/1/document_id")
            .and_then(Value::as_i64),
        Some(7001)
    );

    let invalid_anti_spam = client
        .call_tool(CallToolRequestParams::new("toggle_anti_spam").with_arguments(
            json!({ "chatId": "-202", "enabled": true }).as_object().expect("object").clone(),
        ))
        .await?;
    assert_eq!(invalid_anti_spam.is_error, Some(true));

    backend.make_next_chat_anti_spam_outcome_uncertain().await;
    let (anti_spam, anti_spam_retries) = approve_mutation!(
        client,
        bot,
        state_db_path,
        "toggle_anti_spam",
        json!({ "chatId": "-100505", "enabled": true })
    );
    assert_eq!(anti_spam_retries, 1);
    assert_eq!(
        anti_spam.pointer("/settings/anti_spam_enabled").and_then(Value::as_bool),
        Some(true)
    );

    let missing_forum_confirmation = client
        .call_tool(CallToolRequestParams::new("toggle_forum_mode").with_arguments(
            json!({ "chatId": "-100505", "enabled": false }).as_object().expect("object").clone(),
        ))
        .await?;
    assert_eq!(missing_forum_confirmation.is_error, Some(true));

    backend.make_next_chat_forum_outcome_uncertain().await;
    let (forum, forum_retries) = approve_mutation!(
        client,
        bot,
        state_db_path,
        "toggle_forum_mode",
        json!({
            "chatId": "-100505",
            "enabled": false,
            "confirm": true,
            "tabs": true
        })
    );
    assert_eq!(forum_retries, 1);
    assert_eq!(forum.pointer("/settings/forum_enabled").and_then(Value::as_bool), Some(false));
    assert_eq!(forum.pointer("/settings/forum_tabs_enabled").and_then(Value::as_bool), Some(false));

    assert_eq!(backend.chat_moderation_call_counts().await, (1, 1, 1, 1));
    let approval_surface =
        format!("{:?}\n{:?}", bot.sent_mutation_cards().await, bot.mutation_edits().await);
    assert!(approval_surface.contains("DEFAULT PERMISSIONS"));
    assert!(approval_surface.contains("NATIVE ANTI-SPAM"));
    assert!(approval_surface.contains("Отключение удаляет все существующие темы"));
    assert!(!approval_surface.contains("access_hash"));
    assert!(!approval_surface.contains("file_reference"));

    client.cancel().await?;
    server_task.await??;
    worker.abort();
    let _ = worker.await;
    Ok(())
}
