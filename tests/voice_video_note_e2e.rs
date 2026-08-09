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
    McpServerSettings, MutationApproval, MutationApprovalState, MutationLedger, TelegramBackend,
};

const VOICE_OGG_HEX: &str = "4f6767530002000000000000000076fd66ba00000000605442e201134f707573486561640101380180bb00000000004f6767530000000000000000000076fd66ba01000000deb0cd9a013e4f707573546167730d0000004c61766636322e31322e313032010000001d000000656e636f6465723d4c61766336322e32382e313032206c69626f7075734f6767530004b82600000000000076fd66ba02000000cf4e799f0b0f11121212121212121212780be4c136ecc58d8c494699a426b07807c97227e144ea55f1f0c0d5dde952607807c979c8c957c0a21223faef67f364c0e07807c979c8c957c0a21223faef67f364c0e07807c979c8c957c0a21223faef67f364c0e07807c979c8c957c0a21223faef67f364c0e07807c979c8c957c0a21223faef67f364c0e07807c979c8c957c0a21223faef67f364c0e07807c979c8c957c0a21223faef67f364c0e07807c979c8c957c0a21223faef67f364c0e07807c979c8c957c0a21223faef67f364c0e0";
const VIDEO_MP4_HEX: &str = "000000206674797069736f6d0000020069736f6d69736f32617663316d703431000003756d6f6f760000006c6d766864000000000000000000000000000003e8000003e800010000010000000000000000000000000100000000000000000000000000000001000000000000000000000000000040000000000000000000000000000000000000000000000000000000000000020000029f7472616b0000005c746b68640000000300000000000000000000000100000000000003e800000000000000000000000000000000000100000000000000000000000000000001000000000000000000000000000040000000001000000010000000000024656474730000001c656c73740000000000000001000003e80000100000010000000002176d646961000000206d646864000000000000000000000000000028000000280055c400000000002d68646c72000000000000000076696465000000000000000000000000566964656f48616e646c657200000001c26d696e6600000014766d68640000000100000000000000000000002464696e660000001c6472656600000000000000010000000c75726c2000000001000001827374626c000000be737473640000000000000001000000ae61766331000000000000000100000000000000000000000000000000001000100048000000480000000000000001154c61766336322e32382e313032206c696278323634000000000000000000000018ffff00000034617663430164000affe100176764000aacd95ec044000003000400000300283c48965801000668ebe3cb22c0fdf8f80000000010706173700000000100000001000000146274727400000000000017a000000000000000187374747300000000000000010000000500000800000000147374737300000000000000010000000100000038637474730000000000000005000000010000100000000001000028000000000100001000000000010000000000000001000008000000001c737473630000000000000001000000010000000500000001000000287374737a000000000000000000000005000002c40000000c0000000c0000000c0000000c000000147374636f0000000000000001000003a500000062756474610000005a6d657461000000000000002168646c7200000000000000006d6469726170706c0000000000000000000000002d696c737400000025a9746f6f0000001d6461746100000001000000004c61766636322e31322e3130320000000866726565000002fc6d646174000002ad0605ffffa9dc45e9bde6d948b7962cd820d923eeef78323634202d20636f7265203136352072333232322062333536303561202d20482e3236342f4d5045472d342041564320636f646563202d20436f70796c65667420323030332d32303235202d20687474703a2f2f7777772e766964656f6c616e2e6f72672f783236342e68746d6c202d206f7074696f6e733a2063616261633d31207265663d33206465626c6f636b3d313a303a3020616e616c7973653d3078333a3078313133206d653d686578207375626d653d37207073793d31207073795f72643d312e30303a302e3030206d697865645f7265663d31206d655f72616e67653d3136206368726f6d615f6d653d31207472656c6c69733d31203878386463743d312063716d3d3020646561647a6f6e653d32312c313120666173745f70736b69703d31206368726f6d615f71705f6f66667365743d2d3220746872656164733d31206c6f6f6b61686561645f746872656164733d3120736c696365645f746872656164733d30206e723d3020646563696d6174653d3120696e7465726c616365643d3020626c757261795f636f6d7061743d3020636f6e73747261696e65645f696e7472613d3020626672616d65733d3320625f707972616d69643d3220625f61646170743d3120625f626961733d30206469726563743d3120776569676874623d31206f70656e5f676f703d3020776569676874703d32206b6579696e743d323530206b6579696e745f6d696e3d35207363656e656375743d343020696e7472615f726566726573683d302072635f6c6f6b61686561643d34302072633d637266206d62747265653d31206372663d32332e302071636f6d703d302e36302071706d696e3d302071706d61783d3639207170737465703d342069705f726174696f3d312e34302061713d313a312e303000800000000f6588840012fffef7addf814dc32b3d00000008419a246c43fffee000000008419e4278821fab8100000008019e617443ffb38000000008019e636a43ffb381";

fn decode_hex(hex: &str) -> Vec<u8> {
    assert_eq!(hex.len() % 2, 0);
    hex.as_bytes()
        .chunks_exact(2)
        .map(|pair| {
            let high = (pair[0] as char).to_digit(16).expect("hex high nibble");
            let low = (pair[1] as char).to_digit(16).expect("hex low nibble");
            ((high << 4) | low) as u8
        })
        .collect()
}

async fn wait_for_applied(
    state_db_path: &std::path::Path,
    approval_id: &str,
) -> anyhow::Result<MutationApproval> {
    let ledger = MutationLedger::new(state_db_path.to_path_buf());
    let mut last = None;
    for _ in 0..900 {
        let approval = ledger.get_approval(approval_id)?;
        if approval
            .as_ref()
            .is_some_and(|approval| approval.state == MutationApprovalState::Applied)
        {
            return Ok(approval.expect("applied approval"));
        }
        last = approval.map(|approval| {
            (approval.state, approval.last_error_code, approval.automated_retry_count)
        });
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    anyhow::bail!("voice/video-note mutation did not become applied; last={last:?}")
}

async fn prepare_and_approve(
    client: &rmcp::service::RunningService<rmcp::RoleClient, ()>,
    bot: &FakeApprovalBot,
    state_db_path: &std::path::Path,
    tool_name: &'static str,
    arguments: Value,
) -> anyhow::Result<(Value, String, i64, Value)> {
    let prepared = client
        .call_tool(
            CallToolRequestParams::new(tool_name)
                .with_arguments(arguments.as_object().expect("object").clone()),
        )
        .await?;
    assert_eq!(prepared.is_error, Some(false), "prepare {tool_name}");
    let prepared = prepared.structured_content.expect("prepared media mutation");
    let requested = client
        .call_tool(
            CallToolRequestParams::new("request_mutation_approval").with_arguments(
                json!({
                    "mutation_id": prepared.pointer("/data/mutation_id").and_then(Value::as_str).expect("mutation id"),
                    "approved_payload_sha256": prepared.pointer("/data/payload_sha256").and_then(Value::as_str).expect("payload hash")
                })
                .as_object()
                .expect("object")
                .clone(),
            ),
        )
        .await?;
    assert_eq!(requested.is_error, Some(false), "approval request {tool_name}");
    let requested = requested.structured_content.expect("approval request");
    let approval_id = requested
        .pointer("/data/approval_id")
        .and_then(Value::as_str)
        .expect("approval id")
        .to_string();
    let approval_message_id = requested
        .pointer("/data/approval_message_id")
        .and_then(Value::as_i64)
        .expect("approval message id");
    bot.push_mutation_decision(
        &approval_id,
        ApprovalAction::Approve,
        700,
        700,
        approval_message_id,
    )
    .await;
    let approval = wait_for_applied(state_db_path, &approval_id).await?;
    let result = serde_json::to_value(approval.result.expect("mutation result"))?;
    Ok((prepared, approval_id, approval_message_id, result))
}

#[tokio::test]
async fn voice_and_video_note_are_byte_validated_thread_bound_and_exactly_once()
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
            .expect("managed attachment path"),
    )
    .parent()
    .expect("managed attachment directory")
    .to_path_buf();

    let invalid_path = managed_dir.join("invalid-media.bin");
    let invalid_bytes = b"not media";
    std::fs::write(&invalid_path, invalid_bytes)?;
    let invalid_sha256 = format!("{:x}", Sha256::digest(invalid_bytes));
    for (tool_name, arguments) in [
        (
            "send_voice",
            json!({
                "chat_id": "-202",
                "file_path": invalid_path,
                "approved_sha256": invalid_sha256
            }),
        ),
        (
            "send_video_note",
            json!({
                "chat_id": "-202",
                "file_path": invalid_path,
                "approved_sha256": invalid_sha256
            }),
        ),
    ] {
        let invalid = client
            .call_tool(
                CallToolRequestParams::new(tool_name)
                    .with_arguments(arguments.as_object().expect("object").clone()),
            )
            .await?;
        assert_eq!(invalid.is_error, Some(true), "invalid {tool_name}");
        assert_eq!(
            invalid
                .structured_content
                .as_ref()
                .and_then(|value| value.pointer("/error/code"))
                .and_then(Value::as_str),
            Some("INVALID_ARGUMENT")
        );
    }

    let voice_bytes = decode_hex(VOICE_OGG_HEX);
    let voice_path = managed_dir.join("approved-voice.ogg");
    std::fs::write(&voice_path, &voice_bytes)?;
    let voice_sha256 = format!("{:x}", Sha256::digest(&voice_bytes));
    backend.make_next_voice_send_outcome_uncertain().await;
    let (voice, voice_approval, voice_card, voice_result) = prepare_and_approve(
        &client,
        &bot,
        &state_db_path,
        "send_voice",
        json!({
            "chat_id": "-202",
            "file_path": voice_path,
            "caption": "**Launch** approved",
            "parse_mode": "md",
            "topic_id": 44,
            "silent": true,
            "noforwards": true,
            "approved_sha256": voice_sha256
        }),
    )
    .await?;
    assert_eq!(voice.pointer("/data/action/kind").and_then(Value::as_str), Some("send_voice"));
    assert_eq!(voice.pointer("/data/action/topic_id").and_then(Value::as_i64), Some(44));
    assert_eq!(
        voice.pointer("/data/action/caption_parse_mode").and_then(Value::as_str),
        Some("md")
    );
    assert_eq!(voice_result.pointer("/kind").and_then(Value::as_str), Some("voice_sent"));
    assert_eq!(voice_result.pointer("/media/media_type").and_then(Value::as_str), Some("voice"));
    assert_eq!(voice_result.pointer("/media/mime_type").and_then(Value::as_str), Some("audio/ogg"));
    assert_eq!(
        voice_result.pointer("/sha256").and_then(Value::as_str),
        Some(voice_sha256.as_str())
    );
    assert_eq!(backend.voice_send_call_count().await, 1);

    let video_bytes = decode_hex(VIDEO_MP4_HEX);
    let video_path = managed_dir.join("approved-video-note.mp4");
    std::fs::write(&video_path, &video_bytes)?;
    let video_sha256 = format!("{:x}", Sha256::digest(&video_bytes));
    let mismatch = client
        .call_tool(
            CallToolRequestParams::new("send_video_note").with_arguments(
                json!({
                    "chat_id": "-202",
                    "file_path": video_path,
                    "duration_seconds": 9,
                    "approved_sha256": video_sha256
                })
                .as_object()
                .expect("object")
                .clone(),
            ),
        )
        .await?;
    assert_eq!(mismatch.is_error, Some(true));
    assert_eq!(
        mismatch
            .structured_content
            .as_ref()
            .and_then(|value| value.pointer("/error/code"))
            .and_then(Value::as_str),
        Some("INVALID_ARGUMENT")
    );

    backend.make_next_video_note_send_outcome_uncertain().await;
    let (video, video_approval, video_card, video_result) = prepare_and_approve(
        &client,
        &bot,
        &state_db_path,
        "send_video_note",
        json!({
            "chat_id": "-202",
            "file_path": video_path,
            "duration_seconds": 1,
            "length_pixels": 320,
            "topic_id": 44,
            "silent": true,
            "noforwards": true,
            "approved_sha256": video_sha256
        }),
    )
    .await?;
    assert_eq!(video.pointer("/data/action/kind").and_then(Value::as_str), Some("send_video_note"));
    assert_eq!(video.pointer("/data/action/duration_seconds").and_then(Value::as_i64), Some(1));
    assert_eq!(video.pointer("/data/action/length_pixels").and_then(Value::as_i64), Some(320));
    assert_eq!(video_result.pointer("/kind").and_then(Value::as_str), Some("video_note_sent"));
    assert_eq!(
        video_result.pointer("/media/media_type").and_then(Value::as_str),
        Some("video_note")
    );
    assert_eq!(video_result.pointer("/media/duration_seconds").and_then(Value::as_i64), Some(1));
    assert_eq!(video_result.pointer("/media/width_px").and_then(Value::as_i64), Some(320));
    assert_eq!(video_result.pointer("/media/height_px").and_then(Value::as_i64), Some(320));
    assert_eq!(
        video_result.pointer("/sha256").and_then(Value::as_str),
        Some(video_sha256.as_str())
    );
    assert_eq!(backend.video_note_send_call_count().await, 1);

    let sent = backend.sent_messages().await;
    assert_eq!(sent.len(), 2);
    assert_eq!(sent[0].text, "Launch approved");
    assert_eq!(sent[0].reply_to_message_id, Some(44));
    assert_eq!(sent[1].reply_to_message_id, Some(44));
    for message in &sent {
        assert_eq!(backend.get_message_topic_id(None, "-202", message.message_id).await?, Some(44));
    }

    for prepared in [&voice, &video] {
        let artifact_id = prepared
            .pointer("/data/action/artifact/artifact_id")
            .and_then(Value::as_str)
            .expect("artifact id");
        assert!(!state_db_path.with_extension("outbox").join(artifact_id).exists());
    }

    bot.push_mutation_decision(&voice_approval, ApprovalAction::Approve, 700, 700, voice_card)
        .await;
    bot.push_mutation_decision(&video_approval, ApprovalAction::Approve, 700, 700, video_card)
        .await;
    tokio::time::sleep(Duration::from_millis(150)).await;
    assert_eq!(backend.voice_send_call_count().await, 1);
    assert_eq!(backend.video_note_send_call_count().await, 1);
    assert_eq!(backend.sent_messages().await.len(), 2);

    let cards = format!("{:?}\n{:?}", bot.sent_mutation_cards().await, bot.mutation_edits().await);
    assert!(cards.contains("**Launch** approved"));
    assert!(cards.contains("topic_id: Some(44)"));
    assert!(cards.contains("length_pixels: 320"));
    assert!(cards.contains(voice_sha256.as_str()));
    assert!(cards.contains(video_sha256.as_str()));

    client.cancel().await?;
    server_task.abort();
    worker.abort();
    Ok(())
}
