//! Local speech recognition for the transcription tools.
//!
//! `messages.transcribeAudio` is no longer the transcription authority of this
//! server: it spends the account's Premium quota or trial limit, is missing on
//! some accounts, and answers an empty string instead of an error when it
//! cannot recognise anything. A voice or video note is now downloaded to a
//! private temporary file and recognised on this host by the GigaAM-v3 route
//! through the Hermes CLI (`hermes nexus transcribe --backend auto`); the route
//! contract lives in the Hermes repository under `docs/ASR-FRONTIER.md`.
//!
//! The CLI is invoked exactly as the shared call contract requires and its JSON
//! receipt is validated here, so a broken or absent recogniser is a typed
//! failure instead of an empty transcription that looks like success.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::{ExitStatus, Stdio};
use std::time::Duration;

use serde::Deserialize;
use sha2::{Digest, Sha256};
use tokio::io::AsyncReadExt;
use tokio::process::Command;
use tokio::time::timeout;
use uuid::Uuid;

use crate::domain::AudioTranscriptionSummary;
use crate::fs_security::restrict_private_directory;
use crate::telegram::TelegramError;

/// Backend name printed by the CLI log line
/// `Local Russian ASR engine ready model="gigaam-v3-rnnt"`.
pub const ENGINE: &str = "gigaam-v3-rnnt";

/// Provider label carried by every transcription receipt produced here.
pub const PROVIDER: &str = "local:gigaam-v3-rnnt";

/// Explicit CLI override; the first lookup step of the call contract.
pub const BIN_ENV: &str = "HERMES_ASR_BIN";

/// Per-clip wall-clock budget; override with whole seconds.
pub const TIMEOUT_ENV: &str = "HERMES_ASR_TIMEOUT_SECONDS";

pub const DEFAULT_TIMEOUT_SECONDS: u64 = 180;

pub const MAX_TIMEOUT_SECONDS: u64 = 3_600;

/// A voice or video note is small; this bound refuses an accidental document
/// before its bytes are pulled from Telegram.
pub const MAX_MEDIA_BYTES: u64 = 64 * 1024 * 1024;

/// Prefix of the deterministic transcription identity.
pub const LOCAL_ID_PREFIX: &str = "local:";

/// Hex characters of the media SHA-256 kept in [`LOCAL_ID_PREFIX`] identities.
pub const LOCAL_ID_HEX_LEN: usize = 16;

/// Binary name looked up in `PATH` and `~/.local/bin`.
const BINARY_NAME: &str = "hermes";

const STDERR_TAIL_CHARS: usize = 300;

/// Resolved local recogniser: the CLI path plus one clip's time budget.
#[derive(Debug, Clone)]
pub struct LocalAsr {
    binary: PathBuf,
    timeout: Duration,
}

/// One recognised clip.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalTranscript {
    pub text: String,
    pub word_count: usize,
    /// `local:<16 hex digits of the media SHA-256>`; stable across calls.
    pub transcription_id: String,
}

/// Private per-call directory holding the downloaded media and CLI output.
pub struct MediaWorkspace {
    dir: PathBuf,
}

impl MediaWorkspace {
    pub fn create() -> Result<Self, TelegramError> {
        let dir = std::env::temp_dir()
            .join(format!("telegram-personal-mcp-asr-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&dir)?;
        restrict_private_directory(&dir)?;
        Ok(Self { dir })
    }

    /// Path of the downloaded media inside the workspace.
    pub fn media_path(&self) -> PathBuf {
        self.dir.join("media")
    }
}

impl Drop for MediaWorkspace {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

impl LocalAsr {
    pub fn new(binary: PathBuf, timeout: Duration) -> Self {
        Self { binary, timeout }
    }

    /// Resolve the CLI from the real environment.
    pub fn discover() -> Result<Self, TelegramError> {
        let binary = resolve_binary(|name| std::env::var_os(name))?;
        Ok(Self::new(binary, timeout_from_env(|name| std::env::var_os(name))))
    }

    pub fn binary(&self) -> &Path {
        &self.binary
    }

    pub fn timeout(&self) -> Duration {
        self.timeout
    }

    /// Recognise one media file and validate the CLI receipt.
    pub async fn transcribe(&self, media: &Path) -> Result<LocalTranscript, TelegramError> {
        let transcription_id = format!("{LOCAL_ID_PREFIX}{}", media_fingerprint(media).await?);
        let workspace = MediaWorkspace::create()?;
        let output_path = workspace.dir.join("transcript.json");
        self.run(media, &output_path).await?;
        let raw = tokio::fs::read(&output_path).await.map_err(|error| {
            TelegramError::AsrFailed(format!(
                "the local ASR wrote no transcript to {}: {error}",
                output_path.display()
            ))
        })?;
        let parsed: CliTranscript = serde_json::from_slice(&raw).map_err(|error| {
            TelegramError::AsrFailed(format!("the local ASR wrote unreadable JSON: {error}"))
        })?;
        let text = parsed.text.trim().to_string();
        if text.is_empty() {
            return Err(TelegramError::EmptyTranscript(format!(
                "{} produced no text for non-empty audio",
                self.binary.display()
            )));
        }
        Ok(LocalTranscript { text, word_count: parsed.words.len(), transcription_id })
    }

    async fn run(&self, media: &Path, output: &Path) -> Result<(), TelegramError> {
        let mut command = Command::new(&self.binary);
        command
            .args(["nexus", "transcribe"])
            .arg("--input")
            .arg(media)
            .arg("--output")
            .arg(output)
            .args(["--format", "json", "--backend", "auto"])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        let child = command.spawn().map_err(|error| {
            if error.kind() == std::io::ErrorKind::NotFound {
                TelegramError::AsrBinaryMissing(format!(
                    "{} is not an executable file anymore",
                    self.binary.display()
                ))
            } else {
                TelegramError::AsrFailed(format!(
                    "could not start {}: {error}",
                    self.binary.display()
                ))
            }
        })?;
        let finished = match timeout(self.timeout, child.wait_with_output()).await {
            Ok(Ok(finished)) => finished,
            Ok(Err(error)) => {
                return Err(TelegramError::AsrFailed(format!(
                    "could not collect the exit status of {}: {error}",
                    self.binary.display()
                )));
            },
            Err(_) => {
                return Err(TelegramError::AsrTimeout { seconds: self.timeout.as_secs() });
            },
        };
        if !finished.status.success() {
            return Err(TelegramError::AsrFailed(format!(
                "{} {} ; stderr tail: {}",
                self.binary.display(),
                status_text(&finished.status),
                stderr_tail(&finished.stderr)
            )));
        }
        Ok(())
    }
}

#[derive(Deserialize)]
struct CliTranscript {
    text: String,
    #[serde(default)]
    words: Vec<serde_json::Value>,
}

/// Resolve the CLI as the shared call contract requires: `HERMES_ASR_BIN`,
/// then `hermes` in `PATH`, then `~/.local/bin/hermes`.
///
/// A set but unusable override fails closed instead of silently falling
/// through to another binary.
pub fn resolve_binary<E>(mut env: E) -> Result<PathBuf, TelegramError>
where
    E: FnMut(&str) -> Option<OsString>,
{
    if let Some(explicit) = env(BIN_ENV).filter(|value| !value.is_empty()) {
        let path = PathBuf::from(explicit);
        if path.is_file() {
            return Ok(path);
        }
        return Err(TelegramError::AsrBinaryMissing(format!(
            "{BIN_ENV} points at {}, which is not a file",
            path.display()
        )));
    }
    let mut path_entries = 0_usize;
    if let Some(path_value) = env("PATH") {
        for directory in std::env::split_paths(&path_value) {
            if directory.as_os_str().is_empty() {
                continue;
            }
            path_entries += 1;
            let candidate = directory.join(BINARY_NAME);
            if candidate.is_file() {
                return Ok(candidate);
            }
        }
    }
    if let Some(home) = env("HOME").map(PathBuf::from) {
        let candidate = home.join(".local/bin").join(BINARY_NAME);
        if candidate.is_file() {
            return Ok(candidate);
        }
    }
    Err(TelegramError::AsrBinaryMissing(format!(
        "{BIN_ENV} is not set and `{BINARY_NAME}` is not in PATH ({path_entries} directories) or ~/.local/bin"
    )))
}

/// Read the per-clip timeout from the environment, bounded to a usable range.
pub fn timeout_from_env<E>(mut env: E) -> Duration
where
    E: FnMut(&str) -> Option<OsString>,
{
    let seconds = env(TIMEOUT_ENV)
        .and_then(|value| value.to_str().and_then(|value| value.trim().parse::<u64>().ok()))
        .unwrap_or(DEFAULT_TIMEOUT_SECONDS)
        .clamp(1, MAX_TIMEOUT_SECONDS);
    Duration::from_secs(seconds)
}

/// Build the tool receipt from a media file that is already on disk.
///
/// Kept apart from the MTProto download so the local path is exercisable
/// without a network or a Telegram session.
pub async fn transcribe_downloaded_media(
    chat_id: &str,
    message_id: i32,
    media: &Path,
    asr: &LocalAsr,
) -> Result<AudioTranscriptionSummary, TelegramError> {
    let transcript = asr.transcribe(media).await?;
    tracing::debug!(
        chat_id,
        message_id,
        provider = PROVIDER,
        words = transcript.word_count,
        "recognised audio on this host"
    );
    Ok(AudioTranscriptionSummary {
        chat_id: chat_id.to_string(),
        message_id,
        transcription_id: transcript.transcription_id,
        text: transcript.text,
        pending: false,
        trial_remains_num: None,
        trial_remains_until_unix: None,
        provider: PROVIDER.to_string(),
    })
}

/// First [`LOCAL_ID_HEX_LEN`] hex digits of the media SHA-256.
async fn media_fingerprint(media: &Path) -> Result<String, TelegramError> {
    let mut file = tokio::fs::File::open(media).await?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0_u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer).await?;
        if read == 0 {
            break;
        }
        hasher.update(buffer.get(..read).unwrap_or_default());
    }
    let digest = hasher.finalize();
    let mut fingerprint = String::with_capacity(LOCAL_ID_HEX_LEN);
    for byte in digest.iter().take(LOCAL_ID_HEX_LEN / 2) {
        fingerprint.push_str(&format!("{byte:02x}"));
    }
    Ok(fingerprint)
}

fn status_text(status: &ExitStatus) -> String {
    match status.code() {
        Some(code) => format!("exited with code {code}"),
        None => "was terminated by a signal".to_string(),
    }
}

/// Last [`STDERR_TAIL_CHARS`] characters of the CLI diagnostics, collapsed to
/// one line so a failure message stays readable.
fn stderr_tail(stderr: &[u8]) -> String {
    let text = String::from_utf8_lossy(stderr);
    let collapsed = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if collapsed.is_empty() {
        return "(no stderr output)".to_string();
    }
    let characters: Vec<char> = collapsed.chars().collect();
    let start = characters.len().saturating_sub(STDERR_TAIL_CHARS);
    characters.into_iter().skip(start).collect()
}

/// Validate a transcription identity: a canonical positive decimal Telegram id
/// or a local `local:<16 lowercase hex>` fingerprint.
pub fn is_local_transcription_id(value: &str) -> bool {
    let Some(fingerprint) = value.strip_prefix(LOCAL_ID_PREFIX) else {
        return false;
    };
    fingerprint.len() == LOCAL_ID_HEX_LEN
        && fingerprint.bytes().all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    struct Script {
        dir: tempfile::TempDir,
    }

    impl Script {
        fn new(body: &str) -> Self {
            let dir = tempfile::tempdir().expect("tempdir");
            // The documented binary name, so a PATH lookup finds it too.
            let path = dir.path().join("hermes");
            std::fs::write(&path, body).expect("write fake CLI");
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700))
                .expect("chmod fake CLI");
            Self { dir }
        }

        fn path(&self) -> PathBuf {
            self.dir.path().join("hermes")
        }

        /// The fake CLI is resolved through `HERMES_ASR_BIN`, exactly as a real
        /// deployment can override the binary.
        fn env(&self, name: &str) -> Option<OsString> {
            match name {
                BIN_ENV => Some(self.path().into_os_string()),
                _ => None,
            }
        }

        fn asr(&self) -> LocalAsr {
            let binary = resolve_binary(|name| self.env(name)).expect("resolve fake CLI");
            LocalAsr::new(binary, Duration::from_secs(20))
        }

        fn media(&self) -> PathBuf {
            let path = self.dir.path().join("media.bin");
            std::fs::write(&path, b"fake-voice-note-bytes").expect("write media");
            path
        }
    }

    #[test]
    fn resolves_the_explicit_override_first() {
        let script = Script::new("#!/bin/sh\nexit 0\n");
        let resolved = resolve_binary(|name| match name {
            BIN_ENV => Some(script.path().into_os_string()),
            "PATH" => Some(OsString::from("/nonexistent-path-entry")),
            _ => None,
        })
        .expect("explicit override wins");
        assert_eq!(resolved, script.path());
    }

    #[test]
    fn falls_back_to_the_first_path_entry() {
        let script = Script::new("#!/bin/sh\nexit 0\n");
        let resolved = resolve_binary(|name| match name {
            "PATH" => Some(script.dir.path().as_os_str().to_os_string()),
            "HOME" => Some(OsString::from("/nonexistent-home")),
            _ => None,
        })
        .expect("PATH lookup");
        assert_eq!(resolved, script.path());
    }

    #[test]
    fn falls_back_to_the_local_bin_directory() {
        let home = tempfile::tempdir().expect("tempdir");
        let bin = home.path().join(".local/bin");
        std::fs::create_dir_all(&bin).expect("create .local/bin");
        let binary = bin.join("hermes");
        std::fs::write(&binary, "#!/bin/sh\nexit 0\n").expect("write CLI");
        let resolved = resolve_binary(|name| match name {
            "PATH" => Some(OsString::from("")),
            "HOME" => Some(home.path().as_os_str().to_os_string()),
            _ => None,
        })
        .expect("home lookup");
        assert_eq!(resolved, binary);
    }

    #[test]
    fn missing_binary_is_typed() {
        let error = resolve_binary(|name| match name {
            "PATH" => Some(OsString::from("/nonexistent-path-entry")),
            "HOME" => Some(OsString::from("/nonexistent-home")),
            _ => None,
        })
        .expect_err("no binary");
        assert!(matches!(error, TelegramError::AsrBinaryMissing(_)), "{error}");
        assert!(error.to_string().contains(BIN_ENV), "{error}");
    }

    #[test]
    fn unusable_override_fails_closed() {
        let error = resolve_binary(|name| match name {
            BIN_ENV => Some(OsString::from("/nonexistent/hermes")),
            "PATH" => Some(OsString::from("/usr/bin")),
            _ => None,
        })
        .expect_err("broken override");
        assert!(matches!(error, TelegramError::AsrBinaryMissing(_)), "{error}");
        assert!(error.to_string().contains("/nonexistent/hermes"), "{error}");
    }

    #[test]
    fn timeout_is_bounded() {
        assert_eq!(timeout_from_env(|_| None), Duration::from_secs(DEFAULT_TIMEOUT_SECONDS));
        assert_eq!(
            timeout_from_env(|name| (name == TIMEOUT_ENV).then(|| OsString::from("42"))),
            Duration::from_secs(42)
        );
        assert_eq!(
            timeout_from_env(|name| (name == TIMEOUT_ENV).then(|| OsString::from("999999"))),
            Duration::from_secs(MAX_TIMEOUT_SECONDS)
        );
        assert_eq!(
            timeout_from_env(|name| (name == TIMEOUT_ENV).then(|| OsString::from("nonsense"))),
            Duration::from_secs(DEFAULT_TIMEOUT_SECONDS)
        );
    }

    #[tokio::test]
    async fn invokes_the_shared_cli_contract() {
        let script = Script::new("#!/bin/sh\nprintf '%s ' \"$@\" >&2\nexit 7\n");
        let media = script.media();
        let error = script.asr().transcribe(&media).await.expect_err("exit 7");
        let TelegramError::AsrFailed(detail) = &error else {
            panic!("expected asr_failed, got {error}");
        };
        assert!(detail.contains("exited with code 7"), "{detail}");
        assert!(
            detail.contains(&format!(
                "nexus transcribe --input {} --output",
                media.display()
            )),
            "{detail}"
        );
        assert!(
            detail.contains("--format json --backend auto"),
            "the contract flags are missing: {detail}"
        );
    }

    #[tokio::test]
    async fn reads_text_and_derives_the_local_identity() {
        let script = Script::new(
            "#!/bin/sh\nprintf '%s' '{\"text\":\"  Привет, мир  \",\"words\":[{\"word\":\"Привет\"},{\"word\":\"мир\"}]}' > \"$6\"\n",
        );
        let media = script.media();
        let asr = script.asr();
        let summary = transcribe_downloaded_media("user:101", 12, &media, &asr)
            .await
            .expect("local transcription");
        assert_eq!(summary.chat_id, "user:101");
        assert_eq!(summary.message_id, 12);
        assert_eq!(summary.text, "Привет, мир");
        assert_eq!(summary.provider, PROVIDER);
        assert_eq!(summary.provider, "local:gigaam-v3-rnnt");
        assert!(!summary.pending);
        assert_eq!(summary.trial_remains_num, None);
        assert_eq!(summary.trial_remains_until_unix, None);
        let expected = format!("{LOCAL_ID_PREFIX}{}", media_fingerprint(&media).await.expect("hash"));
        assert_eq!(summary.transcription_id, expected);
        assert!(is_local_transcription_id(&summary.transcription_id));
        assert_eq!(summary.transcription_id.len(), LOCAL_ID_PREFIX.len() + LOCAL_ID_HEX_LEN);
        // Deterministic: the same media file always yields the same identity.
        let repeated = transcribe_downloaded_media("user:101", 12, &media, &asr)
            .await
            .expect("repeat");
        assert_eq!(repeated.transcription_id, summary.transcription_id);

        let transcript = asr.transcribe(&media).await.expect("transcript");
        assert_eq!(transcript.word_count, 2);
    }

    #[tokio::test]
    async fn reports_the_stderr_tail_when_the_cli_fails() {
        let script = Script::new(
            "#!/bin/sh\nprintf '%s' 'model load failed: gigaam weights are missing' >&2\nexit 1\n",
        );
        let error = script.asr().transcribe(&script.media()).await.expect_err("exit 1");
        let TelegramError::AsrFailed(detail) = &error else {
            panic!("expected asr_failed, got {error}");
        };
        assert!(detail.contains("exited with code 1"), "{detail}");
        assert!(detail.contains("model load failed: gigaam weights are missing"), "{detail}");
    }

    #[test]
    fn the_stderr_tail_is_bounded() {
        let noise = "x".repeat(5_000);
        let tail = stderr_tail(format!("line one\n{noise}\nfinal line\n").as_bytes());
        assert_eq!(tail.chars().count(), STDERR_TAIL_CHARS);
        assert!(tail.ends_with("final line"), "{tail}");
    }

    #[tokio::test]
    async fn rejects_an_empty_transcript() {
        let script = Script::new("#!/bin/sh\nprintf '%s' '{\"text\":\"   \",\"words\":[]}' > \"$6\"\n");
        let summary =
            transcribe_downloaded_media("user:101", 12, &script.media(), &script.asr()).await;
        assert!(matches!(summary, Err(TelegramError::EmptyTranscript(_))), "{summary:?}");
    }

    #[tokio::test]
    async fn rejects_a_missing_output_file() {
        let script = Script::new("#!/bin/sh\nexit 0\n");
        let error = script.asr().transcribe(&script.media()).await.expect_err("no output");
        assert!(matches!(error, TelegramError::AsrFailed(_)), "{error}");
        assert!(error.to_string().contains("wrote no transcript"), "{error}");
    }

    #[tokio::test]
    async fn times_out_a_stuck_cli() {
        let script = Script::new("#!/bin/sh\nsleep 30\n");
        let asr = LocalAsr::new(script.path(), Duration::from_secs(1));
        let error = asr.transcribe(&script.media()).await.expect_err("timeout");
        assert!(matches!(error, TelegramError::AsrTimeout { seconds: 1 }), "{error}");
    }

    #[tokio::test]
    async fn a_disappeared_binary_is_typed() {
        let script = Script::new("#!/bin/sh\nexit 0\n");
        let asr = LocalAsr::new(script.path(), Duration::from_secs(5));
        std::fs::remove_file(script.path()).expect("remove fake CLI");
        let error = asr.transcribe(&script.media()).await.expect_err("missing CLI");
        assert!(matches!(error, TelegramError::AsrBinaryMissing(_)), "{error}");
    }

    #[test]
    fn recognises_only_local_identities() {
        assert!(is_local_transcription_id("local:0123456789abcdef"));
        assert!(!is_local_transcription_id("local:0123456789ABCDEF"));
        assert!(!is_local_transcription_id("local:0123456789abcde"));
        assert!(!is_local_transcription_id("local:0123456789abcdef0"));
        assert!(!is_local_transcription_id("local:g123456789abcdef"));
        assert!(!is_local_transcription_id("123456789"));
    }

    #[test]
    fn workspace_is_private_and_removed() {
        let workspace = MediaWorkspace::create().expect("workspace");
        let dir = workspace.dir.clone();
        assert!(dir.is_dir());
        let mode = std::fs::metadata(&dir).expect("metadata").permissions().mode() & 0o777;
        assert_eq!(mode, 0o700, "{mode:o}");
        drop(workspace);
        assert!(!dir.exists());
    }
}
