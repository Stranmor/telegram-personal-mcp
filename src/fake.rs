use crate::backend::TelegramBackend;
use crate::domain::{
    AttachmentDescriptor, Completeness, FileSendMode, MessagePage, SearchMessagesRequest,
};
use crate::fs_security::restrict_private_file;
use crate::telegram::{DialogSummary, MessageSummary, TelegramError, UserInfo};
use async_trait::async_trait;
use std::collections::HashMap;
use std::path::PathBuf;
use tokio::io::AsyncWriteExt;
use tokio::sync::Mutex;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FakeSentMessage {
    pub chat_id: String,
    pub text: String,
    pub message_id: i32,
    pub file_name: Option<String>,
    pub reply_to_message_id: Option<i32>,
    pub file_mode: Option<FileSendMode>,
}

#[derive(Debug)]
struct FakeState {
    account: UserInfo,
    session_path: PathBuf,
    dialogs: Vec<DialogSummary>,
    messages: HashMap<i64, Vec<MessageSummary>>,
    attachments: HashMap<(i64, i32), (AttachmentDescriptor, Vec<u8>)>,
    sent: Vec<FakeSentMessage>,
    sent_by_random_id: HashMap<i64, i32>,
    next_message_id: i32,
}

#[derive(Debug)]
pub struct FakeTelegramBackend {
    state: Mutex<FakeState>,
}

impl Default for FakeTelegramBackend {
    fn default() -> Self {
        Self::fixture()
    }
}

impl FakeTelegramBackend {
    pub fn fixture() -> Self {
        let dialogs = vec![
            DialogSummary {
                chat_id: 101,
                canonical_chat_id: "101".to_string(),
                chat_ref: "@alex".to_string(),
                chat_kind: "user".to_string(),
                title: "Alex".to_string(),
                username: Some("alex".to_string()),
                last_message_id: Some(11),
                last_message_text: Some("Please check contract.pdf".to_string()),
                last_message_outgoing: Some(false),
                last_message_date: Some(1_700_000_011),
            },
            DialogSummary {
                chat_id: 202,
                canonical_chat_id: "-202".to_string(),
                chat_ref: "-202".to_string(),
                chat_kind: "group".to_string(),
                title: "Project Room".to_string(),
                username: None,
                last_message_id: Some(21),
                last_message_text: Some("Release passed".to_string()),
                last_message_outgoing: Some(true),
                last_message_date: Some(1_700_000_021),
            },
        ];
        let messages = HashMap::from([
            (
                101,
                vec![
                    MessageSummary {
                        message_id: 11,
                        chat_ref: Some("101".to_string()),
                        sender_ref: Some("101".to_string()),
                        outgoing: false,
                        text: Some("Please check contract.pdf".to_string()),
                        date: 1_700_000_011,
                        edit_date: None,
                        reply_to_message_id: Some(10),
                        views: None,
                        forwards: None,
                        reply_count: None,
                        media_type: Some("document".to_string()),
                    },
                    MessageSummary {
                        message_id: 10,
                        chat_ref: Some("101".to_string()),
                        sender_ref: Some("101".to_string()),
                        outgoing: false,
                        text: Some("Here is the latest contract".to_string()),
                        date: 1_700_000_010,
                        edit_date: None,
                        reply_to_message_id: None,
                        views: None,
                        forwards: None,
                        reply_count: Some(1),
                        media_type: None,
                    },
                ],
            ),
            (
                202,
                vec![MessageSummary {
                    message_id: 21,
                    chat_ref: Some("-202".to_string()),
                    sender_ref: Some("7".to_string()),
                    outgoing: true,
                    text: Some("Release passed".to_string()),
                    date: 1_700_000_021,
                    edit_date: None,
                    reply_to_message_id: None,
                    views: None,
                    forwards: None,
                    reply_count: None,
                    media_type: None,
                }],
            ),
        ]);
        Self {
            state: Mutex::new(FakeState {
                account: UserInfo {
                    user_id: 7,
                    first_name: "Fixture".to_string(),
                    username: Some("fixture_account".to_string()),
                },
                session_path: PathBuf::from("/tmp/telegram-personal-mcp-fixture.session"),
                dialogs,
                messages,
                attachments: HashMap::from([(
                    (101, 11),
                    (
                        AttachmentDescriptor {
                            attachment_id: "@alex:11:0".to_string(),
                            chat_id: "@alex".to_string(),
                            message_id: 11,
                            kind: "document".to_string(),
                            file_name: Some("contract.pdf".to_string()),
                            mime_type: Some("application/pdf".to_string()),
                            size_bytes: Some(26),
                            downloadable: true,
                        },
                        b"%PDF-1.4\nfixture contract\n".to_vec(),
                    ),
                )]),
                sent: Vec::new(),
                sent_by_random_id: HashMap::new(),
                next_message_id: 100,
            }),
        }
    }

    pub async fn sent_messages(&self) -> Vec<FakeSentMessage> {
        self.state.lock().await.sent.clone()
    }

    #[doc(hidden)]
    pub async fn set_fixture_attachment_size_hint(&self, size_bytes: Option<u64>) {
        let mut state = self.state.lock().await;
        if let Some((descriptor, _)) = state.attachments.get_mut(&(101, 11)) {
            descriptor.size_bytes = size_bytes;
        }
    }

    #[doc(hidden)]
    pub async fn set_fixture_account_user_id(&self, user_id: i64) {
        self.state.lock().await.account.user_id = user_id;
    }

    fn resolve_dialog(dialogs: &[DialogSummary], identifier: &str) -> Option<i64> {
        let wanted = identifier.trim().trim_start_matches('@');
        dialogs.iter().find_map(|dialog| {
            let id_matches =
                wanted.parse::<i64>().is_ok_and(|peer_id| match dialog.chat_kind.as_str() {
                    "channel" => {
                        peer_id
                            .to_string()
                            .strip_prefix("-100")
                            .and_then(|value| value.parse::<i64>().ok())
                            == Some(dialog.chat_id)
                    },
                    "group" => peer_id.checked_abs() == Some(dialog.chat_id) && peer_id < 0,
                    _ => peer_id == dialog.chat_id && peer_id > 0,
                });
            let ref_matches = dialog.chat_ref.trim_start_matches('@').eq_ignore_ascii_case(wanted);
            let canonical_matches = dialog.canonical_chat_id == wanted;
            let username_matches = dialog
                .username
                .as_deref()
                .is_some_and(|username| username.eq_ignore_ascii_case(wanted));
            (id_matches || canonical_matches || ref_matches || username_matches)
                .then_some(dialog.chat_id)
        })
    }

    fn canonical_chat_ref(dialogs: &[DialogSummary], chat_id: i64) -> String {
        dialogs
            .iter()
            .find(|dialog| dialog.chat_id == chat_id)
            .map_or_else(|| chat_id.to_string(), |dialog| dialog.canonical_chat_id.clone())
    }
}

#[async_trait]
impl TelegramBackend for FakeTelegramBackend {
    async fn get_me(
        &self,
        _session_path: Option<PathBuf>,
    ) -> Result<(UserInfo, PathBuf), TelegramError> {
        let state = self.state.lock().await;
        Ok((state.account.clone(), state.session_path.clone()))
    }

    async fn resolve_chat_id(
        &self,
        _session_path: Option<PathBuf>,
        chat_id: &str,
    ) -> Result<String, TelegramError> {
        let state = self.state.lock().await;
        let canonical_id = Self::resolve_dialog(&state.dialogs, chat_id)
            .ok_or_else(|| TelegramError::PeerNotFound(chat_id.to_string()))?;
        Ok(Self::canonical_chat_ref(&state.dialogs, canonical_id))
    }

    async fn list_dialogs(
        &self,
        _session_path: Option<PathBuf>,
        limit: usize,
    ) -> Result<Vec<DialogSummary>, TelegramError> {
        let state = self.state.lock().await;
        Ok(state.dialogs.iter().take(limit).cloned().collect())
    }

    async fn history(
        &self,
        _session_path: Option<PathBuf>,
        chat_id: &str,
        limit: usize,
        offset_id: Option<i32>,
        add_offset: i32,
    ) -> Result<Vec<MessageSummary>, TelegramError> {
        let state = self.state.lock().await;
        let canonical_id = Self::resolve_dialog(&state.dialogs, chat_id)
            .ok_or_else(|| TelegramError::PeerNotFound(chat_id.to_string()))?;
        let messages = state.messages.get(&canonical_id).cloned().unwrap_or_default();
        let base = offset_id
            .and_then(|offset| messages.iter().position(|message| message.message_id == offset))
            .map_or(0, |index| index.saturating_add(1));
        let offset = usize::try_from(add_offset.unsigned_abs()).unwrap_or(usize::MAX);
        let start = if add_offset.is_negative() {
            base.saturating_sub(offset)
        } else {
            base.saturating_add(offset)
        };
        Ok(messages.into_iter().skip(start).take(limit).collect())
    }

    async fn search_messages(
        &self,
        request: SearchMessagesRequest,
    ) -> Result<MessagePage, TelegramError> {
        let state = self.state.lock().await;
        let query = request.query.to_lowercase();
        let chat_scope = request
            .chat_id
            .as_deref()
            .map(|chat_id| {
                Self::resolve_dialog(&state.dialogs, chat_id)
                    .ok_or_else(|| TelegramError::PeerNotFound(chat_id.to_string()))
            })
            .transpose()?;
        let mut messages = state
            .messages
            .iter()
            .filter(|(chat_id, _)| chat_scope.is_none_or(|scope| **chat_id == scope))
            .flat_map(|(_, messages)| messages.iter().cloned())
            .filter(|message| {
                request.offset_id.is_none_or(|offset| message.message_id < offset)
                    && message
                        .text
                        .as_deref()
                        .is_some_and(|text| text.to_lowercase().contains(&query))
            })
            .collect::<Vec<_>>();
        messages.sort_by(|left, right| {
            right.date.cmp(&left.date).then_with(|| right.message_id.cmp(&left.message_id))
        });
        messages.truncate(request.limit);
        let next_offset_id = chat_scope
            .is_some()
            .then(|| {
                (messages.len() == request.limit).then(|| messages.last().map(|m| m.message_id))
            })
            .flatten()
            .flatten();
        let completeness = if chat_scope.is_none() {
            Completeness::FirstPageOnly
        } else if next_offset_id.is_some() {
            Completeness::Page
        } else {
            Completeness::Complete
        };
        Ok(MessagePage { messages, next_offset_id, completeness })
    }

    async fn get_message(
        &self,
        _session_path: Option<PathBuf>,
        chat_id: &str,
        message_id: i32,
    ) -> Result<Option<MessageSummary>, TelegramError> {
        let state = self.state.lock().await;
        let canonical_id = Self::resolve_dialog(&state.dialogs, chat_id)
            .ok_or_else(|| TelegramError::PeerNotFound(chat_id.to_string()))?;
        Ok(state
            .messages
            .get(&canonical_id)
            .and_then(|messages| messages.iter().find(|message| message.message_id == message_id))
            .cloned())
    }

    async fn list_attachments(
        &self,
        _session_path: Option<PathBuf>,
        chat_id: &str,
        message_id: i32,
    ) -> Result<Vec<AttachmentDescriptor>, TelegramError> {
        let state = self.state.lock().await;
        let canonical_id = Self::resolve_dialog(&state.dialogs, chat_id)
            .ok_or_else(|| TelegramError::PeerNotFound(chat_id.to_string()))?;
        Ok(state
            .attachments
            .get(&(canonical_id, message_id))
            .map(|(descriptor, _)| vec![descriptor.clone()])
            .unwrap_or_default())
    }

    async fn download_attachment_to(
        &self,
        _session_path: Option<PathBuf>,
        chat_id: &str,
        message_id: i32,
        destination: &std::path::Path,
        max_bytes: u64,
    ) -> Result<u64, TelegramError> {
        let bytes = {
            let state = self.state.lock().await;
            let canonical_id = Self::resolve_dialog(&state.dialogs, chat_id)
                .ok_or_else(|| TelegramError::PeerNotFound(chat_id.to_string()))?;
            state
                .attachments
                .get(&(canonical_id, message_id))
                .map(|(_, bytes)| bytes.clone())
                .ok_or_else(|| {
                    TelegramError::AttachmentNotFound(format!("{chat_id}:{message_id}"))
                })?
        };
        let byte_count = u64::try_from(bytes.len()).unwrap_or(u64::MAX);
        let mut file = tokio::fs::File::create(destination).await?;
        restrict_private_file(destination)?;
        if byte_count > max_bytes {
            let allowed = usize::try_from(max_bytes).unwrap_or(usize::MAX).min(bytes.len());
            let partial = bytes.get(..allowed).ok_or_else(|| {
                TelegramError::Io(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "fixture download limit exceeded its source buffer",
                ))
            })?;
            file.write_all(partial).await?;
            file.flush().await?;
            file.sync_all().await?;
            return Err(TelegramError::AttachmentTooLarge { max_bytes });
        }
        file.write_all(&bytes).await?;
        file.flush().await?;
        file.sync_all().await?;
        Ok(byte_count)
    }

    async fn send_file(
        &self,
        session_path: Option<PathBuf>,
        chat_id: &str,
        file_path: &std::path::Path,
        caption: &str,
        reply_to_message_id: Option<i32>,
        mode: FileSendMode,
    ) -> Result<i32, TelegramError> {
        let metadata = tokio::fs::metadata(file_path).await?;
        if !metadata.is_file() {
            return Err(TelegramError::Io(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "upload path is not a regular file",
            )));
        }
        let file_name =
            file_path.file_name().and_then(|value| value.to_str()).unwrap_or("attachment");
        let text = if caption.is_empty() {
            format!("[file: {file_name}]")
        } else {
            format!("{caption} [file: {file_name}]")
        };
        let message_id = self.send_message(session_path, chat_id, &text).await?;
        let mut state = self.state.lock().await;
        let canonical_id = Self::resolve_dialog(&state.dialogs, chat_id)
            .ok_or_else(|| TelegramError::PeerNotFound(chat_id.to_string()))?;
        if let Some(sent) = state.sent.iter_mut().find(|sent| sent.message_id == message_id) {
            sent.file_name = Some(file_name.to_string());
            sent.reply_to_message_id = reply_to_message_id;
            sent.file_mode = Some(mode);
        }
        if let Some(messages) = state.messages.get_mut(&canonical_id)
            && let Some(message) =
                messages.iter_mut().find(|message| message.message_id == message_id)
        {
            message.reply_to_message_id = reply_to_message_id;
            message.media_type = Some(
                match mode {
                    FileSendMode::Photo => "photo",
                    FileSendMode::Auto | FileSendMode::Document => "document",
                }
                .to_string(),
            );
        }
        Ok(message_id)
    }

    async fn send_text_with_random_id(
        &self,
        _session_path: Option<PathBuf>,
        chat_id: &str,
        text: &str,
        reply_to_message_id: Option<i32>,
        random_id: i64,
    ) -> Result<i32, TelegramError> {
        let mut state = self.state.lock().await;
        if let Some(message_id) = state.sent_by_random_id.get(&random_id) {
            return Ok(*message_id);
        }
        let canonical_id = Self::resolve_dialog(&state.dialogs, chat_id)
            .ok_or_else(|| TelegramError::PeerNotFound(chat_id.to_string()))?;
        let message_id = state.next_message_id;
        state.next_message_id = state.next_message_id.saturating_add(1);
        state.sent_by_random_id.insert(random_id, message_id);
        state.sent.push(FakeSentMessage {
            chat_id: canonical_id.to_string(),
            text: text.to_string(),
            message_id,
            file_name: None,
            reply_to_message_id: None,
            file_mode: None,
        });
        let account_user_id = state.account.user_id;
        let chat_ref = Self::canonical_chat_ref(&state.dialogs, canonical_id);
        state.messages.entry(canonical_id).or_default().insert(
            0,
            MessageSummary {
                message_id,
                chat_ref: Some(chat_ref),
                sender_ref: Some(account_user_id.to_string()),
                outgoing: true,
                text: Some(text.to_string()),
                date: 1_700_000_000_i32.saturating_add(message_id),
                edit_date: None,
                reply_to_message_id,
                views: None,
                forwards: None,
                reply_count: None,
                media_type: None,
            },
        );
        Ok(message_id)
    }

    async fn send_message(
        &self,
        _session_path: Option<PathBuf>,
        chat_id: &str,
        text: &str,
    ) -> Result<i32, TelegramError> {
        let mut state = self.state.lock().await;
        let canonical_id = Self::resolve_dialog(&state.dialogs, chat_id)
            .ok_or_else(|| TelegramError::PeerNotFound(chat_id.to_string()))?;
        let message_id = state.next_message_id;
        state.next_message_id = state.next_message_id.saturating_add(1);
        state.sent.push(FakeSentMessage {
            chat_id: canonical_id.to_string(),
            text: text.to_string(),
            message_id,
            file_name: None,
            reply_to_message_id: None,
            file_mode: None,
        });
        let account_user_id = state.account.user_id;
        let chat_ref = Self::canonical_chat_ref(&state.dialogs, canonical_id);
        state.messages.entry(canonical_id).or_default().insert(
            0,
            MessageSummary {
                message_id,
                chat_ref: Some(chat_ref),
                sender_ref: Some(account_user_id.to_string()),
                outgoing: true,
                text: Some(text.to_string()),
                date: 1_700_000_000_i32.saturating_add(message_id),
                edit_date: None,
                reply_to_message_id: None,
                views: None,
                forwards: None,
                reply_count: None,
                media_type: None,
            },
        );
        if let Some(dialog) = state.dialogs.iter_mut().find(|dialog| dialog.chat_id == canonical_id)
        {
            dialog.last_message_id = Some(message_id);
            dialog.last_message_text = Some(text.to_string());
            dialog.last_message_outgoing = Some(true);
        }
        Ok(message_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn fixture_supports_read_and_send() {
        let backend = FakeTelegramBackend::fixture();
        let messages = backend.history(None, "@alex", 10, None, 0).await.expect("read fixture");
        assert_eq!(messages.len(), 2);

        let sent_id = backend
            .send_message(None, "@alex", "Updated contract attached")
            .await
            .expect("send fixture");
        assert_eq!(sent_id, 100);
        assert_eq!(backend.sent_messages().await.len(), 1);
    }

    #[tokio::test]
    async fn fixture_stops_at_download_limit_with_a_partial_file_for_wrapper_cleanup() {
        let backend = FakeTelegramBackend::fixture();
        let temp = tempfile::tempdir().expect("tempdir");
        let destination = temp.path().join("contract.pdf");

        let error = backend
            .download_attachment_to(None, "@alex", 11, &destination, 10)
            .await
            .expect_err("fixture attachment exceeds limit");
        assert!(matches!(error, TelegramError::AttachmentTooLarge { max_bytes: 10 }));
        assert_eq!(std::fs::metadata(destination).expect("partial metadata").len(), 10);
    }
}
