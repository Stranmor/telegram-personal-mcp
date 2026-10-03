use sha2::{Digest, Sha256};

pub(crate) fn opaque_invite_handle(raw_link: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"telegram-personal-mcp:invite-link:v1\0");
    hasher.update(raw_link.as_bytes());
    format!("invite_{}", format!("{:x}", hasher.finalize()).chars().take(32).collect::<String>())
}

#[cfg(test)]
mod tests {
    use super::opaque_invite_handle;

    #[test]
    fn invite_handles_are_stable_and_do_not_contain_the_link() {
        let link = "fixture-secret-token";
        let first = opaque_invite_handle(link);
        assert_eq!(first, opaque_invite_handle(link));
        assert!(first.starts_with("invite_"));
        assert!(!first.contains(link));
        assert_ne!(first, opaque_invite_handle("another-token"));
    }
}
