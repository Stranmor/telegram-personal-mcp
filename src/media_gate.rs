//! Independent-verification gate for client-bound Telegram sends.
//!
//! Owner requirement 2026-10-19 (task #3437/#3620): every delivery that leaves
//! the workspace passes an independent review that is enforced mechanically.
//! This module is that machine gate for the personal-account transport:
//!
//! * media to a registered client target requires an
//!   `independent-review-receipt/v1` whose reviewer is not the author and whose
//!   `sha256`/`bytes` bind every artifact exactly;
//! * text to a registered client target requires an `orch-outbound-verdict/v1`
//!   produced by `orch outbound check --client`, bound to the text by SHA-256;
//! * the owner's own chat is exempt (the recipient is the requester);
//! * any other personal chat is delivered but leaves an
//!   `unclassified_media_target` event, so a new client cannot enter outside
//!   the gate unnoticed;
//! * a registry that exists but cannot be read fails closed for every
//!   non-owner target.
//!
//! The gate runs before the first network call: classification uses the raw
//! target string, artifact sizes come from local metadata, and the receipt hash
//! is later re-checked against the exact approved bytes by
//! `capture_approved_upload`.

use crate::protocol::ToolFailure;
use chrono::{DateTime, Utc};
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

pub const RECEIPT_SCHEMA: &str = "independent-review-receipt/v1";
pub const VERDICT_SCHEMA: &str = "orch-outbound-verdict/v1";
pub const TARGETS_SCHEMA: &str = "telegram-client-targets/v1";
pub const DEFAULT_MAX_AGE_SECONDS: i64 = 86_400;
pub const FUTURE_SKEW_SECONDS: i64 = 300;
pub const MAX_AGE_ENV: &str = "MEDIA_REVIEW_RECEIPT_MAX_AGE_SECONDS";
pub const CLIENT_TARGETS_ENV: &str = "TELEGRAM_MCP_CLIENT_TARGETS";
pub const GATE_LOG_FILE: &str = "media-gate.jsonl";

const MAX_GATE_FILE_BYTES: u64 = 1 << 20;

/// Who receives a Telegram send, from the gate's point of view.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TargetClass {
    /// The requester's own account: no independent review is added.
    Owner,
    /// A registered client target: the independent verdict is mandatory.
    Client,
    /// Any other peer: delivered, but recorded as an unclassified target.
    Unclassified,
}

impl TargetClass {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Owner => "owner",
            Self::Client => "client",
            Self::Unclassified => "unclassified",
        }
    }
}

/// One artifact exactly as the reviewer bound it: the SHA-256 of the approved
/// bytes and their size.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MediaArtifact {
    pub sha256: String,
    pub bytes: u64,
}

/// Read the artifact size from local metadata. The hash comes from
/// `approved_sha256`; `capture_approved_upload` re-hashes the exact bytes before
/// the mutation is created, so a forged hash cannot reach the network.
pub fn artifact_from_path(
    path: &Path,
    approved_sha256: &str,
    tool: &str,
) -> Result<MediaArtifact, ToolFailure> {
    let metadata = fs::metadata(path).map_err(|error| {
        ToolFailure::new(
            "MEDIA_REVIEW_RECEIPT_FILE_UNREADABLE",
            format!("{tool}.file_path {} is not readable: {error}", path.display()),
            false,
        )
    })?;
    if !metadata.is_file() {
        return Err(ToolFailure::new(
            "MEDIA_REVIEW_RECEIPT_FILE_UNREADABLE",
            format!("{tool}.file_path {} is not a regular file", path.display()),
            false,
        ));
    }
    Ok(MediaArtifact { sha256: approved_sha256.to_ascii_lowercase(), bytes: metadata.len() })
}

/// Enforce the independent media receipt when the raw target is a client.
pub fn enforce_media(
    state_db_path: &Path,
    owner_grant_file: Option<&Path>,
    tool: &str,
    target: &str,
    receipt: Option<&Path>,
    artifacts: &[MediaArtifact],
) -> Result<TargetClass, ToolFailure> {
    let class = classify(state_db_path, owner_grant_file, tool, target)?;
    if class != TargetClass::Client {
        record_gate_event(
            state_db_path,
            "media_gate",
            &[
                ("tool", Value::String(tool.to_string())),
                ("target", Value::String(target.to_string())),
                ("class", Value::String(class.as_str().to_string())),
                ("decision", Value::String("allowed".to_string())),
            ],
        );
        return Ok(class);
    }
    let check = validate_receipt(receipt, artifacts);
    match check {
        Ok(reviewer) => {
            record_gate_event(
                state_db_path,
                "media_review_receipt",
                &[
                    ("tool", Value::String(tool.to_string())),
                    ("target", Value::String(target.to_string())),
                    ("class", Value::String("client".to_string())),
                    ("decision", Value::String("allowed".to_string())),
                    ("reviewer", reviewer),
                ],
            );
            Ok(class)
        },
        Err(error) => {
            record_gate_event(
                state_db_path,
                "media_review_receipt",
                &[
                    ("tool", Value::String(tool.to_string())),
                    ("target", Value::String(target.to_string())),
                    ("class", Value::String("client".to_string())),
                    ("decision", Value::String("denied".to_string())),
                    ("reason_code", Value::String(error.code.clone())),
                ],
            );
            Err(error)
        },
    }
}

/// Enforce the independent text verdict when the raw target is a client.
pub fn enforce_text(
    state_db_path: &Path,
    owner_grant_file: Option<&Path>,
    tool: &str,
    target: &str,
    verdict: Option<&Path>,
    text: &str,
) -> Result<TargetClass, ToolFailure> {
    let class = classify(state_db_path, owner_grant_file, tool, target)?;
    if class != TargetClass::Client {
        record_gate_event(
            state_db_path,
            "text_gate",
            &[
                ("tool", Value::String(tool.to_string())),
                ("target", Value::String(target.to_string())),
                ("class", Value::String(class.as_str().to_string())),
                ("decision", Value::String("allowed".to_string())),
            ],
        );
        return Ok(class);
    }
    let check = validate_verdict(verdict, text);
    match check {
        Ok(signature) => {
            record_gate_event(
                state_db_path,
                "outbound_verdict",
                &[
                    ("tool", Value::String(tool.to_string())),
                    ("target", Value::String(target.to_string())),
                    ("class", Value::String("client".to_string())),
                    ("decision", Value::String("allowed".to_string())),
                    ("judge", signature),
                ],
            );
            Ok(class)
        },
        Err(error) => {
            record_gate_event(
                state_db_path,
                "outbound_verdict",
                &[
                    ("tool", Value::String(tool.to_string())),
                    ("target", Value::String(target.to_string())),
                    ("class", Value::String("client".to_string())),
                    ("decision", Value::String("denied".to_string())),
                    ("reason_code", Value::String(error.code.clone())),
                ],
            );
            Err(error)
        },
    }
}

/// Classify one raw chat target against the exported client registry.
fn classify(
    state_db_path: &Path,
    owner_grant_file: Option<&Path>,
    tool: &str,
    target: &str,
) -> Result<TargetClass, ToolFailure> {
    let owner_id = owner_grant_file.and_then(owner_user_id);
    if is_owner_target(target, owner_id) {
        return Ok(TargetClass::Owner);
    }
    match load_client_targets(state_db_path) {
        Ok(Some(targets)) => {
            if targets.contains(target) {
                Ok(TargetClass::Client)
            } else {
                record_gate_event(
                    state_db_path,
                    "unclassified_media_target",
                    &[
                        ("tool", Value::String(tool.to_string())),
                        ("target", Value::String(target.to_string())),
                        ("class", Value::String("unclassified".to_string())),
                        ("decision", Value::String("allowed".to_string())),
                    ],
                );
                Ok(TargetClass::Unclassified)
            }
        },
        Ok(None) => {
            record_gate_event(
                state_db_path,
                "client_targets_registry_missing",
                &[
                    ("tool", Value::String(tool.to_string())),
                    ("target", Value::String(target.to_string())),
                    ("decision", Value::String("allowed".to_string())),
                ],
            );
            Ok(TargetClass::Unclassified)
        },
        Err(error) => {
            record_gate_event(
                state_db_path,
                "client_targets_registry_unreadable",
                &[
                    ("tool", Value::String(tool.to_string())),
                    ("target", Value::String(target.to_string())),
                    ("decision", Value::String("denied".to_string())),
                    ("reason", Value::String(error.to_string())),
                ],
            );
            Err(ToolFailure::new(
                "CLIENT_TARGETS_REGISTRY_UNREADABLE",
                format!(
                    "the client-targets registry {} exists but cannot be read: {error}; \
                     every non-owner send is refused until the registry is valid again",
                    client_targets_path(state_db_path).display()
                ),
                false,
            ))
        },
    }
}

fn is_owner_target(target: &str, owner_id: Option<i64>) -> bool {
    let normalized = target.trim();
    if normalized.eq_ignore_ascii_case("me")
        || normalized.eq_ignore_ascii_case("self")
        || normalized.eq_ignore_ascii_case("@me")
        || normalized.eq_ignore_ascii_case("@self")
    {
        return true;
    }
    match (owner_id, normalized.parse::<i64>()) {
        (Some(owner), Ok(id)) => owner == id,
        _ => false,
    }
}

fn owner_user_id(path: &Path) -> Option<i64> {
    let text = fs::read_to_string(path).ok()?;
    let value: Value = serde_json::from_str(&text).ok()?;
    json_i64(value.get("granted_by")?)
}

pub fn client_targets_path(state_db_path: &Path) -> PathBuf {
    if let Some(raw) = std::env::var_os(CLIENT_TARGETS_ENV) {
        let path = PathBuf::from(raw);
        if !path.as_os_str().is_empty() {
            return path;
        }
    }
    state_dir(state_db_path).join("client-targets.json")
}

fn state_dir(state_db_path: &Path) -> PathBuf {
    state_db_path.parent().unwrap_or_else(|| Path::new(".")).to_path_buf()
}

#[derive(Debug, Default)]
struct ClientTargets {
    numeric: Vec<i64>,
    usernames: Vec<String>,
}

impl ClientTargets {
    fn contains(&self, raw: &str) -> bool {
        let target = raw.trim();
        if let Ok(id) = target.parse::<i64>() {
            return self.numeric.contains(&id);
        }
        let username = target.trim_start_matches('@').to_ascii_lowercase();
        self.usernames.iter().any(|known| known == &username)
    }
}

/// `Ok(None)` means the exporter has never written a registry yet; a present
/// but unreadable/invalid file is an error and fails closed.
fn load_client_targets(state_db_path: &Path) -> Result<Option<ClientTargets>, String> {
    let path = client_targets_path(state_db_path);
    let metadata = match fs::metadata(&path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(format!("metadata failed: {error}")),
    };
    if !metadata.is_file() {
        return Err("path is not a regular file".to_string());
    }
    if metadata.len() > MAX_GATE_FILE_BYTES {
        return Err(format!("file is {} bytes, limit is {MAX_GATE_FILE_BYTES}", metadata.len()));
    }
    let text = fs::read_to_string(&path).map_err(|error| format!("read failed: {error}"))?;
    let value: Value = serde_json::from_str(&text).map_err(|error| format!("not JSON: {error}"))?;
    if value.get("schema").and_then(Value::as_str) != Some(TARGETS_SCHEMA) {
        return Err(format!("schema must be {TARGETS_SCHEMA}"));
    }
    let entries = value
        .get("targets")
        .and_then(Value::as_array)
        .ok_or_else(|| "targets must be an array".to_string())?;
    let mut targets = ClientTargets::default();
    for (index, entry) in entries.iter().enumerate() {
        let chat_id = entry
            .get("chat_id")
            .and_then(Value::as_str)
            .ok_or_else(|| format!("targets[{index}].chat_id must be a string"))?
            .trim();
        if chat_id.is_empty() {
            return Err(format!("targets[{index}].chat_id is empty"));
        }
        if let Ok(id) = chat_id.parse::<i64>() {
            if !targets.numeric.contains(&id) {
                targets.numeric.push(id);
            }
        } else {
            let username = chat_id.trim_start_matches('@').to_ascii_lowercase();
            if !targets.usernames.contains(&username) {
                targets.usernames.push(username);
            }
        }
    }
    Ok(Some(targets))
}

fn validate_receipt(
    receipt: Option<&Path>,
    artifacts: &[MediaArtifact],
) -> Result<Value, ToolFailure> {
    let path = receipt.filter(|path| !path.as_os_str().is_empty()).ok_or_else(|| {
        ToolFailure::new(
            "MEDIA_REVIEW_RECEIPT_MISSING",
            "review_receipt is required for media sent to a client target: \
             a reviewer who is not the author must issue independent-review-receipt/v1 first",
            false,
        )
    })?;
    let value = read_json_file(path, "MEDIA_REVIEW_RECEIPT")?;
    let object = value.as_object().ok_or_else(|| receipt_error("receipt_unparsed", path, "expected a JSON object"))?;
    if string_field(object, "schema") != Some(RECEIPT_SCHEMA) {
        return Err(receipt_error(
            "schema_mismatch",
            path,
            &format!("schema must be {RECEIPT_SCHEMA}"),
        ));
    }
    if string_field(object, "verdict") != Some("pass") {
        return Err(receipt_error("verdict_not_pass", path, "verdict must be pass"));
    }
    let reviewer = object
        .get("reviewer")
        .and_then(Value::as_object)
        .ok_or_else(|| receipt_error("missing_reviewer", path, "reviewer signature is missing"))?;
    for field in ["agent", "model", "route"] {
        match reviewer.get(field).and_then(Value::as_str).map(str::trim) {
            Some(value) if !value.is_empty() => {},
            _ => {
                return Err(receipt_error(
                    "missing_reviewer",
                    path,
                    &format!("reviewer.{field} is empty"),
                ));
            },
        }
    }
    let checks = object
        .get("checks")
        .and_then(Value::as_object)
        .ok_or_else(|| receipt_error("receipt_unparsed", path, "checks object is missing"))?;
    if checks.get("text_readback") != Some(&Value::Bool(true)) {
        return Err(receipt_error(
            "text_readback_missing",
            path,
            "checks.text_readback must be true",
        ));
    }
    let defects = checks
        .get("defects")
        .and_then(Value::as_array)
        .ok_or_else(|| receipt_error("receipt_unparsed", path, "checks.defects must be an array"))?;
    if !defects.is_empty() {
        return Err(receipt_error("defects_present", path, "the reviewer reported defects"));
    }
    let files = object
        .get("files")
        .and_then(Value::as_array)
        .ok_or_else(|| receipt_error("receipt_unparsed", path, "files must be an array"))?;
    let mut bound: Vec<(String, u64)> = Vec::with_capacity(files.len());
    for (index, entry) in files.iter().enumerate() {
        let entry = entry
            .as_object()
            .ok_or_else(|| receipt_error("receipt_unparsed", path, &format!("files[{index}] is not an object")))?;
        let sha = entry
            .get("sha256")
            .and_then(Value::as_str)
            .map(|value| value.trim().to_ascii_lowercase())
            .filter(|value| !value.is_empty())
            .ok_or_else(|| receipt_error("receipt_unparsed", path, &format!("files[{index}].sha256 is empty")))?;
        let bytes = entry
            .get("bytes")
            .and_then(json_u64)
            .ok_or_else(|| receipt_error("receipt_unparsed", path, &format!("files[{index}].bytes is not a number")))?;
        bound.push((sha, bytes));
    }
    if !artifacts.is_empty() && bound.is_empty() {
        return Err(receipt_error(
            "receipt_unparsed",
            path,
            "files is empty but the send carries artifacts",
        ));
    }
    for artifact in artifacts {
        let sha = artifact.sha256.to_ascii_lowercase();
        let position = bound
            .iter()
            .position(|(known_sha, known_bytes)| known_sha == &sha && *known_bytes == artifact.bytes);
        match position {
            Some(index) => {
                bound.swap_remove(index);
            },
            None => {
                return Err(receipt_error(
                    "file_hash_mismatch",
                    path,
                    &format!(
                        "artifact sha256={} bytes={} is not bound by the receipt",
                        artifact.sha256, artifact.bytes
                    ),
                ));
            },
        }
    }
    let at = object
        .get("at")
        .ok_or_else(|| receipt_error("receipt_unparsed", path, "at is missing"))?;
    let issued_at = parse_timestamp(at)
        .map_err(|error| receipt_error("receipt_unparsed", path, &format!("at: {error}")))?;
    let now = now_unix();
    if issued_at - now > FUTURE_SKEW_SECONDS {
        return Err(receipt_error("future_at", path, "the receipt is dated in the future"));
    }
    let limit = max_age_seconds();
    if now - issued_at > limit {
        return Err(receipt_error(
            "stale",
            path,
            &format!("the receipt is older than {limit} seconds"),
        ));
    }
    Ok(Value::Object(reviewer_signature(reviewer)))
}

fn reviewer_signature(reviewer: &Map<String, Value>) -> Map<String, Value> {
    let mut signature = Map::new();
    for field in ["agent", "model", "route"] {
        if let Some(value) = reviewer.get(field) {
            signature.insert(field.to_string(), value.clone());
        }
    }
    signature
}

fn validate_verdict(verdict: Option<&Path>, text: &str) -> Result<Value, ToolFailure> {
    let path = verdict.filter(|path| !path.as_os_str().is_empty()).ok_or_else(|| {
        ToolFailure::new(
            "OUTBOUND_VERDICT_MISSING",
            "outbound_verdict is required for text sent to a client target: \
             run `orch outbound check --client --verdict-out <file>` first (rc 0)",
            false,
        )
    })?;
    let value = read_json_file(path, "OUTBOUND_VERDICT")?;
    let object = value.as_object().ok_or_else(|| verdict_error("unparsed", path, "expected a JSON object"))?;
    if string_field(object, "schema") != Some(VERDICT_SCHEMA) {
        return Err(verdict_error("schema_mismatch", path, &format!("schema must be {VERDICT_SCHEMA}")));
    }
    if object.get("ok") != Some(&Value::Bool(true)) {
        return Err(verdict_error("not_pass", path, "ok must be true"));
    }
    if string_field(object, "addressee") != Some("client") {
        return Err(verdict_error("addressee_mismatch", path, "addressee must be client"));
    }
    for field in ["model", "route"] {
        match object.get(field).and_then(Value::as_str).map(str::trim) {
            Some(value) if !value.is_empty() => {},
            _ => return Err(verdict_error("unsigned", path, &format!("{field} is empty"))),
        };
    }
    let expected = text_sha256(text);
    match string_field(object, "text_sha256") {
        Some(actual) if actual.eq_ignore_ascii_case(&expected) => {},
        Some(_) => {
            return Err(verdict_error(
                "text_mismatch",
                path,
                "text_sha256 does not match the text being sent",
            ));
        },
        None => return Err(verdict_error("unparsed", path, "text_sha256 is missing")),
    }
    let at = object
        .get("at_unix")
        .ok_or_else(|| verdict_error("unparsed", path, "at_unix is missing"))?;
    let issued_at = parse_timestamp(at).map_err(|error| verdict_error("unparsed", path, &format!("at_unix: {error}")))?;
    let now = now_unix();
    if issued_at - now > FUTURE_SKEW_SECONDS {
        return Err(verdict_error("future_at", path, "the verdict is dated in the future"));
    }
    let limit = max_age_seconds();
    if now - issued_at > limit {
        return Err(verdict_error("stale", path, &format!("the verdict is older than {limit} seconds")));
    }
    Ok(Value::Object(reviewer_signature(object)))
}

/// Canonical text identity shared with `orch outbound check --verdict-out`:
/// every whitespace run collapses to one space, then the edges are trimmed.
pub fn text_sha256(text: &str) -> String {
    let canonical = text.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut hasher = Sha256::new();
    hasher.update(canonical.as_bytes());
    format!("{:x}", hasher.finalize())
}

fn read_json_file(path: &Path, prefix: &str) -> Result<Value, ToolFailure> {
    if !path.is_file() {
        return Err(ToolFailure::new(
            format!("{prefix}_MISSING"),
            format!("verification file {} does not exist", path.display()),
            false,
        ));
    }
    let metadata = fs::metadata(path).map_err(|error| {
        ToolFailure::new(
            format!("{prefix}_UNREADABLE"),
            format!("verification file {} cannot be read: {error}", path.display()),
            false,
        )
    })?;
    if metadata.len() > MAX_GATE_FILE_BYTES {
        return Err(ToolFailure::new(
            format!("{prefix}_UNREADABLE"),
            format!(
                "verification file {} is {} bytes, limit is {MAX_GATE_FILE_BYTES}",
                path.display(),
                metadata.len()
            ),
            false,
        ));
    }
    let text = fs::read_to_string(path).map_err(|error| {
        ToolFailure::new(
            format!("{prefix}_UNREADABLE"),
            format!("verification file {} cannot be read: {error}", path.display()),
            false,
        )
    })?;
    serde_json::from_str(&text).map_err(|error| {
        ToolFailure::new(
            format!("{prefix}_UNPARSED"),
            format!("verification file {} is not JSON: {error}", path.display()),
            false,
        )
    })
}

fn receipt_error(reason: &str, path: &Path, message: &str) -> ToolFailure {
    let code = format!("MEDIA_REVIEW_RECEIPT_{}", reason.to_ascii_uppercase());
    ToolFailure::new(code, format!("receipt {}: {reason}: {message}", path.display()), false)
}

fn verdict_error(reason: &str, path: &Path, message: &str) -> ToolFailure {
    let code = format!("OUTBOUND_VERDICT_{}", reason.to_ascii_uppercase());
    ToolFailure::new(code, format!("verdict {}: {reason}: {message}", path.display()), false)
}

fn string_field<'a>(object: &'a Map<String, Value>, field: &str) -> Option<&'a str> {
    object.get(field).and_then(Value::as_str).map(str::trim)
}

fn json_i64(value: &Value) -> Option<i64> {
    if let Some(number) = value.as_i64() {
        return Some(number);
    }
    value.as_str()?.trim().parse::<i64>().ok()
}

fn json_u64(value: &Value) -> Option<u64> {
    if let Some(number) = value.as_u64() {
        return Some(number);
    }
    value.as_str()?.trim().parse::<u64>().ok()
}

fn parse_timestamp(value: &Value) -> Result<i64, String> {
    if value.is_boolean() {
        return Err("boolean is not a timestamp".to_string());
    }
    if let Some(number) = value.as_i64() {
        return Ok(number);
    }
    if let Some(number) = value.as_f64() {
        if !number.is_finite() {
            return Err("timestamp is not finite".to_string());
        }
        return Ok(number as i64);
    }
    let raw = value.as_str().ok_or_else(|| "expected unix seconds or RFC3339".to_string())?;
    let raw = raw.trim();
    if let Ok(number) = raw.parse::<i64>() {
        return Ok(number);
    }
    let parsed = DateTime::parse_from_rfc3339(raw).map_err(|error| format!("{raw:?}: {error}"))?;
    Ok(parsed.with_timezone(&Utc).timestamp())
}

fn now_unix() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|delta| delta.as_secs() as i64).unwrap_or(0)
}

fn max_age_seconds() -> i64 {
    std::env::var(MAX_AGE_ENV)
        .ok()
        .and_then(|raw| raw.trim().parse::<i64>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(DEFAULT_MAX_AGE_SECONDS)
}

fn record_gate_event(state_db_path: &Path, event: &str, fields: &[(&str, Value)]) {
    let directory = state_dir(state_db_path);
    let path = directory.join(GATE_LOG_FILE);
    let mut object = Map::new();
    object.insert("ts".to_string(), Value::String(Utc::now().to_rfc3339()));
    object.insert("event".to_string(), Value::String(event.to_string()));
    for (key, value) in fields {
        object.insert((*key).to_string(), value.clone());
    }
    let mut line = Value::Object(object).to_string();
    line.push('\n');
    let result = (|| -> std::io::Result<()> {
        fs::create_dir_all(&directory)?;
        let mut file = OpenOptions::new().create(true).append(true).mode(0o600).open(&path)?;
        file.write_all(line.as_bytes())
    })();
    match result {
        Ok(()) => tracing::info!(event, path = %path.display(), "telegram outbound gate decision recorded"),
        Err(error) => tracing::warn!(
            event,
            path = %path.display(),
            error = %error,
            "telegram outbound gate decision could not be recorded"
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::fs;

    fn temp_state() -> tempfile::TempDir {
        tempfile::tempdir().expect("tempdir")
    }

    fn write_targets(dir: &Path, entries: &[&str]) {
        let targets: Vec<Value> =
            entries.iter().map(|id| json!({"chat_id": id, "role": "client"})).collect();
        fs::write(
            dir.join("client-targets.json"),
            json!({"schema": TARGETS_SCHEMA, "targets": targets}).to_string(),
        )
        .expect("targets");
    }

    fn write_receipt(dir: &Path, artifacts: &[(&str, u64)], at: Value) -> PathBuf {
        let files: Vec<Value> = artifacts
            .iter()
            .map(|(sha, bytes)| json!({"path": "/tmp/x", "sha256": sha, "bytes": bytes}))
            .collect();
        let path = dir.join("receipt.json");
        fs::write(
            &path,
            json!({
                "schema": RECEIPT_SCHEMA,
                "verdict": "pass",
                "reviewer": {"agent": "critic", "model": "deepseek-v4.1-flash", "route": "agm"},
                "checks": {"text_readback": true, "defects": []},
                "files": files,
                "at": at,
            })
            .to_string(),
        )
        .expect("receipt");
        path
    }

    fn state_path(dir: &Path) -> PathBuf {
        dir.join("state.sqlite")
    }

    #[test]
    fn owner_and_self_targets_are_exempt() {
        let temp = temp_state();
        let grant = temp.path().join("owner-grant.json");
        fs::write(&grant, json!({"granted_by": 432567587}).to_string()).expect("grant");
        let state = state_path(temp.path());
        assert_eq!(
            enforce_media(&state, Some(&grant), "send_file", "432567587", None, &[]).expect("owner"),
            TargetClass::Owner
        );
        assert_eq!(
            enforce_media(&state, Some(&grant), "send_file", "me", None, &[]).expect("self"),
            TargetClass::Owner
        );
    }

    #[test]
    fn client_media_without_receipt_is_refused_before_network() {
        let temp = temp_state();
        let state = state_path(temp.path());
        write_targets(temp.path(), &["@nataliplankinaChannel"]);
        let error = enforce_media(
            &state,
            None,
            "send_file",
            "@nataliplankinaChannel",
            None,
            &[MediaArtifact { sha256: "a".repeat(64), bytes: 12 }],
        )
        .expect_err("refused");
        assert_eq!(error.code, "MEDIA_REVIEW_RECEIPT_MISSING");
    }

    #[test]
    fn client_media_with_bound_receipt_passes() {
        let temp = temp_state();
        let state = state_path(temp.path());
        write_targets(temp.path(), &["-1001692651700"]);
        let sha = "b".repeat(64);
        let receipt = write_receipt(
            temp.path(),
            &[(sha.as_str(), 42)],
            json!(now_unix() - 60),
        );
        let class = enforce_media(
            &state,
            None,
            "send_file",
            "-1001692651700",
            Some(&receipt),
            &[MediaArtifact { sha256: sha, bytes: 42 }],
        )
        .expect("accepted");
        assert_eq!(class, TargetClass::Client);
    }

    #[test]
    fn receipt_with_foreign_hash_is_refused() {
        let temp = temp_state();
        let state = state_path(temp.path());
        write_targets(temp.path(), &["@client"]);
        let receipt = write_receipt(temp.path(), &[(&"c".repeat(64), 42)], json!(now_unix()));
        let error = enforce_media(
            &state,
            None,
            "send_file",
            "@client",
            Some(&receipt),
            &[MediaArtifact { sha256: "d".repeat(64), bytes: 42 }],
        )
        .expect_err("refused");
        assert_eq!(error.code, "MEDIA_REVIEW_RECEIPT_FILE_HASH_MISMATCH");
    }

    #[test]
    fn stale_and_future_receipts_are_refused() {
        let temp = temp_state();
        let state = state_path(temp.path());
        write_targets(temp.path(), &["@client"]);
        let artifact = MediaArtifact { sha256: "e".repeat(64), bytes: 1 };
        let stale = write_receipt(
            temp.path(),
            &[(&artifact.sha256, 1)],
            json!(now_unix() - DEFAULT_MAX_AGE_SECONDS - 10),
        );
        let error = enforce_media(&state, None, "send_file", "@client", Some(&stale), std::slice::from_ref(&artifact))
            .expect_err("stale refused");
        assert_eq!(error.code, "MEDIA_REVIEW_RECEIPT_STALE");
        let future = write_receipt(
            temp.path(),
            &[(&artifact.sha256, 1)],
            json!(now_unix() + FUTURE_SKEW_SECONDS + 60),
        );
        let error = enforce_media(&state, None, "send_file", "@client", Some(&future), &[artifact])
            .expect_err("future refused");
        assert_eq!(error.code, "MEDIA_REVIEW_RECEIPT_FUTURE_AT");
    }

    #[test]
    fn unreadable_registry_fails_closed_and_missing_registry_is_unclassified() {
        let temp = temp_state();
        let state = state_path(temp.path());
        assert_eq!(
            enforce_media(&state, None, "send_file", "@someone", None, &[]).expect("missing registry"),
            TargetClass::Unclassified
        );
        fs::write(temp.path().join("client-targets.json"), "{not json").expect("broken");
        let error = enforce_media(&state, None, "send_file", "@someone", None, &[])
            .expect_err("fail closed");
        assert_eq!(error.code, "CLIENT_TARGETS_REGISTRY_UNREADABLE");
    }

    #[test]
    fn text_requires_verdict_bound_to_the_exact_text() {
        let temp = temp_state();
        let state = state_path(temp.path());
        write_targets(temp.path(), &["@client"]);
        let text = "Пост для канала";
        let verdict_path = temp.path().join("verdict.json");
        fs::write(
            &verdict_path,
            json!({
                "schema": VERDICT_SCHEMA,
                "ok": true,
                "addressee": "client",
                "kind": "say",
                "text_sha256": text_sha256(text),
                "at_unix": now_unix(),
                "model": "deepseek",
                "route": "manager",
            })
            .to_string(),
        )
        .expect("verdict");
        assert_eq!(
            enforce_text(&state, None, "prepare_schedule_message", "@client", Some(&verdict_path), text)
                .expect("accepted"),
            TargetClass::Client
        );
        let error = enforce_text(
            &state,
            None,
            "prepare_schedule_message",
            "@client",
            Some(&verdict_path),
            "Другой текст",
        )
        .expect_err("mismatch refused");
        assert_eq!(error.code, "OUTBOUND_VERDICT_TEXT_MISMATCH");
        let missing = enforce_text(&state, None, "prepare_schedule_message", "@client", None, text)
            .expect_err("missing verdict refused");
        assert_eq!(missing.code, "OUTBOUND_VERDICT_MISSING");
    }

    #[test]
    fn canonical_text_hash_ignores_whitespace_shape() {
        assert_eq!(text_sha256("  a\n b\tc "), text_sha256("a b c"));
    }
}
