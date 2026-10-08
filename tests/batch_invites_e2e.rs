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
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;
use telegram_personal_mcp::{
    ApprovalAction, ApprovalPolicy, ContactSummary, FakeApprovalBot, FakeInviteOutcome,
    FakeTelegramBackend, InviteConfirmation, InviteTargetState, McpServer, McpServerSettings,
    MutationApproval, MutationApprovalState, MutationLedger,
};

fn fixture_contact(user_id: i64) -> ContactSummary {
    ContactSummary {
        user_id,
        first_name: format!("Batch{user_id}"),
        last_name: Some("Invite".to_string()),
        display_name: format!("Batch {user_id} Invite"),
        username: Some(format!("batch_invite_{user_id}")),
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

fn settings(state_db_path: &Path, root: &Path) -> McpServerSettings {
    McpServerSettings {
        attachment_cache_dir: root.join("attachments"),
        max_download_bytes: 1024 * 1024,
        max_upload_bytes: 1024 * 1024,
        state_db_path: state_db_path.to_path_buf(),
    }
}

fn policy() -> ApprovalPolicy {
    ApprovalPolicy {
        approval_chat_id: 700,
        approval_user_id: 700,
        ttl_seconds: 3_600,
        poll_timeout_seconds: 1,
        owner_grant_file: None,
    }
}

async fn call_tool(
    client: &rmcp::service::RunningService<rmcp::RoleClient, ()>,
    tool: &str,
    arguments: Value,
) -> anyhow::Result<rmcp::model::CallToolResult> {
    client
        .call_tool(
            CallToolRequestParams::new(tool.to_string())
                .with_arguments(arguments.as_object().expect("object").clone()),
        )
        .await
        .map_err(Into::into)
}

async fn prepare_batch(
    client: &rmcp::service::RunningService<rmcp::RoleClient, ()>,
    tool: &str,
    arguments: Value,
) -> anyhow::Result<(String, String)> {
    let prepared = call_tool(client, tool, arguments).await?;
    assert_eq!(prepared.is_error, Some(false), "prepare {tool}: {prepared:?}");
    let data = prepared.structured_content.as_ref().expect("prepared data");
    Ok((
        data.pointer("/data/mutation_id").and_then(Value::as_str).expect("mutation id").to_string(),
        data.pointer("/data/payload_sha256")
            .and_then(Value::as_str)
            .expect("payload hash")
            .to_string(),
    ))
}

async fn request_approval(
    client: &rmcp::service::RunningService<rmcp::RoleClient, ()>,
    mutation_id: &str,
    payload_sha256: &str,
) -> anyhow::Result<(String, i64)> {
    let requested = call_tool(
        client,
        "request_mutation_approval",
        json!({
            "purpose": "тест: подготовка мутации к подтверждению",
            "mutation_id": mutation_id,
            "approved_payload_sha256": payload_sha256
        }),
    )
    .await?;
    assert_eq!(requested.is_error, Some(false), "request approval: {requested:?}");
    let data = requested.structured_content.as_ref().expect("approval data");
    Ok((
        data.pointer("/data/approval_id").and_then(Value::as_str).expect("approval id").to_string(),
        data.pointer("/data/approval_message_id")
            .and_then(Value::as_i64)
            .expect("approval message id"),
    ))
}

async fn wait_for_state(
    state_db_path: &Path,
    approval_id: &str,
    wanted: MutationApprovalState,
    timeout: Duration,
) -> anyhow::Result<MutationApproval> {
    let started = tokio::time::Instant::now();
    loop {
        let approval =
            MutationLedger::new(state_db_path.to_path_buf()).get_approval(approval_id)?;
        if approval.as_ref().is_some_and(|approval| approval.state == wanted) {
            return Ok(approval.expect("checked"));
        }
        let last = approval.map(|approval| {
            (approval.state, approval.next_retry_at_unix, approval.last_error_code)
        });
        if started.elapsed() >= timeout {
            anyhow::bail!("approval did not reach {wanted:?}; last={last:?}");
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
}

#[tokio::test]
async fn batch_invites_preserve_order_and_per_user_outcomes() -> anyhow::Result<()> {
    let backend = Arc::new(FakeTelegramBackend::fixture());
    for user_id in 1_001..=1_005 {
        backend.add_fixture_contact(fixture_contact(user_id)).await;
    }
    backend.script_chat_member_invites("-100505", 1_002, vec![FakeInviteOutcome::Rejected]).await;
    backend
        .script_chat_member_invites(
            "-100505",
            1_004,
            vec![FakeInviteOutcome::RetryableNoEffect, FakeInviteOutcome::Success],
        )
        .await;
    backend
        .script_chat_member_invites("-100505", 1_003, vec![FakeInviteOutcome::UncertainAfterApply])
        .await;

    let bot = Arc::new(FakeApprovalBot::default());
    let temp = tempfile::tempdir()?;
    let state_db_path = temp.path().join("state.sqlite");
    let server = McpServer::with_approval(
        backend.clone(),
        settings(&state_db_path, temp.path()),
        policy(),
        bot.clone(),
    )
    .map_err(anyhow::Error::msg)?;
    let worker = server.start_approval_worker()?.expect("approval worker");
    let (server_transport, client_transport) = tokio::io::duplex(128 * 1024);
    let server_task = tokio::spawn(async move {
        server.serve(server_transport).await?.waiting().await?;
        anyhow::Ok(())
    });
    let client = ().serve(client_transport).await?;

    let users = json!([
        "@alex",
        "@rin_restricted",
        "@kai_kicked",
        "@batch_invite_1001",
        "@batch_invite_1002",
        "@batch_invite_1004",
        "@batch_invite_1003",
        "@batch_invite_1005"
    ]);
    let (mutation_id, payload_sha256) = prepare_batch(
        &client,
        "prepare_invite_chat_members",
        json!({ "chat_id": "-100505", "user_refs": users }),
    )
    .await?;
    let initial = MutationLedger::new(state_db_path.clone())
        .get_invite_progress(&mutation_id)?
        .expect("initial progress");
    assert_eq!(initial.total_count, 8);
    assert_eq!(initial.already_present_count, 2);
    assert_eq!(initial.rejected_count, 1);
    assert_eq!(initial.pending_count, 5);

    let (approval_id, message_id) =
        request_approval(&client, &mutation_id, &payload_sha256).await?;
    let card = bot.sent_mutation_cards().await.last().expect("approval card").clone();
    assert!(card.rendered_text.encode_utf16().count() <= 3_800);
    assert!(card.rendered_text.contains("durable-состояние"));
    bot.push_mutation_decision(&approval_id, ApprovalAction::Approve, 700, 700, message_id).await;

    let applied = wait_for_state(
        &state_db_path,
        &approval_id,
        MutationApprovalState::Applied,
        Duration::from_secs(18),
    )
    .await?;
    let result = serde_json::to_value(applied.result.expect("result"))?;
    assert_eq!(result.pointer("/kind").and_then(Value::as_str), Some("chat_members_invited"));
    let outcomes = result.pointer("/outcomes").and_then(Value::as_array).expect("outcomes");
    let states = outcomes
        .iter()
        .map(|outcome| outcome.pointer("/state").and_then(Value::as_str).unwrap_or_default())
        .collect::<Vec<_>>();
    assert_eq!(
        states,
        vec![
            "already_present",
            "already_present",
            "rejected",
            "confirmed",
            "rejected",
            "confirmed",
            "confirmed",
            "confirmed"
        ]
    );
    assert_eq!(
        outcomes[5].pointer("/confirmation").and_then(Value::as_str),
        Some("provider_receipt_and_readback")
    );
    assert_eq!(
        outcomes[6].pointer("/confirmation").and_then(Value::as_str),
        Some("active_observed_after_uncertain")
    );
    let calls = backend.chat_member_invite_calls().await;
    assert_eq!(
        calls,
        vec![
            ("-100505".to_string(), 1_001),
            ("-100505".to_string(), 1_002),
            ("-100505".to_string(), 1_004),
            ("-100505".to_string(), 1_004),
            ("-100505".to_string(), 1_003),
            ("-100505".to_string(), 1_005),
        ]
    );
    assert_eq!(calls.iter().filter(|(_, user_id)| *user_id == 1_003).count(), 1);
    assert!(!calls.iter().any(|(_, user_id)| *user_id == 707));

    bot.push_mutation_decision(&approval_id, ApprovalAction::Approve, 700, 700, message_id).await;
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert_eq!(backend.chat_member_invite_calls().await, calls);

    client.cancel().await?;
    server_task.await??;
    worker.abort();
    let _ = worker.await;
    Ok(())
}

#[tokio::test]
async fn uncertain_invite_survives_restart_without_replay() -> anyhow::Result<()> {
    let backend = Arc::new(FakeTelegramBackend::fixture());
    backend.add_fixture_contact(fixture_contact(1_101)).await;
    backend.add_fixture_contact(fixture_contact(1_102)).await;
    backend
        .script_chat_member_invites("-100505", 1_101, vec![FakeInviteOutcome::UncertainBeforeApply])
        .await;
    let bot = Arc::new(FakeApprovalBot::default());
    let temp = tempfile::tempdir()?;
    let state_db_path = temp.path().join("state.sqlite");

    let server = McpServer::with_approval(
        backend.clone(),
        settings(&state_db_path, temp.path()),
        policy(),
        bot.clone(),
    )
    .map_err(anyhow::Error::msg)?;
    let worker = server.start_approval_worker()?.expect("approval worker");
    let (server_transport, client_transport) = tokio::io::duplex(128 * 1024);
    let server_task = tokio::spawn(async move {
        server.serve(server_transport).await?.waiting().await?;
        anyhow::Ok(())
    });
    let client = ().serve(client_transport).await?;
    let (mutation_id, payload_sha256) = prepare_batch(
        &client,
        "invite_chat_members",
        json!({
            "chat_id": "-100505",
            "users": ["@batch_invite_1101", "@batch_invite_1102"]
        }),
    )
    .await?;
    let (approval_id, message_id) =
        request_approval(&client, &mutation_id, &payload_sha256).await?;
    bot.push_mutation_decision(&approval_id, ApprovalAction::Approve, 700, 700, message_id).await;
    wait_for_state(
        &state_db_path,
        &approval_id,
        MutationApprovalState::OutcomeUnknown,
        Duration::from_secs(3),
    )
    .await?;
    assert_eq!(backend.chat_member_invite_calls().await, vec![("-100505".to_string(), 1_101)]);

    client.cancel().await?;
    server_task.await??;
    worker.abort();
    let _ = worker.await;

    let restarted_bot = Arc::new(FakeApprovalBot::default());
    let restarted = McpServer::with_approval(
        backend.clone(),
        settings(&state_db_path, temp.path()),
        policy(),
        restarted_bot,
    )
    .map_err(anyhow::Error::msg)?;
    let restarted_worker = restarted.start_approval_worker()?.expect("restarted worker");
    let (restarted_server_transport, restarted_client_transport) = tokio::io::duplex(128 * 1024);
    let restarted_task = tokio::spawn(async move {
        restarted.serve(restarted_server_transport).await?.waiting().await?;
        anyhow::Ok(())
    });
    let restarted_client = ().serve(restarted_client_transport).await?;

    let started = tokio::time::Instant::now();
    loop {
        let calls = backend.chat_member_invite_calls().await;
        if calls.iter().any(|(_, user_id)| *user_id == 1_102) {
            assert_eq!(calls.iter().filter(|(_, user_id)| *user_id == 1_101).count(), 1);
            break;
        }
        if started.elapsed() > Duration::from_secs(9) {
            anyhow::bail!("restarted worker did not continue the later safe invite");
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    let progress = MutationLedger::new(state_db_path.clone())
        .get_invite_progress(&mutation_id)?
        .expect("progress after restart");
    assert_eq!(progress.targets[0].state, InviteTargetState::OutcomeUnknown);
    assert_eq!(progress.targets[0].attempt_count, 1);
    assert!(progress.targets[0].observation_count >= 1);
    assert_eq!(progress.targets[1].state, InviteTargetState::Confirmed);
    assert_eq!(
        progress.targets[1].confirmation,
        Some(InviteConfirmation::ProviderReceiptAndReadback)
    );

    restarted_client.cancel().await?;
    restarted_task.await??;
    restarted_worker.abort();
    let _ = restarted_worker.await;
    Ok(())
}

#[tokio::test]
async fn batch_invite_limits_aliases_basic_groups_and_card_size_are_e2e() -> anyhow::Result<()> {
    let backend = Arc::new(FakeTelegramBackend::fixture());
    for user_id in 2_000..=2_050 {
        backend.add_fixture_contact(fixture_contact(user_id)).await;
    }
    let bot = Arc::new(FakeApprovalBot::default());
    let temp = tempfile::tempdir()?;
    let state_db_path = temp.path().join("state.sqlite");
    let server = McpServer::with_approval(
        backend.clone(),
        settings(&state_db_path, temp.path()),
        policy(),
        bot.clone(),
    )
    .map_err(anyhow::Error::msg)?;
    let worker = server.start_approval_worker()?.expect("approval worker");
    let (server_transport, client_transport) = tokio::io::duplex(256 * 1024);
    let server_task = tokio::spawn(async move {
        server.serve(server_transport).await?.waiting().await?;
        anyhow::Ok(())
    });
    let client = ().serve(client_transport).await?;

    for invalid in [
        json!([]),
        Value::Array((2_000..=2_050).map(|user_id| Value::from(user_id.to_string())).collect()),
    ] {
        let result = call_tool(
            &client,
            "prepare_invite_chat_members",
            json!({ "chat_id": "-100505", "user_refs": invalid }),
        )
        .await?;
        assert_eq!(result.is_error, Some(true));
    }
    let duplicate = call_tool(
        &client,
        "prepare_invite_chat_members",
        json!({ "chat_id": "-100505", "user_refs": ["2000", "@batch_invite_2000"] }),
    )
    .await?;
    assert_eq!(duplicate.is_error, Some(true));
    let self_invite = call_tool(
        &client,
        "prepare_invite_chat_members",
        json!({ "chat_id": "-100505", "user_refs": ["7"] }),
    )
    .await?;
    assert_eq!(self_invite.is_error, Some(true));
    let user_chat = call_tool(
        &client,
        "prepare_invite_chat_members",
        json!({ "chat_id": "@alex", "user_refs": ["2000"] }),
    )
    .await?;
    assert_eq!(user_chat.is_error, Some(true));

    let fifty =
        Value::Array((2_000..2_050).map(|user_id| Value::from(user_id.to_string())).collect());
    let (mutation_id, payload_sha256) = prepare_batch(
        &client,
        "prepare_invite_chat_members",
        json!({ "chat_id": "-100505", "user_ids": fifty }),
    )
    .await?;
    let (_approval_id, _message_id) =
        request_approval(&client, &mutation_id, &payload_sha256).await?;
    let card = bot.sent_mutation_cards().await.last().expect("50-target card").clone();
    assert!(card.rendered_text.encode_utf16().count() <= 3_800);
    assert_eq!(card.card.invite_progress.as_ref().map(|progress| progress.total_count), Some(50));

    let (group_mutation_id, group_hash) = prepare_batch(
        &client,
        "invite_to_group",
        json!({ "chat_id": "-202", "user_refs": ["2050"] }),
    )
    .await?;
    let (group_approval_id, group_message_id) =
        request_approval(&client, &group_mutation_id, &group_hash).await?;
    bot.push_mutation_decision(
        &group_approval_id,
        ApprovalAction::Approve,
        700,
        700,
        group_message_id,
    )
    .await;
    let group_applied = wait_for_state(
        &state_db_path,
        &group_approval_id,
        MutationApprovalState::Applied,
        Duration::from_secs(3),
    )
    .await?;
    let group_result = serde_json::to_value(group_applied.result.expect("group result"))?;
    assert_eq!(group_result.pointer("/chat_id").and_then(Value::as_str), Some("-202"));
    assert_eq!(backend.chat_member_invite_calls().await.last(), Some(&("-202".to_string(), 2_050)));

    client.cancel().await?;
    server_task.await??;
    worker.abort();
    let _ = worker.await;
    Ok(())
}
