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
    for _ in 0..1_500 {
        let approval =
            MutationLedger::new(state_db_path.to_path_buf()).get_approval(approval_id)?;
        if approval.as_ref().map(|approval| approval.state) == Some(wanted) {
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
    anyhow::bail!("Business chat-link mutation did not reach {wanted:?}; last={last:?}")
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
        if approval.as_ref().and_then(|approval| approval.last_error_code.as_deref())
            == Some(wanted)
        {
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
    anyhow::bail!("Business chat-link mutation did not reach error {wanted}; last={last:?}")
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
    assert_eq!(rejected.is_error, Some(true), "{tool} must reject the input");
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
    tool: &'static str,
    arguments: Value,
) -> anyhow::Result<(String, i64, Value)> {
    let prepared = client
        .call_tool(
            CallToolRequestParams::new(tool)
                .with_arguments(arguments.as_object().expect("object").clone()),
        )
        .await?;
    assert_eq!(prepared.is_error, Some(false), "prepare {tool}: {:?}", prepared.structured_content);
    let prepared = prepared.structured_content.expect("prepared Business chat-link mutation");
    let mutation_id =
        prepared.pointer("/data/mutation_id").and_then(Value::as_str).expect("mutation id");
    let payload_sha256 =
        prepared.pointer("/data/payload_sha256").and_then(Value::as_str).expect("payload hash");
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
    assert_eq!(requested.is_error, Some(false), "request {tool}");
    let requested = requested.structured_content.expect("Business chat-link approval request");
    Ok((
        requested
            .pointer("/data/approval_id")
            .and_then(Value::as_str)
            .expect("approval id")
            .to_string(),
        requested
            .pointer("/data/approval_message_id")
            .and_then(Value::as_i64)
            .expect("approval message id"),
        prepared,
    ))
}

async fn approve(bot: &FakeApprovalBot, approval_id: &str, message_id: i64) {
    bot.push_mutation_decision(approval_id, ApprovalAction::Approve, 700, 700, message_id).await;
}

async fn business_links(
    client: &rmcp::service::RunningService<rmcp::RoleClient, ()>,
) -> anyhow::Result<Value> {
    let response = client.call_tool(CallToolRequestParams::new("get_business_chat_links")).await?;
    assert_eq!(response.is_error, Some(false));
    Ok(response.structured_content.expect("Business chat links"))
}

#[tokio::test]
async fn business_chat_links_are_typed_private_and_recover_without_duplicate_actuation()
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

    let tools = client.list_all_tools().await?;
    for (name, read_only, destructive) in [
        ("get_business_chat_links", true, false),
        ("resolve_business_chat_link", true, false),
        ("create_business_chat_link", false, false),
        ("edit_business_chat_link", false, false),
        ("delete_business_chat_link", false, true),
    ] {
        let tool = tools.iter().find(|tool| tool.name == name).expect("Business chat-link tool");
        assert_eq!(tool.input_schema.get("additionalProperties"), Some(&json!(false)));
        assert!(tool.output_schema.is_some());
        assert_eq!(
            tool.annotations.as_ref().and_then(|value| value.read_only_hint),
            Some(read_only)
        );
        assert_eq!(
            tool.annotations.as_ref().and_then(|value| value.destructive_hint),
            Some(destructive)
        );
    }

    let initial = business_links(&client).await?;
    assert_eq!(initial.pointer("/data/count").and_then(Value::as_u64), Some(2));
    assert_eq!(
        initial.pointer("/data/links/0/slug").and_then(Value::as_str),
        Some("fixture-welcome")
    );
    assert_eq!(
        initial.pointer("/data/links/0/link").and_then(Value::as_str),
        Some("https://t.me/m/fixture-welcome")
    );
    assert_eq!(
        initial.pointer("/data/links/0/entities_sha256").and_then(Value::as_str).map(str::len),
        Some(64)
    );
    assert!(
        initial
            .pointer("/data/related_users")
            .and_then(Value::as_array)
            .is_some_and(|users| !users.is_empty())
    );
    assert!(initial.pointer("/data/links/0/access_hash").is_none());

    let resolved = client
        .call_tool(CallToolRequestParams::new("resolve_business_chat_link").with_arguments(
            json!({ "slug": "fixture-support" }).as_object().expect("object").clone(),
        ))
        .await?;
    assert_eq!(resolved.is_error, Some(false));
    let resolved = resolved.structured_content.expect("resolved Business chat link");
    assert_eq!(resolved.pointer("/data/peer_ref").and_then(Value::as_str), Some("7"));
    assert_eq!(
        resolved.pointer("/data/message").and_then(Value::as_str),
        Some("Describe the support request")
    );

    for (tool, arguments) in [
        ("resolve_business_chat_link", json!({ "slug": "" })),
        ("resolve_business_chat_link", json!({ "slug": "fixture/support" })),
        ("resolve_business_chat_link", json!({ "slug": " fixture" })),
        ("create_business_chat_link", json!({ "message": "" })),
        ("create_business_chat_link", json!({ "message": "x".repeat(4_097) })),
        ("create_business_chat_link", json!({ "message": "valid", "title": "x".repeat(33) })),
        ("edit_business_chat_link", json!({ "slug": "fixture-welcome", "message": "" })),
    ] {
        assert_eq!(rejected_code(&client, tool, arguments).await?, "INVALID_ARGUMENT");
    }
    assert_eq!(
        rejected_code(
            &client,
            "edit_business_chat_link",
            json!({
                "slug": "fixture-welcome",
                "message": "Welcome to the fixture account",
                "title": "Welcome"
            }),
        )
        .await?,
        "NO_CHANGE"
    );
    let unknown_field = client
        .call_tool(CallToolRequestParams::new("create_business_chat_link").with_arguments(
            json!({ "message": "valid", "extra": true }).as_object().expect("object").clone(),
        ))
        .await;
    assert!(unknown_field.is_err());

    let (create_id, create_message_id, created) = prepare_and_request(
        &client,
        "create_business_chat_link",
        json!({
            "message": "<b>New customer</b>",
            "title": "New lead",
            "parseMode": "html"
        }),
    )
    .await?;
    assert_eq!(
        created.pointer("/data/action/kind").and_then(Value::as_str),
        Some("create_business_chat_link")
    );
    assert_eq!(
        created.pointer("/data/action/content/rendered_message").and_then(Value::as_str),
        Some("New customer")
    );
    assert_eq!(
        created.pointer("/data/action/content/entity_count").and_then(Value::as_u64),
        Some(1)
    );
    let create_card = bot.sent_mutation_cards().await.last().expect("create card").clone();
    assert!(create_card.rendered_text.contains("СОЗДАНИЕ TELEGRAM BUSINESS CHAT LINK"));
    assert!(create_card.rendered_text.contains("автоматический повтор отключён"));
    assert!(create_card.rendered_text.contains("URL из результата считается недоверенным"));
    assert!(!create_card.rendered_text.contains("access_hash"));
    approve(&bot, &create_id, create_message_id).await;
    wait_for_mutation_state(&state_db_path, &create_id, MutationApprovalState::Applied).await?;
    assert_eq!(backend.business_chat_link_call_counts().await, (1, 0, 0));
    assert_eq!(
        business_links(&client).await?.pointer("/data/links/2/slug").and_then(Value::as_str),
        Some("fixture-created-1")
    );

    backend.make_next_business_chat_link_create_outcome_uncertain().await;
    let (uncertain_create_id, uncertain_create_message_id, _) = prepare_and_request(
        &client,
        "create_business_chat_link",
        json!({ "message": "Created before receipt loss", "title": "Uncertain" }),
    )
    .await?;
    approve(&bot, &uncertain_create_id, uncertain_create_message_id).await;
    wait_for_mutation_state(
        &state_db_path,
        &uncertain_create_id,
        MutationApprovalState::OutcomeUnknown,
    )
    .await?;
    wait_for_error_code(&state_db_path, &uncertain_create_id, "MUTATION_OUTCOME_UNKNOWN_NO_RETRY")
        .await?;
    tokio::time::sleep(Duration::from_millis(250)).await;
    let uncertain_create = MutationLedger::new(state_db_path.clone())
        .get_approval(&uncertain_create_id)?
        .expect("uncertain create approval");
    assert_eq!(uncertain_create.next_retry_at_unix, None);
    assert_eq!(backend.business_chat_link_call_counts().await, (2, 0, 0));
    assert!(
        business_links(&client)
            .await?
            .pointer("/data/links")
            .and_then(Value::as_array)
            .is_some_and(|links| links.iter().any(|link| {
                link.pointer("/slug").and_then(Value::as_str) == Some("fixture-created-2")
            }))
    );

    let (edit_id, edit_message_id, _) = prepare_and_request(
        &client,
        "edit_business_chat_link",
        json!({
            "slug": "fixture-welcome",
            "message": "<i>Updated welcome</i>",
            "title": "Welcome 2",
            "parse_mode": "html"
        }),
    )
    .await?;
    approve(&bot, &edit_id, edit_message_id).await;
    wait_for_mutation_state(&state_db_path, &edit_id, MutationApprovalState::Applied).await?;
    assert_eq!(backend.business_chat_link_call_counts().await, (2, 1, 0));

    backend.make_next_business_chat_link_edit_outcome_uncertain().await;
    let (recover_edit_id, recover_edit_message_id, _) = prepare_and_request(
        &client,
        "edit_business_chat_link",
        json!({
            "slug": "fixture-welcome",
            "message": "Recovered edit",
            "title": "Recovered"
        }),
    )
    .await?;
    approve(&bot, &recover_edit_id, recover_edit_message_id).await;
    wait_for_mutation_state(&state_db_path, &recover_edit_id, MutationApprovalState::Applied)
        .await?;
    let recovered_edit = MutationLedger::new(state_db_path.clone())
        .get_approval(&recover_edit_id)?
        .expect("recovered edit approval");
    assert_eq!(recovered_edit.automated_retry_count, 1);
    assert_eq!(backend.business_chat_link_call_counts().await, (2, 2, 0));

    let (delete_id, delete_message_id, _) = prepare_and_request(
        &client,
        "delete_business_chat_link",
        json!({ "slug": "fixture-created-1" }),
    )
    .await?;
    let delete_card = bot.sent_mutation_cards().await.last().expect("delete card").clone();
    assert!(delete_card.rendered_text.contains("УДАЛЕНИЕ TELEGRAM BUSINESS CHAT LINK"));
    assert!(delete_card.rendered_text.contains("не открывается"));
    approve(&bot, &delete_id, delete_message_id).await;
    wait_for_mutation_state(&state_db_path, &delete_id, MutationApprovalState::Applied).await?;
    assert_eq!(backend.business_chat_link_call_counts().await, (2, 2, 1));

    backend.make_next_business_chat_link_delete_outcome_uncertain().await;
    let (recover_delete_id, recover_delete_message_id, _) = prepare_and_request(
        &client,
        "delete_business_chat_link",
        json!({ "slug": "fixture-support" }),
    )
    .await?;
    approve(&bot, &recover_delete_id, recover_delete_message_id).await;
    wait_for_mutation_state(&state_db_path, &recover_delete_id, MutationApprovalState::Applied)
        .await?;
    let recovered_delete = MutationLedger::new(state_db_path.clone())
        .get_approval(&recover_delete_id)?
        .expect("recovered delete approval");
    assert_eq!(recovered_delete.automated_retry_count, 1);
    assert_eq!(backend.business_chat_link_call_counts().await, (2, 2, 2));
    assert!(
        business_links(&client)
            .await?
            .pointer("/data/links")
            .and_then(Value::as_array)
            .is_some_and(|links| {
                links.iter().all(|link| {
                    !matches!(
                        link.pointer("/slug").and_then(Value::as_str),
                        Some("fixture-created-1" | "fixture-support")
                    )
                })
            })
    );

    client.cancel().await?;
    server_task.await??;
    worker.abort();
    let _ = worker.await;
    Ok(())
}
