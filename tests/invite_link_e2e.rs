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
    for _ in 0..300 {
        let state = MutationLedger::new(state_db_path.to_path_buf())
            .get_approval(approval_id)?
            .map(|approval| approval.state);
        if state == Some(wanted) {
            return Ok(());
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    anyhow::bail!("mutation approval did not reach {wanted:?}")
}

fn assert_opaque_invite_handle(handle: &str) {
    let suffix = handle.strip_prefix("invite_").expect("opaque invite prefix");
    assert_eq!(suffix.len(), 32);
    assert!(
        suffix.as_bytes().iter().all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(byte))
    );
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
                        "purpose": "тест: подготовка мутации к подтверждению",
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
        let result = MutationLedger::new($state_db_path.to_path_buf())
            .get_approval(&approval_id)?
            .expect("applied mutation")
            .result
            .map(|result| serde_json::to_value(result).expect("result json"))
            .expect("mutation result");
        (result, approval_id, message_id)
    }};
}

#[tokio::test]
async fn invite_link_lifecycle_is_e2e_idempotent_and_secret_safe() -> anyhow::Result<()> {
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
            owner_grant_file: None,
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

    let active = client
        .call_tool(
            CallToolRequestParams::new("list_invite_links").with_arguments(
                json!({ "chat_id": "-100505", "revoked": false, "limit": 100 })
                    .as_object()
                    .expect("object")
                    .clone(),
            ),
        )
        .await?;
    assert_eq!(active.is_error, Some(false));
    let active_json = active.structured_content.clone().expect("active invite data");
    assert_eq!(active_json.pointer("/data/admin_user_id").and_then(Value::as_i64), Some(7));
    let active_handle = active_json
        .pointer("/data/invite_links/0/invite_handle")
        .and_then(Value::as_str)
        .expect("active opaque handle")
        .to_string();
    assert_opaque_invite_handle(&active_handle);
    let active_serialized = serde_json::to_string(&active_json)?;
    assert!(!active_serialized.contains("fixture-active-invite-token"));
    assert!(!active_serialized.contains("fixture-revoked-invite-token"));
    assert!(!active_serialized.contains("https://t.me/+"));

    let moderator_links = client
        .call_tool(
            CallToolRequestParams::new("list_invite_links").with_arguments(
                json!({
                    "chat_id": "-100505",
                    "adminId": "@mira_ops",
                    "revoked": false,
                    "limit": 100
                })
                .as_object()
                .expect("object")
                .clone(),
            ),
        )
        .await?;
    assert_eq!(moderator_links.is_error, Some(false));
    assert_eq!(
        moderator_links
            .structured_content
            .as_ref()
            .and_then(|value| value.pointer("/data/admin_user_id"))
            .and_then(Value::as_i64),
        Some(303)
    );
    let moderator_handle = moderator_links
        .structured_content
        .as_ref()
        .and_then(|value| value.pointer("/data/invite_links/0/invite_handle"))
        .and_then(Value::as_str)
        .expect("moderator opaque handle")
        .to_string();
    assert_opaque_invite_handle(&moderator_handle);
    let moderator_readback = client
        .call_tool(
            CallToolRequestParams::new("get_invite_link").with_arguments(
                json!({
                    "chat_id": "-100505",
                    "adminId": "303",
                    "invite_handle": moderator_handle
                })
                .as_object()
                .expect("object")
                .clone(),
            ),
        )
        .await?;
    assert_eq!(moderator_readback.is_error, Some(false));
    assert_eq!(
        moderator_readback
            .structured_content
            .as_ref()
            .and_then(|value| value.pointer("/data/admin_user_id"))
            .and_then(Value::as_i64),
        Some(303)
    );
    assert_eq!(
        moderator_readback
            .structured_content
            .as_ref()
            .and_then(|value| value.pointer("/data/invite/title"))
            .and_then(Value::as_str),
        Some("Moderator access")
    );
    let wrong_admin_readback = client
        .call_tool(
            CallToolRequestParams::new("get_invite_link").with_arguments(
                json!({
                    "chat_id": "-100505",
                    "invite_handle": moderator_handle
                })
                .as_object()
                .expect("object")
                .clone(),
            ),
        )
        .await?;
    assert_eq!(wrong_admin_readback.is_error, Some(false));
    assert!(
        wrong_admin_readback
            .structured_content
            .as_ref()
            .and_then(|value| value.pointer("/data/invite"))
            .is_none_or(Value::is_null)
    );
    assert_eq!(
        moderator_links
            .structured_content
            .as_ref()
            .and_then(|value| value.pointer("/data/returned_count"))
            .and_then(Value::as_u64),
        Some(1)
    );
    assert_eq!(
        moderator_links
            .structured_content
            .as_ref()
            .and_then(|value| value.pointer("/data/invite_links/0/title"))
            .and_then(Value::as_str),
        Some("Moderator access")
    );
    assert_eq!(
        moderator_links
            .structured_content
            .as_ref()
            .and_then(|value| value.pointer("/data/invite_links/0/admin_user_id"))
            .and_then(Value::as_i64),
        Some(303)
    );

    let raw_handle_rejected = client
        .call_tool(
            CallToolRequestParams::new("get_invite_link").with_arguments(
                json!({
                    "chat_id": "-100505",
                    "invite_handle": "https://t.me/+fixture-active-invite-token"
                })
                .as_object()
                .expect("object")
                .clone(),
            ),
        )
        .await?;
    assert_eq!(raw_handle_rejected.is_error, Some(true));
    assert_eq!(
        raw_handle_rejected
            .structured_content
            .as_ref()
            .and_then(|value| value.pointer("/error/code"))
            .and_then(Value::as_str),
        Some("INVALID_ARGUMENT")
    );

    let active_delete = client
        .call_tool(
            CallToolRequestParams::new("delete_revoked_invite_link").with_arguments(
                json!({ "chat_id": "-100505", "invite_handle": active_handle })
                    .as_object()
                    .expect("object")
                    .clone(),
            ),
        )
        .await?;
    assert_eq!(active_delete.is_error, Some(true));
    assert_eq!(
        active_delete
            .structured_content
            .as_ref()
            .and_then(|value| value.pointer("/error/code"))
            .and_then(Value::as_str),
        Some("INVITE_LINK_ACTIVE")
    );

    let (exported, export_approval_id, export_message_id) = approve_mutation!(
        client,
        bot,
        state_db_path,
        "export_invite_link",
        json!({
            "chat_id": "-100505",
            "expire_date_unix": 1_900_000_000,
            "usage_limit": 25,
            "request_needed": false,
            "title": "  Partner access  "
        })
    );
    assert_eq!(exported.pointer("/kind").and_then(Value::as_str), Some("invite_link_exported"));
    let exported_handle = exported
        .pointer("/invite/invite_handle")
        .and_then(Value::as_str)
        .expect("exported opaque handle")
        .to_string();
    assert_opaque_invite_handle(&exported_handle);
    assert_eq!(exported.pointer("/invite/title").and_then(Value::as_str), Some("Partner access"));
    assert_eq!(exported.pointer("/invite/usage_limit").and_then(Value::as_i64), Some(25));

    let exported_readback = client
        .call_tool(
            CallToolRequestParams::new("get_invite_link").with_arguments(
                json!({ "chat_id": "-100505", "invite_handle": exported_handle })
                    .as_object()
                    .expect("object")
                    .clone(),
            ),
        )
        .await?;
    assert_eq!(exported_readback.is_error, Some(false));
    assert_eq!(
        exported_readback
            .structured_content
            .as_ref()
            .and_then(|value| value.pointer("/data/invite/title"))
            .and_then(Value::as_str),
        Some("Partner access")
    );

    let export_count = backend.invite_link_call_counts().await.0;
    bot.push_mutation_decision(
        &export_approval_id,
        ApprovalAction::Approve,
        700,
        700,
        export_message_id,
    )
    .await;
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert_eq!(backend.invite_link_call_counts().await.0, export_count);

    let (revoked, _, _) = approve_mutation!(
        client,
        bot,
        state_db_path,
        "revoke_invite_link",
        json!({ "chat_id": "-100505", "invite_handle": active_handle })
    );
    assert_eq!(revoked.pointer("/kind").and_then(Value::as_str), Some("invite_link_revoked"));
    assert_eq!(revoked.pointer("/invite/revoked").and_then(Value::as_bool), Some(true));

    let (deleted, _, _) = approve_mutation!(
        client,
        bot,
        state_db_path,
        "delete_revoked_invite_link",
        json!({ "chat_id": "-100505", "invite_handle": active_handle })
    );
    assert_eq!(
        deleted.pointer("/kind").and_then(Value::as_str),
        Some("revoked_invite_link_deleted")
    );
    assert_eq!(
        deleted.pointer("/invite_handle").and_then(Value::as_str),
        Some(active_handle.as_str())
    );

    let deleted_readback = client
        .call_tool(
            CallToolRequestParams::new("get_invite_link").with_arguments(
                json!({ "chat_id": "-100505", "invite_handle": active_handle })
                    .as_object()
                    .expect("object")
                    .clone(),
            ),
        )
        .await?;
    assert_eq!(deleted_readback.is_error, Some(false));
    assert!(
        deleted_readback
            .structured_content
            .as_ref()
            .and_then(|value| value.pointer("/data/invite"))
            .is_none_or(Value::is_null)
    );

    let (moderator_revoked, _, _) = approve_mutation!(
        client,
        bot,
        state_db_path,
        "revoke_invite_link",
        json!({
            "chat_id": "-100505",
            "adminId": "303",
            "invite_handle": moderator_handle
        })
    );
    assert_eq!(
        moderator_revoked.pointer("/invite/admin_user_id").and_then(Value::as_i64),
        Some(303)
    );
    assert_eq!(moderator_revoked.pointer("/invite/revoked").and_then(Value::as_bool), Some(true));
    let (moderator_deleted, _, _) = approve_mutation!(
        client,
        bot,
        state_db_path,
        "delete_revoked_invite_link",
        json!({
            "chat_id": "-100505",
            "adminId": "303",
            "invite_handle": moderator_handle
        })
    );
    assert_eq!(
        moderator_deleted.pointer("/invite_handle").and_then(Value::as_str),
        Some(moderator_handle.as_str())
    );
    assert_eq!(backend.invite_link_call_counts().await, (1, 2, 2));

    tokio::time::sleep(Duration::from_millis(50)).await;
    let approval_surfaces =
        format!("{:?}\n{:?}", bot.sent_mutation_cards().await, bot.mutation_edits().await);
    for raw_secret in [
        "fixture-active-invite-token",
        "fixture-revoked-invite-token",
        "fixture-generated-invite-505-1",
        "https://t.me/+",
    ] {
        assert!(!approval_surfaces.contains(raw_secret), "approval surface leaked {raw_secret}");
    }

    client.cancel().await?;
    server_task.await??;
    worker.abort();
    let _ = worker.await;
    Ok(())
}
