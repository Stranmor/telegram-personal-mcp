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
use std::sync::Arc;
use std::time::Duration;
use telegram_personal_mcp::{
    ApprovalAction, ApprovalPolicy, DraftSummary, FakeApprovalBot, FakeTelegramBackend, McpServer,
    McpServerSettings, MutationApprovalState, MutationLedger, TelegramBackend,
};

async fn wait_for_mutation_state(
    state_db_path: &std::path::Path,
    approval_id: &str,
    wanted: MutationApprovalState,
) -> anyhow::Result<()> {
    let mut last = None;
    for _ in 0..1_500 {
        let approval =
            MutationLedger::new(state_db_path.to_path_buf()).get_approval(approval_id)?;
        let state = approval.as_ref().map(|approval| approval.state);
        if state == Some(wanted) {
            return Ok(());
        }
        last = approval.map(|approval| {
            (
                approval.state,
                approval.last_error_code,
                approval.automated_retry_count,
                approval.next_retry_at_unix,
            )
        });
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    anyhow::bail!("mutation approval did not reach {wanted:?}; last={last:?}")
}

async fn wait_for_error_code(
    state_db_path: &std::path::Path,
    approval_id: &str,
    wanted: &str,
) -> anyhow::Result<()> {
    let mut last = None;
    for _ in 0..1_500 {
        let approval =
            MutationLedger::new(state_db_path.to_path_buf()).get_approval(approval_id)?;
        let code = approval.as_ref().and_then(|approval| approval.last_error_code.as_deref());
        if code == Some(wanted) {
            return Ok(());
        }
        last = approval.map(|approval| {
            (
                approval.state,
                approval.last_error_code,
                approval.automated_retry_count,
                approval.next_retry_at_unix,
            )
        });
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    anyhow::bail!("mutation approval did not reach error {wanted}; last={last:?}")
}

async fn rejected_code(
    client: &rmcp::service::RunningService<rmcp::RoleClient, ()>,
) -> anyhow::Result<String> {
    let rejected = client
        .call_tool(
            CallToolRequestParams::new("clear_all_drafts")
                .with_arguments(json!({}).as_object().expect("object").clone()),
        )
        .await?;
    assert_eq!(rejected.is_error, Some(true));
    Ok(rejected
        .structured_content
        .as_ref()
        .and_then(|value| value.pointer("/error/code"))
        .and_then(Value::as_str)
        .expect("typed error code")
        .to_string())
}

async fn prepare_and_request(
    client: &rmcp::service::RunningService<rmcp::RoleClient, ()>,
) -> anyhow::Result<(String, i64, Value)> {
    let prepared = client
        .call_tool(
            CallToolRequestParams::new("clear_all_drafts")
                .with_arguments(json!({}).as_object().expect("object").clone()),
        )
        .await?;
    assert_eq!(prepared.is_error, Some(false));
    let prepared = prepared.structured_content.expect("prepared all-drafts clear");
    let mutation_id = prepared
        .pointer("/data/mutation_id")
        .and_then(Value::as_str)
        .expect("mutation id")
        .to_string();
    let payload_sha256 = prepared
        .pointer("/data/payload_sha256")
        .and_then(Value::as_str)
        .expect("payload hash")
        .to_string();
    let requested = client
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
    assert_eq!(requested.is_error, Some(false));
    let approval = requested.structured_content.expect("all-drafts approval");
    let approval_id = approval
        .pointer("/data/approval_id")
        .and_then(Value::as_str)
        .expect("approval id")
        .to_string();
    let approval_message_id = approval
        .pointer("/data/approval_message_id")
        .and_then(Value::as_i64)
        .expect("approval message id");
    Ok((approval_id, approval_message_id, prepared))
}

async fn approve(bot: &FakeApprovalBot, approval_id: &str, approval_message_id: i64) {
    bot.push_mutation_decision(approval_id, ApprovalAction::Approve, 700, 700, approval_message_id)
        .await;
}

fn rich_draft(message: &str, hash_byte: char) -> DraftSummary {
    DraftSummary {
        message: message.to_string(),
        date_unix: 1_700_009_500,
        no_webpage: false,
        invert_media: true,
        has_reply_to: true,
        has_entities: true,
        has_media: true,
        effect_id: Some(77),
        has_suggested_post: true,
        has_rich_message: true,
        state_sha256: hash_byte.to_string().repeat(64),
    }
}

#[tokio::test]
async fn clear_all_drafts_binds_complete_state_and_never_erases_a_later_draft() -> anyhow::Result<()>
{
    let backend = Arc::new(FakeTelegramBackend::fixture());
    backend.save_draft(None, "-202", Some("Group release draft")).await?;
    backend.set_fixture_draft_summary("-100505", Some(rich_draft("Rich channel draft", 'b'))).await;
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

    let closed_schema = client
        .call_tool(CallToolRequestParams::new("clear_all_drafts").with_arguments(
            json!({ "confirmAllChats": true }).as_object().expect("object").clone(),
        ))
        .await;
    assert!(closed_schema.is_err(), "unknown fields must fail at the JSON-RPC boundary");

    let listed = client
        .call_tool(
            CallToolRequestParams::new("list_drafts")
                .with_arguments(json!({ "limit": 10 }).as_object().expect("object").clone()),
        )
        .await?
        .structured_content
        .expect("draft list");
    assert_eq!(listed.pointer("/data/returned_count").and_then(Value::as_u64), Some(3));
    assert!(
        listed.pointer("/data/drafts").and_then(Value::as_array).expect("draft array").iter().all(
            |draft| {
                draft.pointer("/draft/state_sha256").and_then(Value::as_str).is_some_and(|hash| {
                    hash.len() == 64 && hash.bytes().all(|byte| byte.is_ascii_hexdigit())
                })
            }
        )
    );

    backend.make_next_draft_clear_all_outcome_uncertain().await;
    let (approval_id, approval_message_id, prepared) = prepare_and_request(&client).await?;
    assert_eq!(
        prepared.pointer("/data/action/kind").and_then(Value::as_str),
        Some("clear_all_drafts")
    );
    let before = prepared
        .pointer("/data/action/before")
        .and_then(Value::as_array)
        .expect("complete draft snapshot");
    assert_eq!(before.len(), 3);
    assert_eq!(before[0].pointer("/chat_id").and_then(Value::as_str), Some("-100505"));
    assert_eq!(before[1].pointer("/chat_id").and_then(Value::as_str), Some("-202"));
    assert_eq!(before[2].pointer("/chat_id").and_then(Value::as_str), Some("101"));
    assert_eq!(
        before[0].pointer("/draft/state_sha256").and_then(Value::as_str),
        Some("bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb")
    );
    let card_debug = format!("{:?}", bot.sent_mutation_cards().await.last());
    assert!(card_debug.contains("Rich channel draft"));
    assert!(card_debug.contains("bbbbbbbbbbbbbbbb"));
    approve(&bot, &approval_id, approval_message_id).await;
    wait_for_mutation_state(&state_db_path, &approval_id, MutationApprovalState::Applied).await?;
    let applied = MutationLedger::new(state_db_path.clone())
        .get_approval(&approval_id)?
        .expect("applied all-drafts clear");
    assert_eq!(applied.automated_retry_count, 1);
    assert_eq!(backend.draft_clear_all_call_count().await, 1);
    let result = serde_json::to_value(applied.result.expect("all-drafts receipt"))?;
    assert_eq!(result.pointer("/kind").and_then(Value::as_str), Some("all_drafts_cleared"));
    assert_eq!(
        result.pointer("/cleared_chat_ids").and_then(Value::as_array).map(Vec::len),
        Some(3)
    );
    assert_eq!(result.pointer("/remaining").and_then(Value::as_array).map(Vec::len), Some(0));
    assert!(backend.list_drafts(None, 10).await?.is_empty());
    assert_eq!(rejected_code(&client).await?, "NO_CHANGE");

    backend.save_draft(None, "101", Some("First restored draft")).await?;
    backend.save_draft(None, "-202", Some("Second restored draft")).await?;
    let (stale_id, stale_message_id, _) = prepare_and_request(&client).await?;
    backend.save_draft(None, "-202", Some("Changed after approval")).await?;
    approve(&bot, &stale_id, stale_message_id).await;
    wait_for_mutation_state(&state_db_path, &stale_id, MutationApprovalState::Failed).await?;
    let stale = MutationLedger::new(state_db_path.clone())
        .get_approval(&stale_id)?
        .expect("stale all-drafts approval");
    assert_eq!(stale.last_error_code.as_deref(), Some("MUTATION_PRECONDITION_FAILED"));
    assert_eq!(backend.draft_clear_all_call_count().await, 1);
    assert_eq!(backend.list_drafts(None, 10).await?.len(), 2);

    backend.make_next_draft_clear_all_outcome_uncertain().await;
    let (changed_id, changed_message_id, _) = prepare_and_request(&client).await?;
    approve(&bot, &changed_id, changed_message_id).await;
    wait_for_mutation_state(&state_db_path, &changed_id, MutationApprovalState::OutcomeUnknown)
        .await?;
    assert_eq!(backend.draft_clear_all_call_count().await, 2);
    backend.save_draft(None, "-100505", Some("Created after uncertain clear")).await?;
    wait_for_error_code(&state_db_path, &changed_id, "MUTATION_OUTCOME_UNKNOWN_NO_RETRY").await?;
    let changed = MutationLedger::new(state_db_path.clone())
        .get_approval(&changed_id)?
        .expect("changed recovery approval");
    assert_eq!(changed.state, MutationApprovalState::OutcomeUnknown);
    assert_eq!(changed.automated_retry_count, 2);
    assert_eq!(changed.next_retry_at_unix, None);
    assert_eq!(backend.draft_clear_all_call_count().await, 2);
    let remaining = backend.list_drafts(None, 10).await?;
    assert_eq!(remaining.len(), 1);
    assert_eq!(remaining[0].chat_id, "-100505");
    assert_eq!(remaining[0].draft.message, "Created after uncertain clear");

    client.cancel().await?;
    server_task.await??;
    worker.abort();
    Ok(())
}
