use sha2::{Digest, Sha256};

pub(crate) fn opaque_invite_handle(raw_link: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"telegram-personal-mcp:invite-link:v1\0");
    hasher.update(raw_link.as_bytes());
    format!("invite_{}", format!("{:x}", hasher.finalize()).chars().take(32).collect::<String>())
}

