#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error("the release's manifest is unreadable: {0}")]
    Manifest(#[from] serde_json::Error),
    #[error("the package is not the one the release signed")]
    Signature,
    #[error("the package was signed for {signed}, not for {announced}")]
    SignedVersion { signed: String, announced: String },
    #[error("the package holds no {0}")]
    Missing(String),
}
