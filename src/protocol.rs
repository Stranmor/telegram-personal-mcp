use crate::telegram::TelegramError;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ToolFailure {
    pub code: String,
    pub message: String,
    pub retryable: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(schema_with = "crate::opt_non_negative_integer_schema")]
    pub retry_after_seconds: Option<u64>,
    /// True when Telegram accepted the message but the durable receipt could
    /// not be recorded. The caller must not resend: the delivery already
    /// happened and a retry would duplicate it.
    #[serde(default, skip_serializing_if = "is_false")]
    pub delivery_confirmed: bool,
}

fn is_false(value: &bool) -> bool {
    !*value
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct ToolOutcome<T> {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<T>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<ToolFailure>,
}

impl<T> ToolOutcome<T> {
    pub fn success(data: T) -> Self {
        Self { data: Some(data), error: None }
    }

    pub fn failure(error: ToolFailure) -> Self {
        Self { data: None, error: Some(error) }
    }
}

impl ToolFailure {
    pub fn new(code: impl Into<String>, message: impl Into<String>, retryable: bool) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
            retryable,
            retry_after_seconds: None,
            delivery_confirmed: false,
        }
    }

    pub fn invalid_argument(message: impl Into<String>) -> Self {
        Self::new("INVALID_ARGUMENT", message, false)
    }

    /// The provider accepted the message but the durable state write failed
    /// after reconciliation. Never retry: a retry would send a duplicate.
    pub fn delivery_state_unrecorded(message: impl Into<String>) -> Self {
        Self {
            code: "DELIVERY_STATE_UNRECORDED".to_string(),
            message: message.into(),
            retryable: false,
            retry_after_seconds: None,
            delivery_confirmed: true,
        }
    }
}

impl From<TelegramError> for ToolFailure {
    fn from(error: TelegramError) -> Self {
        if let TelegramError::FloodWait { seconds } = &error {
            return Self {
                code: "FLOOD_WAIT".to_string(),
                message: error.to_string(),
                retryable: true,
                retry_after_seconds: Some(*seconds),
                delivery_confirmed: false,
            };
        }
        let (code, retryable) = match &error {
            TelegramError::Telegram(_) => ("TELEGRAM_API_ERROR", true),
            TelegramError::SendRejected(_) => ("SEND_REJECTED", false),
            TelegramError::SendRetryable(_) => ("SEND_RETRYABLE", true),
            TelegramError::DeliveryUncertain(_) => ("DELIVERY_UNCERTAIN", true),
            TelegramError::FloodWait { .. } => unreachable!("handled above"),
            TelegramError::Io(_) => ("IO_ERROR", true),
            TelegramError::NoSession(_) => ("NO_SESSION", false),
            TelegramError::AmbiguousSession(_) => ("AMBIGUOUS_SESSION", false),
            TelegramError::InvalidSessionTarget(_) => ("INVALID_SESSION_TARGET", false),
            TelegramError::SessionLocked(_) => ("SESSION_LOCKED", true),
            TelegramError::Unauthorized => ("UNAUTHORIZED", false),
            TelegramError::InvalidPeer(_) => ("INVALID_PEER", false),
            TelegramError::PeerNotFound(_) => ("PEER_NOT_FOUND", false),
            TelegramError::MessageNotFound(_) => ("MESSAGE_NOT_FOUND", false),
            TelegramError::AttachmentNotFound(_) => ("ATTACHMENT_NOT_FOUND", false),
            TelegramError::AttachmentTooLarge { .. } => ("ATTACHMENT_TOO_LARGE", false),
            TelegramError::InvalidLoginCode => ("INVALID_LOGIN_CODE", false),
            TelegramError::SignUpRequired => ("SIGN_UP_REQUIRED", false),
            TelegramError::InvalidPassword => ("INVALID_PASSWORD", false),
            TelegramError::SessionSaveFailed(_) => ("SESSION_SAVE_FAILED", false),
            TelegramError::SessionSaveUncertain(_) => ("SESSION_SAVE_UNCERTAIN", true),
        };
        let message = match &error {
            TelegramError::NoSession(_) => {
                "No configured Telegram session is available to the MCP server".to_string()
            },
            TelegramError::AmbiguousSession(_) => {
                "Multiple Telegram sessions are available; select one in the server configuration"
                    .to_string()
            },
            TelegramError::InvalidSessionTarget(_) => {
                "The configured Telegram session target is invalid".to_string()
            },
            TelegramError::SessionLocked(_) => {
                "The configured Telegram session is locked by another process".to_string()
            },
            _ => error.to_string(),
        };
        Self {
            code: code.to_string(),
            message,
            retryable,
            retry_after_seconds: None,
            delivery_confirmed: false,
        }
    }
}
