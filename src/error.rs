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

#[derive(Debug)]
pub enum AppError {
    Usage(String),
    InputFile(String),
    MalformedXml(String),
    Selection(String),
    Render(String),
}

impl AppError {
    pub fn exit_code(&self) -> ExitCode {
        match self {
            AppError::Usage(_) => ExitCode::Usage,
            AppError::InputFile(_) => ExitCode::InputFile,
            AppError::MalformedXml(_) => ExitCode::MalformedXml,
            AppError::Selection(_) => ExitCode::Selection,
            AppError::Render(_) => ExitCode::Render,
        }
    }

    /// Stable machine-readable identifier for `--error-format json`.
    pub fn kind(&self) -> &'static str {
        match self {
            AppError::Usage(_) => "usage",
            AppError::InputFile(_) => "input_file",
            AppError::MalformedXml(_) => "malformed_xml",
            AppError::Selection(_) => "selection",
            AppError::Render(_) => "render",
        }
    }

    pub fn message(&self) -> &str {
        match self {
            AppError::Usage(m)
            | AppError::InputFile(m)
            | AppError::MalformedXml(m)
            | AppError::Selection(m)
            | AppError::Render(m) => m,
        }
    }

    /// One JSON object per line, on stderr.
    pub fn to_json_line(&self) -> String {
        let value = serde_json::json!({
            "level": "error",
            "kind": self.kind(),
            "exit_code": self.exit_code() as i32,
            "message": self.message(),
        });
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
