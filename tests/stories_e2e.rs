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
    anyhow::bail!("story mutation did not reach {wanted:?}; last={last:?}")
}

async fn wait_for_path_missing(path: &std::path::Path) -> anyhow::Result<()> {
    for _ in 0..300 {
        if !path.exists() {
            return Ok(());
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    anyhow::bail!("story artifact was not cleaned: {}", path.display())
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

#[tokio::test]
async fn stories_read_and_viewer_surfaces_are_e2e_typed_and_bounded() -> anyhow::Result<()> {
    let backend = Arc::new(FakeTelegramBackend::fixture());
    let cache = tempfile::tempdir()?;
    let server = McpServer::with_settings(
        backend,
        McpServerSettings {
            attachment_cache_dir: cache.path().join("attachments"),
            max_download_bytes: 1024 * 1024,
            max_upload_bytes: 1024 * 1024,
            state_db_path: cache.path().join("state.sqlite"),
        },
    );
    let (server_transport, client_transport) = tokio::io::duplex(64 * 1024);
    let server_task = tokio::spawn(async move {
        server.serve(server_transport).await?.waiting().await?;
        anyhow::Ok(())
    });
    let client = ().serve(client_transport).await?;

    let list = client.call_tool(CallToolRequestParams::new("list_stories")).await?;
    assert_eq!(list.is_error, Some(false));
    assert_eq!(
        list.structured_content
            .as_ref()
            .and_then(|value| value.pointer("/data/page/count"))
            .and_then(Value::as_i64),
        Some(3)
    );
    assert_eq!(
        list.structured_content
            .as_ref()
            .and_then(|value| value.pointer("/data/page/peers/0/peer_ref"))
            .and_then(Value::as_str),
        Some("-100505")
    );
    let serialized = serde_json::to_string(&list.structured_content)?;
    assert!(!serialized.contains("access_hash"));
    assert!(!serialized.contains("file_reference"));

    let peer =
        client
            .call_tool(CallToolRequestParams::new("get_peer_stories").with_arguments(
                json!({ "chat_id": "-100505" }).as_object().expect("object").clone(),
            ))
            .await?;
    assert_eq!(peer.is_error, Some(false));
    assert_eq!(
        peer.structured_content
            .as_ref()
            .and_then(|value| value.pointer("/data/page/peer/stories/0/story_id"))
            .and_then(Value::as_i64),
        Some(41)
    );
    assert_eq!(
        peer.structured_content
            .as_ref()
            .and_then(|value| value.pointer("/data/page/peer/stories/0/media_kind"))
            .and_then(Value::as_str),
        Some("photo")
    );

    let by_id = client
        .call_tool(
            CallToolRequestParams::new("get_stories_by_id").with_arguments(
                json!({ "chat_id": "-100505", "story_ids": [31, 41] })
                    .as_object()
                    .expect("object")
                    .clone(),
            ),
        )
        .await?;
    assert_eq!(by_id.is_error, Some(false));
    assert_eq!(
        by_id
            .structured_content
            .as_ref()
            .and_then(|value| value.pointer("/data/page/stories/0/story_id"))
            .and_then(Value::as_i64),
        Some(31)
    );
    assert_eq!(
        by_id
            .structured_content
            .as_ref()
            .and_then(|value| value.pointer("/data/page/pinned_to_top_ids/0"))
            .and_then(Value::as_i64),
        Some(41)
    );

    let archive = client
        .call_tool(CallToolRequestParams::new("get_stories_archive").with_arguments(
            json!({ "chat_id": "-100505", "limit": 1 }).as_object().expect("object").clone(),
        ))
        .await?;
    assert_eq!(archive.is_error, Some(false));
    assert_eq!(
        archive
            .structured_content
            .as_ref()
            .and_then(|value| value.pointer("/data/page/stories/0/caption"))
            .and_then(Value::as_str),
        Some("Archived release")
    );
    assert_eq!(
        archive
            .structured_content
            .as_ref()
            .and_then(|value| value.pointer("/data/page/next_offset_id"))
            .and_then(Value::as_i64),
        Some(31)
    );

    let link = client
        .call_tool(CallToolRequestParams::new("export_story_link").with_arguments(
            json!({ "chat_id": "-100505", "story_id": 31 }).as_object().expect("object").clone(),
        ))
        .await?;
    assert_eq!(link.is_error, Some(false));
    assert_eq!(
        link.structured_content
            .as_ref()
            .and_then(|value| value.pointer("/data/story_link/story_id"))
            .and_then(Value::as_i64),
        Some(31)
    );
    assert!(
        link.structured_content
            .as_ref()
            .and_then(|value| value.pointer("/data/story_link/link"))
            .and_then(Value::as_str)
            .is_some_and(|link| link.starts_with("https://t.me/"))
    );

    let viewers = client
        .call_tool(
            CallToolRequestParams::new("get_story_viewers").with_arguments(
                json!({
                    "chat_id": "-100505",
                    "story_id": 41,
                    "limit": 1,
                    "reactions_first": true
                })
                .as_object()
                .expect("object")
                .clone(),
            ),
        )
        .await?;
    assert_eq!(viewers.is_error, Some(false));
    assert_eq!(
        viewers
            .structured_content
            .as_ref()
            .and_then(|value| value.pointer("/data/page/count"))
            .and_then(Value::as_i64),
        Some(2)
    );
    assert_eq!(
        viewers
            .structured_content
            .as_ref()
            .and_then(|value| value.pointer("/data/page/viewers/0/user/username"))
            .and_then(Value::as_str),
        Some("alex")
    );
    assert_eq!(
        viewers
            .structured_content
            .as_ref()
            .and_then(|value| value.pointer("/data/page/viewers/0/reaction_emoji"))
            .and_then(Value::as_str),
        Some("👍")
    );

    let invalid = client
        .call_tool(CallToolRequestParams::new("get_story_viewers").with_arguments(
            json!({ "chat_id": "-100505", "story_id": 0 }).as_object().expect("object").clone(),
        ))
        .await?;
    assert_eq!(invalid.is_error, Some(true));

    let duplicate_ids = client
        .call_tool(
            CallToolRequestParams::new("get_stories_by_id").with_arguments(
                json!({ "chat_id": "-100505", "story_ids": [41, 41] })
                    .as_object()
                    .expect("object")
                    .clone(),
            ),
        )
        .await?;
    assert_eq!(duplicate_ids.is_error, Some(true));

    client.cancel().await?;
    server_task.abort();
    Ok(())
}

#[tokio::test]
async fn story_views_require_one_exact_idempotent_operator_approval() -> anyhow::Result<()> {
    let backend = Arc::new(FakeTelegramBackend::fixture());
    let bot = Arc::new(FakeApprovalBot::default());
    let cache = tempfile::tempdir()?;
    let state_db_path = cache.path().join("state.sqlite");
    let server = McpServer::with_approval(
        backend.clone(),
        McpServerSettings {
            attachment_cache_dir: cache.path().join("attachments"),
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

    let prepared = client
        .call_tool(
            CallToolRequestParams::new("view_story").with_arguments(
                json!({ "chat_id": "-100505", "story_ids": [41, 42] })
                    .as_object()
                    .expect("object")
                    .clone(),
            ),
        )
        .await?;
    assert_eq!(prepared.is_error, Some(false));
    let prepared_data = prepared.structured_content.expect("prepared story view");
    assert_eq!(
        prepared_data.pointer("/data/action/peer_ref").and_then(Value::as_str),
        Some("-100505")
    );
    assert!(prepared_data.pointer("/data/action/before/0/caption").is_none());
    let mutation_id =
        prepared_data.pointer("/data/mutation_id").and_then(Value::as_str).expect("mutation id");
    let payload_sha256 = prepared_data
        .pointer("/data/payload_sha256")
        .and_then(Value::as_str)
        .expect("payload hash");
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
    let requested_data = requested.structured_content.expect("approval");
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
    wait_for_mutation_state(&state_db_path, &approval_id, MutationApprovalState::Applied).await?;
    let applied = MutationLedger::new(state_db_path.clone())
        .get_approval(&approval_id)?
        .expect("applied approval");
    let result = serde_json::to_value(applied.result.expect("story view result"))?;
    assert_eq!(result.pointer("/kind").and_then(Value::as_str), Some("stories_viewed"));
    assert_eq!(result.pointer("/story_ids/1").and_then(Value::as_i64), Some(42));
    assert_eq!(backend.story_view_call_count().await, 1);

    bot.push_mutation_decision(&approval_id, ApprovalAction::Approve, 700, 700, message_id).await;
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert_eq!(backend.story_view_call_count().await, 1);

    let duplicate = client
        .call_tool(
            CallToolRequestParams::new("view_story").with_arguments(
                json!({ "chat_id": "-100505", "story_ids": [41, 41] })
                    .as_object()
                    .expect("object")
                    .clone(),
            ),
        )
        .await?;
    assert_eq!(duplicate.is_error, Some(true));

    let cards = format!("{:?}\n{:?}", bot.sent_mutation_cards().await, bot.mutation_edits().await);
    assert!(!cards.contains("Release rehearsal"));
    assert!(!cards.contains("access_hash"));
    assert!(!cards.contains("file_reference"));

    client.cancel().await?;
    server_task.abort();
    worker.abort();
    Ok(())
}

#[tokio::test]
async fn story_publish_and_delete_are_hash_bound_exactly_once_and_read_back() -> anyhow::Result<()>
{
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

    let downloaded = client
        .call_tool(CallToolRequestParams::new("download_attachment").with_arguments(
            json!({ "chat_id": "@alex", "message_id": 11 }).as_object().expect("object").clone(),
        ))
        .await?;
    assert_eq!(downloaded.is_error, Some(false));
    let downloaded_data = downloaded.structured_content.expect("downloaded attachment");
    let downloaded_path = std::path::Path::new(
        downloaded_data
            .pointer("/data/local_path")
            .and_then(Value::as_str)
            .expect("managed download path"),
    )
    .to_path_buf();
    let managed_dir = downloaded_path.parent().expect("managed account cache").to_path_buf();
    let story_path = managed_dir.join("story-e2e.png");
    let story_bytes = [
        0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, b'I', b'H', b'D',
        b'R', 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1f,
        0x15, 0xc4, 0x89,
    ];
    std::fs::write(&story_path, story_bytes)?;
    let story_sha256 = format!("{:x}", Sha256::digest(story_bytes));

    let invalid_media = client
        .call_tool(
            CallToolRequestParams::new("send_story").with_arguments(
                json!({
                    "chat_id": "-100505",
                    "file_path": downloaded_path,
                    "approved_sha256": downloaded_data
                        .pointer("/data/sha256")
                        .and_then(Value::as_str)
                        .expect("download hash")
                })
                .as_object()
                .expect("object")
                .clone(),
            ),
        )
        .await?;
    assert_eq!(invalid_media.is_error, Some(true));

    for invalid_args in [
        json!({
            "chat_id": "-100505",
            "file_path": story_path,
            "approved_sha256": story_sha256,
            "privacy": "unknown"
        }),
        json!({
            "chat_id": "-100505",
            "file_path": story_path,
            "approved_sha256": story_sha256,
            "privacy": "everyone",
            "selected_user_refs": ["@alex"]
        }),
        json!({
            "chat_id": "-100505",
            "file_path": story_path,
            "approved_sha256": story_sha256,
            "period_seconds": 100
        }),
        json!({
            "chat_id": "-100505",
            "file_path": story_path,
            "approved_sha256": "0".repeat(64)
        }),
    ] {
        let invalid = client
            .call_tool(
                CallToolRequestParams::new("send_story")
                    .with_arguments(invalid_args.as_object().expect("object").clone()),
            )
            .await?;
        assert_eq!(invalid.is_error, Some(true));
    }

    let (sent, send_approval_id, send_message_id, send_prepared) = approve_mutation!(
        client,
        bot,
        state_db_path,
        "send_story",
        json!({
            "chat_id": "-100505",
            "file_path": story_path,
            "approved_sha256": story_sha256,
            "caption": "Exact story release",
            "privacy": "selected_contacts",
            "selected_user_refs": ["@alex"],
            "pinned": true,
            "noforwards": true,
            "period_seconds": 43200
        })
    );
    assert_eq!(sent.pointer("/kind").and_then(Value::as_str), Some("story_sent"));
    assert_eq!(sent.pointer("/story/caption").and_then(Value::as_str), Some("Exact story release"));
    assert_eq!(sent.pointer("/story/selected_contacts").and_then(Value::as_bool), Some(true));
    assert_eq!(sent.pointer("/story/pinned").and_then(Value::as_bool), Some(true));
    assert_eq!(sent.pointer("/story/noforwards").and_then(Value::as_bool), Some(true));
    let story_id = sent.pointer("/story/story_id").and_then(Value::as_i64).expect("sent story id");
    assert_eq!(
        send_prepared.pointer("/data/action/privacy/selected_user_ids/0").and_then(Value::as_i64),
        Some(101)
    );
    assert_eq!(
        send_prepared.pointer("/data/action/artifact/sha256").and_then(Value::as_str),
        Some(story_sha256.as_str())
    );
    let artifact_id = send_prepared
        .pointer("/data/action/artifact/artifact_id")
        .and_then(Value::as_str)
        .expect("story artifact id");
    wait_for_path_missing(&state_db_path.with_extension("outbox").join(artifact_id)).await?;
    assert_eq!(backend.story_mutation_call_counts().await, (0, 1, 0));

    bot.push_mutation_decision(
        &send_approval_id,
        ApprovalAction::Approve,
        700,
        700,
        send_message_id,
    )
    .await;
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert_eq!(backend.story_mutation_call_counts().await, (0, 1, 0));

    let readback =
        client
            .call_tool(CallToolRequestParams::new("get_peer_stories").with_arguments(
                json!({ "chat_id": "-100505" }).as_object().expect("object").clone(),
            ))
            .await?;
    assert_eq!(readback.is_error, Some(false));
    assert!(
        readback
            .structured_content
            .as_ref()
            .and_then(|value| value.pointer("/data/page/peer/stories"))
            .and_then(Value::as_array)
            .is_some_and(|stories| stories.iter().any(|story| {
                story.pointer("/story_id").and_then(Value::as_i64) == Some(story_id)
            }))
    );

    let invalid_edit = client
        .call_tool(
            CallToolRequestParams::new("edit_story").with_arguments(
                json!({
                    "chat_id": "-100505",
                    "story_id": story_id,
                    "file_path": story_path,
                    "caption": "Edited without a bound artifact hash"
                })
                .as_object()
                .expect("object")
                .clone(),
            ),
        )
        .await?;
    assert_eq!(invalid_edit.is_error, Some(true));

    let (edited, edit_approval_id, edit_message_id, edit_prepared) = approve_mutation!(
        client,
        bot,
        state_db_path,
        "edit_story",
        json!({
            "chat_id": "-100505",
            "story_id": story_id,
            "file_path": story_path,
            "approved_sha256": story_sha256,
            "caption": "Edited exact story",
            "privacy": "contacts",
            "disallowed_user_refs": ["@mira_ops"]
        })
    );
    assert_eq!(edited.pointer("/kind").and_then(Value::as_str), Some("story_edited"));
    assert_eq!(
        edited.pointer("/story/caption").and_then(Value::as_str),
        Some("Edited exact story")
    );
    assert_eq!(edited.pointer("/story/contacts").and_then(Value::as_bool), Some(true));
    assert_eq!(
        edited.pointer("/source_sha256").and_then(Value::as_str),
        Some(story_sha256.as_str())
    );
    assert_eq!(
        edit_prepared.pointer("/data/action/privacy/disallowed_user_ids/0").and_then(Value::as_i64),
        Some(303)
    );
    let edit_artifact_id = edit_prepared
        .pointer("/data/action/artifact/artifact_id")
        .and_then(Value::as_str)
        .expect("edit story artifact id");
    wait_for_path_missing(&state_db_path.with_extension("outbox").join(edit_artifact_id)).await?;
    assert_eq!(backend.story_edit_call_count().await, 1);

    bot.push_mutation_decision(
        &edit_approval_id,
        ApprovalAction::Approve,
        700,
        700,
        edit_message_id,
    )
    .await;
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert_eq!(backend.story_edit_call_count().await, 1);

    let duplicate_delete = client
        .call_tool(
            CallToolRequestParams::new("delete_story").with_arguments(
                json!({ "chat_id": "-100505", "story_ids": [story_id, story_id] })
                    .as_object()
                    .expect("object")
                    .clone(),
            ),
        )
        .await?;
    assert_eq!(duplicate_delete.is_error, Some(true));

    let (deleted, delete_approval_id, delete_message_id, _) = approve_mutation!(
        client,
        bot,
        state_db_path,
        "delete_story",
        json!({ "chat_id": "-100505", "story_ids": [story_id] })
    );
    assert_eq!(deleted.pointer("/kind").and_then(Value::as_str), Some("stories_deleted"));
    assert_eq!(deleted.pointer("/story_ids/0").and_then(Value::as_i64), Some(story_id));
    assert_eq!(backend.story_mutation_call_counts().await, (0, 1, 1));

    bot.push_mutation_decision(
        &delete_approval_id,
        ApprovalAction::Approve,
        700,
        700,
        delete_message_id,
    )
    .await;
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert_eq!(backend.story_mutation_call_counts().await, (0, 1, 1));

    let after_delete =
        client
            .call_tool(CallToolRequestParams::new("get_peer_stories").with_arguments(
                json!({ "chat_id": "-100505" }).as_object().expect("object").clone(),
            ))
            .await?;
    assert_eq!(after_delete.is_error, Some(false));
    assert!(
        after_delete
            .structured_content
            .as_ref()
            .and_then(|value| value.pointer("/data/page/peer/stories"))
            .and_then(Value::as_array)
            .is_some_and(|stories| stories.iter().all(|story| {
                story.pointer("/story_id").and_then(Value::as_i64) != Some(story_id)
            }))
    );

    let cards = format!("{:?}\n{:?}", bot.sent_mutation_cards().await, bot.mutation_edits().await);
    assert!(cards.contains(&story_sha256));
    assert!(cards.contains("selected_contacts"));
    assert!(cards.contains("contacts"));
    assert!(!cards.contains("@alex"));
    assert!(!cards.contains("@mira_ops"));
    assert!(!cards.contains("access_hash"));
    assert!(!cards.contains("file_reference"));

    client.cancel().await?;
    server_task.abort();
    worker.abort();
    Ok(())
}

#[tokio::test]
async fn story_state_mutations_are_approved_read_back_and_toggle_safe() -> anyhow::Result<()> {
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

    let (read, _, _, _) = approve_mutation!(
        client,
        bot,
        state_db_path,
        "read_stories",
        json!({ "chat_id": "-100505", "max_id": 42 })
    );
    assert_eq!(read.pointer("/kind").and_then(Value::as_str), Some("stories_read"));
    assert_eq!(read.pointer("/max_id").and_then(Value::as_i64), Some(42));

    let invalid_big = client
        .call_tool(
            CallToolRequestParams::new("react_to_story").with_arguments(
                json!({
                    "chat_id": "-100505",
                    "story_id": 42,
                    "reaction": { "kind": "emoji", "emoji": "🔥", "big": true }
                })
                .as_object()
                .expect("object")
                .clone(),
            ),
        )
        .await?;
    assert_eq!(invalid_big.is_error, Some(true));

    let (reacted, _, _, _) = approve_mutation!(
        client,
        bot,
        state_db_path,
        "react_to_story",
        json!({
            "chat_id": "-100505",
            "story_id": 42,
            "reaction": { "kind": "emoji", "emoji": "🔥", "add_to_recent": true }
        })
    );
    assert_eq!(reacted.pointer("/kind").and_then(Value::as_str), Some("story_reaction_updated"));
    assert_eq!(reacted.pointer("/reaction/emoji").and_then(Value::as_str), Some("🔥"));

    let (pinned, _, _, _) = approve_mutation!(
        client,
        bot,
        state_db_path,
        "toggle_story_pinned",
        json!({ "chat_id": "-100505", "story_ids": [42], "pinned": true })
    );
    assert_eq!(pinned.pointer("/kind").and_then(Value::as_str), Some("stories_pinned_updated"));
    assert_eq!(pinned.pointer("/pinned").and_then(Value::as_bool), Some(true));

    let (top_pinned, top_approval_id, top_message_id, _) = approve_mutation!(
        client,
        bot,
        state_db_path,
        "toggle_story_pinned_to_top",
        json!({
            "chat_id": "-100505",
            "story_ids": [42],
            "pinned_to_top": true
        })
    );
    assert_eq!(
        top_pinned.pointer("/kind").and_then(Value::as_str),
        Some("stories_pinned_to_top_updated")
    );
    assert_eq!(top_pinned.pointer("/pinned_to_top").and_then(Value::as_bool), Some(true));

    let no_change_top = client
        .call_tool(
            CallToolRequestParams::new("toggle_story_pinned_to_top").with_arguments(
                json!({
                    "chat_id": "-100505",
                    "story_ids": [41, 42],
                    "pinned_to_top": true
                })
                .as_object()
                .expect("object")
                .clone(),
            ),
        )
        .await?;
    assert_eq!(no_change_top.is_error, Some(true));

    let invalid_stealth = client
        .call_tool(CallToolRequestParams::new("activate_stealth_mode").with_arguments(
            json!({ "past": false, "future": false }).as_object().expect("object").clone(),
        ))
        .await?;
    assert_eq!(invalid_stealth.is_error, Some(true));

    let (stealth, stealth_approval_id, stealth_message_id, _) = approve_mutation!(
        client,
        bot,
        state_db_path,
        "activate_stealth_mode",
        json!({ "past": true, "future": true })
    );
    assert_eq!(
        stealth.pointer("/kind").and_then(Value::as_str),
        Some("story_stealth_mode_activated")
    );
    assert_eq!(
        stealth.pointer("/mode/active_until_unix").and_then(Value::as_i64),
        Some(2_000_000_000)
    );

    let readback = client
        .call_tool(CallToolRequestParams::new("get_stories_by_id").with_arguments(
            json!({ "chat_id": "-100505", "story_ids": [42] }).as_object().expect("object").clone(),
        ))
        .await?;
    assert_eq!(readback.is_error, Some(false));
    assert_eq!(
        readback
            .structured_content
            .as_ref()
            .and_then(|value| value.pointer("/data/page/stories/0/pinned"))
            .and_then(Value::as_bool),
        Some(true)
    );
    assert_eq!(
        readback
            .structured_content
            .as_ref()
            .and_then(|value| value.pointer("/data/page/stories/0/sent_reaction/emoji"))
            .and_then(Value::as_str),
        Some("🔥")
    );
    assert!(
        readback
            .structured_content
            .as_ref()
            .and_then(|value| value.pointer("/data/page/pinned_to_top_ids"))
            .and_then(Value::as_array)
            .is_some_and(|ids| ids.iter().any(|id| id.as_i64() == Some(42)))
    );
    assert_eq!(backend.story_extended_call_counts().await, (1, 1, 1, 1, 1));

    bot.push_mutation_decision(&top_approval_id, ApprovalAction::Approve, 700, 700, top_message_id)
        .await;
    bot.push_mutation_decision(
        &stealth_approval_id,
        ApprovalAction::Approve,
        700,
        700,
        stealth_message_id,
    )
    .await;
    tokio::time::sleep(Duration::from_millis(150)).await;
    assert_eq!(backend.story_extended_call_counts().await, (1, 1, 1, 1, 1));

    let cards = format!("{:?}\n{:?}", bot.sent_mutation_cards().await, bot.mutation_edits().await);
    assert!(!cards.contains("Release rehearsal"));
    assert!(!cards.contains("Ship it"));
    assert!(!cards.contains("access_hash"));
    assert!(!cards.contains("file_reference"));

    client.cancel().await?;
    server_task.abort();
    worker.abort();
    Ok(())
}

#[tokio::test]
async fn story_reporting_preserves_the_provider_flow_and_is_exactly_once() -> anyhow::Result<()> {
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

    for invalid_args in [
        json!({ "chat_id": "-100505", "story_ids": [41, 41] }),
        json!({ "chat_id": "-100505", "story_ids": [41], "option_hex": "0A" }),
        json!({ "chat_id": "-100505", "story_ids": [41], "option_hex": "0" }),
    ] {
        let invalid = client
            .call_tool(
                CallToolRequestParams::new("report_story")
                    .with_arguments(invalid_args.as_object().expect("object").clone()),
            )
            .await?;
        assert_eq!(invalid.is_error, Some(true));
    }

    let (choose, choose_approval_id, choose_message_id, choose_prepared) = approve_mutation!(
        client,
        bot,
        state_db_path,
        "report_story",
        json!({ "chat_id": "-100505", "story_ids": [41] })
    );
    assert_eq!(choose.pointer("/kind").and_then(Value::as_str), Some("story_report_step"));
    assert_eq!(choose.pointer("/report/kind").and_then(Value::as_str), Some("choose_option"));
    assert_eq!(choose.pointer("/report/options/1/option_hex").and_then(Value::as_str), Some("02"));
    assert_eq!(
        choose_prepared.pointer("/data/action/before/0/story_id").and_then(Value::as_i64),
        Some(41)
    );
    assert!(choose_prepared.pointer("/data/action/before/0/caption").is_none());
    assert_eq!(backend.story_report_call_count().await, 1);

    bot.push_mutation_decision(
        &choose_approval_id,
        ApprovalAction::Approve,
        700,
        700,
        choose_message_id,
    )
    .await;
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert_eq!(backend.story_report_call_count().await, 1);

    let (comment, _, _, _) = approve_mutation!(
        client,
        bot,
        state_db_path,
        "report_story",
        json!({
            "chat_id": "-100505",
            "story_ids": [41],
            "option_hex": "02"
        })
    );
    assert_eq!(comment.pointer("/report/kind").and_then(Value::as_str), Some("add_comment"));
    assert_eq!(comment.pointer("/report/optional").and_then(Value::as_bool), Some(false));
    assert_eq!(backend.story_report_call_count().await, 2);

    let (reported, reported_approval_id, reported_message_id, _) = approve_mutation!(
        client,
        bot,
        state_db_path,
        "report_story",
        json!({
            "chat_id": "-100505",
            "story_ids": [41],
            "option_hex": "02",
            "message": "The exact story depicts violence"
        })
    );
    assert_eq!(reported.pointer("/report/kind").and_then(Value::as_str), Some("reported"));
    assert_eq!(backend.story_report_call_count().await, 3);

    bot.push_mutation_decision(
        &reported_approval_id,
        ApprovalAction::Approve,
        700,
        700,
        reported_message_id,
    )
    .await;
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert_eq!(backend.story_report_call_count().await, 3);

    let cards = format!("{:?}\n{:?}", bot.sent_mutation_cards().await, bot.mutation_edits().await);
    assert!(cards.contains("The exact story depicts violence"));
    assert!(cards.contains("option_hex"));
    assert!(!cards.contains("Release rehearsal"));
    assert!(!cards.contains("access_hash"));
    assert!(!cards.contains("file_reference"));

    client.cancel().await?;
    server_task.abort();
    worker.abort();
    Ok(())
}

#[tokio::test]
async fn uncertain_story_report_recovery_never_replays_the_provider_call() -> anyhow::Result<()> {
    let backend = Arc::new(FakeTelegramBackend::fixture());
    backend.set_story_report_uncertain_once().await;
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

    let prepared = client
        .call_tool(CallToolRequestParams::new("report_story").with_arguments(
            json!({ "chat_id": "-100505", "story_ids": [42] }).as_object().expect("object").clone(),
        ))
        .await?;
    assert_eq!(prepared.is_error, Some(false));
    let prepared_data = prepared.structured_content.expect("prepared report");
    let requested = client
        .call_tool(
            CallToolRequestParams::new("request_mutation_approval").with_arguments(
                json!({
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
    let requested_data = requested.structured_content.expect("approval");
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

    let ledger = MutationLedger::new(state_db_path.clone());
    let mut last = None;
    for _ in 0..900 {
        let approval = ledger.get_approval(&approval_id)?.expect("report approval");
        last = Some((approval.state, approval.automated_retry_count));
        if approval.state == MutationApprovalState::OutcomeUnknown
            && approval.automated_retry_count >= 2
        {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    let approval = ledger.get_approval(&approval_id)?.expect("report approval");
    assert_eq!(
        approval.state,
        MutationApprovalState::OutcomeUnknown,
        "last recovery state: {last:?}"
    );
    assert!(approval.automated_retry_count >= 2, "last recovery state: {last:?}");
    assert_eq!(backend.story_report_call_count().await, 1);

    client.cancel().await?;
    server_task.abort();
    worker.abort();
    Ok(())
}
