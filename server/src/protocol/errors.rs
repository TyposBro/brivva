use thiserror::Error;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum ProtocolError {
    #[error("frame too short: need at least {expected} bytes, got {actual}")]
    FrameTooShort { expected: usize, actual: usize },
    #[error("unknown message type: 0x{0:02x}")]
    UnknownType(u8),
    #[error("payload size mismatch: declared {declared}, actual {actual}")]
    PayloadSizeMismatch { declared: usize, actual: usize },
    #[error("invalid utf-8 metadata")]
    InvalidUtf8,
    #[error("invalid json metadata")]
    InvalidJson,
    #[error("invalid chunk kind: {0}")]
    InvalidChunkKind(u8),
}
