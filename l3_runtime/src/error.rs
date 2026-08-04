use std::{error::Error, fmt, io::Error as IoError};

use l3_location::Location;

#[derive(Debug, Clone)]
pub struct StacktraceFrame {
    pub function_name: String,
    pub call_location: Location,
}

#[derive(Debug, Clone)]
pub enum RuntimeError {
    UnsupportedOperation {
        message: String,
        location: Option<Location>,
    },
    ValueError {
        message: String,
        location: Option<Location>,
    },
    TypeError {
        message: String,
        location: Option<Location>,
    },
    NameError {
        message: String,
        location: Option<Location>,
    },
    UndefinedVariable {
        message: String,
        location: Option<Location>,
    },
    Internal {
        message: String,
        location: Option<Location>,
    },
}

impl RuntimeError {
    pub fn unsupported(msg: impl Into<String>) -> Self {
        Self::UnsupportedOperation {
            message: msg.into(),
            location: None,
        }
    }
    pub fn value(msg: impl Into<String>) -> Self {
        Self::ValueError {
            message: msg.into(),
            location: None,
        }
    }
    pub fn type_error(msg: impl Into<String>) -> Self {
        Self::TypeError {
            message: msg.into(),
            location: None,
        }
    }
    pub fn name(msg: impl Into<String>) -> Self {
        Self::NameError {
            message: msg.into(),
            location: None,
        }
    }
    pub fn undefined(msg: impl Into<String>) -> Self {
        Self::UndefinedVariable {
            message: msg.into(),
            location: None,
        }
    }
    pub fn generic(msg: impl Into<String>) -> Self {
        Self::Internal {
            message: msg.into(),
            location: None,
        }
    }

    #[must_use]
    pub fn with_location(mut self, loc: Location) -> Self {
        match &mut self {
            Self::UnsupportedOperation { location: l, .. }
            | Self::ValueError { location: l, .. }
            | Self::TypeError { location: l, .. }
            | Self::NameError { location: l, .. }
            | Self::UndefinedVariable { location: l, .. }
            | Self::Internal { location: l, .. } => *l = Some(loc),
        }
        self
    }

    #[must_use]
    pub fn message(&self) -> &str {
        match self {
            Self::UnsupportedOperation { message: m, .. }
            | Self::ValueError { message: m, .. }
            | Self::TypeError { message: m, .. }
            | Self::NameError { message: m, .. }
            | Self::UndefinedVariable { message: m, .. }
            | Self::Internal { message: m, .. } => m,
        }
    }
}

impl fmt::Display for RuntimeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedOperation { message, .. } => {
                write!(f, "UnsupportedOperation: {message}")
            },
            Self::ValueError { message, .. } => write!(f, "ValueError: {message}"),
            Self::TypeError { message, .. } => write!(f, "TypeError: {message}"),
            Self::NameError { message, .. } => write!(f, "NameError: {message}"),
            Self::UndefinedVariable { message, .. } => {
                write!(f, "UndefinedVariable: {message}")
            },
            Self::Internal { message, .. } => write!(f, "RuntimeError: {message}"),
        }
    }
}

impl Error for RuntimeError {}

impl From<IoError> for RuntimeError {
    fn from(err: IoError) -> Self {
        Self::Internal {
            message: err.to_string(),
            location: None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct CompileError {
    pub message: String,
}

impl CompileError {
    pub fn new(msg: impl Into<String>) -> Self {
        Self {
            message: msg.into(),
        }
    }
}

impl fmt::Display for CompileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "CompileError: {}", self.message)
    }
}

impl Error for CompileError {}
