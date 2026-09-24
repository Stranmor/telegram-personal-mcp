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

macro_rules! prepare_mutation {
    ($client:expr, $tool:expr, $arguments:expr) => {{
        let prepared = $client
            .call_tool(
                CallToolRequestParams::new($tool)
                    .with_arguments($arguments.as_object().expect("object").clone()),
            )
            .await?;
        assert_eq!(prepared.is_error, Some(false), "prepare {}", $tool);
        let data = prepared.structured_content.clone().expect("prepared data");
        (
            data.pointer("/data/mutation_id")
                .and_then(Value::as_str)
                .expect("mutation id")
                .to_string(),
            data.pointer("/data/payload_sha256")
                .and_then(Value::as_str)
                .expect("payload hash")
                .to_string(),
        )
    }};
}

macro_rules! approve_prepared {
    ($client:expr, $bot:expr, $state_db_path:expr, $mutation_id:expr, $payload_hash:expr) => {{
        let requested = $client
            .call_tool(
                CallToolRequestParams::new("request_mutation_approval").with_arguments(
                    json!({
                        "mutation_id": $mutation_id,
                        "approved_payload_sha256": $payload_hash
                    })
                    .as_object()
                    .expect("object")
                    .clone(),
                ),
            )
            .await?;
        assert_eq!(requested.is_error, Some(false));
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

macro_rules! prepare_and_approve {
    ($client:expr, $bot:expr, $state_db_path:expr, $tool:expr, $arguments:expr) => {{
        let (mutation_id, payload_hash) = prepare_mutation!($client, $tool, $arguments);
        approve_prepared!($client, $bot, $state_db_path, mutation_id, payload_hash)
    }};
}

#[tokio::test]
async fn join_request_lifecycle_is_snapshot_bound_and_e2e() -> anyhow::Result<()> {
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

    let links = client
        .call_tool(CallToolRequestParams::new("list_invite_links").with_arguments(
            json!({ "chat_id": "-100505", "revoked": false }).as_object().expect("object").clone(),
        ))
        .await?;
    let invite_handle = links
        .structured_content
        .as_ref()
        .and_then(|value| value.pointer("/data/invite_links/0/invite_handle"))
        .and_then(Value::as_str)
        .expect("active invite handle")
        .to_string();

    let joiners = client
        .call_tool(
            CallToolRequestParams::new("list_invite_joiners").with_arguments(
                json!({
                    "chat_id": "-100505",
                    "invite_handle": invite_handle,
                    "limit": 100
                })
                .as_object()
                .expect("object")
                .clone(),
            ),
        )
        .await?;
    assert_eq!(joiners.is_error, Some(false));
    assert_eq!(
        joiners
            .structured_content
            .as_ref()
            .and_then(|value| value.pointer("/data/page/total_count"))
            .and_then(Value::as_u64),
        Some(1)
    );

    let first_page = client
        .call_tool(CallToolRequestParams::new("list_join_requests").with_arguments(
            json!({ "chat_id": "-100505", "limit": 1 }).as_object().expect("object").clone(),
        ))
        .await?;
    assert_eq!(first_page.is_error, Some(false));
    let first_data = first_page.structured_content.clone().expect("first request page");
    assert_eq!(first_data.pointer("/data/page/completeness").and_then(Value::as_str), Some("page"));
    assert_eq!(
        first_data.pointer("/data/page/entries/0/user_id").and_then(Value::as_i64),
        Some(404)
    );
    assert!(
        first_data.pointer("/data/page/entries/0/source_invite_handle").is_none_or(Value::is_null)
    );
    let offset_date = first_data
        .pointer("/data/page/next_offset_date_unix")
        .and_then(Value::as_i64)
        .expect("next date");
    let offset_user = first_data
        .pointer("/data/page/next_offset_user_id")
        .and_then(Value::as_i64)
        .expect("next user");
    let second_page = client
        .call_tool(
            CallToolRequestParams::new("list_join_requests").with_arguments(
                json!({
                    "chat_id": "-100505",
                    "limit": 1,
                    "offset_date_unix": offset_date,
                    "offset_user_id": offset_user
                })
                .as_object()
                .expect("object")
                .clone(),
            ),
        )
        .await?;
    assert_eq!(
        second_page
            .structured_content
            .as_ref()
            .and_then(|value| value.pointer("/data/page/entries/0/user_id"))
            .and_then(Value::as_i64),
        Some(606)
    );

    let filtered = client
        .call_tool(
            CallToolRequestParams::new("list_join_requests").with_arguments(
                json!({
                    "chat_id": "-100505",
                    "invite_handle": invite_handle,
                    "limit": 100
                })
                .as_object()
                .expect("object")
                .clone(),
            ),
        )
        .await?;
    assert_eq!(
        filtered
            .structured_content
            .as_ref()
            .and_then(|value| value.pointer("/data/page/entries/0/user_id"))
            .and_then(Value::as_i64),
        Some(404)
    );
    assert_eq!(
        filtered
            .structured_content
            .as_ref()
            .and_then(|value| value.pointer("/data/page/entries/0/source_invite_handle"))
            .and_then(Value::as_str),
        Some(invite_handle.as_str())
    );

    let (approved, approval_id, approval_message_id) = prepare_and_approve!(
        client,
        bot,
        state_db_path,
        "approve_join_request",
        json!({ "chat_id": "-100505", "user_ref": "@dana_candidate" })
    );
    assert_eq!(approved.pointer("/kind").and_then(Value::as_str), Some("join_request_processed"));
    assert_eq!(approved.pointer("/user_id").and_then(Value::as_i64), Some(404));
    assert_eq!(approved.pointer("/approved").and_then(Value::as_bool), Some(true));
    assert_eq!(backend.join_request_call_count().await, 1);
    bot.push_mutation_decision(
        &approval_id,
        ApprovalAction::Approve,
        700,
        700,
        approval_message_id,
    )
    .await;
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert_eq!(backend.join_request_call_count().await, 1);

    let (hide_all_mutation_id, hide_all_payload_hash) =
        prepare_mutation!(client, "hide_all_join_requests", json!({ "chat_id": "-100505" }));
    backend.add_fixture_join_request(505, 707, 1_700_000_090).await;
    let (hidden_snapshot, _, _) =
        approve_prepared!(client, bot, state_db_path, hide_all_mutation_id, hide_all_payload_hash);
    assert_eq!(hidden_snapshot.pointer("/user_ids/0").and_then(Value::as_i64), Some(606));
    assert_eq!(
        hidden_snapshot.pointer("/user_ids").and_then(Value::as_array).map(Vec::len),
        Some(1)
    );
    assert_eq!(backend.join_request_call_count().await, 2);

    let remaining = client
        .call_tool(CallToolRequestParams::new("list_join_requests").with_arguments(
            json!({ "chat_id": "-100505", "limit": 100 }).as_object().expect("object").clone(),
        ))
        .await?;
    assert_eq!(
        remaining
            .structured_content
            .as_ref()
            .and_then(|value| value.pointer("/data/page/entries/0/user_id"))
            .and_then(Value::as_i64),
        Some(707)
    );

    let (hidden_single, _, _) = prepare_and_approve!(
        client,
        bot,
        state_db_path,
        "hide_join_request",
        json!({ "chat_id": "-100505", "user_ref": "707" })
    );
    assert_eq!(hidden_single.pointer("/approved").and_then(Value::as_bool), Some(false));
    assert_eq!(backend.join_request_call_count().await, 3);

    backend.add_fixture_join_request(505, 808, 1_700_000_100).await;
    backend.add_fixture_join_request(505, 809, 1_700_000_101).await;
    let (approved_all, _, _) = prepare_and_approve!(
        client,
        bot,
        state_db_path,
        "approve_all_join_requests",
        json!({ "chat_id": "-100505" })
    );
    assert_eq!(approved_all.pointer("/approved").and_then(Value::as_bool), Some(true));
    assert_eq!(approved_all.pointer("/user_ids").and_then(Value::as_array).map(Vec::len), Some(2));
    assert_eq!(backend.join_request_call_count().await, 5);

    let empty = client
        .call_tool(CallToolRequestParams::new("list_join_requests").with_arguments(
            json!({ "chat_id": "-100505", "limit": 100 }).as_object().expect("object").clone(),
        ))
        .await?;
    assert_eq!(
        empty
            .structured_content
            .as_ref()
            .and_then(|value| value.pointer("/data/page/total_count"))
            .and_then(Value::as_u64),
        Some(0)
    );

    tokio::time::sleep(Duration::from_millis(50)).await;
    let approval_surfaces =
        format!("{:?}\n{:?}", bot.sent_mutation_cards().await, bot.mutation_edits().await);
    for untrusted_or_secret in [
        "I work with the release team",
        "arrived after mutation preparation",
        "fixture-active-invite-token",
        "https://t.me/+",
    ] {
        assert!(
            !approval_surfaces.contains(untrusted_or_secret),
            "approval surface leaked {untrusted_or_secret}"
        );
    }

    client.cancel().await?;
    server_task.await??;
    worker.abort();
    let _ = worker.await;
    Ok(())
}
