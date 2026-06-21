use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error("config error: {0}")]
    Config(String),

    #[error("git error: {0}")]
    Git(String),

    #[error("provider error: {0}")]
    Provider(String),

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("user aborted")]
    UserAbort,
}

pub type Result<T> = std::result::Result<T, Error>;
