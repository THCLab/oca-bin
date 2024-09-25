use std::{io, path::PathBuf};

use oca_rs::facade::build::ValidationError;
use said::SelfAddressingIdentifier;
use thiserror::Error;

use crate::{
    build::CacheError, dependency_graph::GraphError, presentation_command::PresentationError,
    tui::bundle_list::BundleListError,
};

#[derive(Debug, Error)]
pub enum CliError {
    #[error(transparent)]
    Input(#[from] io::Error),
    #[error("Presentation command error: {0}")]
    Presentation(#[from] PresentationError),
    #[error("Error getting current directory: {0}")]
    CurrentDirFailed(std::io::Error),
    #[error("Error writing file: {0}")]
    WriteFileFailed(std::io::Error),
    #[error("Error reading file: {0}. Kind: {1}")]
    ReadFileFailed(PathBuf, std::io::Error),
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
    #[error("Error while building file: {0}, reason: {1}")]
    BuildingError(PathBuf, BuildingFailures),
    #[error(transparent)]
    GraphError(#[from] GraphError),
    #[error("Publishing error: file: {0}, reason: {1:?}")]
    PublishError(SelfAddressingIdentifier, Vec<String>),
    #[error("Selected element isn't build properly: {0}")]
    SelectionError(PathBuf),
    #[error("Oca bundle of said {0} not found")]
    OCABundleSAIDNotFound(SelfAddressingIdentifier),
    #[error("Oca bundle of  refn {0} not found")]
    OCABundleRefnNotFound(String),
    #[error("Missing refn in file: {0}")]
    MissingRefn(PathBuf),
    #[error("Wrong repository url: {0}. Check `repository_url` in config file.")]
    UrlError(#[from] url::ParseError),
    #[error("No repository path set. You can set it by adding `repository_url` to config file.")]
    UnknownRemoteRepoUrl,
    #[error("Unexpected error occurred: {0}")]
    Panic(String),
    #[error("Cache error: {0}")]
    CacheError(#[from] CacheError),
}

impl From<Vec<oca_rs::facade::build::Error>> for BuildingFailures {
    fn from(value: Vec<oca_rs::facade::build::Error>) -> Self {
        Self(value)
    }
}

#[derive(Debug)]
pub struct BuildingFailures(pub(crate) Vec<oca_rs::facade::build::Error>);
impl std::fmt::Display for BuildingFailures {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let errs = self
            .0
            .iter()
            .flat_map(|e| match e {
                oca_rs::facade::build::Error::ValidationError(valdation_errors) => {
                    valdation_errors.iter().map(|e| e.to_string())
                }
            })
            .collect::<Vec<_>>();
        write!(f, "{}", errs.join("\n"))
    }
}

impl From<BundleListError> for CliError {
    fn from(value: BundleListError) -> Self {
        match value {
            BundleListError::AllRefnUnknown => CliError::AllRefnUnknown("".into()),
            BundleListError::GraphError(g) => CliError::GraphError(g),
            BundleListError::ErrorSelected(p) => CliError::SelectionError(p),
            BundleListError::RefnMissing(p) => CliError::MissingRefn(p),
        }
    }
}
