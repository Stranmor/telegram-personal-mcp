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
async fn account_profile_lifecycle_is_e2e_validated_and_idempotent() -> anyhow::Result<()> {
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
            owner_permission_file: None,
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

    let initial = client.call_tool(CallToolRequestParams::new("get_account_profile")).await?;
    assert_eq!(initial.is_error, Some(false));
    let initial = initial.structured_content.expect("initial profile");
    assert_eq!(initial.pointer("/data/user_id").and_then(Value::as_i64), Some(7));
    assert_eq!(initial.pointer("/data/username").and_then(Value::as_str), Some("fixture_account"));

    let (profile, profile_approval_id, profile_message_id) = approve_mutation!(
        client,
        bot,
        state_db_path,
        "update_profile",
        json!({
            "first_name": "  Release  ",
            "last_name": "   ",
            "about": "  Safe profile E2E  "
        })
    );
    assert_eq!(profile.pointer("/kind").and_then(Value::as_str), Some("account_profile_updated"));
    assert_eq!(profile.pointer("/profile/first_name").and_then(Value::as_str), Some("Release"));
    assert!(profile.pointer("/profile/last_name").is_none_or(Value::is_null));
    assert_eq!(profile.pointer("/profile/about").and_then(Value::as_str), Some("Safe profile E2E"));

    let profile_call_count = backend.account_profile_call_counts().await.0;
    bot.push_mutation_decision(
        &profile_approval_id,
        ApprovalAction::Approve,
        700,
        700,
        profile_message_id,
    )
    .await;
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert_eq!(backend.account_profile_call_counts().await.0, profile_call_count);

    let (username, _, _) = approve_mutation!(
        client,
        bot,
        state_db_path,
        "update_username",
        json!({ "username": "  release_fixture  " })
    );
    assert_eq!(
        username.pointer("/profile/username").and_then(Value::as_str),
        Some("release_fixture")
    );

    let (birthday, _, _) = approve_mutation!(
        client,
        bot,
        state_db_path,
        "set_birthday",
        json!({ "birthday": { "day": 22, "month": 7, "year": 1991 } })
    );
    assert_eq!(birthday.pointer("/profile/birthday/day").and_then(Value::as_i64), Some(22));
    assert_eq!(birthday.pointer("/profile/birthday/year").and_then(Value::as_i64), Some(1991));

    let (birthday_cleared, _, _) =
        approve_mutation!(client, bot, state_db_path, "set_birthday", json!({ "birthday": null }));
    assert!(birthday_cleared.pointer("/profile/birthday").is_none_or(Value::is_null));

    let (presence, _, _) =
        approve_mutation!(client, bot, state_db_path, "set_presence", json!({ "online": true }));
    assert_eq!(presence.pointer("/kind").and_then(Value::as_str), Some("account_presence_updated"));
    assert_eq!(presence.pointer("/online").and_then(Value::as_bool), Some(true));

    for (arguments, expected_code) in [
        (json!({ "username": "bad space" }), "INVALID_ARGUMENT"),
        (json!({ "birthday": { "day": 31, "month": 4, "year": 1991 } }), "INVALID_ARGUMENT"),
    ] {
        let tool =
            if arguments.get("username").is_some() { "update_username" } else { "set_birthday" };
        let rejected = client
            .call_tool(
                CallToolRequestParams::new(tool)
                    .with_arguments(arguments.as_object().expect("object").clone()),
            )
            .await?;
        assert_eq!(rejected.is_error, Some(true));
        assert_eq!(
            rejected
                .structured_content
                .as_ref()
                .and_then(|value| value.pointer("/error/code"))
                .and_then(Value::as_str),
            Some(expected_code)
        );
    }

    let final_profile = client.call_tool(CallToolRequestParams::new("get_account_profile")).await?;
    let final_profile = final_profile.structured_content.expect("final profile");
    assert_eq!(final_profile.pointer("/data/first_name").and_then(Value::as_str), Some("Release"));
    assert_eq!(
        final_profile.pointer("/data/username").and_then(Value::as_str),
        Some("release_fixture")
    );
    assert!(final_profile.pointer("/data/birthday").is_none_or(Value::is_null));
    assert_eq!(backend.account_profile_call_counts().await, (1, 1, 2, 1));

    let approval_surface =
        format!("{:?}\n{:?}", bot.sent_mutation_cards().await, bot.mutation_edits().await);
    assert!(approval_surface.contains("Safe profile E2E"));
    assert!(approval_surface.contains("release_fixture"));

    client.cancel().await?;
    server_task.await??;
    worker.abort();
    let _ = worker.await;
    Ok(())
}
