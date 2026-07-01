use crate::error::{Result, WebDavError};

#[derive(Debug, Clone)]
pub struct WebDavPath {
    pub root: String,
}

impl WebDavPath {
    pub fn new(root: impl Into<String>) -> Self {
        let root = root.into().trim_matches('/').to_string();
        Self { root }
    }

    pub fn sanitize_subpath(raw: &str) -> Result<String> {
        let trimmed = raw.trim();

        if trimmed.is_empty() {
            return Ok(String::new());
        }

        if trimmed.starts_with('/') {
            return Err(WebDavError::PathTraversal {
                path: raw.to_string(),
                reason: "absolute path not allowed".into(),
            });
        }

        let mut normalized = String::with_capacity(trimmed.len());

        for segment in trimmed.split('/') {
            if segment == ".." {
                return Err(WebDavError::PathTraversal {
                    path: raw.to_string(),
                    reason: "contains '..' segment".into(),
                });
            }

            if segment == "." {
                continue;
            }

            if segment.is_empty() {
                continue;
            }

            if !normalized.is_empty() {
                normalized.push('/');
            }
            normalized.push_str(segment);
        }

        Ok(normalized)
    }

    pub fn room_dir(&self, room_id: &str) -> String {
        format!("/{}/{}/", self.root, room_id)
    }

    pub fn bot_snapshot_path(
        &self,
        snapshot_prefix: &str,
        bot_id: &str,
        room_key: &str,
    ) -> String {
        let prefix = snapshot_prefix.trim_matches('/');
        let bot = bot_id.trim_matches('/');
        let room = room_key.trim_matches('/');
        format!("/{}/{}/{}/{}/snapshot.json", self.root, prefix, bot, room)
    }

    pub fn memory_dir(&self, room_id: &str) -> String {
        format!("/{}/{}/memory/", self.root, room_id)
    }

    pub fn image_dir(&self, room_id: &str) -> String {
        format!("/{}/{}/images/", self.root, room_id)
    }

    pub fn workspace_dir(&self, room_id: &str) -> String {
        format!("/{}/{}/workspace/", self.root, room_id)
    }

    pub fn image_path(&self, room_id: &str, name: &str) -> Result<String> {
        let cleaned = Self::sanitize_subpath(name)?;
        if cleaned.is_empty() {
            return Err(WebDavError::PathTraversal {
                path: name.to_string(),
                reason: "image name must not be empty after sanitization".into(),
            });
        }
        Ok(format!(
            "/{}/{}/images/{}",
            self.root,
            room_id,
            cleaned
        ))
    }

    pub fn config_backup_path(&self, filename: &str) -> String {
        format!(
            "/{}/config/{}",
            self.root,
            filename.trim_start_matches('/')
        )
    }

    pub fn room_path(&self, room_id: &str, file_path: &str) -> Result<String> {
        let cleaned = Self::sanitize_subpath(file_path)?;
        if cleaned.is_empty() {
            Ok(format!("/{}/{}/", self.root, room_id))
        } else {
            Ok(format!("/{}/{}/{}", self.root, room_id, cleaned))
        }
    }

    pub fn parent_path(path: &str) -> String {
        let trimmed = path.trim_end_matches('/');
        match trimmed.rfind('/') {
            Some(pos) if pos > 0 => trimmed[..pos].to_string(),
            _ => "/".to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_room_dir() {
        let p = WebDavPath::new("rockbot");
        assert_eq!(p.room_dir("general"), "/rockbot/general/");
    }

    #[test]
    fn test_bot_snapshot_path() {
        let p = WebDavPath::new("");
        assert_eq!(
            p.bot_snapshot_path(".snapshots", "threefalcon", "d-DTI"),
            "//.snapshots/threefalcon/d-DTI/snapshot.json"
        );
    }

    #[test]
    fn test_bot_snapshot_path_with_root() {
        let p = WebDavPath::new("CLAW");
        assert_eq!(
            p.bot_snapshot_path(".snapshots", "oneshark", "d-DTI"),
            "/CLAW/.snapshots/oneshark/d-DTI/snapshot.json"
        );
    }

    #[test]
    fn test_bot_snapshot_path_trims_slashes() {
        let p = WebDavPath::new("CLAW");
        assert_eq!(
            p.bot_snapshot_path("/.snapshots/", "/threefalcon/", "/d-DTI/"),
            "/CLAW/.snapshots/threefalcon/d-DTI/snapshot.json"
        );
    }

    #[test]
    fn test_root_trim_slashes() {
        let p = WebDavPath::new("/rockbot/");
        assert_eq!(p.root, "rockbot");
    }

    #[test]
    fn test_memory_dir() {
        let p = WebDavPath::new("rockbot");
        assert_eq!(p.memory_dir("dm-alice"), "/rockbot/dm-alice/memory/");
    }

    #[test]
    fn test_image_path() {
        let p = WebDavPath::new("rockbot");
        assert_eq!(
            p.image_path("general", "photo.png").unwrap(),
            "/rockbot/general/images/photo.png"
        );
        assert_eq!(
            p.image_path("general", "subdir/photo.png").unwrap(),
            "/rockbot/general/images/subdir/photo.png"
        );
        // Absolute paths are rejected
        assert!(p.image_path("general", "/etc/passwd.png").is_err());
    }

    #[test]
    fn test_image_dir() {
        let p = WebDavPath::new("rockbot");
        assert_eq!(p.image_dir("general"), "/rockbot/general/images/");
    }

    #[test]
    fn test_workspace_dir() {
        let p = WebDavPath::new("rockbot");
        assert_eq!(p.workspace_dir("general"), "/rockbot/general/workspace/");
    }

    #[test]
    fn test_config_backup_path() {
        let p = WebDavPath::new("rockbot");
        assert_eq!(
            p.config_backup_path("config_backup.toml"),
            "/rockbot/config/config_backup.toml"
        );
    }

    #[test]
    fn test_room_path() {
        let p = WebDavPath::new("rockbot");
        assert_eq!(
            p.room_path("general", "notes.txt").unwrap(),
            "/rockbot/general/notes.txt"
        );
        assert_eq!(
            p.room_path("general", "sub/notes.txt").unwrap(),
            "/rockbot/general/sub/notes.txt"
        );
        assert_eq!(
            p.room_path("general", "").unwrap(),
            "/rockbot/general/"
        );
        // Absolute paths are rejected
        assert!(p.room_path("general", "/secrets.toml").is_err());
    }

    #[test]
    fn test_parent_path() {
        assert_eq!(WebDavPath::parent_path("/a/b/c"), "/a/b");
        assert_eq!(WebDavPath::parent_path("/a/b/c/"), "/a/b");
        assert_eq!(WebDavPath::parent_path("/a/b"), "/a");
        assert_eq!(WebDavPath::parent_path("/a"), "/");
        assert_eq!(WebDavPath::parent_path("/"), "/");
    }

    #[test]
    fn test_room_path_empty_root() {
        let p = WebDavPath::new("");
        assert_eq!(
            p.room_path("general", "notes.txt").unwrap(),
            "//general/notes.txt"
        );
        assert_eq!(p.room_dir("general"), "//general/");
    }

    #[test]
    fn test_sanitize_rejects_dot_dot() {
        assert!(WebDavPath::sanitize_subpath("../secrets.toml").is_err());
        assert!(WebDavPath::sanitize_subpath("foo/../../bar").is_err());
        assert!(WebDavPath::sanitize_subpath("..").is_err());
        assert!(WebDavPath::sanitize_subpath("/etc/../passwd").is_err());
    }

    #[test]
    fn test_sanitize_rejects_absolute_paths() {
        assert!(WebDavPath::sanitize_subpath("/etc/passwd").is_err());
    }

    #[test]
    fn test_sanitize_normalizes_dots_and_slashes() {
        assert_eq!(
            WebDavPath::sanitize_subpath("./foo").unwrap(),
            "foo"
        );
        assert_eq!(
            WebDavPath::sanitize_subpath("foo/./bar").unwrap(),
            "foo/bar"
        );
        assert_eq!(
            WebDavPath::sanitize_subpath("foo//bar").unwrap(),
            "foo/bar"
        );
        assert_eq!(
            WebDavPath::sanitize_subpath("  foo/bar  ").unwrap(),
            "foo/bar"
        );
        assert_eq!(
            WebDavPath::sanitize_subpath("foo/bar/").unwrap(),
            "foo/bar"
        );
    }

    #[test]
    fn test_sanitize_empty_path() {
        assert_eq!(WebDavPath::sanitize_subpath("").unwrap(), "");
        assert_eq!(WebDavPath::sanitize_subpath("   ").unwrap(), "");
        assert_eq!(WebDavPath::sanitize_subpath(".").unwrap(), "");
        assert_eq!(WebDavPath::sanitize_subpath("././.").unwrap(), "");
    }

    #[test]
    fn test_room_path_rejects_traversal() {
        let p = WebDavPath::new("rockbot");
        assert!(p.room_path("general", "../other-room/secrets.toml").is_err());
        assert!(p.room_path("general", "../../secrets.toml").is_err());
    }

    #[test]
    fn test_image_path_rejects_traversal() {
        let p = WebDavPath::new("rockbot");
        assert!(p.image_path("general", "../other-room/evil.png").is_err());
        assert!(p.image_path("general", "..").is_err());
    }

    #[test]
    fn test_sanitize_allows_hidden_files() {
        assert_eq!(
            WebDavPath::sanitize_subpath(".hidden").unwrap(),
            ".hidden"
        );
        assert_eq!(
            WebDavPath::sanitize_subpath("dir/.gitignore").unwrap(),
            "dir/.gitignore"
        );
    }
}
