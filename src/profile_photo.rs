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

