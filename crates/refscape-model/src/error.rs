use std::{fmt, path::PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorKind {
    InvalidData,
    Io,
    Protocol,
    BackendUnavailable,
    Unsupported,
    Timeout,
    Cancelled,
    Stale,
    InternalInvariant,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RefscapeError {
    pub kind: ErrorKind,
    pub message: String,
    pub operation: Option<Box<str>>,
    pub path: Option<Box<std::path::Path>>,
    pub method: Option<Box<str>>,
    pub cause: Option<Box<str>>,
}

impl RefscapeError {
    pub fn new(kind: ErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
            operation: None,
            path: None,
            method: None,
            cause: None,
        }
    }
    pub fn with_operation(mut self, operation: impl ToString) -> Self {
        self.operation = Some(operation.to_string().into_boxed_str());
        self
    }
    pub fn with_path(mut self, path: impl Into<PathBuf>) -> Self {
        self.path = Some(path.into().into_boxed_path());
        self
    }
    pub fn with_method(mut self, method: impl Into<String>) -> Self {
        self.method = Some(method.into().into_boxed_str());
        self
    }
    pub fn with_cause(mut self, cause: impl ToString) -> Self {
        self.cause = Some(cause.to_string().into_boxed_str());
        self
    }
}
impl fmt::Display for RefscapeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.message.fmt(f)
    }
}
impl std::error::Error for RefscapeError {}
impl From<std::io::Error> for RefscapeError {
    fn from(error: std::io::Error) -> Self {
        Self::new(ErrorKind::Io, error.to_string())
    }
}
impl From<String> for RefscapeError {
    fn from(message: String) -> Self {
        Self::new(ErrorKind::InternalInvariant, message)
    }
}
impl From<&str> for RefscapeError {
    fn from(message: &str) -> Self {
        Self::new(ErrorKind::InternalInvariant, message)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FeatureResult<T> {
    Supported(T),
    Unsupported,
}
