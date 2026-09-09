use ferogram::DcEntry;
use ferogram::session_backend::{CachedPeer, PersistedSession, SessionBackend, UpdateStateChange};
use std::fs::File;
use std::io;
use std::path::PathBuf;

/// The storage lifetime owns the lease, including detached blocking writes.
/// Every Ferogram task retains this backend through its ClientInner or a
/// direct Arc clone, so signalling shutdown cannot release ownership early.
pub(crate) struct LockedSessionBackend {
    path: PathBuf,
    access: std::sync::Mutex<()>,
    _lock: File,
}

impl LockedSessionBackend {
    pub(crate) fn new(path: PathBuf, lock: File) -> Self {
        Self { path, access: std::sync::Mutex::new(()), _lock: lock }
    }

    fn with_inner<T>(&self, operation: impl FnOnce() -> io::Result<T>) -> io::Result<T> {
        let _guard =
            self.access.lock().map_err(|_| io::Error::other("session storage lock poisoned"))?;
        operation()
    }

    fn load_unlocked(&self) -> io::Result<Option<PersistedSession>> {
        match std::fs::read(&self.path) {
            Ok(bytes) => PersistedSession::from_bytes(&bytes).map(Some),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(error),
        }
    }

    fn save_unlocked(&self, session: &PersistedSession) -> io::Result<()> {
        session.save(&self.path)?;
        crate::fs_security::restrict_private_file(&self.path)
    }

    fn update(&self, operation: impl FnOnce(&mut PersistedSession)) -> io::Result<()> {
        self.with_inner(|| {
            let mut session = self.load_unlocked()?.unwrap_or_default();
            operation(&mut session);
            self.save_unlocked(&session)
        })
    }
}

impl SessionBackend for LockedSessionBackend {
    fn save(&self, session: &PersistedSession) -> io::Result<()> {
        self.with_inner(|| self.save_unlocked(session))
    }

    fn load(&self) -> io::Result<Option<PersistedSession>> {
        self.with_inner(|| self.load_unlocked())
    }

    fn delete(&self) -> io::Result<()> {
        self.with_inner(|| match std::fs::remove_file(&self.path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error),
        })
    }

    fn name(&self) -> &str {
        "locked-binary-file"
    }

    fn update_dc(&self, entry: &DcEntry) -> io::Result<()> {
        self.update(|session| {
            if let Some(existing) = session.dcs.iter_mut().find(|current| {
                current.dc_id == entry.dc_id && current.is_ipv6() == entry.is_ipv6()
            }) {
                *existing = entry.clone();
            } else {
                session.dcs.push(entry.clone());
            }
        })
    }

    fn set_home_dc(&self, dc_id: i32) -> io::Result<()> {
        self.update(|session| session.home_dc_id = dc_id)
    }

    fn apply_update_state(&self, update: UpdateStateChange) -> io::Result<()> {
        self.update(|session| update.apply_to(&mut session.updates_state))
    }

    fn cache_peer(&self, peer: &CachedPeer) -> io::Result<()> {
        self.update(|session| {
            if let Some(existing) = session.peers.iter_mut().find(|current| current.id == peer.id) {
                *existing = peer.clone();
            } else {
                session.peers.push(peer.clone());
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fs2::FileExt;
    use std::sync::{Arc, mpsc};
    use std::time::Duration;

    #[test]
    fn pending_storage_work_retains_the_exclusive_session_lease() -> anyhow::Result<()> {
        let dir = tempfile::tempdir()?;
        let lock_path = dir.path().join("account.session.lock");
        let lock = File::create(&lock_path)?;
        lock.try_lock_exclusive()?;
        let owner = Arc::new(LockedSessionBackend::new(dir.path().join("account.session"), lock));
        let background = Arc::clone(&owner);
        let (started_tx, started_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let writer = std::thread::spawn(move || -> io::Result<()> {
            let _ = started_tx.send(());
            release_rx.recv_timeout(Duration::from_secs(3)).map_err(io::Error::other)?;
            background.save(&PersistedSession::default())
        });
        started_rx.recv_timeout(Duration::from_secs(3))?;
        drop(owner);
        let successor = File::open(&lock_path)?;
        assert!(successor.try_lock_exclusive().is_err(), "old storage work lost its lease");
        release_tx.send(())?;
        writer.join().map_err(|_| io::Error::other("writer panicked"))??;
        successor.try_lock_exclusive()?;
        assert!(dir.path().join("account.session").is_file());
        Ok(())
    }

    #[test]
    fn concurrent_partial_writes_preserve_both_peers_and_dc_entries() -> anyhow::Result<()> {
        let dir = tempfile::tempdir()?;
        let lock = File::create(dir.path().join("account.session.lock"))?;
        lock.try_lock_exclusive()?;
        let owner = Arc::new(LockedSessionBackend::new(dir.path().join("account.session"), lock));
        owner.save(&PersistedSession::default())?;
        let barrier = Arc::new(std::sync::Barrier::new(2));
        let peers = Arc::clone(&owner);
        let peer_barrier = Arc::clone(&barrier);
        let writer = std::thread::spawn(move || -> io::Result<()> {
            peer_barrier.wait();
            for id in 1..=32 {
                peers.cache_peer(&CachedPeer {
                    id,
                    access_hash: id,
                    is_channel: false,
                    is_chat: false,
                    channel_kind: None,
                })?;
            }
            Ok(())
        });
        barrier.wait();
        for dc_id in 1..=32 {
            owner.update_dc(&DcEntry {
                dc_id,
                addr: "127.0.0.1:443".to_string(),
                auth_key: None,
                first_salt: 0,
                time_offset: 0,
                flags: ferogram::DcFlags::NONE,
            })?;
        }
        writer.join().map_err(|_| io::Error::other("writer panicked"))??;
        let session = owner.load()?.ok_or_else(|| io::Error::other("session missing"))?;
        assert_eq!(session.peers.len(), 32);
        assert_eq!(session.dcs.len(), 32);
        Ok(())
    }

    #[test]
    fn corrupt_session_is_preserved_and_rejected() -> anyhow::Result<()> {
        let dir = tempfile::tempdir()?;
        let path = dir.path().join("account.session");
        std::fs::write(&path, b"invalid session")?;
        let lock = File::create(dir.path().join("account.session.lock"))?;
        lock.try_lock_exclusive()?;
        let owner = LockedSessionBackend::new(path.clone(), lock);
        assert!(owner.load().is_err());
        assert!(owner.set_home_dc(2).is_err());
        assert_eq!(std::fs::read(&path)?, b"invalid session");
        assert!(!path.with_extension("bak").exists());
        Ok(())
    }
}
