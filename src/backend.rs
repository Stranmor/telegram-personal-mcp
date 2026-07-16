use crate::domain::{AttachmentDescriptor, FileSendMode, MessagePage, SearchMessagesRequest};
use crate::telegram::{DialogSummary, MessageSummary, TelegramError, UserInfo};
use async_trait::async_trait;
use std::path::PathBuf;

#[async_trait]
pub trait TelegramBackend: Send + Sync + 'static {
    async fn get_me(
        &self,
        session_path: Option<PathBuf>,
    ) -> Result<(UserInfo, PathBuf), TelegramError>;

    async fn resolve_chat_id(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
    ) -> Result<String, TelegramError>;

    async fn list_dialogs(
        &self,
        session_path: Option<PathBuf>,
        limit: usize,
    ) -> Result<Vec<DialogSummary>, TelegramError>;

    async fn history(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        limit: usize,
        offset_id: Option<i32>,
        add_offset: i32,
    ) -> Result<Vec<MessageSummary>, TelegramError>;

    async fn search_messages(
        &self,
        request: SearchMessagesRequest,
    ) -> Result<MessagePage, TelegramError>;

    async fn get_message(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        message_id: i32,
    ) -> Result<Option<MessageSummary>, TelegramError>;

    async fn list_attachments(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        message_id: i32,
    ) -> Result<Vec<AttachmentDescriptor>, TelegramError>;

    async fn download_attachment_to(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        message_id: i32,
        destination: &std::path::Path,
        max_bytes: u64,
    ) -> Result<u64, TelegramError>;

    async fn send_file(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        file_path: &std::path::Path,
        caption: &str,
        reply_to_message_id: Option<i32>,
        mode: FileSendMode,
    ) -> Result<i32, TelegramError>;

    async fn send_text_with_random_id(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        text: &str,
        reply_to_message_id: Option<i32>,
        random_id: i64,
    ) -> Result<i32, TelegramError>;

    async fn send_message(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        text: &str,
    ) -> Result<i32, TelegramError>;
}
