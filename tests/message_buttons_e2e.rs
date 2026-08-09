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
    ApprovalAction, ApprovalPolicy, FakeApprovalBot, FakeTelegramBackend, McpServer,
    McpServerSettings, MutationApprovalState, MutationLedger,
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

async fn prepare_and_request(
    client: &rmcp::service::RunningService<rmcp::RoleClient, ()>,
    arguments: Value,
) -> anyhow::Result<(String, i64, Value)> {
    let prepared = client
        .call_tool(
            CallToolRequestParams::new("press_button")
                .with_arguments(arguments.as_object().expect("object").clone()),
        )
        .await?;
    assert_eq!(prepared.is_error, Some(false));
    let prepared = prepared.structured_content.expect("prepared callback button");
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
    let approval = requested.structured_content.expect("callback approval");
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

async fn rejected_code(
    client: &rmcp::service::RunningService<rmcp::RoleClient, ()>,
    arguments: Value,
) -> anyhow::Result<String> {
    let rejected = client
        .call_tool(
            CallToolRequestParams::new("press_button")
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

#[tokio::test]
async fn message_buttons_are_typed_approval_bound_and_never_blindly_replayed() -> anyhow::Result<()>
{
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

    let closed_schema = client
        .call_tool(
            CallToolRequestParams::new("press_button").with_arguments(
                json!({
                    "chatId": "@alex",
                    "messageId": 11,
                    "row": 0,
                    "column": 0,
                    "openUrl": true
                })
                .as_object()
                .expect("object")
                .clone(),
            ),
        )
        .await;
    assert!(closed_schema.is_err(), "unknown fields must fail at the JSON-RPC schema boundary");

    let exact = client
        .call_tool(CallToolRequestParams::new("get_message_buttons").with_arguments(
            json!({ "chatId": "@alex", "messageId": 11 }).as_object().expect("object").clone(),
        ))
        .await?;
    assert_eq!(exact.is_error, Some(false));
    let exact = exact.structured_content.expect("button inventory");
    assert_eq!(exact.pointer("/data/chat_id").and_then(Value::as_str), Some("101"));
    assert_eq!(exact.pointer("/data/message_id").and_then(Value::as_i64), Some(11));
    assert_eq!(exact.pointer("/data/markup_type").and_then(Value::as_str), Some("inline_keyboard"));
    assert_eq!(exact.pointer("/data/buttons").and_then(Value::as_array).map(Vec::len), Some(6));
    assert_eq!(exact.pointer("/data/buttons/0/kind").and_then(Value::as_str), Some("callback"));
    assert_eq!(exact.pointer("/data/buttons/0/label").and_then(Value::as_str), Some("Approve"));
    assert_eq!(
        exact.pointer("/data/buttons/1/url").and_then(Value::as_str),
        Some("https://example.com/terms")
    );
    assert_eq!(
        exact.pointer("/data/buttons/2/requires_password").and_then(Value::as_bool),
        Some(true)
    );
    assert_eq!(
        exact.pointer("/data/buttons/3/switch_query").and_then(Value::as_str),
        Some("contract 11")
    );
    assert_eq!(exact.pointer("/data/buttons/4/user_id").and_then(Value::as_str), Some("101"));
    assert_eq!(
        exact.pointer("/data/buttons/5/copy_text").and_then(Value::as_str),
        Some("CONTRACT-11")
    );
    let callback_data = exact
        .pointer("/data/buttons/0/callback_data_base64")
        .and_then(Value::as_str)
        .expect("callback base64")
        .to_string();
    let callback_sha256 = exact
        .pointer("/data/buttons/0/callback_data_sha256")
        .and_then(Value::as_str)
        .expect("callback hash")
        .to_string();

    let no_markup = client
        .call_tool(CallToolRequestParams::new("get_message_buttons").with_arguments(
            json!({ "chatId": "@alex", "messageId": 10 }).as_object().expect("object").clone(),
        ))
        .await?;
    assert_eq!(no_markup.is_error, Some(false));
    let no_markup = no_markup.structured_content.expect("no markup inventory");
    assert_eq!(no_markup.pointer("/data/markup_type").and_then(Value::as_str), Some("none"));
    assert_eq!(no_markup.pointer("/data/buttons").and_then(Value::as_array).map(Vec::len), Some(0));

    let scanned = client
        .call_tool(CallToolRequestParams::new("get_message_buttons").with_arguments(
            json!({ "chatId": "@alex", "limit": 3 }).as_object().expect("object").clone(),
        ))
        .await?;
    assert_eq!(scanned.is_error, Some(false));
    assert_eq!(
        scanned
            .structured_content
            .as_ref()
            .and_then(|value| value.pointer("/data/message_id"))
            .and_then(Value::as_i64),
        Some(11)
    );

    assert_eq!(
        rejected_code(&client, json!({ "chatId": "@alex", "messageId": 11, "row": 0 })).await?,
        "INVALID_ARGUMENT"
    );
    assert_eq!(
        rejected_code(
            &client,
            json!({ "chatId": "@alex", "messageId": 11, "row": 0, "column": 1 })
        )
        .await?,
        "BUTTON_NOT_CALLABLE"
    );
    assert_eq!(
        rejected_code(
            &client,
            json!({ "chatId": "@alex", "messageId": 11, "row": 1, "column": 0 })
        )
        .await?,
        "BUTTON_REQUIRES_PASSWORD"
    );
    assert_eq!(
        rejected_code(
            &client,
            json!({ "chatId": "@alex", "messageId": 11, "data": "bm90LXByZXNlbnQ=" })
        )
        .await?,
        "BUTTON_NOT_FOUND"
    );
    assert_eq!(
        rejected_code(
            &client,
            json!({
                "chatId": "@alex",
                "messageId": 11,
                "row": 0,
                "column": 0,
                "buttonIndex": 0
            })
        )
        .await?,
        "INVALID_ARGUMENT"
    );

    let selectors = [
        json!({ "chatId": "@alex", "messageId": 11, "row": 0, "column": 0 }),
        json!({ "chatId": "@alex", "messageId": 11, "buttonIndex": 0 }),
        json!({ "chatId": "@alex", "buttonText": "approve", "limit": 3 }),
        json!({ "chatId": "@alex", "messageId": 11, "data": callback_data }),
    ];
    for (index, selector) in selectors.into_iter().enumerate() {
        let (approval_id, approval_message_id, prepared) =
            prepare_and_request(&client, selector).await?;
        assert_eq!(
            prepared.pointer("/data/action/kind").and_then(Value::as_str),
            Some("press_callback_button")
        );
        assert_eq!(
            prepared.pointer("/data/action/button/chat_id").and_then(Value::as_str),
            Some("101")
        );
        assert_eq!(
            prepared.pointer("/data/action/button/message_id").and_then(Value::as_i64),
            Some(11)
        );
        assert_eq!(prepared.pointer("/data/action/button/row").and_then(Value::as_u64), Some(0));
        assert_eq!(prepared.pointer("/data/action/button/column").and_then(Value::as_u64), Some(0));
        assert_eq!(
            prepared.pointer("/data/action/button/callback_data_sha256").and_then(Value::as_str),
            Some(callback_sha256.as_str())
        );
        let serialized = serde_json::to_string(&prepared)?;
        assert!(!serialized.contains("callback_data_base64"));
        assert!(!serialized.contains(&callback_data));
        assert_eq!(backend.callback_button_press_call_count().await, index);

        let card_debug = format!("{:?}", bot.sent_mutation_cards().await.last());
        assert!(!card_debug.contains(&callback_data));
        assert!(card_debug.contains(&callback_sha256));

        bot.push_mutation_decision(
            &approval_id,
            ApprovalAction::Approve,
            700,
            700,
            approval_message_id,
        )
        .await;
        wait_for_mutation_state(&state_db_path, &approval_id, MutationApprovalState::Applied)
            .await?;
        let applied = MutationLedger::new(state_db_path.clone())
            .get_approval(&approval_id)?
            .expect("applied callback approval");
        let result = serde_json::to_value(applied.result.expect("callback result"))?;
        assert_eq!(
            result.pointer("/kind").and_then(Value::as_str),
            Some("callback_button_pressed")
        );
        assert_eq!(result.pointer("/answer/alert").and_then(Value::as_bool), Some(true));
        assert_eq!(result.pointer("/answer/has_url").and_then(Value::as_bool), Some(true));
        assert_eq!(
            result.pointer("/answer/message").and_then(Value::as_str),
            Some("Contract approval recorded")
        );
        assert_eq!(result.pointer("/answer/cache_time_seconds").and_then(Value::as_i64), Some(30));
    }
    assert_eq!(backend.callback_button_press_call_count().await, 4);
    let pressed = backend.pressed_callback_buttons().await;
    assert_eq!(pressed.len(), 4);
    assert!(pressed.windows(2).all(|pair| pair[0] == pair[1]));

    backend.make_next_callback_button_press_outcome_uncertain().await;
    let (approval_id, approval_message_id, _) = prepare_and_request(
        &client,
        json!({ "chatId": "@alex", "messageId": 11, "row": 0, "column": 0 }),
    )
    .await?;
    bot.push_mutation_decision(
        &approval_id,
        ApprovalAction::Approve,
        700,
        700,
        approval_message_id,
    )
    .await;
    wait_for_mutation_state(&state_db_path, &approval_id, MutationApprovalState::OutcomeUnknown)
        .await?;
    assert_eq!(backend.callback_button_press_call_count().await, 5);
    tokio::time::sleep(Duration::from_millis(1_250)).await;
    assert_eq!(backend.callback_button_press_call_count().await, 5);
    let uncertain = MutationLedger::new(state_db_path.clone())
        .get_approval(&approval_id)?
        .expect("uncertain callback approval");
    assert_eq!(uncertain.state, MutationApprovalState::OutcomeUnknown);
    assert_eq!(uncertain.automated_retry_count, 1);
    assert_eq!(uncertain.next_retry_at_unix, None);
    assert_eq!(uncertain.last_error_code.as_deref(), Some("MUTATION_OUTCOME_UNKNOWN_NO_RETRY"));

    client.cancel().await?;
    server_task.await??;
    worker.abort();
    Ok(())
}
