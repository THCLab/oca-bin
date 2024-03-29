use std::{io, path::PathBuf};

use oca_rs::facade::build::ValidationError;
use thiserror::Error;

use crate::{dependency_graph::GraphError, presentation_command::PresentationError};

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
    #[error("No such file or directory: {0}")]
    NonexistentPath(PathBuf),
    #[error("Not a directory: {0}")]
    NotDirectory(PathBuf),
    #[error("Can't read directory: {0}")]
    DirectoryReadFailed(io::Error),
    #[error("All references are unknown. Run `build -d {0}` first")]
    AllRefnUnknown(PathBuf),
    #[error("Validation error: file: {0}, reason: {1:?}")]
    GrammarError(PathBuf, Vec<ValidationError>),
    #[error("Validation error: file: {0}, reason: {1:?}")]
    BuildingError(PathBuf, Vec<oca_rs::facade::build::Error>),
    #[error(transparent)]
    GraphError(#[from] GraphError),
}
