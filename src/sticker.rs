use sha2::{Digest, Sha256};

pub(crate) fn opaque_sticker_handle(document_id: i64) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"telegram-personal-mcp:sticker:v1\0");
    hasher.update(document_id.to_le_bytes());
    format!("sticker_{}", format!("{:x}", hasher.finalize()).chars().take(32).collect::<String>())
}

#[cfg(test)]
mod tests {
    use super::opaque_sticker_handle;

    #[test]
    fn sticker_handles_are_stable_opaque_and_document_bound() {
        let first = opaque_sticker_handle(9_101);
        assert_eq!(first, opaque_sticker_handle(9_101));
        assert!(first.starts_with("sticker_"));
        assert!(!first.contains("9101"));
        assert_ne!(first, opaque_sticker_handle(9_102));
        let suffix = first.strip_prefix("sticker_").expect("opaque sticker prefix");
        assert_eq!(suffix.len(), 32);
        assert!(
            suffix
                .as_bytes()
                .iter()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(byte))
        );
    }
}
