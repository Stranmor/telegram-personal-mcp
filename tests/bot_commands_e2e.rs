use rmcp::{ServiceExt, model::CallToolRequestParams};
use serde_json::{Value, json};
use std::sync::Arc;
use std::time::Duration;
use telegram_personal_mcp::{
    ApprovalAction, ApprovalPolicy, BotCommandSummary, FakeApprovalBot, FakeTelegramBackend,
    McpServer, McpServerSettings, MutationApprovalState, MutationLedger, TelegramBackend,
};

async fn wait_for_mutation_state(
    state_db_path: &std::path::Path,
    approval_id: &str,
    wanted: MutationApprovalState,
) -> anyhow::Result<()> {
    let mut last = None;
    for _ in 0..1_000 {
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

async fn rejected_code(
    client: &rmcp::service::RunningService<rmcp::RoleClient, ()>,
    tool: &'static str,
    arguments: Value,
) -> anyhow::Result<String> {
    let rejected = client
        .call_tool(
            CallToolRequestParams::new(tool)
                .with_arguments(arguments.as_object().expect("object").clone()),
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
    commands: Value,
) -> anyhow::Result<(String, i64, Value)> {
    let prepared = client
        .call_tool(
            CallToolRequestParams::new("set_bot_commands").with_arguments(
                json!({
                    "bot": "@fixture_account",
                    "langCode": "en",
                    "commands": commands
                })
                .as_object()
                .expect("object")
                .clone(),
            ),
        )
        .await?;
    assert_eq!(prepared.is_error, Some(false));
    let prepared = prepared.structured_content.expect("prepared bot commands");
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
    let approval = requested.structured_content.expect("bot-command approval");
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

#[tokio::test]
async fn bot_commands_are_account_bound_hash_approved_and_read_back() -> anyhow::Result<()> {
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

    let closed_schema = client
        .call_tool(
            CallToolRequestParams::new("get_bot_commands").with_arguments(
                json!({
                    "bot": "@fixture_account",
                    "includePrivateToken": true
                })
                .as_object()
                .expect("object")
                .clone(),
            ),
        )
        .await;
    assert!(closed_schema.is_err(), "unknown fields must fail at the JSON-RPC boundary");

    assert_eq!(
        rejected_code(&client, "get_bot_commands", json!({ "bot": "@fixture_account" })).await?,
        "BOT_ACCOUNT_REQUIRED"
    );
    backend.set_fixture_account_bot(true).await;
    assert_eq!(
        rejected_code(&client, "get_bot_commands", json!({ "bot": "@fixture_inline_bot" })).await?,
        "BOT_ACCOUNT_MISMATCH"
    );
    assert_eq!(
        rejected_code(
            &client,
            "set_bot_commands",
            json!({
                "bot": "@fixture_account",
                "commands": [{ "command": "/start", "description": "Invalid slash" }]
            })
        )
        .await?,
        "INVALID_ARGUMENT"
    );

    let initial = client
        .call_tool(CallToolRequestParams::new("get_bot_commands").with_arguments(
            json!({ "botUsername": "@fixture_account" }).as_object().expect("object").clone(),
        ))
        .await?;
    assert_eq!(initial.is_error, Some(false));
    let initial = initial.structured_content.expect("initial bot commands");
    assert_eq!(initial.pointer("/data/bot/user_id").and_then(Value::as_i64), Some(7));
    assert_eq!(initial.pointer("/data/bot/bot").and_then(Value::as_bool), Some(true));
    assert_eq!(initial.pointer("/data/language_code").and_then(Value::as_str), Some("en"));
    assert_eq!(initial.pointer("/data/commands/0/command").and_then(Value::as_str), Some("start"));

    backend.make_next_bot_command_set_outcome_uncertain().await;
    let desired = json!([
        {
            "command": "start",
            "description": "Start the release bot"
        },
        {
            "command": "release_status",
            "description": "Show release status",
            "ephemeral": true
        }
    ]);
    let (approval_id, approval_message_id, prepared) =
        prepare_and_request(&client, desired.clone()).await?;
    assert_eq!(
        prepared.pointer("/data/action/kind").and_then(Value::as_str),
        Some("set_bot_commands")
    );
    assert_eq!(
        prepared.pointer("/data/action/before/commands/0/command").and_then(Value::as_str),
        Some("start")
    );
    assert_eq!(
        prepared.pointer("/data/action/commands/1/ephemeral").and_then(Value::as_bool),
        Some(true)
    );
    assert_eq!(backend.bot_command_set_call_count().await, 0);
    let card_debug = format!("{:?}", bot.sent_mutation_cards().await.last());
    assert!(card_debug.contains("release_status"));
    assert!(card_debug.contains("Show release status"));

    bot.push_mutation_decision(
        &approval_id,
        ApprovalAction::Approve,
        700,
        700,
        approval_message_id,
    )
    .await;
    wait_for_mutation_state(&state_db_path, &approval_id, MutationApprovalState::Applied).await?;
    let applied = MutationLedger::new(state_db_path.clone())
        .get_approval(&approval_id)?
        .expect("applied bot commands");
    assert_eq!(applied.automated_retry_count, 1);
    assert_eq!(backend.bot_command_set_call_count().await, 1);
    let result = serde_json::to_value(applied.result.expect("bot command receipt"))?;
    assert_eq!(result.pointer("/kind").and_then(Value::as_str), Some("bot_commands_set"));
    assert_eq!(
        result.pointer("/state/commands/1/command").and_then(Value::as_str),
        Some("release_status")
    );

    let readback = client
        .call_tool(CallToolRequestParams::new("get_bot_commands").with_arguments(
            json!({ "bot": "7", "language_code": "en" }).as_object().expect("object").clone(),
        ))
        .await?;
    assert_eq!(readback.is_error, Some(false));
    assert_eq!(
        readback
            .structured_content
            .as_ref()
            .and_then(|value| value.pointer("/data/commands/1/description"))
            .and_then(Value::as_str),
        Some("Show release status")
    );

    let (reset_id, reset_message_id, reset_prepared) =
        prepare_and_request(&client, json!([])).await?;
    assert_eq!(
        reset_prepared.pointer("/data/action/commands").and_then(Value::as_array).map(Vec::len),
        Some(0)
    );
    bot.push_mutation_decision(&reset_id, ApprovalAction::Approve, 700, 700, reset_message_id)
        .await;
    wait_for_mutation_state(&state_db_path, &reset_id, MutationApprovalState::Applied).await?;
    assert_eq!(backend.bot_command_set_call_count().await, 2);
    assert!(backend.get_bot_commands(None, "en").await?.is_empty());

    let external_before = vec![BotCommandSummary {
        command: "external".to_string(),
        description: "External owner state".to_string(),
        ephemeral: false,
    }];
    backend.set_bot_commands(None, "en", &external_before).await?;
    let (guarded_id, guarded_message_id, _) = prepare_and_request(
        &client,
        json!([{ "command": "approved", "description": "Approved replacement" }]),
    )
    .await?;
    let changed_after_prepare = vec![BotCommandSummary {
        command: "changed".to_string(),
        description: "Changed after preparation".to_string(),
        ephemeral: false,
    }];
    backend.set_bot_commands(None, "en", &changed_after_prepare).await?;
    assert_eq!(backend.bot_command_set_call_count().await, 4);
    bot.push_mutation_decision(&guarded_id, ApprovalAction::Approve, 700, 700, guarded_message_id)
        .await;
    wait_for_mutation_state(&state_db_path, &guarded_id, MutationApprovalState::Failed).await?;
    let guarded = MutationLedger::new(state_db_path.clone())
        .get_approval(&guarded_id)?
        .expect("guarded bot-command approval");
    assert_eq!(guarded.last_error_code.as_deref(), Some("MUTATION_PRECONDITION_FAILED"));
    assert_eq!(backend.bot_command_set_call_count().await, 4);
    assert_eq!(backend.get_bot_commands(None, "en").await?, changed_after_prepare);

    client.cancel().await?;
    server_task.await??;
    worker.abort();
    Ok(())
}
