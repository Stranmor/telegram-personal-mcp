#![cfg_attr(not(unix), allow(dead_code))]
#![cfg_attr(
    test,
    allow(
        clippy::arithmetic_side_effects,
        clippy::cast_possible_truncation,
        clippy::cast_possible_wrap,
        clippy::expect_used,
        clippy::indexing_slicing,
        clippy::panic,
        clippy::unwrap_used,
    )
)]

#[cfg(not(unix))]
compile_error!("the telegram-personal-mcp daemon transport is supported only on Unix");

use fs2::FileExt;
use rmcp::ServiceExt;
use std::ffi::OsString;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::ops::Deref;
use std::os::unix::fs::{FileTypeExt, MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::time::Duration;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::watch;
use tokio::task::JoinSet;
use tokio::time::Instant;

const APP_DIR_NAME: &str = "telegram-personal-mcp";
const SOCKET_FILE_NAME: &str = "daemon.sock";
const LOCK_FILE_NAME: &str = "daemon.lock";
const LOG_FILE_NAME: &str = "daemon.log";
const IDENTITY_FILE_NAME: &str = "daemon.identity";
const IDENTITY_FORMAT: &str = "telegram-personal-mcp-daemon-identity-v1";
const SOCKET_POLL_INTERVAL: Duration = Duration::from_millis(100);

unsafe extern "C" {
    fn geteuid() -> u32;
}

/// Canonical filesystem locations used by one daemon and all of its stdio proxies.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DaemonPaths {
    pub base_dir: PathBuf,
    pub socket_path: PathBuf,
    pub lock_path: PathBuf,
    pub log_path: PathBuf,
    pub identity_path: PathBuf,
}

impl DaemonPaths {
    /// Derive every daemon path from an explicitly selected private directory.
    pub fn new(base_dir: impl Into<PathBuf>) -> Self {
        let base_dir = base_dir.into();
        Self {
            socket_path: base_dir.join(SOCKET_FILE_NAME),
            lock_path: base_dir.join(LOCK_FILE_NAME),
            log_path: base_dir.join(LOG_FILE_NAME),
            identity_path: base_dir.join(IDENTITY_FILE_NAME),
            base_dir,
        }
    }

    /// Resolve the daemon directory without reading any credential-bearing state.
    ///
    /// The order is an explicit override, the per-user runtime directory, the
    /// per-user state directory, the conventional state directory below HOME,
    /// and finally a UID-scoped temporary directory.
    pub fn resolve_default() -> Self {
        resolve_with_env(|name| std::env::var_os(name), std::env::temp_dir(), effective_user_id())
    }

    /// Create or repair the daemon directory to owner-only permissions.
    pub fn ensure_private_base_dir(&self) -> io::Result<()> {
        ensure_private_directory(&self.base_dir)
    }

    /// Open the daemon log for append with owner-only permissions.
    ///
    /// The caller owns log routing and must not write MCP protocol bytes here.
    pub fn open_log(&self) -> io::Result<File> {
        self.ensure_private_base_dir()?;
        open_private_append_file(&self.log_path)
    }
}

/// Secret-free digest of every setting that changes daemon runtime behavior.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DaemonConfigIdentity {
    sha256: String,
}

impl DaemonConfigIdentity {
    pub fn from_sha256(sha256: String) -> io::Result<Self> {
        if sha256.len() != 64
            || !sha256.bytes().all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "daemon configuration identity must be a lowercase SHA-256",
            ));
        }
        Ok(Self { sha256 })
    }

    pub fn sha256(&self) -> &str {
        &self.sha256
    }
}

impl Default for DaemonPaths {
    fn default() -> Self {
        Self::resolve_default()
    }
}

/// Result of a nonblocking singleton-lock attempt.
#[derive(Debug)]
pub enum AcquireOutcome {
    Acquired(DaemonLease),
    AlreadyRunning,
}

/// Exclusive ownership of the daemon paths for one process lifetime.
#[derive(Debug)]
pub struct DaemonLease {
    paths: DaemonPaths,
    lock_file: File,
    bound_socket: Option<FileIdentity>,
    bound_identity: Option<FileIdentity>,
    active: bool,
}

impl DaemonLease {
    /// Try to acquire the singleton lease without waiting.
    ///
    /// The PID is replaced only after the exclusive lock has been acquired.
    pub fn try_acquire(paths: DaemonPaths) -> io::Result<AcquireOutcome> {
        paths.ensure_private_base_dir()?;
        drop(paths.open_log()?);

        let mut lock_file = open_private_lock_file(&paths.lock_path)?;
        match FileExt::try_lock_exclusive(&lock_file) {
            Ok(()) => {},
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                return Ok(AcquireOutcome::AlreadyRunning);
            },
            Err(error) => return Err(error),
        }

        if let Err(error) = write_current_pid(&mut lock_file) {
            let _ = FileExt::unlock(&lock_file);
            return Err(error);
        }

        Ok(AcquireOutcome::Acquired(Self {
            paths,
            lock_file,
            bound_socket: None,
            bound_identity: None,
            active: true,
        }))
    }

    pub fn paths(&self) -> &DaemonPaths {
        &self.paths
    }

    fn remove_stale_socket(&mut self) -> io::Result<()> {
        if !self.active {
            return Err(io::Error::new(
                io::ErrorKind::NotConnected,
                "daemon lease is no longer active",
            ));
        }

        let metadata = match fs::symlink_metadata(&self.paths.socket_path) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
            Err(error) => return Err(error),
        };
        if !metadata.file_type().is_socket() {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                "daemon socket path is occupied by a non-socket entry",
            ));
        }
        if metadata.uid() != effective_user_id() {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "daemon socket is not owned by the current user",
            ));
        }

        fs::remove_file(&self.paths.socket_path)
    }

    fn write_identity(&mut self, identity: &DaemonConfigIdentity) -> io::Result<()> {
        if !self.active {
            return Err(io::Error::new(
                io::ErrorKind::NotConnected,
                "daemon lease is no longer active",
            ));
        }
        let mut file = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(&self.paths.identity_path)?;
        enforce_private_regular_file(&file)?;
        write!(
            file,
            "{IDENTITY_FORMAT}\npid={}\nconfig_sha256={}\n",
            std::process::id(),
            identity.sha256()
        )?;
        file.flush()?;
        file.sync_data()?;
        self.bound_identity = Some(FileIdentity::from_metadata(&file.metadata()?));
        Ok(())
    }
}

impl Drop for DaemonLease {
    fn drop(&mut self) {
        if !self.active {
            return;
        }

        if let Some(identity) = self.bound_socket.take() {
            remove_socket_if_same(&self.paths.socket_path, identity);
        }
        if let Some(identity) = self.bound_identity.take() {
            remove_regular_file_if_same(&self.paths.identity_path, identity);
        }

        let _ = self.lock_file.set_len(0);
        let _ = self.lock_file.sync_data();
        let _ = FileExt::unlock(&self.lock_file);
        self.active = false;
    }
}

/// A listener whose lifetime is statically tied to its singleton lease.
#[derive(Debug)]
pub struct PrivateListener<'lease> {
    listener: UnixListener,
    _lease: &'lease mut DaemonLease,
}

impl Deref for PrivateListener<'_> {
    type Target = UnixListener;

    fn deref(&self) -> &Self::Target {
        &self.listener
    }
}

/// Bind an owner-only Unix listener after proving singleton ownership.
///
/// A leftover socket is removed only through the held lease. Regular files,
/// directories, and foreign-owned sockets are never removed.
pub fn bind_private_listener<'lease>(
    lease: &'lease mut DaemonLease,
) -> io::Result<PrivateListener<'lease>> {
    lease.remove_stale_socket()?;
    let listener = UnixListener::bind(&lease.paths.socket_path)?;

    let initial_metadata = fs::symlink_metadata(&lease.paths.socket_path)?;
    if !initial_metadata.file_type().is_socket() || initial_metadata.uid() != effective_user_id() {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "bound daemon socket ownership changed unexpectedly",
        ));
    }
    let identity = FileIdentity::from_metadata(&initial_metadata);

    if let Err(error) =
        fs::set_permissions(&lease.paths.socket_path, fs::Permissions::from_mode(0o600))
    {
        remove_socket_if_same(&lease.paths.socket_path, identity);
        return Err(error);
    }

    let metadata = match fs::symlink_metadata(&lease.paths.socket_path) {
        Ok(metadata) => metadata,
        Err(error) => {
            remove_socket_if_same(&lease.paths.socket_path, identity);
            return Err(error);
        },
    };
    if !metadata.file_type().is_socket()
        || metadata.uid() != effective_user_id()
        || metadata.mode() & 0o777 != 0o600
        || FileIdentity::from_metadata(&metadata) != identity
    {
        remove_socket_if_same(&lease.paths.socket_path, identity);
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "daemon socket did not satisfy owner-only invariants",
        ));
    }

    lease.bound_socket = Some(identity);
    Ok(PrivateListener { listener, _lease: lease })
}

/// Connect to the currently bound private daemon socket.
pub async fn connect_existing(paths: &DaemonPaths) -> io::Result<UnixStream> {
    validate_private_socket(paths)?;
    UnixStream::connect(&paths.socket_path).await
}

/// Connect only when the live daemon proves it was started from the same
/// effective configuration as the requesting stdio proxy.
pub async fn connect_existing_matching(
    paths: &DaemonPaths,
    expected: &DaemonConfigIdentity,
) -> io::Result<UnixStream> {
    let stream = connect_existing(paths).await?;
    let before = read_runtime_identity(paths).map_err(|error| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "connected daemon has no trustworthy configuration identity: {error}; stop and restart that daemon"
            ),
        )
    })?;
    validate_runtime_identity(paths, &before, expected)?;
    let after = read_runtime_identity(paths).map_err(|error| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("daemon configuration identity became unreadable: {error}"),
        )
    })?;
    if before != after {
        return Err(io::Error::new(
            io::ErrorKind::ConnectionAborted,
            "daemon identity changed while the proxy connected; retry the request",
        ));
    }
    Ok(stream)
}

/// Wait up to `timeout` for a daemon socket to become connectable.
///
/// Missing and not-yet-listening sockets are polled at most every 100 ms.
pub async fn wait_for_socket(paths: &DaemonPaths, timeout: Duration) -> io::Result<UnixStream> {
    let deadline = Instant::now().checked_add(timeout).ok_or_else(|| {
        io::Error::new(io::ErrorKind::InvalidInput, "daemon socket timeout is too large")
    })?;
    loop {
        match connect_existing(paths).await {
            Ok(stream) => return Ok(stream),
            Err(error) if is_socket_startup_error(&error) => {},
            Err(error) => return Err(error),
        }

        let now = Instant::now();
        if now >= deadline {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "daemon socket did not become ready before the timeout",
            ));
        }
        tokio::time::sleep(SOCKET_POLL_INTERVAL.min(deadline.saturating_duration_since(now))).await;
    }
}

/// Wait for a daemon that is both connectable and configuration-compatible.
pub async fn wait_for_matching_socket(
    paths: &DaemonPaths,
    expected: &DaemonConfigIdentity,
    timeout: Duration,
) -> io::Result<UnixStream> {
    let deadline = Instant::now().checked_add(timeout).ok_or_else(|| {
        io::Error::new(io::ErrorKind::InvalidInput, "matching socket timeout is too large")
    })?;
    loop {
        match connect_existing_matching(paths, expected).await {
            Ok(stream) => return Ok(stream),
            Err(error) if is_socket_startup_error(&error) => {},
            Err(error) => return Err(error),
        }

        let now = Instant::now();
        if now >= deadline {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "matching daemon socket did not become ready before the timeout",
            ));
        }
        tokio::time::sleep(SOCKET_POLL_INTERVAL.min(deadline.saturating_duration_since(now))).await;
    }
}

/// Relay a stdio MCP transport byte-for-byte through a daemon connection.
pub async fn proxy_stdio(stream: UnixStream) -> io::Result<()> {
    proxy_io(tokio::io::stdin(), tokio::io::stdout(), stream).await
}

/// Ignore terminal hangup and interrupt signals in a stdio proxy.
///
/// SIGTERM is deliberately not registered and retains its existing behavior.
pub fn install_proxy_signal_ignores() -> io::Result<()> {
    let runtime = tokio::runtime::Handle::try_current()
        .map_err(|_| io::Error::other("proxy signal handling requires an active Tokio runtime"))?;
    let mut hangup = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::hangup())?;
    let mut interrupt = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::interrupt())?;

    runtime.spawn(async move {
        loop {
            tokio::select! {
                signal = hangup.recv() => {
                    if signal.is_none() {
                        break;
                    }
                }
                signal = interrupt.recv() => {
                    if signal.is_none() {
                        break;
                    }
                }
            }
        }
    });
    Ok(())
}

/// Errors that prevent the daemon itself from running.
#[derive(Debug, thiserror::Error)]
pub enum DaemonError {
    #[error("daemon is already running")]
    AlreadyRunning,
    #[error("daemon I/O operation failed")]
    Io(#[from] io::Error),
}

/// Exclusive daemon ownership acquired before any backend or worker may start.
#[derive(Debug)]
pub struct PreparedDaemon {
    lease: DaemonLease,
}

/// Acquire singleton ownership and publish the effective configuration
/// identity before constructing side-effecting runtime components.
pub fn prepare_mcp_daemon(
    paths: DaemonPaths,
    identity: DaemonConfigIdentity,
) -> Result<PreparedDaemon, DaemonError> {
    let mut lease = match DaemonLease::try_acquire(paths)? {
        AcquireOutcome::Acquired(lease) => lease,
        AcquireOutcome::AlreadyRunning => return Err(DaemonError::AlreadyRunning),
    };
    lease.write_identity(&identity)?;
    Ok(PreparedDaemon { lease })
}

/// Run one shared MCP server/backend over a prepared private singleton lease.
pub async fn run_mcp_daemon(
    server: crate::mcp::McpServer,
    mut prepared: PreparedDaemon,
) -> Result<(), DaemonError> {
    let listener = bind_private_listener(&mut prepared.lease)?;
    let mut terminate = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
    let mut interrupt = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::interrupt())?;
    let (shutdown_tx, shutdown_rx) = watch::channel(false);
    let mut connections = JoinSet::new();

    let accept_error = loop {
        tokio::select! {
            _ = terminate.recv() => break None,
            _ = interrupt.recv() => break None,
            completed = connections.join_next(), if !connections.is_empty() => {
                if completed.is_some_and(|result| result.is_err()) {
                    tracing::warn!("daemon MCP connection task ended unexpectedly");
                }
            }
            accepted = listener.accept() => {
                match accepted {
                    Ok((stream, _address)) => {
                        connections.spawn(serve_connection(
                            server.clone(),
                            stream,
                            shutdown_rx.clone(),
                        ));
                    }
                    Err(error) => break Some(error),
                }
            }
        }
    };

    drop(listener);
    let _ = shutdown_tx.send(true);
    while let Some(result) = connections.join_next().await {
        if result.is_err() {
            tracing::warn!("daemon MCP connection task ended unexpectedly during shutdown");
        }
    }

    match accept_error {
        Some(error) => Err(DaemonError::Io(error)),
        None => Ok(()),
    }
}

async fn serve_connection(
    server: crate::mcp::McpServer,
    stream: UnixStream,
    mut shutdown: watch::Receiver<bool>,
) {
    if *shutdown.borrow() {
        return;
    }

    let initializing = server.serve(stream);
    tokio::pin!(initializing);
    let running = tokio::select! {
        result = &mut initializing => {
            match result {
                Ok(running) => running,
                Err(_) => {
                    tracing::warn!("daemon MCP connection initialization failed");
                    return;
                }
            }
        }
        _ = shutdown.changed() => return,
    };

    let cancellation = running.cancellation_token();
    let waiting = running.waiting();
    tokio::pin!(waiting);
    tokio::select! {
        result = &mut waiting => {
            if result.is_err() {
                tracing::warn!("daemon MCP connection ended unexpectedly");
            }
        }
        _ = shutdown.changed() => {
            cancellation.cancel();
            if waiting.await.is_err() {
                tracing::warn!("daemon MCP connection cleanup failed");
            }
        }
    }
}

async fn proxy_io<R, W>(input: R, output: W, stream: UnixStream) -> io::Result<()>
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
{
    let (mut socket_read, mut socket_write) = stream.into_split();
    let upstream = async move {
        let mut input = input;
        tokio::io::copy(&mut input, &mut socket_write).await?;
        socket_write.shutdown().await
    };
    let downstream = async move {
        let mut output = output;
        let mut buffer = [0_u8; 16 * 1024];
        loop {
            let read = socket_read.read(&mut buffer).await?;
            if read == 0 {
                break;
            }
            let chunk = buffer.get(..read).ok_or_else(|| {
                io::Error::new(io::ErrorKind::InvalidData, "socket read exceeded relay buffer")
            })?;
            output.write_all(chunk).await?;
            output.flush().await?;
        }
        Ok(())
    };
    tokio::pin!(upstream);
    tokio::pin!(downstream);

    tokio::select! {
        result = &mut upstream => {
            result?;
            downstream.await
        }
        result = &mut downstream => result,
    }
}

fn resolve_with_env<F>(mut env: F, temp_dir: PathBuf, effective_uid: u32) -> DaemonPaths
where
    F: FnMut(&str) -> Option<OsString>,
{
    if let Some(path) = nonempty_env_path(env("TELEGRAM_MCP_DAEMON_DIR")) {
        return DaemonPaths::new(path);
    }
    if let Some(path) = nonempty_env_path(env("XDG_RUNTIME_DIR")) {
        return DaemonPaths::new(path.join(APP_DIR_NAME));
    }
    if let Some(path) = nonempty_env_path(env("XDG_STATE_HOME")) {
        return DaemonPaths::new(path.join(APP_DIR_NAME).join("daemon"));
    }
    if let Some(path) = nonempty_env_path(env("HOME")) {
        return DaemonPaths::new(
            path.join(".local").join("state").join(APP_DIR_NAME).join("daemon"),
        );
    }
    DaemonPaths::new(temp_dir.join(format!("{APP_DIR_NAME}-{effective_uid}")))
}

fn nonempty_env_path(value: Option<OsString>) -> Option<PathBuf> {
    value.filter(|value| !value.is_empty()).map(PathBuf::from)
}

fn ensure_private_directory(path: &Path) -> io::Result<()> {
    fs::create_dir_all(path)?;
    let metadata = fs::symlink_metadata(path)?;
    if !metadata.file_type().is_dir() || metadata.file_type().is_symlink() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "daemon base path is not a directory",
        ));
    }
    if metadata.uid() != effective_user_id() {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "daemon base directory is not owned by the current user",
        ));
    }
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
    Ok(())
}

fn open_private_lock_file(path: &Path) -> io::Result<File> {
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o600)
        .open(path)?;
    enforce_private_regular_file(&file)?;
    Ok(file)
}

fn open_private_append_file(path: &Path) -> io::Result<File> {
    let file = OpenOptions::new().append(true).create(true).mode(0o600).open(path)?;
    enforce_private_regular_file(&file)?;
    Ok(file)
}

fn enforce_private_regular_file(file: &File) -> io::Result<()> {
    let metadata = file.metadata()?;
    if !metadata.file_type().is_file() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "daemon path is not a regular file",
        ));
    }
    if metadata.uid() != effective_user_id() {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "daemon file is not owned by the current user",
        ));
    }
    file.set_permissions(fs::Permissions::from_mode(0o600))?;
    Ok(())
}

fn write_current_pid(file: &mut File) -> io::Result<()> {
    file.set_len(0)?;
    file.seek(SeekFrom::Start(0))?;
    writeln!(file, "{}", std::process::id())?;
    file.flush()
}

fn validate_private_socket(paths: &DaemonPaths) -> io::Result<()> {
    let directory = fs::symlink_metadata(&paths.base_dir)?;
    if !directory.file_type().is_dir()
        || directory.file_type().is_symlink()
        || directory.uid() != effective_user_id()
        || directory.mode() & 0o777 != 0o700
    {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "daemon base directory is not private",
        ));
    }

    let socket = fs::symlink_metadata(&paths.socket_path)?;
    if !socket.file_type().is_socket()
        || socket.uid() != effective_user_id()
        || socket.mode() & 0o777 != 0o600
    {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "daemon socket is not private",
        ));
    }
    Ok(())
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct RuntimeIdentity {
    pid: u32,
    config_sha256: String,
    file_identity: FileIdentity,
}

fn read_runtime_identity(paths: &DaemonPaths) -> io::Result<RuntimeIdentity> {
    let (contents, file_identity) = read_private_regular_file(&paths.identity_path)?;
    let mut lines = contents.lines();
    if lines.next() != Some(IDENTITY_FORMAT) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "daemon identity format is unsupported",
        ));
    }
    let pid = lines
        .next()
        .and_then(|line| line.strip_prefix("pid="))
        .ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidData, "daemon identity PID is missing")
        })?
        .parse::<u32>()
        .map_err(|_| {
            io::Error::new(io::ErrorKind::InvalidData, "daemon identity PID is invalid")
        })?;
    let config_sha256 = lines
        .next()
        .and_then(|line| line.strip_prefix("config_sha256="))
        .ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidData, "daemon configuration digest is missing")
        })?
        .to_string();
    DaemonConfigIdentity::from_sha256(config_sha256.clone())?;
    if lines.next().is_some() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "daemon identity contains unexpected fields",
        ));
    }
    Ok(RuntimeIdentity { pid, config_sha256, file_identity })
}

fn validate_runtime_identity(
    paths: &DaemonPaths,
    actual: &RuntimeIdentity,
    expected: &DaemonConfigIdentity,
) -> io::Result<()> {
    let (lock_contents, _) = read_private_regular_file(&paths.lock_path)?;
    let lock_pid = lock_contents.trim().parse::<u32>().map_err(|_| {
        io::Error::new(io::ErrorKind::InvalidData, "daemon singleton lock has no valid PID")
    })?;
    if actual.pid != lock_pid {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "daemon identity PID does not match its singleton lock",
        ));
    }
    if actual.config_sha256 != expected.sha256() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!(
                "running daemon configuration differs from this proxy (expected {}, found {}); stop it or use a separate TELEGRAM_MCP_DAEMON_DIR",
                expected.sha256(),
                actual.config_sha256
            ),
        ));
    }
    Ok(())
}

fn read_private_regular_file(path: &Path) -> io::Result<(String, FileIdentity)> {
    let before = fs::symlink_metadata(path)?;
    if !before.file_type().is_file()
        || before.file_type().is_symlink()
        || before.uid() != effective_user_id()
        || before.mode() & 0o777 != 0o600
    {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "daemon metadata file is not an owner-only regular file",
        ));
    }
    let expected = FileIdentity::from_metadata(&before);
    let mut file = File::open(path)?;
    let opened = file.metadata()?;
    if !opened.file_type().is_file()
        || opened.uid() != effective_user_id()
        || opened.mode() & 0o777 != 0o600
        || FileIdentity::from_metadata(&opened) != expected
    {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "daemon metadata file changed while opening",
        ));
    }
    let mut contents = String::new();
    file.read_to_string(&mut contents)?;
    Ok((contents, expected))
}

fn is_socket_startup_error(error: &io::Error) -> bool {
    matches!(
        error.kind(),
        io::ErrorKind::NotFound
            | io::ErrorKind::ConnectionRefused
            | io::ErrorKind::ConnectionReset
            | io::ErrorKind::AddrNotAvailable
            | io::ErrorKind::WouldBlock
    )
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct FileIdentity {
    device: u64,
    inode: u64,
}

impl FileIdentity {
    fn from_metadata(metadata: &fs::Metadata) -> Self {
        Self { device: metadata.dev(), inode: metadata.ino() }
    }
}

fn remove_socket_if_same(path: &Path, expected: FileIdentity) {
    let Ok(metadata) = fs::symlink_metadata(path) else {
        return;
    };
    if metadata.file_type().is_socket()
        && FileIdentity::from_metadata(&metadata) == expected
        && metadata.uid() == effective_user_id()
    {
        let _ = fs::remove_file(path);
    }
}

fn remove_regular_file_if_same(path: &Path, expected: FileIdentity) {
    let Ok(metadata) = fs::symlink_metadata(path) else {
        return;
    };
    if metadata.file_type().is_file()
        && !metadata.file_type().is_symlink()
        && FileIdentity::from_metadata(&metadata) == expected
        && metadata.uid() == effective_user_id()
    {
        let _ = fs::remove_file(path);
    }
}

fn effective_user_id() -> u32 {
    // SAFETY: geteuid has no arguments, does not dereference memory, and is
    // available on the Unix targets supported by this module.
    unsafe { geteuid() }
}

