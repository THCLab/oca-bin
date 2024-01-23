use thiserror::Error;

use crate::presentation_command::PresentationError;

#[derive(Debug, Error)]
pub enum CliError {
    #[error("Presentation command error: {0}")]
    Presentation(#[from] PresentationError),
    #[error("Error getting current directory: {0}")]
    CurrentDirFailed(std::io::Error),
    #[error("Error writing file: {0}")]
    WriteFileFailed(std::io::Error),
    #[error("Error reading file: {0}")]
    ReadFileFailed(std::io::Error),
    #[error("Oca errors: {0:?}")]
    OcaBundleError(Vec<oca_rs::facade::build::Error>),
    #[error("Oca bundle ast errors: {0:?}")]
    OcaBundleAstError(Vec<String>),
    #[error("Invalid said: {0}")]
    InvalidSaid(#[from] said::error::Error),
    #[error("Field to read oca bundle: {0}")]
    ReadOcaError(serde_json::error::Error),
    #[error("Field to read oca bundle: {0}")]
    WriteOcaError(serde_json::error::Error),
    #[error("Unsupported format {0}")]
    FormatError(String),
    #[error("Unsupported extension format {0}")]
    FileExtensionError(String),
}
