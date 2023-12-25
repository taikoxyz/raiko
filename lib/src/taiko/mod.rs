use std::path::PathBuf;

pub mod block_builder;
#[cfg(not(target_os = "zkvm"))]
pub mod execute;
#[cfg(not(target_os = "zkvm"))]
pub mod host;
pub mod precheck;
pub mod prepare;
pub mod protocol_instance;
pub mod utils;

pub enum Layer {
    L1,
    L2,
}

#[derive(Debug)]
pub struct FileUrl(Option<PathBuf>, Option<String>);

impl From<(Option<PathBuf>, Option<String>)> for FileUrl {
    fn from(file_or_url: (Option<PathBuf>, Option<String>)) -> Self {
        FileUrl(file_or_url.0, file_or_url.1)
    }
}

impl From<String> for FileUrl {
    fn from(s: String) -> Self {
        FileUrl(None, Some(s))
    }
}

impl From<PathBuf> for FileUrl {
    fn from(p: PathBuf) -> Self {
        FileUrl(Some(p), None)
    }
}

impl FileUrl {
    pub fn into_file_url(self) -> (Option<PathBuf>, Option<String>) {
        (self.0, self.1)
    }
}
