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

/// Client TL library that performs the deserialization.
pub const TL_LIBRARY: &str = "ferogram-tl-types";

/// Version of [`TL_LIBRARY`] the running binary was built against, read from
/// `Cargo.lock` by `build.rs`.
pub const TL_LIBRARY_VERSION: &str = env!("TL_STACK_FEROGRAM_TL_TYPES");

/// TL layer compiled into the running binary.
pub const TL_CLIENT_LAYER: i32 = ferogram::LAYER;

/// Marker the TL deserializer emits for a constructor it cannot decode.
const CONSTRUCTOR_MARKER: &str = "unexpected constructor id: ";

/// A TL deserialization failure attributed to a specific constructor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TlSchemaFault {
    /// Constructor the server sent, as reported by the deserializer (`0x…`).
    pub constructor: String,
}

impl TlSchemaFault {
    /// Recognize a TL deserialization failure in a transport error message.
    ///
    /// Returns `None` for every other error, so unrelated failures keep their
    /// existing classification.
    pub fn parse(message: &str) -> Option<Self> {
        let rest = message.split(CONSTRUCTOR_MARKER).nth(1)?;
        let token: String = rest
            .trim_start()
            .chars()
            .take_while(|ch| ch.is_ascii_alphanumeric())
            .collect();
        let digits = token
            .strip_prefix("0x")
            .or_else(|| token.strip_prefix("0X"))?;
        if digits.is_empty() || !digits.chars().all(|ch| ch.is_ascii_hexdigit()) {
            return None;
        }
        Some(Self {
            constructor: format!("0x{}", digits.to_ascii_lowercase()),
        })
    }

    /// Reader-facing message: what broke, in which library, at which layer, and
    /// where the repair belongs.
    pub fn describe(&self) -> String {
        format!(
            "TL schema mismatch: Telegram sent TL constructor {} that the client cannot decode; \
client TL schema is {TL_LIBRARY} {TL_LIBRARY_VERSION} (layer {TL_CLIENT_LAYER}). \
The read path itself is healthy — the pinned TL layer is behind the server's serialization. \
Repair belongs to the bridge (bump the TL layer); retrying the same request will not help.",
            self.constructor
        )
    }

    /// Machine-readable event line, one key per field, for the journal.
    pub fn event_line(&self) -> String {
        format!(
            "event=tl_schema_mismatch library={TL_LIBRARY} version={TL_LIBRARY_VERSION} \
layer={TL_CLIENT_LAYER} constructor={}",
            self.constructor
        )
    }
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
        // A TL deserialization failure is schema drift, not a Telegram outage:
        // name the library, version, layer and constructor before the generic
        // classification can report it as a transport error. Only read-path
        // errors carry the raw transport text; sends keep their
        // delivery-uncertain semantics because the response never arrived.
        if let TelegramError::Telegram(text) = &error {
            if let Some(fault) = TlSchemaFault::parse(text) {
                tracing::warn!("{}", fault.event_line());
                return Self {
                    code: "TL_SCHEMA_MISMATCH".to_string(),
                    message: fault.describe(),
                    retryable: false,
                    retry_after_seconds: None,
                    delivery_confirmed: false,
                };
            }
        }
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

#[cfg(test)]
mod tests {
    use super::*;

    /// The literal text ferogram emits when the server constructor is unknown.
    const LIVE_FAILURE: &str = "deserialize error: unexpected constructor id: 0x3ae56482";

    #[test]
    fn live_constructor_failure_is_attributed_to_the_tl_stack() {
        let fault = TlSchemaFault::parse(LIVE_FAILURE).expect("live text must classify");
        assert_eq!(fault.constructor, "0x3ae56482");
        assert_eq!(fault.event_line(), format!(
            "event=tl_schema_mismatch library=ferogram-tl-types version={TL_LIBRARY_VERSION} layer={TL_CLIENT_LAYER} constructor=0x3ae56482"
        ));
        println!("{}", fault.event_line());
    }

    #[test]
    fn describe_names_library_version_layer_and_constructor() {
        let fault = TlSchemaFault::parse(LIVE_FAILURE).unwrap();
        let described = fault.describe();
        assert!(described.contains("0x3ae56482"), "{described}");
        assert!(described.contains(TL_LIBRARY), "{described}");
        assert!(described.contains(TL_LIBRARY_VERSION), "{described}");
        assert!(
            described.contains(&format!("layer {TL_CLIENT_LAYER}")),
            "{described}"
        );
    }

    #[test]
    fn unrelated_errors_stay_unclassified() {
        assert!(TlSchemaFault::parse("Telegram API error: FLOOD_WAIT_30").is_none());
        assert!(TlSchemaFault::parse("Telegram API error: PEER_ID_INVALID").is_none());
        assert!(TlSchemaFault::parse("unexpected constructor id: ").is_none());
    }
}
