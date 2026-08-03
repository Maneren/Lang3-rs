use l3_location::Location;
use std::fmt;

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
        RuntimeError::UnsupportedOperation {
            message: msg.into(),
            location: None,
        }
    }
    pub fn value(msg: impl Into<String>) -> Self {
        RuntimeError::ValueError {
            message: msg.into(),
            location: None,
        }
    }
    pub fn type_error(msg: impl Into<String>) -> Self {
        RuntimeError::TypeError {
            message: msg.into(),
            location: None,
        }
    }
    pub fn name(msg: impl Into<String>) -> Self {
        RuntimeError::NameError {
            message: msg.into(),
            location: None,
        }
    }
    pub fn undefined(msg: impl Into<String>) -> Self {
        RuntimeError::UndefinedVariable {
            message: msg.into(),
            location: None,
        }
    }
    pub fn generic(msg: impl Into<String>) -> Self {
        RuntimeError::Internal {
            message: msg.into(),
            location: None,
        }
    }

    #[must_use]
    pub fn with_location(mut self, loc: Location) -> Self {
        match &mut self {
            RuntimeError::UnsupportedOperation { location: l, .. }
            | RuntimeError::ValueError { location: l, .. }
            | RuntimeError::TypeError { location: l, .. }
            | RuntimeError::NameError { location: l, .. }
            | RuntimeError::UndefinedVariable { location: l, .. }
            | RuntimeError::Internal { location: l, .. } => *l = Some(loc),
        }
        self
    }

    #[must_use]
    pub fn message(&self) -> &str {
        match self {
            RuntimeError::UnsupportedOperation { message: m, .. }
            | RuntimeError::ValueError { message: m, .. }
            | RuntimeError::TypeError { message: m, .. }
            | RuntimeError::NameError { message: m, .. }
            | RuntimeError::UndefinedVariable { message: m, .. }
            | RuntimeError::Internal { message: m, .. } => m,
        }
    }
}

impl fmt::Display for RuntimeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RuntimeError::UnsupportedOperation { message, .. } => {
                write!(f, "UnsupportedOperation: {message}")
            }
            RuntimeError::ValueError { message, .. } => write!(f, "ValueError: {message}"),
            RuntimeError::TypeError { message, .. } => write!(f, "TypeError: {message}"),
            RuntimeError::NameError { message, .. } => write!(f, "NameError: {message}"),
            RuntimeError::UndefinedVariable { message, .. } => {
                write!(f, "UndefinedVariable: {message}")
            }
            RuntimeError::Internal { message, .. } => write!(f, "RuntimeError: {message}"),
        }
    }
}

impl std::error::Error for RuntimeError {}

impl From<std::io::Error> for RuntimeError {
    fn from(err: std::io::Error) -> Self {
        RuntimeError::Internal {
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

impl std::error::Error for CompileError {}
