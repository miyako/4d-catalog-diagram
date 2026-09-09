//! Error taxonomy. Exit codes are part of the CLI contract (BUILD-SPEC §5) and
//! must stay stable — an agent branches on them.

use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(i32)]
pub enum ExitCode {
    Success = 0,
    Usage = 1,
    InputFile = 2,
    MalformedXml = 3,
    Selection = 4,
    Render = 5,
}

/// Where in the input a parse failure happened, plus the offending source line.
/// "Not well-formed XML" on its own is useless against a several-thousand-line
/// catalog.
#[derive(Debug, Clone)]
pub struct XmlPosition {
    pub line: u32,
    pub column: u32,
    /// The source line itself, trimmed of any trailing newline.
    pub excerpt: String,
}

#[derive(Debug)]
pub enum AppError {
    Usage(String),
    InputFile(String),
    MalformedXml(String, Option<XmlPosition>),
    Selection(String),
    Render(String),
}

impl AppError {
    pub fn exit_code(&self) -> ExitCode {
        match self {
            AppError::Usage(_) => ExitCode::Usage,
            AppError::InputFile(_) => ExitCode::InputFile,
            AppError::MalformedXml(..) => ExitCode::MalformedXml,
            AppError::Selection(_) => ExitCode::Selection,
            AppError::Render(_) => ExitCode::Render,
        }
    }

    /// Stable machine-readable identifier for `--error-format json`.
    pub fn kind(&self) -> &'static str {
        match self {
            AppError::Usage(_) => "usage",
            AppError::InputFile(_) => "input_file",
            AppError::MalformedXml(..) => "malformed_xml",
            AppError::Selection(_) => "selection",
            AppError::Render(_) => "render",
        }
    }

    pub fn message(&self) -> &str {
        match self {
            AppError::Usage(m)
            | AppError::InputFile(m)
            | AppError::MalformedXml(m, _)
            | AppError::Selection(m)
            | AppError::Render(m) => m,
        }
    }

    pub fn position(&self) -> Option<&XmlPosition> {
        match self {
            AppError::MalformedXml(_, position) => position.as_ref(),
            _ => None,
        }
    }

    /// One JSON object per line, on stderr.
    pub fn to_json_line(&self) -> String {
        let mut value = serde_json::json!({
            "level": "error",
            "kind": self.kind(),
            "exit_code": self.exit_code() as i32,
            "message": self.message(),
        });
        if let (Some(position), Some(object)) = (self.position(), value.as_object_mut()) {
            object.insert("line".into(), position.line.into());
            object.insert("column".into(), position.column.into());
        }
        value.to_string()
    }
}

impl fmt::Display for AppError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.message())
    }
}

impl std::error::Error for AppError {}

pub type Result<T> = std::result::Result<T, AppError>;
