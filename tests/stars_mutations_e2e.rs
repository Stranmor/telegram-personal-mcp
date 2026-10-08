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
    anyhow::bail!("Stars mutation did not reach {wanted:?}; last={last:?}")
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
    anyhow::bail!("Stars mutation did not reach error {wanted}; last={last:?}")
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
    let prepared = prepared.structured_content.expect("prepared Stars mutation");
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
    let requested = requested.structured_content.expect("Stars approval request");
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

async fn saved_gifts(
    client: &rmcp::service::RunningService<rmcp::RoleClient, ()>,
    peer: &str,
) -> anyhow::Result<Value> {
    let response = client
        .call_tool(CallToolRequestParams::new("get_saved_star_gifts").with_arguments(
            json!({ "chatId": peer, "limit": 100 }).as_object().expect("object").clone(),
        ))
        .await?;
    assert_eq!(response.is_error, Some(false));
    Ok(response.structured_content.expect("saved Star Gifts"))
}

#[tokio::test]
async fn star_gift_visibility_and_subscription_mutations_are_exact_and_recoverable()
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
    for (name, destructive) in [
        ("save_star_gift", false),
        ("convert_star_gift", true),
        ("change_stars_subscription", false),
    ] {
        let tool = tools.iter().find(|tool| tool.name == name).expect("Stars mutation tool");
        assert_eq!(tool.input_schema.get("additionalProperties"), Some(&json!(false)));
        assert!(tool.output_schema.is_some());
        assert_eq!(tool.annotations.as_ref().and_then(|value| value.read_only_hint), Some(false));
        assert_eq!(
            tool.annotations.as_ref().and_then(|value| value.destructive_hint),
            Some(destructive)
        );
    }

    for (tool, arguments) in [
        ("save_star_gift", json!({})),
        ("save_star_gift", json!({ "msgId": 31, "chatId": "-100505", "savedId": "80001" })),
        ("save_star_gift", json!({ "chatId": "-100505" })),
        ("convert_star_gift", json!({ "msgId": 0 })),
        ("convert_star_gift", json!({ "chatId": "-100505", "savedId": "080001" })),
        (
            "change_stars_subscription",
            json!({ "chatId": "me", "subscriptionId": "", "canceled": true }),
        ),
    ] {
        assert_eq!(rejected_code(&client, tool, arguments).await?, "INVALID_ARGUMENT");
    }
    let unknown_field = client
        .call_tool(
            CallToolRequestParams::new("save_star_gift").with_arguments(
                json!({ "msgId": 31, "unsave": true, "extra": true })
                    .as_object()
                    .expect("object")
                    .clone(),
            ),
        )
        .await;
    assert!(unknown_field.is_err());
    assert_eq!(
        rejected_code(&client, "save_star_gift", json!({ "msgId": 31 })).await?,
        "NO_CHANGE"
    );
    assert_eq!(
        rejected_code(
            &client,
            "change_stars_subscription",
            json!({ "chatId": "me", "subscriptionId": "sub-fixture", "canceled": false }),
        )
        .await?,
        "NO_CHANGE"
    );
    assert_eq!(
        rejected_code(&client, "convert_star_gift", json!({ "msgId": 33 })).await?,
        "STAR_GIFT_NOT_CONVERTIBLE"
    );

    let (hide_id, hide_message_id, hidden_prepared) =
        prepare_and_request(&client, "save_star_gift", json!({ "msgId": 31, "unsave": true }))
            .await?;
    assert_eq!(
        hidden_prepared.pointer("/data/action/kind").and_then(Value::as_str),
        Some("set_star_gift_saved")
    );
    assert_eq!(
        hidden_prepared.pointer("/data/action/before/reference/message_id").and_then(Value::as_i64),
        Some(31)
    );
    approve(&bot, &hide_id, hide_message_id).await;
    wait_for_mutation_state(&state_db_path, &hide_id, MutationApprovalState::Applied).await?;
    assert_eq!(backend.star_gift_save_call_count().await, 1);
    let gifts = saved_gifts(&client, "me").await?;
    assert!(
        gifts
            .pointer("/data/gifts")
            .and_then(Value::as_array)
            .and_then(|gifts| gifts
                .iter()
                .find(|gift| gift.pointer("/message_id") == Some(&json!(31))))
            .and_then(|gift| gift.pointer("/unsaved"))
            .and_then(Value::as_bool)
            .is_some_and(|unsaved| unsaved)
    );

    backend.make_next_star_gift_save_outcome_uncertain().await;
    let (show_id, show_message_id, _) =
        prepare_and_request(&client, "save_star_gift", json!({ "msgId": 31, "unsave": false }))
            .await?;
    approve(&bot, &show_id, show_message_id).await;
    wait_for_mutation_state(&state_db_path, &show_id, MutationApprovalState::Applied).await?;
    let recovered = MutationLedger::new(state_db_path.clone())
        .get_approval(&show_id)?
        .expect("recovered saved-state approval");
    assert_eq!(recovered.automated_retry_count, 1);
    assert_eq!(backend.star_gift_save_call_count().await, 2);

    let (channel_id, channel_message_id, channel_prepared) = prepare_and_request(
        &client,
        "save_star_gift",
        json!({ "chatId": "-100505", "savedId": "80001", "unsave": true }),
    )
    .await?;
    assert_eq!(
        channel_prepared.pointer("/data/action/before/reference/chat_id").and_then(Value::as_str),
        Some("-100505")
    );
    approve(&bot, &channel_id, channel_message_id).await;
    wait_for_mutation_state(&state_db_path, &channel_id, MutationApprovalState::Applied).await?;
    assert_eq!(backend.star_gift_save_call_count().await, 3);
    assert_eq!(
        saved_gifts(&client, "-100505")
            .await?
            .pointer("/data/gifts/0/unsaved")
            .and_then(Value::as_bool),
        Some(true)
    );

    let (cancel_id, cancel_message_id, canceled_prepared) = prepare_and_request(
        &client,
        "change_stars_subscription",
        json!({ "chatId": "me", "subscriptionId": "sub-fixture", "canceled": true }),
    )
    .await?;
    assert_eq!(
        canceled_prepared.pointer("/data/action/wallet_peer_ref").and_then(Value::as_str),
        Some("7")
    );
    approve(&bot, &cancel_id, cancel_message_id).await;
    wait_for_mutation_state(&state_db_path, &cancel_id, MutationApprovalState::Applied).await?;
    assert_eq!(backend.stars_subscription_change_call_count().await, 1);

    backend.make_next_stars_subscription_change_outcome_uncertain().await;
    let (restore_id, restore_message_id, _) = prepare_and_request(
        &client,
        "change_stars_subscription",
        json!({ "chatId": "7", "subscriptionId": "sub-fixture", "canceled": false }),
    )
    .await?;
    approve(&bot, &restore_id, restore_message_id).await;
    wait_for_mutation_state(&state_db_path, &restore_id, MutationApprovalState::Applied).await?;
    let recovered = MutationLedger::new(state_db_path.clone())
        .get_approval(&restore_id)?
        .expect("recovered subscription approval");
    assert_eq!(recovered.automated_retry_count, 1);
    assert_eq!(backend.stars_subscription_change_call_count().await, 2);
    let subscriptions = client
        .call_tool(
            CallToolRequestParams::new("get_stars_subscriptions")
                .with_arguments(json!({ "chatId": "me" }).as_object().expect("object").clone()),
        )
        .await?
        .structured_content
        .expect("Stars subscriptions");
    assert_eq!(
        subscriptions.pointer("/data/subscriptions/0/canceled").and_then(Value::as_bool),
        Some(false)
    );

    client.cancel().await?;
    server_task.await??;
    worker.abort();
    let _ = worker.await;
    Ok(())
}

#[tokio::test]
async fn star_gift_conversion_is_read_back_and_never_replayed_after_uncertain_delivery()
-> anyhow::Result<()> {
    for uncertain in [false, true] {
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

        let (approval_id, message_id, prepared) =
            prepare_and_request(&client, "convert_star_gift", json!({ "msgId": 31 })).await?;
        assert_eq!(
            prepared.pointer("/data/action/kind").and_then(Value::as_str),
            Some("convert_star_gift")
        );
        assert_eq!(
            prepared.pointer("/data/action/convert_stars").and_then(Value::as_str),
            Some("42")
        );
        assert_eq!(
            prepared.pointer("/data/action/before_balance/amount").and_then(Value::as_str),
            Some("125")
        );
        let card = bot.sent_mutation_cards().await.last().expect("conversion card").clone();
        assert!(card.rendered_text.contains("НЕОБРАТИМАЯ КОНВЕРТАЦИЯ"));
        assert!(card.rendered_text.contains("безвозвратно удалён"));
        assert!(card.rendered_text.contains("автоматический повтор отключён"));
        assert!(!card.rendered_text.contains("prepaid_upgrade_hash"));
        assert!(!card.rendered_text.contains("owner_address"));
        assert!(!card.rendered_text.contains("access_hash"));

        if uncertain {
            backend.make_next_star_gift_convert_outcome_uncertain().await;
        }
        approve(&bot, &approval_id, message_id).await;
        if uncertain {
            wait_for_mutation_state(
                &state_db_path,
                &approval_id,
                MutationApprovalState::OutcomeUnknown,
            )
            .await?;
            wait_for_error_code(&state_db_path, &approval_id, "MUTATION_OUTCOME_UNKNOWN_NO_RETRY")
                .await?;
            tokio::time::sleep(Duration::from_millis(250)).await;
            let approval = MutationLedger::new(state_db_path.clone())
                .get_approval(&approval_id)?
                .expect("uncertain conversion approval");
            assert_eq!(approval.state, MutationApprovalState::OutcomeUnknown);
            assert_eq!(approval.automated_retry_count, 1);
            assert_eq!(approval.next_retry_at_unix, None);
        } else {
            wait_for_mutation_state(&state_db_path, &approval_id, MutationApprovalState::Applied)
                .await?;
            let approval = MutationLedger::new(state_db_path.clone())
                .get_approval(&approval_id)?
                .expect("applied conversion approval");
            assert_eq!(
                approval
                    .result
                    .as_ref()
                    .and_then(|result| serde_json::to_value(result).ok())
                    .as_ref()
                    .and_then(|result| result.pointer("/wallet_balance/amount"))
                    .and_then(Value::as_str),
                Some("167")
            );
        }
        assert_eq!(backend.star_gift_convert_call_count().await, 1);
        let gifts = saved_gifts(&client, "me").await?;
        assert!(!gifts.pointer("/data/gifts").and_then(Value::as_array).is_some_and(|gifts| {
            gifts.iter().any(|gift| gift.pointer("/message_id") == Some(&json!(31)))
        }));
        let status = client
            .call_tool(
                CallToolRequestParams::new("get_stars_status")
                    .with_arguments(json!({ "peer": "me" }).as_object().expect("object").clone()),
            )
            .await?
            .structured_content
            .expect("Stars status");
        assert_eq!(status.pointer("/data/balance/amount").and_then(Value::as_str), Some("167"));
        assert_eq!(
            status.pointer("/data/balance/nanos").and_then(Value::as_i64),
            Some(500_000_000)
        );

        client.cancel().await?;
        server_task.await??;
        worker.abort();
        let _ = worker.await;
    }
    Ok(())
}
