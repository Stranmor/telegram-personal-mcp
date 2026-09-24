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
use std::time::Duration;
use telegram_personal_mcp::{
    ApprovalAction, ApprovalPolicy, FakeApprovalBot, FakeTelegramBackend, McpServer,
    McpServerSettings, MutationApprovalState, MutationLedger, TelegramBackend,
};

const VIDEO_MP4_HEX: &str = "000000206674797069736f6d0000020069736f6d69736f32617663316d703431000003756d6f6f760000006c6d766864000000000000000000000000000003e8000003e800010000010000000000000000000000000100000000000000000000000000000001000000000000000000000000000040000000000000000000000000000000000000000000000000000000000000020000029f7472616b0000005c746b68640000000300000000000000000000000100000000000003e800000000000000000000000000000000000100000000000000000000000000000001000000000000000000000000000040000000001000000010000000000024656474730000001c656c73740000000000000001000003e80000100000010000000002176d646961000000206d646864000000000000000000000000000028000000280055c400000000002d68646c72000000000000000076696465000000000000000000000000566964656f48616e646c657200000001c26d696e6600000014766d68640000000100000000000000000000002464696e660000001c6472656600000000000000010000000c75726c2000000001000001827374626c000000be737473640000000000000001000000ae61766331000000000000000100000000000000000000000000000000001000100048000000480000000000000001154c61766336322e32382e313032206c696278323634000000000000000000000018ffff00000034617663430164000affe100176764000aacd95ec044000003000400000300283c48965801000668ebe3cb22c0fdf8f80000000010706173700000000100000001000000146274727400000000000017a000000000000000187374747300000000000000010000000500000800000000147374737300000000000000010000000100000038637474730000000000000005000000010000100000000001000028000000000100001000000000010000000000000001000008000000001c737473630000000000000001000000010000000500000001000000287374737a000000000000000000000005000002c40000000c0000000c0000000c0000000c000000147374636f0000000000000001000003a500000062756474610000005a6d657461000000000000002168646c7200000000000000006d6469726170706c0000000000000000000000002d696c737400000025a9746f6f0000001d6461746100000001000000004c61766636322e31322e3130320000000866726565000002fc6d646174000002ad0605ffffa9dc45e9bde6d948b7962cd820d923eeef78323634202d20636f7265203136352072333232322062333536303561202d20482e3236342f4d5045472d342041564320636f646563202d20436f70796c65667420323030332d32303235202d20687474703a2f2f7777772e766964656f6c616e2e6f72672f783236342e68746d6c202d206f7074696f6e733a2063616261633d31207265663d33206465626c6f636b3d313a303a3020616e616c7973653d3078333a3078313133206d653d686578207375626d653d37207073793d31207073795f72643d312e30303a302e3030206d697865645f7265663d31206d655f72616e67653d3136206368726f6d615f6d653d31207472656c6c69733d31203878386463743d312063716d3d3020646561647a6f6e653d32312c313120666173745f70736b69703d31206368726f6d615f71705f6f66667365743d2d3220746872656164733d31206c6f6f6b61686561645f746872656164733d3120736c696365645f746872656164733d30206e723d3020646563696d6174653d3120696e7465726c616365643d3020626c757261795f636f6d7061743d3020636f6e73747261696e65645f696e7472613d3020626672616d65733d3320625f707972616d69643d3220625f61646170743d3120625f626961733d30206469726563743d3120776569676874623d31206f70656e5f676f703d3020776569676874703d32206b6579696e743d323530206b6579696e745f6d696e3d35207363656e656375743d343020696e7472615f726566726573683d302072635f6c6f6b61686561643d34302072633d637266206d62747265653d31206372663d32332e302071636f6d703d302e36302071706d696e3d302071706d61783d3639207170737465703d342069705f726174696f3d312e34302061713d313a312e303000800000000f6588840012fffef7addf814dc32b3d00000008419a246c43fffee000000008419e4278821fab8100000008019e617443ffb38000000008019e636a43ffb381";
const ONE_PIXEL_PNG: &[u8] = b"\x89PNG\r\n\x1a\n\x00\x00\x00\rIHDR\x00\x00\x00\x01\x00\x00\x00\x01\x08\x06\x00\x00\x00\x1f\x15\xc4\x89\x00\x00\x00\x0dIDAT\x08\xd7c\xf8\xcf\xc0\xf0\x1f\x00\x05\x00\x01\xff\x89\x99=\x1d\x00\x00\x00\x00IEND\xaeB\x60\x82";

fn decode_hex(hex: &str) -> Vec<u8> {
    hex.as_bytes()
        .chunks_exact(2)
        .map(|pair| {
            let high = (pair[0] as char).to_digit(16).expect("high nibble");
            let low = (pair[1] as char).to_digit(16).expect("low nibble");
            ((high << 4) | low) as u8
        })
        .collect()
}

async fn wait_for_applied(
    state_db_path: &std::path::Path,
    approval_id: &str,
) -> anyhow::Result<()> {
    let ledger = MutationLedger::new(state_db_path.to_path_buf());
    for _ in 0..900 {
        if ledger
            .get_approval(approval_id)?
            .as_ref()
            .is_some_and(|approval| approval.state == MutationApprovalState::Applied)
        {
            return Ok(());
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    let observed = ledger.get_approval(approval_id)?;
    anyhow::bail!("album mutation did not become applied: {observed:?}")
}

#[tokio::test]
async fn mixed_album_is_grouped_topic_bound_and_recovered_without_duplicates() -> anyhow::Result<()>
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
            json!({ "chat_id": "@alex", "message_id": 11 }).as_object().unwrap().clone(),
        ))
        .await?;
    let managed_dir = std::path::Path::new(
        downloaded
            .structured_content
            .as_ref()
            .and_then(|value| value.pointer("/data/local_path"))
            .and_then(Value::as_str)
            .expect("managed path"),
    )
    .parent()
    .unwrap()
    .to_path_buf();
    let photo_path = managed_dir.join("album-photo.png");
    let video_path = managed_dir.join("album-video.mp4");
    let video_bytes = decode_hex(VIDEO_MP4_HEX);
    std::fs::write(&photo_path, ONE_PIXEL_PNG)?;
    std::fs::write(&video_path, &video_bytes)?;
    let photo_sha256 = format!("{:x}", Sha256::digest(ONE_PIXEL_PNG));
    let video_sha256 = format!("{:x}", Sha256::digest(&video_bytes));

    let invalid_count = client
        .call_tool(
            CallToolRequestParams::new("send_album").with_arguments(
                json!({
                    "chat_id": "-202",
                    "items": [{
                        "file_path": photo_path,
                        "approved_sha256": photo_sha256
                    }]
                })
                .as_object()
                .unwrap()
                .clone(),
            ),
        )
        .await?;
    assert_eq!(invalid_count.is_error, Some(true));

    backend.make_next_album_send_outcome_uncertain().await;
    let prepared = client
        .call_tool(
            CallToolRequestParams::new("send_album").with_arguments(
                json!({
                    "chat_id": "-202",
                    "items": [
                        {
                            "file_path": photo_path,
                            "caption": "Photo item",
                            "approved_sha256": photo_sha256
                        },
                        {
                            "file_path": video_path,
                            "caption": "Second **video**",
                            "approved_sha256": video_sha256
                        }
                    ],
                    "caption": "<b>Album</b> caption",
                    "parse_mode": "html",
                    "topic_id": 44,
                    "silent": true,
                    "noforwards": true
                })
                .as_object()
                .unwrap()
                .clone(),
            ),
        )
        .await?;
    assert_eq!(prepared.is_error, Some(false));
    let prepared = prepared.structured_content.expect("prepared album");
    assert_eq!(prepared.pointer("/data/action/kind").and_then(Value::as_str), Some("send_album"));
    assert_eq!(
        prepared.pointer("/data/action/items/0/media_kind").and_then(Value::as_str),
        Some("photo")
    );
    assert_eq!(
        prepared.pointer("/data/action/items/1/media_kind").and_then(Value::as_str),
        Some("video")
    );
    assert_eq!(
        prepared.pointer("/data/action/items/0/caption").and_then(Value::as_str),
        Some("<b>Album</b> caption")
    );
    assert_eq!(
        prepared.pointer("/data/action/caption_parse_mode").and_then(Value::as_str),
        Some("html")
    );

    let requested = client
        .call_tool(CallToolRequestParams::new("request_mutation_approval").with_arguments(
            json!({
                "mutation_id": prepared.pointer("/data/mutation_id").and_then(Value::as_str).unwrap(),
                "approved_payload_sha256": prepared.pointer("/data/payload_sha256").and_then(Value::as_str).unwrap()
            }).as_object().unwrap().clone(),
        ))
        .await?;
    assert_eq!(requested.is_error, Some(false));
    let requested = requested.structured_content.expect("approval request");
    let approval_id =
        requested.pointer("/data/approval_id").and_then(Value::as_str).unwrap().to_string();
    let approval_message_id =
        requested.pointer("/data/approval_message_id").and_then(Value::as_i64).unwrap();
    bot.push_mutation_decision(
        &approval_id,
        ApprovalAction::Approve,
        700,
        700,
        approval_message_id,
    )
    .await;
    wait_for_applied(&state_db_path, &approval_id).await?;

    let approval = MutationLedger::new(state_db_path.clone()).get_approval(&approval_id)?.unwrap();
    let result = serde_json::to_value(approval.result.unwrap())?;
    assert_eq!(result.pointer("/kind").and_then(Value::as_str), Some("album_sent"));
    assert_eq!(result.pointer("/message_ids").and_then(Value::as_array).map(Vec::len), Some(2));
    assert!(result.pointer("/grouped_id").and_then(Value::as_i64).unwrap() > 0);
    assert_eq!(result.pointer("/sha256s/0").and_then(Value::as_str), Some(photo_sha256.as_str()));
    assert_eq!(result.pointer("/sha256s/1").and_then(Value::as_str), Some(video_sha256.as_str()));
    assert_eq!(backend.album_send_call_count().await, 2);

    let sent = backend.sent_messages().await;
    assert_eq!(sent.len(), 2);
    assert_eq!(sent[0].text, "Album caption");
    assert_eq!(sent[1].text, "Second **video**");
    let grouped_id =
        backend.get_message_grouped_id(None, "-202", sent[0].message_id).await?.unwrap();
    for message in &sent {
        assert_eq!(
            backend.get_message_grouped_id(None, "-202", message.message_id).await?,
            Some(grouped_id)
        );
        assert_eq!(backend.get_message_topic_id(None, "-202", message.message_id).await?, Some(44));
        assert_eq!(message.reply_to_message_id, Some(44));
    }
    for index in 0..2 {
        let artifact_id = prepared
            .pointer(&format!("/data/action/items/{index}/artifact/artifact_id"))
            .and_then(Value::as_str)
            .unwrap();
        let artifact_path = state_db_path.with_extension("outbox").join(artifact_id);
        let mut removed = !artifact_path.exists();
        for _ in 0..100 {
            if removed {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
            removed = !artifact_path.exists();
        }
        assert!(removed, "album artifact was not cleaned: {}", artifact_path.display());
    }

    bot.push_mutation_decision(
        &approval_id,
        ApprovalAction::Approve,
        700,
        700,
        approval_message_id,
    )
    .await;
    tokio::time::sleep(Duration::from_millis(150)).await;
    assert_eq!(backend.album_send_call_count().await, 2);
    assert_eq!(backend.sent_messages().await.len(), 2);
    let cards = format!("{:?}\n{:?}", bot.sent_mutation_cards().await, bot.mutation_edits().await);
    assert!(cards.contains("SendAlbum"));
    assert!(cards.contains(photo_sha256.as_str()));
    assert!(cards.contains(video_sha256.as_str()));

    let original_message_ids = sent.iter().map(|message| message.message_id).collect::<Vec<_>>();
    let prepared_delete = client
        .call_tool(
            CallToolRequestParams::new("prepare_delete_messages").with_arguments(
                json!({
                    "chat_id": "-202",
                    "message_ids": original_message_ids,
                    "revoke": true
                })
                .as_object()
                .unwrap()
                .clone(),
            ),
        )
        .await?;
    assert_eq!(prepared_delete.is_error, Some(false));
    let prepared_delete = prepared_delete.structured_content.expect("prepared album deletion");
    assert_eq!(
        prepared_delete
            .pointer("/data/action/messages/0/restore/grouped_id")
            .and_then(Value::as_i64),
        Some(grouped_id)
    );
    assert!(
        prepared_delete
            .pointer("/data/action/messages/0/restore/media/artifact/sha256")
            .and_then(Value::as_str)
            .is_some()
    );
    let requested_delete = client
        .call_tool(
            CallToolRequestParams::new("request_mutation_approval").with_arguments(
                json!({
                    "mutation_id": prepared_delete
                        .pointer("/data/mutation_id")
                        .and_then(Value::as_str)
                        .unwrap(),
                    "approved_payload_sha256": prepared_delete
                        .pointer("/data/payload_sha256")
                        .and_then(Value::as_str)
                        .unwrap()
                })
                .as_object()
                .unwrap()
                .clone(),
            ),
        )
        .await?;
    let requested_delete = requested_delete.structured_content.expect("delete approval");
    let deletion_approval_id =
        requested_delete.pointer("/data/approval_id").and_then(Value::as_str).unwrap().to_string();
    bot.push_mutation_decision(
        &deletion_approval_id,
        ApprovalAction::Approve,
        700,
        700,
        requested_delete.pointer("/data/approval_message_id").and_then(Value::as_i64).unwrap(),
    )
    .await;
    wait_for_applied(&state_db_path, &deletion_approval_id).await?;
    for message_id in &original_message_ids {
        assert!(backend.get_message(None, "-202", *message_id).await?.is_none());
    }

    let prepared_restore = client
        .call_tool(
            CallToolRequestParams::new("prepare_restore_deleted_messages").with_arguments(
                json!({
                    "deletion_approval_id": deletion_approval_id,
                    "message_ids": original_message_ids
                })
                .as_object()
                .unwrap()
                .clone(),
            ),
        )
        .await?;
    assert_eq!(prepared_restore.is_error, Some(false));
    let prepared_restore = prepared_restore.structured_content.expect("prepared album restoration");
    assert_eq!(
        prepared_restore.pointer("/data/action/kind").and_then(Value::as_str),
        Some("restore_deleted_messages")
    );
    let requested_restore = client
        .call_tool(
            CallToolRequestParams::new("request_mutation_approval").with_arguments(
                json!({
                    "mutation_id": prepared_restore
                        .pointer("/data/mutation_id")
                        .and_then(Value::as_str)
                        .unwrap(),
                    "approved_payload_sha256": prepared_restore
                        .pointer("/data/payload_sha256")
                        .and_then(Value::as_str)
                        .unwrap()
                })
                .as_object()
                .unwrap()
                .clone(),
            ),
        )
        .await?;
    let requested_restore = requested_restore.structured_content.expect("restore approval");
    let restore_approval_id =
        requested_restore.pointer("/data/approval_id").and_then(Value::as_str).unwrap().to_string();
    bot.push_mutation_decision(
        &restore_approval_id,
        ApprovalAction::Approve,
        700,
        700,
        requested_restore.pointer("/data/approval_message_id").and_then(Value::as_i64).unwrap(),
    )
    .await;
    wait_for_applied(&state_db_path, &restore_approval_id).await?;
    let restored_approval =
        MutationLedger::new(state_db_path.clone()).get_approval(&restore_approval_id)?.unwrap();
    let restored_result = serde_json::to_value(restored_approval.result.unwrap())?;
    let mappings =
        restored_result.pointer("/mappings").and_then(Value::as_array).expect("restore mappings");
    assert_eq!(mappings.len(), 2);
    let restored_message_ids = mappings
        .iter()
        .map(|mapping| {
            mapping
                .pointer("/restored_message_id")
                .and_then(Value::as_i64)
                .and_then(|value| i32::try_from(value).ok())
                .expect("restored message id")
        })
        .collect::<Vec<_>>();
    let restored_grouped_id = backend
        .get_message_grouped_id(None, "-202", restored_message_ids[0])
        .await?
        .expect("restored grouped id");
    assert_ne!(restored_grouped_id, grouped_id);
    for restored_message_id in restored_message_ids {
        let message = backend
            .get_message(None, "-202", restored_message_id)
            .await?
            .expect("restored album message");
        assert!(message.outgoing);
        assert!(message.silent);
        assert!(message.noforwards);
        assert_eq!(message.reply_to_message_id, Some(44));
        assert_eq!(
            backend.get_message_grouped_id(None, "-202", restored_message_id).await?,
            Some(restored_grouped_id)
        );
    }
    assert_eq!(backend.album_send_call_count().await, 3);

    let prepared_restore_again = client
        .call_tool(
            CallToolRequestParams::new("prepare_restore_deleted_messages").with_arguments(
                json!({
                    "deletion_approval_id": deletion_approval_id,
                    "message_ids": original_message_ids
                })
                .as_object()
                .unwrap()
                .clone(),
            ),
        )
        .await?;
    assert_eq!(prepared_restore_again.is_error, Some(false));
    let prepared_restore_again =
        prepared_restore_again.structured_content.expect("prepared idempotent restoration");
    let requested_restore_again = client
        .call_tool(
            CallToolRequestParams::new("request_mutation_approval").with_arguments(
                json!({
                    "mutation_id": prepared_restore_again
                        .pointer("/data/mutation_id")
                        .and_then(Value::as_str)
                        .unwrap(),
                    "approved_payload_sha256": prepared_restore_again
                        .pointer("/data/payload_sha256")
                        .and_then(Value::as_str)
                        .unwrap()
                })
                .as_object()
                .unwrap()
                .clone(),
            ),
        )
        .await?;
    let requested_restore_again =
        requested_restore_again.structured_content.expect("idempotent restore approval");
    let restore_again_approval_id = requested_restore_again
        .pointer("/data/approval_id")
        .and_then(Value::as_str)
        .unwrap()
        .to_string();
    bot.push_mutation_decision(
        &restore_again_approval_id,
        ApprovalAction::Approve,
        700,
        700,
        requested_restore_again
            .pointer("/data/approval_message_id")
            .and_then(Value::as_i64)
            .unwrap(),
    )
    .await;
    wait_for_applied(&state_db_path, &restore_again_approval_id).await?;
    assert_eq!(backend.sent_messages().await.len(), 4);
    assert_eq!(backend.album_send_call_count().await, 4);

    client.cancel().await?;
    server_task.abort();
    worker.abort();
    Ok(())
}
