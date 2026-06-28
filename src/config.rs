use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct Config {
    pub session_path: Option<PathBuf>,
    pub session_cache_dir: Option<PathBuf>,
    pub proxy: Option<String>,
    pub api_id: i32,
    pub api_hash: String,
}

impl Config {
    pub fn session_cache_dir(&self) -> PathBuf {
        self.session_cache_dir.clone().unwrap_or_else(|| PathBuf::from("sessions"))
    }
}
