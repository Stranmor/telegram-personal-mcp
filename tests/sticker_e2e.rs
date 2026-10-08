use rmcp::{ServiceExt, model::CallToolRequestParams};
use serde_json::{Value, json};
use std::sync::Arc;
use std::time::Duration;
use telegram_personal_mcp::{
    ApprovalAction, ApprovalPolicy, FakeApprovalBot, FakeTelegramBackend, McpServer,
    McpServerSettings, MutationApprovalState, MutationLedger,
};

async fn wait_for_applied(
    state_db_path: &std::path::Path,
    approval_id: &str,
) -> anyhow::Result<()> {
    let ledger = MutationLedger::new(state_db_path.to_path_buf());
    let mut last = None;
    for _ in 0..900 {
        let approval = ledger.get_approval(approval_id)?;
        if approval
            .as_ref()
            .is_some_and(|approval| approval.state == MutationApprovalState::Applied)
        {
            return Ok(());
        }
        last = approval.map(|approval| {
            (approval.state, approval.last_error_code, approval.automated_retry_count)
        });
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    anyhow::bail!("sticker mutation did not become applied; last={last:?}")
}

async fn prepare_and_approve(
    client: &rmcp::service::RunningService<rmcp::RoleClient, ()>,
    bot: &FakeApprovalBot,
    state_db_path: &std::path::Path,
    sticker_handle: &str,
) -> anyhow::Result<(String, i64, Value)> {
    prepare_mutation_and_approve(
        client,
        bot,
        state_db_path,
        "send_sticker",
        json!({
            "chat_id": "@alex",
            "set_short_name": "fixture_reactions",
            "sticker_handle": sticker_handle,
            "reply_to_message_id": 11,
            "silent": true,
            "noforwards": true
        }),
    )
    .await
}

async fn prepare_mutation_and_approve(
    client: &rmcp::service::RunningService<rmcp::RoleClient, ()>,
    bot: &FakeApprovalBot,
    state_db_path: &std::path::Path,
    tool_name: &'static str,
    arguments: Value,
) -> anyhow::Result<(String, i64, Value)> {
    let prepared = client
        .call_tool(
            CallToolRequestParams::new(tool_name)
                .with_arguments(arguments.as_object().expect("object").clone()),
        )
        .await?;
    assert_eq!(prepared.is_error, Some(false));
    let prepared_data = prepared.structured_content.expect("prepared sticker send");
    let requested = client
        .call_tool(
            CallToolRequestParams::new("request_mutation_approval").with_arguments(
                json!({
                    "purpose": "тест: подготовка мутации к подтверждению",
                    "mutation_id": prepared_data
                        .pointer("/data/mutation_id")
                        .and_then(Value::as_str)
                        .expect("mutation id"),
                    "approved_payload_sha256": prepared_data
                        .pointer("/data/payload_sha256")
                        .and_then(Value::as_str)
                        .expect("payload hash")
                })
                .as_object()
                .expect("object")
                .clone(),
            ),
        )
        .await?;
    assert_eq!(requested.is_error, Some(false));
    let requested_data = requested.structured_content.expect("approval request");
    let approval_id = requested_data
        .pointer("/data/approval_id")
        .and_then(Value::as_str)
        .expect("approval id")
        .to_string();
    let message_id = requested_data
        .pointer("/data/approval_message_id")
        .and_then(Value::as_i64)
        .expect("approval message id");
    bot.push_mutation_decision(&approval_id, ApprovalAction::Approve, 700, 700, message_id).await;
    wait_for_applied(state_db_path, &approval_id).await?;
    Ok((approval_id, message_id, prepared_data))
}

#[tokio::test]
async fn sticker_catalogues_are_typed_bounded_hash_aware_and_opaque() -> anyhow::Result<()> {
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

    let recent = client
        .call_tool(
            CallToolRequestParams::new("get_recent_stickers")
                .with_arguments(json!({ "limit": 1 }).as_object().expect("object").clone()),
        )
        .await?;
    assert_eq!(recent.is_error, Some(false));
    let recent_data = recent.structured_content.as_ref().expect("recent stickers");
    assert_eq!(recent_data.pointer("/data/hash").and_then(Value::as_i64), Some(5_151));
    assert_eq!(recent_data.pointer("/data/total_count").and_then(Value::as_u64), Some(2));
    assert_eq!(recent_data.pointer("/data/returned_count").and_then(Value::as_u64), Some(1));
    assert_eq!(recent_data.pointer("/data/has_more").and_then(Value::as_bool), Some(true));
    let sticker_handle = recent_data
        .pointer("/data/stickers/0/sticker_handle")
        .and_then(Value::as_str)
        .expect("sticker handle");
    assert!(sticker_handle.starts_with("sticker_"));
    assert!(!recent_data.to_string().contains("access_hash"));
    assert!(!recent_data.to_string().contains("file_reference"));

    let recent_not_modified = client
        .call_tool(
            CallToolRequestParams::new("get_recent_stickers")
                .with_arguments(json!({ "hash": 5_151 }).as_object().expect("object").clone()),
        )
        .await?;
    assert_eq!(
        recent_not_modified
            .structured_content
            .as_ref()
            .and_then(|value| value.pointer("/data/not_modified"))
            .and_then(Value::as_bool),
        Some(true)
    );

    let set = client
        .call_tool(CallToolRequestParams::new("get_sticker_set").with_arguments(
            json!({ "short_name": "fixture_reactions" }).as_object().expect("object").clone(),
        ))
        .await?;
    assert_eq!(set.is_error, Some(false));
    let set_data = set.structured_content.as_ref().expect("sticker set");
    assert_eq!(
        set_data.pointer("/data/sticker_set/short_name").and_then(Value::as_str),
        Some("fixture_reactions")
    );
    assert_eq!(set_data.pointer("/data/stickers").and_then(Value::as_array).map(Vec::len), Some(2));
    assert_eq!(set_data.pointer("/data/packs/1/emoji").and_then(Value::as_str), Some("🔥"));
    assert_eq!(
        set_data.pointer("/data/keywords/0/keywords/1").and_then(Value::as_str),
        Some("release")
    );

    let set_not_modified = client
        .call_tool(
            CallToolRequestParams::new("get_sticker_set").with_arguments(
                json!({ "short_name": "fixture_reactions", "hash": 4_242 })
                    .as_object()
                    .expect("object")
                    .clone(),
            ),
        )
        .await?;
    assert_eq!(
        set_not_modified
            .structured_content
            .as_ref()
            .and_then(|value| value.pointer("/data/not_modified"))
            .and_then(Value::as_bool),
        Some(true)
    );

    let search = client
        .call_tool(
            CallToolRequestParams::new("search_sticker_sets")
                .with_arguments(json!({ "query": "release" }).as_object().expect("object").clone()),
        )
        .await?;
    assert_eq!(search.is_error, Some(false));
    assert_eq!(
        search
            .structured_content
            .as_ref()
            .and_then(|value| value.pointer("/data/sticker_sets/0/short_name"))
            .and_then(Value::as_str),
        Some("fixture_reactions")
    );
    let search_not_modified = client
        .call_tool(CallToolRequestParams::new("search_sticker_sets").with_arguments(
            json!({ "query": "release", "hash": 6_262 }).as_object().expect("object").clone(),
        ))
        .await?;
    assert_eq!(
        search_not_modified
            .structured_content
            .as_ref()
            .and_then(|value| value.pointer("/data/not_modified"))
            .and_then(Value::as_bool),
        Some(true)
    );

    for (tool, arguments) in [
        ("get_recent_stickers", json!({ "limit": 0 })),
        ("get_recent_stickers", json!({ "limit": 101 })),
        ("get_sticker_set", json!({ "short_name": "bad-name" })),
        ("search_sticker_sets", json!({ "query": "" })),
    ] {
        let invalid = client
            .call_tool(
                CallToolRequestParams::new(tool)
                    .with_arguments(arguments.as_object().expect("object").clone()),
            )
            .await?;
        assert_eq!(invalid.is_error, Some(true));
    }
    let unknown_field = client
        .call_tool(
            CallToolRequestParams::new("get_recent_stickers")
                .with_arguments(json!({ "extra": 1 }).as_object().expect("object").clone()),
        )
        .await;
    assert!(unknown_field.is_err());

    client.cancel().await?;
    server_task.await??;
    Ok(())
}

#[tokio::test]
async fn sticker_send_is_approved_read_back_and_recovered_without_duplicate_provider_action()
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

    let set = client
        .call_tool(CallToolRequestParams::new("get_sticker_set").with_arguments(
            json!({ "short_name": "fixture_reactions" }).as_object().expect("object").clone(),
        ))
        .await?;
    let sticker_handle = set
        .structured_content
        .as_ref()
        .and_then(|value| value.pointer("/data/stickers/0/sticker_handle"))
        .and_then(Value::as_str)
        .expect("sticker handle")
        .to_string();

    for arguments in [
        json!({
            "chat_id": "@alex",
            "set_short_name": "fixture_reactions",
            "sticker_handle": "document_123"
        }),
        json!({
            "chat_id": "@alex",
            "set_short_name": "fixture_reactions",
            "sticker_handle": format!("sticker_{}", "0".repeat(32))
        }),
    ] {
        let invalid = client
            .call_tool(
                CallToolRequestParams::new("send_sticker")
                    .with_arguments(arguments.as_object().expect("object").clone()),
            )
            .await?;
        assert_eq!(invalid.is_error, Some(true));
    }

    let (approval_id, approval_message_id, prepared) =
        prepare_and_approve(&client, &bot, &state_db_path, &sticker_handle).await?;
    assert_eq!(prepared.pointer("/data/action/kind").and_then(Value::as_str), Some("send_sticker"));
    assert_eq!(
        prepared.pointer("/data/action/sticker/sticker_handle").and_then(Value::as_str),
        Some(sticker_handle.as_str())
    );
    assert_eq!(backend.sticker_send_call_count().await, 1);

    bot.push_mutation_decision(
        &approval_id,
        ApprovalAction::Approve,
        700,
        700,
        approval_message_id,
    )
    .await;
    tokio::time::sleep(Duration::from_millis(150)).await;
    assert_eq!(backend.sticker_send_call_count().await, 1);

    let approval = MutationLedger::new(state_db_path.clone())
        .get_approval(&approval_id)?
        .expect("sticker approval");
    let message_id = match approval.result.expect("sticker result") {
        telegram_personal_mcp::MutationResult::StickerSent {
            message_id,
            sticker_handle: result_handle,
        } => {
            assert_eq!(result_handle, sticker_handle);
            message_id
        },
        other => anyhow::bail!("unexpected sticker result: {other:?}"),
    };
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
    let readback_data = readback.structured_content.as_ref().expect("sticker readback");
    assert_eq!(
        readback_data.pointer("/data/message/media_type").and_then(Value::as_str),
        Some("sticker")
    );
    assert_eq!(
        readback_data.pointer("/data/message/reply_to_message_id").and_then(Value::as_i64),
        Some(11)
    );
    assert_eq!(readback_data.pointer("/data/message/silent").and_then(Value::as_bool), Some(true));
    assert_eq!(
        readback_data.pointer("/data/message/noforwards").and_then(Value::as_bool),
        Some(true)
    );

    backend.make_next_sticker_send_outcome_uncertain().await;
    let (_, _, _) = prepare_and_approve(&client, &bot, &state_db_path, &sticker_handle).await?;
    assert_eq!(backend.sticker_send_call_count().await, 2);
    assert_eq!(backend.sent_messages().await.len(), 2);

    let cards = format!("{:?}\n{:?}", bot.sent_mutation_cards().await, bot.mutation_edits().await);
    assert!(!cards.contains("access_hash"));
    assert!(!cards.contains("file_reference"));

    client.cancel().await?;
    server_task.abort();
    worker.abort();
    Ok(())
}

#[tokio::test]
async fn sticker_favourites_and_installation_are_approved_read_back_and_recoverable()
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

    let set = client
        .call_tool(CallToolRequestParams::new("get_sticker_set").with_arguments(
            json!({ "short_name": "fixture_reactions" }).as_object().expect("object").clone(),
        ))
        .await?;
    let set_data = set.structured_content.as_ref().expect("sticker set");
    let first_handle = set_data
        .pointer("/data/stickers/0/sticker_handle")
        .and_then(Value::as_str)
        .expect("first sticker")
        .to_string();
    let second_handle = set_data
        .pointer("/data/stickers/1/sticker_handle")
        .and_then(Value::as_str)
        .expect("second sticker")
        .to_string();

    let (_, _, recent_remove_prepared) = prepare_mutation_and_approve(
        &client,
        &bot,
        &state_db_path,
        "add_recent_sticker",
        json!({
            "set_short_name": "fixture_reactions",
            "sticker_handle": second_handle,
            "unsave": true
        }),
    )
    .await?;
    assert_eq!(
        recent_remove_prepared.pointer("/data/action/kind").and_then(Value::as_str),
        Some("set_recent_sticker")
    );
    assert_eq!(backend.recent_sticker_call_count().await, 1);
    let recent = client.call_tool(CallToolRequestParams::new("get_recent_stickers")).await?;
    assert_eq!(
        recent
            .structured_content
            .as_ref()
            .and_then(|value| value.pointer("/data/stickers"))
            .and_then(Value::as_array)
            .map(Vec::len),
        Some(1)
    );
    let no_recent_change = client
        .call_tool(
            CallToolRequestParams::new("add_recent_sticker").with_arguments(
                json!({
                    "set_short_name": "fixture_reactions",
                    "sticker_handle": second_handle,
                    "unsave": true
                })
                .as_object()
                .expect("object")
                .clone(),
            ),
        )
        .await?;
    assert_eq!(no_recent_change.is_error, Some(true));

    backend.make_next_recent_sticker_outcome_uncertain().await;
    let (recent_approval_id, recent_card_id, recent_add_prepared) = prepare_mutation_and_approve(
        &client,
        &bot,
        &state_db_path,
        "add_recent_sticker",
        json!({
            "set_short_name": "fixture_reactions",
            "sticker_handle": second_handle,
            "unsave": false
        }),
    )
    .await?;
    assert_eq!(
        recent_add_prepared.pointer("/data/action/saved").and_then(Value::as_bool),
        Some(true)
    );
    assert_eq!(backend.recent_sticker_call_count().await, 2);
    let recent = client.call_tool(CallToolRequestParams::new("get_recent_stickers")).await?;
    assert_eq!(
        recent
            .structured_content
            .as_ref()
            .and_then(|value| value.pointer("/data/stickers"))
            .and_then(Value::as_array)
            .map(Vec::len),
        Some(2)
    );
    bot.push_mutation_decision(
        &recent_approval_id,
        ApprovalAction::Approve,
        700,
        700,
        recent_card_id,
    )
    .await;
    tokio::time::sleep(Duration::from_millis(150)).await;
    assert_eq!(backend.recent_sticker_call_count().await, 2);

    prepare_mutation_and_approve(
        &client,
        &bot,
        &state_db_path,
        "add_recent_sticker",
        json!({
            "set_short_name": "fixture_reactions",
            "sticker_handle": first_handle,
            "attached": true,
            "unsave": true
        }),
    )
    .await?;
    assert_eq!(backend.recent_sticker_call_count().await, 3);
    let attached_recent = client
        .call_tool(
            CallToolRequestParams::new("get_recent_stickers")
                .with_arguments(json!({ "attached": true }).as_object().expect("object").clone()),
        )
        .await?;
    assert_eq!(
        attached_recent
            .structured_content
            .as_ref()
            .and_then(|value| value.pointer("/data/attached"))
            .and_then(Value::as_bool),
        Some(true)
    );
    assert_eq!(
        attached_recent
            .structured_content
            .as_ref()
            .and_then(|value| value.pointer("/data/stickers"))
            .and_then(Value::as_array)
            .map(Vec::len),
        Some(1)
    );
    let ordinary_recent =
        client.call_tool(CallToolRequestParams::new("get_recent_stickers")).await?;
    assert_eq!(
        ordinary_recent
            .structured_content
            .as_ref()
            .and_then(|value| value.pointer("/data/stickers"))
            .and_then(Value::as_array)
            .map(Vec::len),
        Some(2),
        "attached recent-sticker mutation must not change the ordinary catalogue"
    );

    let favourites = client.call_tool(CallToolRequestParams::new("get_favorite_stickers")).await?;
    let favourites_data = favourites.structured_content.as_ref().expect("favourites");
    assert_eq!(
        favourites_data.pointer("/data/stickers/0/sticker_handle").and_then(Value::as_str),
        Some(first_handle.as_str())
    );
    let favourites_hash =
        favourites_data.pointer("/data/hash").and_then(Value::as_i64).expect("favourites hash");
    let favourites_not_modified = client
        .call_tool(CallToolRequestParams::new("get_favorite_stickers").with_arguments(
            json!({ "hash": favourites_hash }).as_object().expect("object").clone(),
        ))
        .await?;
    assert_eq!(
        favourites_not_modified
            .structured_content
            .as_ref()
            .and_then(|value| value.pointer("/data/not_modified"))
            .and_then(Value::as_bool),
        Some(true)
    );

    let (_, _, favorite_prepared) = prepare_mutation_and_approve(
        &client,
        &bot,
        &state_db_path,
        "set_sticker_favorite",
        json!({
            "set_short_name": "fixture_reactions",
            "sticker_handle": second_handle,
            "favorite": true
        }),
    )
    .await?;
    assert_eq!(
        favorite_prepared.pointer("/data/action/kind").and_then(Value::as_str),
        Some("set_sticker_favorite")
    );
    assert_eq!(backend.sticker_state_call_counts().await, (1, 0, 0));
    let favourites = client.call_tool(CallToolRequestParams::new("get_favorite_stickers")).await?;
    assert_eq!(
        favourites
            .structured_content
            .as_ref()
            .and_then(|value| value.pointer("/data/stickers"))
            .and_then(Value::as_array)
            .map(Vec::len),
        Some(2)
    );

    backend.make_next_sticker_favorite_outcome_uncertain().await;
    prepare_mutation_and_approve(
        &client,
        &bot,
        &state_db_path,
        "set_sticker_favorite",
        json!({
            "set_short_name": "fixture_reactions",
            "sticker_handle": first_handle,
            "favorite": false
        }),
    )
    .await?;
    assert_eq!(backend.sticker_state_call_counts().await, (2, 0, 0));
    let favourites = client.call_tool(CallToolRequestParams::new("get_favorite_stickers")).await?;
    assert_eq!(
        favourites
            .structured_content
            .as_ref()
            .and_then(|value| value.pointer("/data/stickers/0/sticker_handle"))
            .and_then(Value::as_str),
        Some(second_handle.as_str())
    );

    let (uninstall_approval_id, uninstall_card_id, uninstall_prepared) =
        prepare_mutation_and_approve(
            &client,
            &bot,
            &state_db_path,
            "uninstall_sticker_set",
            json!({ "short_name": "fixture_reactions" }),
        )
        .await?;
    assert_eq!(
        uninstall_prepared.pointer("/data/action/kind").and_then(Value::as_str),
        Some("set_sticker_set_installed")
    );
    assert_eq!(backend.sticker_state_call_counts().await, (2, 0, 1));
    let installed = client.call_tool(CallToolRequestParams::new("list_sticker_sets")).await?;
    assert_eq!(
        installed
            .structured_content
            .as_ref()
            .and_then(|value| value.pointer("/data/sticker_sets"))
            .and_then(Value::as_array)
            .map(Vec::len),
        Some(0)
    );
    bot.push_mutation_decision(
        &uninstall_approval_id,
        ApprovalAction::Approve,
        700,
        700,
        uninstall_card_id,
    )
    .await;
    tokio::time::sleep(Duration::from_millis(150)).await;
    assert_eq!(backend.sticker_state_call_counts().await, (2, 0, 1));

    backend.make_next_sticker_install_outcome_uncertain().await;
    prepare_mutation_and_approve(
        &client,
        &bot,
        &state_db_path,
        "install_sticker_set",
        json!({ "short_name": "fixture_reactions", "archived": false }),
    )
    .await?;
    assert_eq!(backend.sticker_state_call_counts().await, (2, 1, 1));
    let installed = client.call_tool(CallToolRequestParams::new("list_sticker_sets")).await?;
    assert_eq!(
        installed
            .structured_content
            .as_ref()
            .and_then(|value| value.pointer("/data/sticker_sets/0/short_name"))
            .and_then(Value::as_str),
        Some("fixture_reactions")
    );

    let no_change = client
        .call_tool(CallToolRequestParams::new("install_sticker_set").with_arguments(
            json!({ "short_name": "fixture_reactions" }).as_object().expect("object").clone(),
        ))
        .await?;
    assert_eq!(no_change.is_error, Some(true));

    let cards = format!("{:?}\n{:?}", bot.sent_mutation_cards().await, bot.mutation_edits().await);
    assert!(cards.contains("НЕДАВНЕГО СТИКЕРА"));
    assert!(!cards.contains("access_hash"));
    assert!(!cards.contains("file_reference"));

    client.cancel().await?;
    server_task.abort();
    worker.abort();
    Ok(())
}
