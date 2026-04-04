use thiserror::Error;

#[derive(Debug, Error)]
pub enum CoreError {
    #[error("not connected")]
    NotConnected,

    #[error("unsupported operation: {0}")]
    Unsupported(&'static str),
}
