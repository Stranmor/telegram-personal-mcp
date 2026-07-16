pub mod backend;
pub mod config;
pub mod domain;
pub mod fake;
mod fs_security;
pub mod ledger;
pub mod mcp;
pub mod protocol;
pub mod telegram;

pub use backend::TelegramBackend;
pub use config::Config;
pub use domain::{
    AttachmentDescriptor, Completeness, DownloadedAttachment, FileSendMode, MessageContext,
    MessagePage, PreparedSend, PreparedSendState, SearchMessagesRequest,
};
pub use fake::FakeTelegramBackend;
pub use ledger::{PreparedSendRecord, SendLedger};
pub use mcp::{McpServer, McpServerSettings};
pub use telegram::{
    LoginCodeChallenge, LoginCodeOutcome, LoginPasswordChallenge, LoginSession, TelegramClient,
    TelegramError, UserInfo,
};
