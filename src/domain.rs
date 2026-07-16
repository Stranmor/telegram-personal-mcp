use crate::telegram::MessageSummary;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct SearchMessagesRequest {
    pub session_path: Option<PathBuf>,
    pub chat_id: Option<String>,
    pub query: String,
    pub limit: usize,
    pub offset_id: Option<i32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Completeness {
    Complete,
    Page,
    FirstPageOnly,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct MessagePage {
    pub messages: Vec<MessageSummary>,
    pub next_offset_id: Option<i32>,
    pub completeness: Completeness,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct MessageContext {
    pub target: MessageSummary,
    pub newer: Vec<MessageSummary>,
    pub older: Vec<MessageSummary>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct AttachmentDescriptor {
    pub attachment_id: String,
    pub chat_id: String,
    pub message_id: i32,
    pub kind: String,
    pub file_name: Option<String>,
    pub mime_type: Option<String>,
    pub size_bytes: Option<u64>,
    pub downloadable: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct DownloadedAttachment {
    pub attachment: AttachmentDescriptor,
    pub local_path: String,
    pub bytes_written: u64,
    pub sha256: String,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum FileSendMode {
    #[default]
    Auto,
    Document,
    Photo,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PreparedSendState {
    Prepared,
    Dispatching,
    Sent,
    DeliveryUncertain,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct PreparedSend {
    pub draft_id: String,
    pub account_user_id: i64,
    pub chat_id: String,
    pub text: String,
    pub reply_to_message_id: Option<i32>,
    pub payload_sha256: String,
    pub state: PreparedSendState,
    pub message_id: Option<i32>,
}
