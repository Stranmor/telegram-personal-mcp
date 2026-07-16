use std::path::PathBuf;

#[derive(Clone)]
pub struct Config {
    pub session_path: Option<PathBuf>,
    pub session_cache_dir: Option<PathBuf>,
    pub proxy: Option<String>,
    pub api_id: i32,
    pub api_hash: String,
}

impl Config {
    pub fn session_cache_dir(&self) -> PathBuf {
        self.session_cache_dir.clone().unwrap_or_else(|| {
            if let Some(path) = std::env::var_os("XDG_DATA_HOME") {
                return PathBuf::from(path).join("telegram-personal-mcp/sessions");
            }
            if let Some(home) = std::env::var_os("HOME") {
                return PathBuf::from(home).join(".local/share/telegram-personal-mcp/sessions");
            }
            std::env::temp_dir().join("telegram-personal-mcp/sessions")
        })
    }

    pub fn session_candidates(&self) -> std::io::Result<Vec<PathBuf>> {
        if let Some(path) = &self.session_path {
            return Ok(if path.is_file() {
                vec![std::fs::canonicalize(path)?]
            } else {
                Vec::new()
            });
        }

        let cache_dir = self.session_cache_dir();
        if !cache_dir.is_dir() {
            return Ok(Vec::new());
        }
        let mut candidates = Vec::new();
        for entry in std::fs::read_dir(cache_dir)? {
            let path = entry?.path();
            if path.is_file()
                && path.extension().and_then(|value| value.to_str()) == Some("session")
            {
                candidates.push(std::fs::canonicalize(path)?);
            }
        }
        candidates.sort();
        candidates.dedup();
        Ok(candidates)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_candidates_are_sorted_files_only() {
        let temp = tempfile::tempdir().expect("tempdir");
        std::fs::write(temp.path().join("b.session"), b"session").expect("write b");
        std::fs::write(temp.path().join("a.session"), b"session").expect("write a");
        std::fs::create_dir(temp.path().join("directory.session")).expect("create directory");
        std::fs::write(temp.path().join("ignored.txt"), b"not a session").expect("write txt");

        let config = Config {
            session_path: None,
            session_cache_dir: Some(temp.path().to_path_buf()),
            proxy: None,
            api_id: 1,
            api_hash: "test".to_string(),
        };
        let canonical_temp = std::fs::canonicalize(temp.path()).expect("canonical tempdir");

        assert_eq!(
            config.session_candidates().expect("candidates"),
            vec![canonical_temp.join("a.session"), canonical_temp.join("b.session")]
        );
    }
}
