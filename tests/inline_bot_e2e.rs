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
    arguments: Value,
) -> anyhow::Result<(String, i64, Value, Value)> {
    let prepared = client
        .call_tool(
            CallToolRequestParams::new("send_inline_bot_result")
                .with_arguments(arguments.as_object().expect("object").clone()),
        )
        .await?;
    assert_eq!(prepared.is_error, Some(false));
    let prepared = prepared.structured_content.expect("prepared inline result");
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
    let approval = requested.structured_content.expect("inline approval");
    let approval_id = approval
        .pointer("/data/approval_id")
        .and_then(Value::as_str)
        .expect("approval id")
        .to_string();
    let approval_message_id = approval
        .pointer("/data/approval_message_id")
        .and_then(Value::as_i64)
        .expect("approval message id");
    Ok((approval_id, approval_message_id, prepared, approval))
}

#[tokio::test]
async fn inline_bot_query_and_send_are_typed_short_lived_and_idempotently_recoverable()
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
            CallToolRequestParams::new("get_inline_bot_results").with_arguments(
                json!({
                    "bot": "@fixture_inline_bot",
                    "query": "",
                    "openResultUrls": true
                })
                .as_object()
                .expect("object")
                .clone(),
            ),
        )
        .await;
    assert!(closed_schema.is_err(), "unknown fields must fail at the JSON-RPC boundary");

    assert_eq!(
        rejected_code(
            &client,
            "get_inline_bot_results",
            json!({ "bot": "@alex", "chatId": "@alex", "query": "weather" })
        )
        .await?,
        "INVALID_PEER"
    );

    let empty_query = client
        .call_tool(
            CallToolRequestParams::new("get_inline_bot_results").with_arguments(
                json!({ "bot": "@fixture_inline_bot", "query": "" })
                    .as_object()
                    .expect("object")
                    .clone(),
            ),
        )
        .await?;
    assert_eq!(empty_query.is_error, Some(false));
    let empty_query = empty_query.structured_content.expect("empty inline query page");
    assert_eq!(empty_query.pointer("/data/bot/user_id").and_then(Value::as_i64), Some(909));
    assert_eq!(empty_query.pointer("/data/context_chat_id").and_then(Value::as_str), Some("7"));
    assert_eq!(empty_query.pointer("/data/query").and_then(Value::as_str), Some(""));
    assert_eq!(empty_query.pointer("/data/offset").and_then(Value::as_str), Some(""));
    assert_eq!(empty_query.pointer("/data/next_offset").and_then(Value::as_str), Some("next:1"));
    assert_eq!(empty_query.pointer("/data/cache_time_seconds").and_then(Value::as_i64), Some(300));
    assert_eq!(empty_query.pointer("/data/gallery").and_then(Value::as_bool), Some(true));
    assert_eq!(
        empty_query.pointer("/data/switch_pm/start_param").and_then(Value::as_str),
        Some("inline_fixture")
    );
    assert_eq!(
        empty_query.pointer("/data/results/0/type").and_then(Value::as_str),
        Some("article")
    );
    assert_eq!(
        empty_query.pointer("/data/results/0/source_kind").and_then(Value::as_str),
        Some("result")
    );
    assert_eq!(
        empty_query.pointer("/data/results/0/has_thumbnail").and_then(Value::as_bool),
        Some(true)
    );
    assert_eq!(
        empty_query.pointer("/data/results/1/media_kind").and_then(Value::as_str),
        Some("photo")
    );
    assert!(empty_query.pointer("/data/results/0/send_message_geo").is_some_and(Value::is_null));
    let empty_query_id =
        empty_query.pointer("/data/query_id").and_then(Value::as_str).expect("query id");
    assert_eq!(empty_query_id.parse::<i64>()?.to_string(), empty_query_id);
    let observed_at = empty_query
        .pointer("/data/observed_at_unix")
        .and_then(Value::as_i64)
        .expect("observed time");
    let expires_at =
        empty_query.pointer("/data/expires_at_unix").and_then(Value::as_i64).expect("expiry time");
    assert_eq!(expires_at - observed_at, 55);

    let paged = client
        .call_tool(
            CallToolRequestParams::new("get_inline_bot_results").with_arguments(
                json!({
                    "bot": "@fixture_inline_bot",
                    "chatId": "@alex",
                    "query": "weather",
                    "offset": "next:1"
                })
                .as_object()
                .expect("object")
                .clone(),
            ),
        )
        .await?;
    assert_eq!(paged.is_error, Some(false));
    let paged = paged.structured_content.expect("paged inline query");
    assert_eq!(paged.pointer("/data/context_chat_id").and_then(Value::as_str), Some("101"));
    assert_eq!(paged.pointer("/data/offset").and_then(Value::as_str), Some("next:1"));
    assert!(paged.pointer("/data/next_offset").is_some_and(Value::is_null));
    let query_id = paged
        .pointer("/data/query_id")
        .and_then(Value::as_str)
        .expect("paged query id")
        .to_string();
    let query_expires_at =
        paged.pointer("/data/expires_at_unix").and_then(Value::as_i64).expect("query expiry");
    assert_eq!(backend.inline_query_call_count().await, 2);

    assert_eq!(
        rejected_code(
            &client,
            "send_inline_bot_result",
            json!({
                "chatId": "@alex",
                "queryId": "1",
                "resultId": "article:0"
            })
        )
        .await?,
        "INLINE_QUERY_NOT_FOUND"
    );
    assert_eq!(
        rejected_code(
            &client,
            "send_inline_bot_result",
            json!({
                "chatId": "@alex",
                "queryId": query_id.clone(),
                "resultId": "missing"
            })
        )
        .await?,
        "INLINE_RESULT_NOT_FOUND"
    );

    backend
        .set_fixture_draft_summary(
            "@alex",
            Some(DraftSummary {
                message: "Rich draft cannot be erased through a lossy snapshot".to_string(),
                date_unix: 1_700_009_998,
                no_webpage: false,
                invert_media: false,
                has_reply_to: false,
                has_entities: true,
                has_media: false,
                effect_id: None,
                has_suggested_post: false,
                has_rich_message: false,
                state_sha256: "a".repeat(64),
            }),
        )
        .await;
    assert_eq!(
        rejected_code(
            &client,
            "send_inline_bot_result",
            json!({
                "chatId": "@alex",
                "queryId": query_id.clone(),
                "resultId": "article:0",
                "clearDraft": true
            })
        )
        .await?,
        "DRAFT_SNAPSHOT_INCOMPLETE"
    );
    assert_eq!(backend.inline_result_send_call_count().await, 0);
    backend.set_fixture_draft("@alex", Some("Review the contract before replying")).await;

    let (approval_id, approval_message_id, prepared, approval) = prepare_and_request(
        &client,
        json!({
            "chatId": "@alex",
            "queryId": query_id.clone(),
            "resultId": "article:0",
            "replyTo": 10,
            "silent": true,
            "hideVia": true,
            "clearDraft": true
        }),
    )
    .await?;
    assert_eq!(
        prepared.pointer("/data/action/kind").and_then(Value::as_str),
        Some("send_inline_bot_result")
    );
    assert_eq!(
        prepared.pointer("/data/action/query/bot/user_id").and_then(Value::as_i64),
        Some(909)
    );
    assert_eq!(
        prepared.pointer("/data/action/query/context_chat_id").and_then(Value::as_str),
        Some("101")
    );
    assert_eq!(
        prepared.pointer("/data/action/target_chat_id").and_then(Value::as_str),
        Some("101")
    );
    assert_eq!(
        prepared.pointer("/data/action/result/id").and_then(Value::as_str),
        Some("article:0")
    );
    assert_eq!(
        prepared.pointer("/data/action/reply_to/message_id").and_then(Value::as_i64),
        Some(10)
    );
    assert_eq!(
        prepared.pointer("/data/action/clear_draft_before/message").and_then(Value::as_str),
        Some("Review the contract before replying")
    );
    assert_eq!(prepared.pointer("/data/action/silent").and_then(Value::as_bool), Some(true));
    assert_eq!(prepared.pointer("/data/action/hide_via").and_then(Value::as_bool), Some(true));
    assert_eq!(prepared.pointer("/data/action/clear_draft").and_then(Value::as_bool), Some(true));
    assert!(
        prepared.pointer("/data/action/random_id").and_then(Value::as_i64).is_some_and(|id| id > 0)
    );
    assert_eq!(backend.inline_result_send_call_count().await, 0);
    let approval_expires_at =
        approval.pointer("/data/expires_at_unix").and_then(Value::as_i64).expect("approval expiry");
    assert!(approval_expires_at <= query_expires_at);
    let card_debug = format!("{:?}", bot.sent_mutation_cards().await.last());
    assert!(card_debug.contains("article:0"));
    assert!(card_debug.contains("fixture_inline_bot"));
    assert!(card_debug.contains("https://example.com/inline/article"));

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
        .expect("applied inline approval");
    let result = serde_json::to_value(applied.result.expect("inline result receipt"))?;
    assert_eq!(result.pointer("/kind").and_then(Value::as_str), Some("inline_bot_result_sent"));
    assert_eq!(result.pointer("/chat_id").and_then(Value::as_str), Some("101"));
    assert_eq!(result.pointer("/query_id").and_then(Value::as_str), Some(query_id.as_str()));
    assert_eq!(result.pointer("/result/id").and_then(Value::as_str), Some("article:0"));
    let message_id =
        result.pointer("/message_id").and_then(Value::as_i64).expect("sent message id");
    assert_eq!(backend.inline_result_send_call_count().await, 1);
    let sends = backend.sent_inline_results().await;
    assert_eq!(sends.len(), 1);
    assert_eq!(sends[0].chat_id, "101");
    assert_eq!(sends[0].query_id, query_id);
    assert_eq!(sends[0].result_id, "article:0");
    assert_eq!(sends[0].reply_to_message_id, Some(10));
    assert!(sends[0].silent);
    assert!(sends[0].hide_via);
    assert!(sends[0].clear_draft);

    let readback = client
        .call_tool(
            CallToolRequestParams::new("get_message").with_arguments(
                json!({ "chat_id": "@alex", "message_id": message_id })
                    .as_object()
                    .expect("object")
                    .clone(),
            ),
        )
        .await?;
    assert_eq!(readback.is_error, Some(false));
    let readback = readback.structured_content.expect("message readback");
    assert_eq!(readback.pointer("/data/message/outgoing").and_then(Value::as_bool), Some(true));
    assert_eq!(readback.pointer("/data/message/silent").and_then(Value::as_bool), Some(true));
    assert_eq!(
        readback.pointer("/data/message/reply_to_message_id").and_then(Value::as_i64),
        Some(10)
    );
    let drafts = client
        .call_tool(
            CallToolRequestParams::new("list_drafts")
                .with_arguments(json!({ "limit": 10 }).as_object().expect("object").clone()),
        )
        .await?;
    assert_eq!(drafts.is_error, Some(false));
    assert_eq!(
        drafts
            .structured_content
            .as_ref()
            .and_then(|value| value.pointer("/data/drafts"))
            .and_then(Value::as_array)
            .map(Vec::len),
        Some(0)
    );

    backend.set_fixture_draft("@alex", Some("Approved draft snapshot")).await;
    let guarded_query = client
        .call_tool(
            CallToolRequestParams::new("get_inline_bot_results").with_arguments(
                json!({
                    "bot": "@fixture_inline_bot",
                    "chatId": "@alex",
                    "query": "draft guard"
                })
                .as_object()
                .expect("object")
                .clone(),
            ),
        )
        .await?;
    let guarded_query = guarded_query.structured_content.expect("guarded query");
    let guarded_query_id = guarded_query
        .pointer("/data/query_id")
        .and_then(Value::as_str)
        .expect("guarded query id")
        .to_string();
    let (guarded_approval_id, guarded_message_id, guarded_prepared, _) = prepare_and_request(
        &client,
        json!({
            "chatId": "@alex",
            "queryId": guarded_query_id,
            "resultId": "article:0",
            "clearDraft": true
        }),
    )
    .await?;
    assert_eq!(
        guarded_prepared.pointer("/data/action/clear_draft_before/message").and_then(Value::as_str),
        Some("Approved draft snapshot")
    );
    backend.set_fixture_draft("@alex", Some("Changed after approval preparation")).await;
    bot.push_mutation_decision(
        &guarded_approval_id,
        ApprovalAction::Approve,
        700,
        700,
        guarded_message_id,
    )
    .await;
    wait_for_mutation_state(&state_db_path, &guarded_approval_id, MutationApprovalState::Failed)
        .await?;
    let guarded_failure = MutationLedger::new(state_db_path.clone())
        .get_approval(&guarded_approval_id)?
        .expect("guarded approval");
    assert_eq!(guarded_failure.last_error_code.as_deref(), Some("MUTATION_PRECONDITION_FAILED"));
    assert_eq!(backend.inline_result_send_call_count().await, 1);

    let recovery_query = client
        .call_tool(
            CallToolRequestParams::new("get_inline_bot_results").with_arguments(
                json!({
                    "bot": "@fixture_inline_bot",
                    "chatId": "@alex",
                    "query": "recovery"
                })
                .as_object()
                .expect("object")
                .clone(),
            ),
        )
        .await?;
    let recovery_query = recovery_query.structured_content.expect("recovery query");
    let recovery_query_id = recovery_query
        .pointer("/data/query_id")
        .and_then(Value::as_str)
        .expect("recovery query id")
        .to_string();
    backend.make_next_inline_result_send_outcome_uncertain().await;
    let (recovery_approval_id, recovery_message_id, _, _) = prepare_and_request(
        &client,
        json!({
            "chatId": "@alex",
            "queryId": recovery_query_id,
            "resultId": "photo:1",
            "replyTo": 10,
            "hideVia": true
        }),
    )
    .await?;
    bot.push_mutation_decision(
        &recovery_approval_id,
        ApprovalAction::Approve,
        700,
        700,
        recovery_message_id,
    )
    .await;
    wait_for_mutation_state(&state_db_path, &recovery_approval_id, MutationApprovalState::Applied)
        .await?;
    let recovered = MutationLedger::new(state_db_path.clone())
        .get_approval(&recovery_approval_id)?
        .expect("recovered inline approval");
    assert_eq!(recovered.automated_retry_count, 1);
    assert_eq!(backend.inline_result_send_call_count().await, 2);
    assert_eq!(backend.sent_inline_results().await.len(), 2);

    client.cancel().await?;
    server_task.await??;
    worker.abort();
    Ok(())
}
