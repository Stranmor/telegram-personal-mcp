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
use sha2::{Digest, Sha256};
use std::sync::Arc;
use telegram_personal_mcp::{FakeTelegramBackend, McpServer, McpServerSettings};

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
    assert_eq!(rejected.is_error, Some(true), "{tool} must reject invalid input");
    Ok(rejected
        .structured_content
        .as_ref()
        .and_then(|value| value.pointer("/error/code"))
        .and_then(Value::as_str)
        .expect("typed error code")
        .to_string())
}

#[tokio::test]
async fn stars_wallet_reads_are_exact_filterable_paginated_and_schema_closed() -> anyhow::Result<()>
{
    let backend = Arc::new(FakeTelegramBackend::fixture());
    let temp = tempfile::tempdir()?;
    let server = McpServer::with_settings(
        backend,
        McpServerSettings {
            attachment_cache_dir: temp.path().join("attachments"),
            max_download_bytes: 1024 * 1024,
            max_upload_bytes: 1024 * 1024,
            state_db_path: temp.path().join("state.sqlite"),
        },
    );
    let (server_transport, client_transport) = tokio::io::duplex(64 * 1024);
    let server_task = tokio::spawn(async move {
        server.serve(server_transport).await?.waiting().await?;
        anyhow::Ok(())
    });
    let client = ().serve(client_transport).await?;

    let tools = client.list_all_tools().await?;
    for name in [
        "get_stars_status",
        "get_stars_transactions",
        "get_available_star_gifts",
        "get_saved_star_gifts",
        "get_stars_topup_options",
        "get_stars_subscriptions",
    ] {
        let tool = tools.iter().find(|tool| tool.name == name).expect("Stars tool");
        assert_eq!(tool.input_schema.get("additionalProperties"), Some(&json!(false)));
        assert!(tool.output_schema.is_some());
        assert_eq!(tool.annotations.as_ref().and_then(|value| value.read_only_hint), Some(true));
    }

    let status = client
        .call_tool(
            CallToolRequestParams::new("get_stars_status")
                .with_arguments(json!({ "peer": "me" }).as_object().unwrap().clone()),
        )
        .await?;
    assert_eq!(status.is_error, Some(false));
    let status = status.structured_content.expect("Stars status");
    assert_eq!(status.pointer("/data/peer_ref").and_then(Value::as_str), Some("7"));
    assert_eq!(status.pointer("/data/balance/kind").and_then(Value::as_str), Some("stars"));
    assert_eq!(status.pointer("/data/balance/amount").and_then(Value::as_str), Some("125"));
    assert_eq!(status.pointer("/data/balance/nanos").and_then(Value::as_i64), Some(500_000_000));
    assert_eq!(
        status.pointer("/data/subscriptions/0/subscription_id").and_then(Value::as_str),
        Some("sub-fixture")
    );
    assert_eq!(
        status.pointer("/data/subscriptions/0/pricing/amount_stars").and_then(Value::as_str),
        Some("25")
    );
    assert_eq!(
        status.pointer("/data/subscriptions/0/has_chat_invite_hash").and_then(Value::as_bool),
        Some(true)
    );
    assert!(status.pointer("/data/subscriptions/0/chat_invite_hash").is_none());
    assert_eq!(
        status.pointer("/data/subscriptions_missing_balance").and_then(Value::as_str),
        Some("25")
    );
    assert_eq!(status.pointer("/data/history").and_then(Value::as_array).map(Vec::len), Some(3));
    assert_eq!(
        status.pointer("/data/history/0/star_gift/kind").and_then(Value::as_str),
        Some("regular")
    );
    assert_eq!(
        status.pointer("/data/history/0/star_gift/gift_id").and_then(Value::as_str),
        Some("9001")
    );
    assert_eq!(
        status.pointer("/data/history/1/transaction_url").and_then(Value::as_str),
        Some("https://example.invalid/untrusted-receipt")
    );
    assert_eq!(
        status.pointer("/data/history/1/bot_payload_length").and_then(Value::as_u64),
        Some(15)
    );
    assert_eq!(
        status.pointer("/data/history/1/bot_payload_sha256").and_then(Value::as_str),
        Some(format!("{:x}", Sha256::digest(b"fixture-payload")).as_str())
    );
    assert!(status.pointer("/data/history/1/bot_payload").is_none());
    assert!(
        status
            .pointer("/data/related_users")
            .and_then(Value::as_array)
            .is_some_and(|users| users.len() >= 2)
    );
    assert!(
        status
            .pointer("/data/related_chats")
            .and_then(Value::as_array)
            .is_some_and(|chats| !chats.is_empty())
    );

    let channel = client
        .call_tool(
            CallToolRequestParams::new("get_stars_status")
                .with_arguments(json!({ "chatId": "-100505" }).as_object().unwrap().clone()),
        )
        .await?;
    assert_eq!(channel.is_error, Some(false));
    assert_eq!(
        channel
            .structured_content
            .as_ref()
            .and_then(|value| value.pointer("/data/balance/amount"))
            .and_then(Value::as_str),
        Some("500")
    );

    let newest_first = client
        .call_tool(
            CallToolRequestParams::new("get_stars_transactions")
                .with_arguments(json!({ "peer": "@me" }).as_object().unwrap().clone()),
        )
        .await?;
    assert_eq!(newest_first.is_error, Some(false));
    assert_eq!(
        newest_first
            .structured_content
            .as_ref()
            .and_then(|value| value.pointer("/data/history/0/transaction_id"))
            .and_then(Value::as_str),
        Some("stars-subscription")
    );

    let inbound = client
        .call_tool(CallToolRequestParams::new("get_stars_transactions").with_arguments(
            json!({ "peer": "self", "inbound": true }).as_object().unwrap().clone(),
        ))
        .await?;
    assert_eq!(inbound.is_error, Some(false));
    assert_eq!(
        inbound
            .structured_content
            .as_ref()
            .and_then(|value| value.pointer("/data/history"))
            .and_then(Value::as_array)
            .map(Vec::len),
        Some(1)
    );
    assert_eq!(
        inbound
            .structured_content
            .as_ref()
            .and_then(|value| value.pointer("/data/history/0/amount/amount"))
            .and_then(Value::as_str),
        Some("50")
    );

    let outbound = client
        .call_tool(CallToolRequestParams::new("get_stars_transactions").with_arguments(
            json!({ "peer_ref": "7", "outbound": true }).as_object().unwrap().clone(),
        ))
        .await?;
    assert_eq!(outbound.is_error, Some(false));
    assert_eq!(
        outbound
            .structured_content
            .as_ref()
            .and_then(|value| value.pointer("/data/history"))
            .and_then(Value::as_array)
            .map(Vec::len),
        Some(2)
    );

    let subscription = client
        .call_tool(CallToolRequestParams::new("get_stars_transactions").with_arguments(
            json!({ "peer": "me", "subscriptionId": "sub-fixture" }).as_object().unwrap().clone(),
        ))
        .await?;
    assert_eq!(subscription.is_error, Some(false));
    assert_eq!(
        subscription
            .structured_content
            .as_ref()
            .and_then(|value| value.pointer("/data/history/0/transaction_id"))
            .and_then(Value::as_str),
        Some("stars-subscription")
    );

    let first_page = client
        .call_tool(CallToolRequestParams::new("get_stars_transactions").with_arguments(
            json!({ "peer": "me", "ascending": true, "limit": 1 }).as_object().unwrap().clone(),
        ))
        .await?;
    assert_eq!(first_page.is_error, Some(false));
    assert_eq!(
        first_page
            .structured_content
            .as_ref()
            .and_then(|value| value.pointer("/data/history/0/transaction_id"))
            .and_then(Value::as_str),
        Some("stars-in-gift")
    );
    assert_eq!(
        first_page
            .structured_content
            .as_ref()
            .and_then(|value| value.pointer("/data/next_offset"))
            .and_then(Value::as_str),
        Some("1")
    );
    let second_page = client
        .call_tool(
            CallToolRequestParams::new("get_stars_transactions").with_arguments(
                json!({ "peer": "me", "ascending": true, "offset": "1", "limit": 1 })
                    .as_object()
                    .unwrap()
                    .clone(),
            ),
        )
        .await?;
    assert_eq!(second_page.is_error, Some(false));
    assert_eq!(
        second_page
            .structured_content
            .as_ref()
            .and_then(|value| value.pointer("/data/history/0/transaction_id"))
            .and_then(Value::as_str),
        Some("stars-out-reaction")
    );

    let available = client
        .call_tool(
            CallToolRequestParams::new("get_available_star_gifts")
                .with_arguments(json!({}).as_object().unwrap().clone()),
        )
        .await?;
    assert_eq!(available.is_error, Some(false));
    let available = available.structured_content.expect("available Star Gifts");
    assert_eq!(available.pointer("/data/not_modified").and_then(Value::as_bool), Some(false));
    assert_eq!(available.pointer("/data/hash").and_then(Value::as_i64), Some(9_900));
    assert_eq!(available.pointer("/data/gifts").and_then(Value::as_array).map(Vec::len), Some(3));
    assert_eq!(available.pointer("/data/gifts/0/kind").and_then(Value::as_str), Some("regular"));
    assert_eq!(available.pointer("/data/gifts/0/gift_id").and_then(Value::as_str), Some("9001"));
    assert_eq!(available.pointer("/data/gifts/0/stars").and_then(Value::as_str), Some("50"));
    assert_eq!(available.pointer("/data/gifts/0/limited").and_then(Value::as_bool), Some(true));
    assert_eq!(available.pointer("/data/gifts/2/kind").and_then(Value::as_str), Some("unique"));
    assert_eq!(
        available.pointer("/data/gifts/2/attributes/0/kind").and_then(Value::as_str),
        Some("model")
    );
    assert_eq!(
        available.pointer("/data/gifts/2/has_owner_address").and_then(Value::as_bool),
        Some(true)
    );
    assert!(available.pointer("/data/gifts/2/owner_address").is_none());
    assert!(available.pointer("/data/gifts/2/gift_address").is_none());

    let saved = client
        .call_tool(
            CallToolRequestParams::new("get_saved_star_gifts")
                .with_arguments(json!({ "chatId": "me" }).as_object().unwrap().clone()),
        )
        .await?;
    assert_eq!(saved.is_error, Some(false));
    let saved = saved.structured_content.expect("saved Star Gifts");
    assert_eq!(saved.pointer("/data/peer_ref").and_then(Value::as_str), Some("7"));
    assert_eq!(saved.pointer("/data/count").and_then(Value::as_i64), Some(3));
    assert_eq!(saved.pointer("/data/gifts").and_then(Value::as_array).map(Vec::len), Some(3));
    assert_eq!(saved.pointer("/data/gifts/0/gift/gift_id").and_then(Value::as_str), Some("9002"));
    assert_eq!(saved.pointer("/data/gifts/1/gift/kind").and_then(Value::as_str), Some("unique"));
    assert_eq!(saved.pointer("/data/gifts/2/gift/gift_id").and_then(Value::as_str), Some("9001"));
    assert_eq!(
        saved.pointer("/data/gifts/2/has_prepaid_upgrade_hash").and_then(Value::as_bool),
        Some(true)
    );
    assert_eq!(
        saved.pointer("/data/gifts/2/prepaid_upgrade_hash_length").and_then(Value::as_u64),
        Some(b"fixture-prepaid-upgrade".len() as u64)
    );
    assert_eq!(
        saved.pointer("/data/gifts/2/prepaid_upgrade_hash_sha256").and_then(Value::as_str),
        Some(format!("{:x}", Sha256::digest(b"fixture-prepaid-upgrade")).as_str())
    );
    assert!(saved.pointer("/data/gifts/2/prepaid_upgrade_hash").is_none());

    let saved_first_page = client
        .call_tool(
            CallToolRequestParams::new("get_saved_star_gifts")
                .with_arguments(json!({ "peer": "self", "limit": 1 }).as_object().unwrap().clone()),
        )
        .await?;
    assert_eq!(saved_first_page.is_error, Some(false));
    assert_eq!(
        saved_first_page
            .structured_content
            .as_ref()
            .and_then(|value| value.pointer("/data/next_offset"))
            .and_then(Value::as_str),
        Some("1")
    );
    let saved_second_page = client
        .call_tool(CallToolRequestParams::new("get_saved_star_gifts").with_arguments(
            json!({ "peer_ref": "7", "limit": 1, "offset": "1" }).as_object().unwrap().clone(),
        ))
        .await?;
    assert_eq!(saved_second_page.is_error, Some(false));
    assert_eq!(
        saved_second_page
            .structured_content
            .as_ref()
            .and_then(|value| value.pointer("/data/gifts/0/gift/kind"))
            .and_then(Value::as_str),
        Some("unique")
    );

    for (arguments, expected_count, expected_first_kind) in [
        (json!({ "chatId": "me", "excludeUnsaved": true }), 2, "regular"),
        (json!({ "chatId": "me", "excludeSaved": true }), 1, "unique"),
        (json!({ "chatId": "me", "excludeUnlimited": true }), 2, "unique"),
        (json!({ "chatId": "me", "excludeLimited": true }), 2, "regular"),
        (json!({ "chatId": "me", "excludeUnique": true }), 2, "regular"),
        (json!({ "chatId": "me", "excludeUpgradable": true }), 2, "regular"),
        (json!({ "chatId": "me", "excludeUnupgradable": true }), 1, "regular"),
        (json!({ "chatId": "me", "peerColorAvailable": true }), 2, "unique"),
        (json!({ "chatId": "me", "excludeHosted": true }), 2, "regular"),
        (json!({ "chatId": "me", "collectionId": 7 }), 2, "unique"),
    ] {
        let filtered = client
            .call_tool(
                CallToolRequestParams::new("get_saved_star_gifts")
                    .with_arguments(arguments.as_object().unwrap().clone()),
            )
            .await?;
        assert_eq!(filtered.is_error, Some(false));
        assert_eq!(
            filtered
                .structured_content
                .as_ref()
                .and_then(|value| value.pointer("/data/count"))
                .and_then(Value::as_i64),
            Some(expected_count)
        );
        assert_eq!(
            filtered
                .structured_content
                .as_ref()
                .and_then(|value| value.pointer("/data/gifts/0/gift/kind"))
                .and_then(Value::as_str),
            Some(expected_first_kind)
        );
    }
    let sorted_by_value = client
        .call_tool(CallToolRequestParams::new("get_saved_star_gifts").with_arguments(
            json!({ "chatId": "me", "sortByValue": true }).as_object().unwrap().clone(),
        ))
        .await?;
    assert_eq!(sorted_by_value.is_error, Some(false));
    assert_eq!(
        sorted_by_value
            .structured_content
            .as_ref()
            .and_then(|value| value.pointer("/data/gifts/0/gift/kind"))
            .and_then(Value::as_str),
        Some("unique")
    );

    let topup = client
        .call_tool(
            CallToolRequestParams::new("get_stars_topup_options")
                .with_arguments(json!({}).as_object().unwrap().clone()),
        )
        .await?;
    assert_eq!(topup.is_error, Some(false));
    let topup = topup.structured_content.expect("Stars top-up options");
    assert_eq!(topup.pointer("/data/options").and_then(Value::as_array).map(Vec::len), Some(2));
    assert_eq!(topup.pointer("/data/options/0/stars").and_then(Value::as_str), Some("100"));
    assert_eq!(topup.pointer("/data/options/0/currency").and_then(Value::as_str), Some("USD"));
    assert_eq!(topup.pointer("/data/options/0/amount").and_then(Value::as_str), Some("199"));
    assert_eq!(topup.pointer("/data/options/1/extended").and_then(Value::as_bool), Some(true));

    let subscriptions = client
        .call_tool(
            CallToolRequestParams::new("get_stars_subscriptions")
                .with_arguments(json!({ "chatId": "@me" }).as_object().unwrap().clone()),
        )
        .await?;
    assert_eq!(subscriptions.is_error, Some(false));
    let subscriptions = subscriptions.structured_content.expect("Stars subscriptions");
    assert_eq!(subscriptions.pointer("/data/peer_ref").and_then(Value::as_str), Some("7"));
    assert_eq!(
        subscriptions.pointer("/data/subscriptions/0/subscription_id").and_then(Value::as_str),
        Some("sub-fixture")
    );
    assert_eq!(subscriptions.pointer("/data/next_offset").and_then(Value::as_str), Some("1"));
    assert!(subscriptions.pointer("/data/subscriptions/0/chat_invite_hash").is_none());
    let subscriptions_second =
        client
            .call_tool(CallToolRequestParams::new("get_stars_subscriptions").with_arguments(
                json!({ "peer": "me", "offset": "1" }).as_object().unwrap().clone(),
            ))
            .await?;
    assert_eq!(subscriptions_second.is_error, Some(false));
    assert_eq!(
        subscriptions_second
            .structured_content
            .as_ref()
            .and_then(|value| value.pointer("/data/subscriptions/0/subscription_id"))
            .and_then(Value::as_str),
        Some("sub-archive")
    );
    let missing_subscriptions = client
        .call_tool(CallToolRequestParams::new("get_stars_subscriptions").with_arguments(
            json!({ "peer": "me", "missingBalance": true }).as_object().unwrap().clone(),
        ))
        .await?;
    assert_eq!(missing_subscriptions.is_error, Some(false));
    assert_eq!(
        missing_subscriptions
            .structured_content
            .as_ref()
            .and_then(|value| value.pointer("/data/subscriptions"))
            .and_then(Value::as_array)
            .map(Vec::len),
        Some(1)
    );
    assert!(
        missing_subscriptions
            .structured_content
            .as_ref()
            .and_then(|value| value.pointer("/data/next_offset"))
            .is_some_and(Value::is_null)
    );

    assert_eq!(
        rejected_code(&client, "get_stars_status", json!({ "peer": "@unknown" })).await?,
        "PEER_NOT_FOUND"
    );
    assert_eq!(
        rejected_code(&client, "get_saved_star_gifts", json!({ "chatId": "@unknown" })).await?,
        "PEER_NOT_FOUND"
    );
    assert_eq!(
        rejected_code(&client, "get_stars_subscriptions", json!({ "chatId": "@unknown" })).await?,
        "PEER_NOT_FOUND"
    );
    for (tool, arguments) in [
        ("get_stars_status", json!({ "peer": "" })),
        ("get_stars_status", json!({ "peer": "x".repeat(129) })),
        ("get_stars_transactions", json!({ "peer": "me", "inbound": true, "outbound": true })),
        ("get_stars_transactions", json!({ "peer": "me", "limit": 0 })),
        ("get_stars_transactions", json!({ "peer": "me", "limit": 101 })),
        ("get_stars_transactions", json!({ "peer": "me", "subscriptionId": "" })),
        ("get_stars_transactions", json!({ "peer": "me", "subscriptionId": "x".repeat(513) })),
        ("get_stars_transactions", json!({ "peer": "me", "offset": "x".repeat(513) })),
        ("get_saved_star_gifts", json!({ "chatId": "" })),
        ("get_saved_star_gifts", json!({ "chatId": "me", "limit": 0 })),
        ("get_saved_star_gifts", json!({ "chatId": "me", "limit": 101 })),
        ("get_saved_star_gifts", json!({ "chatId": "me", "offset": "x".repeat(513) })),
        ("get_saved_star_gifts", json!({ "chatId": "me", "collectionId": 0 })),
        ("get_stars_subscriptions", json!({ "chatId": "" })),
        ("get_stars_subscriptions", json!({ "chatId": "me", "offset": "x".repeat(513) })),
    ] {
        assert_eq!(rejected_code(&client, tool, arguments).await?, "INVALID_ARGUMENT");
    }

    for (tool, arguments) in [
        ("get_stars_status", json!({ "peer": "me", "extra": true })),
        ("get_stars_transactions", json!({ "peer": "me", "extra": true })),
        ("get_available_star_gifts", json!({ "extra": true })),
        ("get_saved_star_gifts", json!({ "chatId": "me", "extra": true })),
        ("get_stars_topup_options", json!({ "extra": true })),
        ("get_stars_subscriptions", json!({ "chatId": "me", "extra": true })),
    ] {
        let rejected = client
            .call_tool(
                CallToolRequestParams::new(tool)
                    .with_arguments(arguments.as_object().expect("object").clone()),
            )
            .await;
        assert!(rejected.is_err(), "{tool} must reject unknown fields at the JSON-RPC boundary");
    }

    client.cancel().await?;
    server_task.await??;
    Ok(())
}
