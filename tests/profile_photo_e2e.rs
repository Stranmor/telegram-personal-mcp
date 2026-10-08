use rmcp::{ServiceExt, model::CallToolRequestParams};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
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

async fn wait_for_path_missing(path: &std::path::Path) -> anyhow::Result<()> {
    for _ in 0..300 {
        if !path.exists() {
            return Ok(());
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    anyhow::bail!("artifact was not cleaned: {}", path.display())
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
        (result, approval_id, message_id, data)
    }};
}

fn assert_opaque_photo_handle(handle: &str) {
    let suffix = handle.strip_prefix("photo_").expect("opaque photo prefix");
    assert_eq!(suffix.len(), 32);
    assert!(
        suffix.as_bytes().iter().all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(byte))
    );
}

#[tokio::test]
async fn profile_photo_lifecycle_is_e2e_hash_bound_and_idempotent() -> anyhow::Result<()> {
    let backend = Arc::new(FakeTelegramBackend::fixture());
    let bot = Arc::new(FakeApprovalBot::default());
    let temp = tempfile::tempdir()?;
    let state_db_path = temp.path().join("state.sqlite");
    let attachment_cache_dir = temp.path().join("attachments");
    let server = McpServer::with_approval(
        backend.clone(),
        McpServerSettings {
            attachment_cache_dir: attachment_cache_dir.clone(),
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

    let initial = client
        .call_tool(CallToolRequestParams::new("list_profile_photos").with_arguments(
            json!({ "offset": 0, "limit": 100 }).as_object().expect("object").clone(),
        ))
        .await?;
    assert_eq!(initial.is_error, Some(false));
    let initial = initial.structured_content.expect("initial photos");
    assert_eq!(initial.pointer("/data/total_count").and_then(Value::as_u64), Some(2));
    let first_handle = initial
        .pointer("/data/photos/0/photo_handle")
        .and_then(Value::as_str)
        .expect("first photo handle")
        .to_string();
    let second_handle = initial
        .pointer("/data/photos/1/photo_handle")
        .and_then(Value::as_str)
        .expect("second photo handle")
        .to_string();
    assert_opaque_photo_handle(&first_handle);
    assert_opaque_photo_handle(&second_handle);

    let raw_rejected = client
        .call_tool(
            CallToolRequestParams::new("get_profile_photo").with_arguments(
                json!({ "photo_handle": "9001:access_hash:file_reference" })
                    .as_object()
                    .expect("object")
                    .clone(),
            ),
        )
        .await?;
    assert_eq!(raw_rejected.is_error, Some(true));

    let downloaded = client
        .call_tool(CallToolRequestParams::new("download_attachment").with_arguments(
            json!({ "chat_id": "@alex", "message_id": 11 }).as_object().expect("object").clone(),
        ))
        .await?;
    let managed_dir = std::path::Path::new(
        downloaded
            .structured_content
            .as_ref()
            .and_then(|value| value.pointer("/data/local_path"))
            .and_then(Value::as_str)
            .expect("download path"),
    )
    .parent()
    .expect("managed account cache")
    .to_path_buf();
    let avatar_path = managed_dir.join("profile-e2e.png");
    let avatar_bytes = [
        0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, b'I', b'H', b'D',
        b'R', 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1f,
        0x15, 0xc4, 0x89,
    ];
    std::fs::write(&avatar_path, avatar_bytes)?;
    let avatar_sha256 = format!("{:x}", Sha256::digest(avatar_bytes));

    let (uploaded, upload_approval_id, upload_message_id, upload_prepared) = approve_mutation!(
        client,
        bot,
        state_db_path,
        "upload_profile_photo",
        json!({
            "file_path": avatar_path,
            "approved_sha256": avatar_sha256,
            "fallback": false
        })
    );
    assert_eq!(
        uploaded.pointer("/kind").and_then(Value::as_str),
        Some("account_profile_photo_uploaded")
    );
    let uploaded_handle = uploaded
        .pointer("/photo/photo_handle")
        .and_then(Value::as_str)
        .expect("uploaded photo handle")
        .to_string();
    assert_opaque_photo_handle(&uploaded_handle);
    let artifact_id = upload_prepared
        .pointer("/data/action/artifact/artifact_id")
        .and_then(Value::as_str)
        .expect("artifact id");
    wait_for_path_missing(&state_db_path.with_extension("outbox").join(artifact_id)).await?;

    let uploaded_readback = client
        .call_tool(CallToolRequestParams::new("get_profile_photo").with_arguments(
            json!({ "photo_handle": uploaded_handle }).as_object().expect("object").clone(),
        ))
        .await?;
    assert_eq!(uploaded_readback.is_error, Some(false));
    assert_eq!(
        uploaded_readback
            .structured_content
            .as_ref()
            .and_then(|value| value.pointer("/data/photo_handle"))
            .and_then(Value::as_str),
        Some(uploaded_handle.as_str())
    );

    let upload_calls = backend.profile_photo_call_counts().await.0;
    bot.push_mutation_decision(
        &upload_approval_id,
        ApprovalAction::Approve,
        700,
        700,
        upload_message_id,
    )
    .await;
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert_eq!(backend.profile_photo_call_counts().await.0, upload_calls);

    let (deleted, delete_approval_id, delete_message_id, _) = approve_mutation!(
        client,
        bot,
        state_db_path,
        "delete_profile_photos",
        json!({ "photo_handles": [uploaded_handle, second_handle] })
    );
    assert_eq!(
        deleted.pointer("/kind").and_then(Value::as_str),
        Some("account_profile_photos_deleted")
    );
    assert_eq!(deleted.pointer("/photo_handles").and_then(Value::as_array).map(Vec::len), Some(2));
    let final_page = client
        .call_tool(CallToolRequestParams::new("list_profile_photos").with_arguments(
            json!({ "offset": 0, "limit": 100 }).as_object().expect("object").clone(),
        ))
        .await?;
    let final_page = final_page.structured_content.expect("final photos");
    assert_eq!(final_page.pointer("/data/total_count").and_then(Value::as_u64), Some(1));
    assert_eq!(
        final_page.pointer("/data/photos/0/photo_handle").and_then(Value::as_str),
        Some(first_handle.as_str())
    );
    assert_eq!(backend.profile_photo_call_counts().await, (1, 1));

    bot.push_mutation_decision(
        &delete_approval_id,
        ApprovalAction::Approve,
        700,
        700,
        delete_message_id,
    )
    .await;
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert_eq!(backend.profile_photo_call_counts().await, (1, 1));

    let approval_surface =
        format!("{:?}\n{:?}", bot.sent_mutation_cards().await, bot.mutation_edits().await);
    assert!(!approval_surface.contains(managed_dir.to_string_lossy().as_ref()));
    assert!(!approval_surface.contains("access_hash"));
    assert!(!approval_surface.contains("file_reference"));

    client.cancel().await?;
    server_task.await??;
    worker.abort();
    let _ = worker.await;
    Ok(())
}
