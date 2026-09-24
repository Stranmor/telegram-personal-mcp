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
            .push_mutation_decision(
                &approval_id,
                ApprovalAction::Approve,
                700,
                700,
                message_id,
            )
            .await;
        if let Err(error) = wait_for_mutation_state(
            &$state_db_path,
            &approval_id,
            MutationApprovalState::Applied,
        )
        .await
        {
            anyhow::bail!("{}: {error}", $tool);
        }
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
async fn dialog_folder_lifecycle_is_e2e_exact_and_idempotent() -> anyhow::Result<()> {
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

    let initial = client.call_tool(CallToolRequestParams::new("list_folders")).await?;
    assert_eq!(initial.is_error, Some(false));
    let initial = initial.structured_content.expect("initial folders");
    assert_eq!(initial.pointer("/data/folders").and_then(Value::as_array).map(Vec::len), Some(2));
    assert_eq!(initial.pointer("/data/folders/0/kind").and_then(Value::as_str), Some("default"));

    let default_delete = client
        .call_tool(
            CallToolRequestParams::new("delete_folder")
                .with_arguments(json!({ "folder_id": 0 }).as_object().expect("object").clone()),
        )
        .await?;
    assert_eq!(default_delete.is_error, Some(true));

    let (created, create_approval_id, create_message_id) = approve_mutation!(
        client,
        bot,
        state_db_path,
        "create_folder",
        json!({
            "title": "Clients",
            "kind": "custom",
            "emoticon": "💼",
            "color": 4,
            "contacts": true,
            "exclude_archived": true,
            "included_chat_ids": ["101"]
        })
    );
    let clients_id =
        created.pointer("/folder/folder_id").and_then(Value::as_i64).expect("created folder id")
            as i32;
    assert_eq!(clients_id, 3);
    assert_eq!(created.pointer("/folder/title").and_then(Value::as_str), Some("Clients"));

    let create_calls = backend.dialog_folder_call_counts().await.0;
    bot.push_mutation_decision(
        &create_approval_id,
        ApprovalAction::Approve,
        700,
        700,
        create_message_id,
    )
    .await;
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert_eq!(backend.dialog_folder_call_counts().await.0, create_calls);

    let (added, _, _) = approve_mutation!(
        client,
        bot,
        state_db_path,
        "add_chat_to_folder",
        json!({ "folder_id": clients_id, "chat_id": "-202" })
    );
    assert_eq!(added.pointer("/folder/included_chat_ids/1").and_then(Value::as_str), Some("-202"));

    let (removed, _, _) = approve_mutation!(
        client,
        bot,
        state_db_path,
        "remove_chat_from_folder",
        json!({ "folder_id": clients_id, "chat_id": "101" })
    );
    assert_eq!(removed.pointer("/folder/excluded_chat_ids/0").and_then(Value::as_str), Some("101"));

    let (edited, _, _) = approve_mutation!(
        client,
        bot,
        state_db_path,
        "edit_folder",
        json!({
            "folder_id": clients_id,
            "preserve_title_entities": true,
            "definition": {
                "title": "Key clients",
                "kind": "custom",
                "emoticon": "⭐",
                "color": 5,
                "contacts": true,
                "groups": true,
                "exclude_archived": true,
                "included_chat_ids": ["-202"],
                "excluded_chat_ids": ["101"]
            }
        })
    );
    assert_eq!(edited.pointer("/folder/title").and_then(Value::as_str), Some("Key clients"));
    assert_eq!(edited.pointer("/folder/groups").and_then(Value::as_bool), Some(true));

    let (second, _, _) = approve_mutation!(
        client,
        bot,
        state_db_path,
        "create_folder",
        json!({
            "title": "Archive watch",
            "kind": "custom",
            "broadcasts": true,
            "included_chat_ids": ["-100505"]
        })
    );
    let second_id =
        second.pointer("/folder/folder_id").and_then(Value::as_i64).expect("second folder id")
            as i32;
    assert_eq!(second_id, 4);

    let (reordered, _, _) = approve_mutation!(
        client,
        bot,
        state_db_path,
        "reorder_folders",
        json!({ "folder_ids": [second_id, clients_id, 2] })
    );
    assert_eq!(
        reordered.pointer("/folder_ids").and_then(Value::as_array),
        Some(&vec![json!(4), json!(3), json!(2)])
    );

    let (deleted, delete_approval_id, delete_message_id) = approve_mutation!(
        client,
        bot,
        state_db_path,
        "delete_folder",
        json!({ "folder_id": second_id })
    );
    assert_eq!(deleted.pointer("/folder_id").and_then(Value::as_i64), Some(4));
    assert_eq!(backend.dialog_folder_call_counts().await, (3, 1, 1, 2));

    bot.push_mutation_decision(
        &delete_approval_id,
        ApprovalAction::Approve,
        700,
        700,
        delete_message_id,
    )
    .await;
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert_eq!(backend.dialog_folder_call_counts().await, (3, 1, 1, 2));

    let final_folders = client.call_tool(CallToolRequestParams::new("list_folders")).await?;
    let final_folders = final_folders.structured_content.expect("final folders");
    assert_eq!(final_folders.pointer("/data/folders/1/folder_id").and_then(Value::as_i64), Some(3));
    assert_eq!(final_folders.pointer("/data/folders/2/folder_id").and_then(Value::as_i64), Some(2));

    client.cancel().await?;
    server_task.await??;
    worker.abort();
    let _ = worker.await;
    Ok(())
}
