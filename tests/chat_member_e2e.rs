use rmcp::{ServiceExt, model::CallToolRequestParams};
use serde_json::{Value, json};
use std::sync::Arc;
use std::time::Duration;
use telegram_personal_mcp::{
    ApprovalAction, ApprovalPolicy, FakeApprovalBot, FakeTelegramBackend, McpServer,
    McpServerSettings, MutationApprovalState, MutationLedger,
};

fn admin_rights(delete_messages: bool, invite_users: bool, manage_topics: bool) -> Value {
    json!({
        "change_info": false,
        "post_messages": false,
        "edit_messages": false,
        "delete_messages": delete_messages,
        "ban_users": false,
        "invite_users": invite_users,
        "pin_messages": false,
        "add_admins": false,
        "anonymous": false,
        "manage_call": false,
        "other": false,
        "manage_topics": manage_topics,
        "post_stories": false,
        "edit_stories": false,
        "delete_stories": false,
        "manage_direct_messages": false,
        "manage_ranks": false,
        "manage_linked_peers": false
    })
}

fn banned_rights(send_messages: bool, send_docs: bool) -> Value {
    json!({
        "view_messages": false,
        "send_messages": send_messages,
        "send_media": false,
        "send_stickers": false,
        "send_gifs": false,
        "send_games": false,
        "send_inline": false,
        "embed_links": false,
        "send_polls": false,
        "change_info": false,
        "invite_users": false,
        "pin_messages": false,
        "manage_topics": false,
        "send_photos": false,
        "send_videos": false,
        "send_roundvideos": false,
        "send_audios": false,
        "send_voices": false,
        "send_docs": send_docs,
        "send_plain": false,
        "edit_rank": false,
        "send_reactions": false,
        "manage_linked_peers": false,
        "until_date_unix": 0
    })
}

async fn wait_for_mutation_state(
    state_db_path: &std::path::Path,
    approval_id: &str,
    wanted: MutationApprovalState,
) -> anyhow::Result<()> {
    let mut last = None;
    for _ in 0..300 {
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
            .push_mutation_decision(&approval_id, ApprovalAction::Approve, 700, 700, message_id)
            .await;
        if let Err(error) =
            wait_for_mutation_state(&$state_db_path, &approval_id, MutationApprovalState::Applied)
                .await
        {
            anyhow::bail!("{}: {error}", $tool);
        }
        MutationLedger::new($state_db_path.to_path_buf())
            .get_approval(&approval_id)?
            .expect("applied mutation")
            .result
            .map(|result| serde_json::to_value(result).expect("result json"))
            .expect("mutation result")
    }};
}

#[tokio::test]
async fn chat_member_lifecycle_is_e2e_and_idempotent() -> anyhow::Result<()> {
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

    let member = client
        .call_tool(
            CallToolRequestParams::new("get_chat_member").with_arguments(
                json!({ "chat_id": "-100505", "user_ref": "@alex" })
                    .as_object()
                    .expect("object")
                    .clone(),
            ),
        )
        .await?;
    assert_eq!(member.is_error, Some(false));
    assert_eq!(
        member
            .structured_content
            .as_ref()
            .and_then(|v| v.pointer("/data/member/user/user_id"))
            .and_then(Value::as_i64),
        Some(101)
    );
    assert_eq!(
        member
            .structured_content
            .as_ref()
            .and_then(|v| v.pointer("/data/member/role"))
            .and_then(Value::as_str),
        Some("member")
    );

    let promoted = approve_mutation!(
        client,
        bot,
        state_db_path,
        "prepare_set_chat_member_admin",
        json!({
            "chat_id": "-100505",
            "user_ref": "@alex",
            "rights": admin_rights(true, true, true),
            "rank": "Release"
        })
    );
    assert_eq!(
        promoted.pointer("/kind").and_then(Value::as_str),
        Some("chat_member_admin_updated")
    );
    assert_eq!(promoted.pointer("/member/role").and_then(Value::as_str), Some("admin"));

    let demoted = approve_mutation!(
        client,
        bot,
        state_db_path,
        "prepare_set_chat_member_admin",
        json!({
            "chat_id": "-100505",
            "user_ref": "@alex",
            "rights": admin_rights(false, false, false)
        })
    );
    assert_eq!(demoted.pointer("/member/role").and_then(Value::as_str), Some("member"));

    let restricted = approve_mutation!(
        client,
        bot,
        state_db_path,
        "prepare_set_chat_member_restrictions",
        json!({
            "chat_id": "-100505",
            "user_ref": "@alex",
            "rights": banned_rights(true, true)
        })
    );
    assert_eq!(restricted.pointer("/member/role").and_then(Value::as_str), Some("restricted"));

    let unrestricted = approve_mutation!(
        client,
        bot,
        state_db_path,
        "prepare_set_chat_member_restrictions",
        json!({
            "chat_id": "-100505",
            "user_ref": "@alex",
            "rights": banned_rights(false, false)
        })
    );
    assert_eq!(unrestricted.pointer("/member/role").and_then(Value::as_str), Some("member"));

    let removed = approve_mutation!(
        client,
        bot,
        state_db_path,
        "prepare_remove_chat_member",
        json!({ "chat_id": "-100505", "user_ref": "@alex" })
    );
    assert_eq!(removed.pointer("/kind").and_then(Value::as_str), Some("chat_member_removed"));
    assert_eq!(removed.pointer("/user_id").and_then(Value::as_i64), Some(101));

    let banned = approve_mutation!(
        client,
        bot,
        state_db_path,
        "ban_member",
        json!({ "chat_id": "-100505", "user_ref": "@alex" })
    );
    assert_eq!(banned.pointer("/member/role").and_then(Value::as_str), Some("banned"));

    let unbanned = approve_mutation!(
        client,
        bot,
        state_db_path,
        "unban_member",
        json!({ "chat_id": "-100505", "user_ref": "@alex" })
    );
    assert_eq!(unbanned.pointer("/member/role").and_then(Value::as_str), Some("left"));

    let added = approve_mutation!(
        client,
        bot,
        state_db_path,
        "prepare_add_chat_member",
        json!({ "chat_id": "-100505", "user_ref": "@dana_candidate" })
    );
    assert_eq!(added.pointer("/member/role").and_then(Value::as_str), Some("member"));
    assert_eq!(added.pointer("/member/user/user_id").and_then(Value::as_i64), Some(404));

    let add_count_before_duplicate = backend.chat_member_call_counts().await.0;
    let last_card = bot.sent_mutation_cards().await.last().expect("last card").clone();
    let last_approval = last_card.card.approval.clone();
    bot.push_mutation_decision(
        &last_approval.approval_id,
        ApprovalAction::Approve,
        700,
        700,
        last_card.message_id,
    )
    .await;
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert_eq!(backend.chat_member_call_counts().await.0, add_count_before_duplicate);

    assert_eq!(backend.chat_member_call_counts().await, (1, 1, 2, 4));

    client.cancel().await?;
    server_task.await??;
    worker.abort();
    let _ = worker.await;
    Ok(())
}
