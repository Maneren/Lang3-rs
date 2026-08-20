use std::{error::Error, fmt, io::Error as IoError};

use l3_location::Location;

pub type RuntimeResult<T> = Result<T, Box<RuntimeError>>;

#[derive(Debug, Clone)]
pub struct StacktraceFrame {
    pub function_name: String,
    pub call_location: Location,
}

#[derive(Debug, Clone)]
enum RuntimeErrorKind {
    UnsupportedOperation { message: String },
    ValueError { message: String },
    TypeError { message: String },
    NameError { message: String },
    UndefinedVariable { message: String },
    Internal { message: String },
}

#[derive(Debug, Clone)]
pub struct RuntimeError {
    kind: RuntimeErrorKind,
    location: Option<Location>,
    stacktrace: Vec<StacktraceFrame>,
}

impl RuntimeError {
    const fn new(kind: RuntimeErrorKind) -> Self {
        Self {
            kind,
            location: None,
            stacktrace: Vec::new(),
        }
    }

    pub fn unsupported(msg: impl Into<String>) -> Box<Self> {
        Box::new(Self::new(RuntimeErrorKind::UnsupportedOperation {
            message: msg.into(),
        }))
    }
    pub fn value(msg: impl Into<String>) -> Box<Self> {
        Box::new(Self::new(RuntimeErrorKind::ValueError {
            message: msg.into(),
        }))
    }
    pub fn type_error(msg: impl Into<String>) -> Box<Self> {
        Box::new(Self::new(RuntimeErrorKind::TypeError {
            message: msg.into(),
        }))
    }
    pub fn name(msg: impl Into<String>) -> Box<Self> {
        Box::new(Self::new(RuntimeErrorKind::NameError {
            message: msg.into(),
        }))
    }
    pub fn undefined(msg: impl Into<String>) -> Box<Self> {
        Box::new(Self::new(RuntimeErrorKind::UndefinedVariable {
            message: msg.into(),
        }))
    }
    pub fn generic(msg: impl Into<String>) -> Box<Self> {
        Box::new(Self::new(RuntimeErrorKind::Internal {
            message: msg.into(),
        }))
    }

    #[must_use]
    pub fn with_location(mut self: Box<Self>, loc: Location) -> Box<Self> {
        self.location = Some(loc);
        self
    }

    #[must_use]
    pub const fn location(&self) -> Option<&Location> {
        self.location.as_ref()
    }

    pub fn set_stacktrace(&mut self, stacktrace: Vec<StacktraceFrame>) {
        self.stacktrace = stacktrace;
    }

    #[must_use]
    pub fn message(&self) -> &str {
        match &self.kind {
            RuntimeErrorKind::UnsupportedOperation { message: m }
            | RuntimeErrorKind::ValueError { message: m }
            | RuntimeErrorKind::TypeError { message: m }
            | RuntimeErrorKind::NameError { message: m }
            | RuntimeErrorKind::UndefinedVariable { message: m }
            | RuntimeErrorKind::Internal { message: m } => m,
        }
    }

    const fn type_name(&self) -> &'static str {
        match &self.kind {
            RuntimeErrorKind::UnsupportedOperation { .. } => "UnsupportedOperation",
            RuntimeErrorKind::ValueError { .. } => "ValueError",
            RuntimeErrorKind::TypeError { .. } => "TypeError",
            RuntimeErrorKind::NameError { .. } => "NameError",
            RuntimeErrorKind::UndefinedVariable { .. } => "UndefinedVariable",
            RuntimeErrorKind::Internal { .. } => "RuntimeError",
        }
    }
}

impl fmt::Display for RuntimeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.type_name(), self.message())?;
        if let Some(ref loc) = self.location {
            write!(f, "\n  at {loc}")?;
        }
        if !self.stacktrace.is_empty() {
            write!(f, "\nstacktrace:")?;
            for frame in self.stacktrace.iter().rev() {
                write!(
                    f,
                    "\n  in {} called at {}",
                    frame.function_name, frame.call_location
                )?;
            }
        }
        Ok(())
    }
}

impl Error for RuntimeError {}

impl From<IoError> for RuntimeError {
    fn from(err: IoError) -> Self {
        Self::new(RuntimeErrorKind::Internal {
            message: err.to_string(),
        })
    }
}

impl From<IoError> for Box<RuntimeError> {
    fn from(err: IoError) -> Self {
        Self::new(RuntimeError::new(RuntimeErrorKind::Internal {
            message: err.to_string(),
        }))
    }
}
