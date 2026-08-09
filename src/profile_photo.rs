use sha2::{Digest, Sha256};

pub(crate) fn opaque_profile_photo_handle(photo_id: i64) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"telegram-personal-mcp:profile-photo:v1\0");
    hasher.update(photo_id.to_le_bytes());
    format!("photo_{}", format!("{:x}", hasher.finalize()).chars().take(32).collect::<String>())
}

pub(crate) fn opaque_chat_photo_handle(chat_id: &str, photo_id: i64) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"telegram-personal-mcp:chat-photo:v1\0");
    hasher.update(chat_id.as_bytes());
    hasher.update([0]);
    hasher.update(photo_id.to_le_bytes());
    format!("chatphoto_{}", format!("{:x}", hasher.finalize()).chars().take(32).collect::<String>())
}

#[cfg(test)]
mod tests {
    use super::{opaque_chat_photo_handle, opaque_profile_photo_handle};

    #[test]
    fn profile_photo_handles_are_stable_and_opaque() {
        let first = opaque_profile_photo_handle(42);
        assert_eq!(first, opaque_profile_photo_handle(42));
        assert!(first.starts_with("photo_"));
        assert!(!first.contains("42"));
        assert_ne!(first, opaque_profile_photo_handle(43));
    }

    #[test]
    fn chat_photo_handles_bind_chat_and_photo_without_exposing_ids() {
        let handle = opaque_chat_photo_handle("-10042", 99);
        assert_eq!(handle, opaque_chat_photo_handle("-10042", 99));
        let suffix = handle.strip_prefix("chatphoto_").expect("opaque chat-photo prefix");
        assert_eq!(suffix.len(), 32);
        assert!(
            suffix
                .as_bytes()
                .iter()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(byte))
        );
        assert!(!handle.contains("-10042"));
        assert!(!handle.contains(':'));
        assert_ne!(handle, opaque_chat_photo_handle("-10043", 99));
        assert_ne!(handle, opaque_chat_photo_handle("-10042", 100));
    }
}
