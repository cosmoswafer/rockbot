#[derive(Debug, Clone)]
pub struct WebDavPath {
    pub root: String,
}

impl WebDavPath {
    pub fn new(root: impl Into<String>) -> Self {
        let root = root.into().trim_matches('/').to_string();
        Self { root }
    }

    pub fn room_dir(&self, room_id: &str) -> String {
        format!("/{}/{}/", self.root, room_id)
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

    pub fn image_path(&self, room_id: &str, name: &str) -> String {
        format!(
            "/{}/{}/images/{}",
            self.root,
            room_id,
            name.trim_start_matches('/')
        )
    }

    pub fn config_backup_path(&self, filename: &str) -> String {
        format!(
            "/{}/config/{}",
            self.root,
            filename.trim_start_matches('/')
        )
    }

    pub fn room_path(&self, room_id: &str, file_path: &str) -> String {
        let file_path = file_path.trim_start_matches('/');
        format!("/{}/{}/{}", self.root, room_id, file_path)
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
            p.image_path("general", "photo.png"),
            "/rockbot/general/images/photo.png"
        );
        assert_eq!(
            p.image_path("general", "/subdir/photo.png"),
            "/rockbot/general/images/subdir/photo.png"
        );
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
            p.room_path("general", "notes.txt"),
            "/rockbot/general/notes.txt"
        );
        assert_eq!(
            p.room_path("general", "/notes.txt"),
            "/rockbot/general/notes.txt"
        );
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
        assert_eq!(p.room_path("general", "notes.txt"), "//general/notes.txt");
        assert_eq!(p.room_dir("general"), "//general/");
    }
}
