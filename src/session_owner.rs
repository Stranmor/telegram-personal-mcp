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

