use thiserror::Error;

pub type PandaResult<T> = Result<T, PandaError>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorCode {
    InvalidArgument,
    Unauthenticated,
    PermissionDenied,
    NotFound,
    Conflict,
    RevisionConflict,
    ContentConflict,
    LeaseConflict,
    PreconditionFailed,
    ResourceExhausted,
    FailedPrecondition,
    Internal,
    Unavailable,
}

impl ErrorCode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::InvalidArgument => "invalid_argument",
            Self::Unauthenticated => "unauthenticated",
            Self::PermissionDenied => "permission_denied",
            Self::NotFound => "not_found",
            Self::Conflict => "conflict",
            Self::RevisionConflict => "revision_conflict",
            Self::ContentConflict => "content_conflict",
            Self::LeaseConflict => "lease_conflict",
            Self::PreconditionFailed => "precondition_failed",
            Self::ResourceExhausted => "resource_exhausted",
            Self::FailedPrecondition => "failed_precondition",
            Self::Internal => "internal",
            Self::Unavailable => "unavailable",
        }
    }

    pub fn http_status(self) -> u16 {
        match self {
            Self::InvalidArgument => 400,
            Self::Unauthenticated => 401,
            Self::PermissionDenied => 403,
            Self::NotFound => 404,
            Self::Conflict
            | Self::RevisionConflict
            | Self::ContentConflict
            | Self::LeaseConflict => 409,
            Self::PreconditionFailed | Self::FailedPrecondition => 428,
            Self::ResourceExhausted => 429,
            Self::Unavailable => 503,
            Self::Internal => 500,
        }
    }

    pub fn retryable(self) -> bool {
        matches!(
            self,
            Self::Unavailable | Self::ResourceExhausted | Self::Internal
        )
    }
}

#[derive(Debug, Error)]
#[error("{}: {message}", code.as_str())]
pub struct PandaError {
    pub code: ErrorCode,
    pub message: String,
    pub current_etag: Option<String>,
    pub details_json: Option<String>,
}

impl PandaError {
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            current_etag: None,
            details_json: None,
        }
    }

    pub fn with_etag(mut self, etag: impl Into<String>) -> Self {
        self.current_etag = Some(etag.into());
        self
    }

    pub fn invalid(msg: impl Into<String>) -> Self {
        Self::new(ErrorCode::InvalidArgument, msg)
    }

    pub fn not_found(msg: impl Into<String>) -> Self {
        Self::new(ErrorCode::NotFound, msg)
    }

    pub fn unauthenticated(msg: impl Into<String>) -> Self {
        Self::new(ErrorCode::Unauthenticated, msg)
    }

    pub fn internal(msg: impl Into<String>) -> Self {
        Self::new(ErrorCode::Internal, msg)
    }
}
