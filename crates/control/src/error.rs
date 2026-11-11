//! Control protocol error type with a JSON-RPC 2.0 code mapping.

/// Failures raised while validating or serving a control request.
#[derive(Debug, thiserror::Error)]
pub enum ControlError {
    /// The request was not valid JSON-RPC 2.0.
    #[error("protocol error: {0}")]
    Protocol(String),
    /// The client token was missing or did not match.
    #[error("authentication failed")]
    Unauthorized,
    /// The method name is not part of the protocol.
    #[error("method not found: {0}")]
    MethodNotFound(String),
    /// The `params` object did not decode into the method's arguments.
    #[error("invalid params: {0}")]
    InvalidParams(String),
    /// A path argument failed validation.
    #[error("invalid path: {0}")]
    InvalidPath(String),
    /// The method is known but not supported by this handler.
    #[error("not supported: {0}")]
    Unsupported(String),
    /// Socket or file failure.
    #[error(transparent)]
    Io(#[from] std::io::Error),
    /// JSON syntax failure.
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    /// Project model validation failure.
    #[error(transparent)]
    Model(#[from] animathio_core::Error),
    /// Project format failure.
    #[error(transparent)]
    Format(#[from] animathio_format::Error),
    /// v1 project import failure.
    #[error(transparent)]
    Import(#[from] animathio_import_v1::ImportError),
    /// Rendering failure.
    #[error(transparent)]
    Render(#[from] animathio_render::Error),
}

impl ControlError {
    /// JSON-RPC 2.0 error code for this failure.
    #[must_use]
    pub fn code(&self) -> i64 {
        match self {
            Self::Protocol(_) => -32_600,
            Self::Unauthorized => -32_001,
            Self::MethodNotFound(_) => -32_601,
            Self::InvalidParams(_) | Self::InvalidPath(_) | Self::Json(_) => -32_602,
            Self::Unsupported(_) => -32_002,
            Self::Io(_)
            | Self::Model(_)
            | Self::Format(_)
            | Self::Import(_)
            | Self::Render(_) => -32_000,
        }
    }
}

impl From<ControlError> for String {
    fn from(value: ControlError) -> Self {
        value.to_string()
    }
}
