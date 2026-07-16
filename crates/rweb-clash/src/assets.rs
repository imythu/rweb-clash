#[derive(Debug)]
pub struct EmbeddedFile {
    pub path: &'static str,
    pub bytes: &'static [u8],
}

#[derive(Debug)]
pub struct EmbeddedAssets {
    pub files: &'static [EmbeddedFile],
}

impl EmbeddedAssets {
    pub fn get(&self, path: &str) -> Option<&'static [u8]> {
        let normalized = path.trim_start_matches('/').replace('\\', "/");
        self.files
            .iter()
            .find(|file| file.path == normalized)
            .map(|file| file.bytes)
    }

    pub fn has_prefix(&self, prefix: &str) -> bool {
        let prefix = prefix.trim_start_matches('/').replace('\\', "/");
        self.files.iter().any(|file| file.path.starts_with(&prefix))
    }
}
