use sha2::{Digest, Sha256};

pub(crate) fn opaque_sticker_handle(document_id: i64) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"telegram-personal-mcp:sticker:v1\0");
    hasher.update(document_id.to_le_bytes());
    format!("sticker_{}", format!("{:x}", hasher.finalize()).chars().take(32).collect::<String>())
}

