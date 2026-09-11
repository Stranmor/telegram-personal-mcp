//! Convert one Telegram Desktop `tdata` account directly to this MCP's
//! Ferogram session format. The Grammers session string is deliberately not
//! used as an intermediate: it is a different wire/storage format.

use anyhow::{Context, Result, bail};
use clap::Parser;
use ferogram::session_backend::PersistedSession;
use ferogram::{DcEntry, DcFlags};
use hermes_tdata::TDesktop;
use sha2::{Digest, Sha256};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

#[derive(Debug, Parser)]
#[command(author, version, about)]
struct Args {
    /// Telegram Desktop tdata directory.
    tdata_path: PathBuf,
    /// Exact output path for a Ferogram `.session` file.
    output: PathBuf,
    /// User ID to select when tdata contains multiple accounts.
    /// Omitted = auto-select when tdata holds exactly one account.
    #[arg(long)]
    user_id: Option<i64>,
}

fn dc_address(dc_id: i32) -> Result<String> {
    match dc_id {
        1 => Ok("149.154.175.53:443".to_owned()),
        2 => Ok("149.154.167.51:443".to_owned()),
        3 => Ok("149.154.175.100:443".to_owned()),
        4 => Ok("149.154.167.91:443".to_owned()),
        5 => Ok("91.108.56.130:443".to_owned()),
        other => bail!("unsupported Telegram DC id {other}"),
    }
}

fn ensure_private_parent(path: &Path) -> Result<&Path> {
    let parent = path.parent().context("session output has no parent")?;
    if let Ok(metadata) = fs::symlink_metadata(parent) {
        if metadata.file_type().is_symlink() {
            bail!("session output parent must not be a symlink: {}", parent.display());
        }
    } else {
        fs::create_dir_all(parent)?;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(parent, fs::Permissions::from_mode(0o700))?;
    }
    Ok(parent)
}

fn write_session(path: &Path, session: &PersistedSession) -> Result<(u64, String)> {
    let parent = ensure_private_parent(path)?;
    if let Ok(metadata) = fs::symlink_metadata(path) {
        if metadata.file_type().is_symlink() || !metadata.file_type().is_file() {
            bail!("session output must be a regular file: {}", path.display());
        }
        bail!("refusing to overwrite existing session: {}", path.display());
    }

    let temporary = parent.join(format!(
        ".{}.{}.tmp",
        path.file_name().unwrap().to_string_lossy(),
        std::process::id()
    ));
    session.save(&temporary)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&temporary, fs::Permissions::from_mode(0o600))?;
    }
    let bytes = fs::read(&temporary)?;
    let digest = Sha256::digest(&bytes);
    fs::rename(&temporary, path)?;
    let mut file = OpenOptions::new().read(true).open(path)?;
    file.flush()?;
    Ok((bytes.len() as u64, format!("{digest:x}")))
}

fn main() -> Result<()> {
    let args = Args::parse();
    let tdesktop = TDesktop::from_path(&args.tdata_path)
        .with_context(|| format!("read Telegram Desktop tdata: {}", args.tdata_path.display()))?;
    let accounts = tdesktop.accounts();
    let account = match args.user_id {
        Some(wanted) => accounts
            .iter()
            .find(|account| account.user_id() == wanted)
            .with_context(|| format!("user id {wanted} was not found in tdata"))?,
        None if accounts.len() == 1 => &accounts[0],
        None => {
            let ids: Vec<i64> = accounts.iter().map(|account| account.user_id()).collect();
            bail!(
                "tdata holds {} accounts; pass --user-id explicitly, one of: {ids:?}",
                accounts.len()
            )
        },
    };
    let user_id = account.user_id();
    let dc_id = account.dc_id();
    let session = PersistedSession {
        home_dc_id: dc_id,
        dcs: vec![DcEntry {
            dc_id,
            addr: dc_address(dc_id)?,
            auth_key: Some(*account.auth_key_bytes()),
            first_salt: 0,
            time_offset: 0,
            flags: DcFlags::NONE,
        }],
        updates_state: Default::default(),
        peers: Vec::new(),
        min_peers: Vec::new(),
        future_auth_token: None,
    };
    let (bytes, sha256) = write_session(&args.output, &session)?;
    println!(
        "{{\"status\":\"converted\",\"user_id\":{},\"dc_id\":{},\"bytes\":{},\"sha256\":\"{}\",\"output\":\"{}\"}}",
        user_id,
        dc_id,
        bytes,
        sha256,
        args.output.display()
    );
    Ok(())
}
