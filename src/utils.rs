use std::{
    fs,
    path::{Path, PathBuf},
};

use url::Url;
use walkdir::WalkDir;

use crate::error::CliError;

pub fn load_ocafiles_all(
    file_path: Option<&PathBuf>,
    dir_path: Option<&PathBuf>,
) -> Result<(Vec<PathBuf>, PathBuf), CliError> {
    Ok(match (file_path, dir_path) {
        (None, None) => panic!("No file or directory provided"),
        (None, Some(dir)) => (visit_dirs_recursive(dir)?, dir.clone()),
        (Some(oca_file), None) => (
            vec![oca_file.clone()],
            oca_file.parent().unwrap().to_path_buf(),
        ),
        (Some(oca_file), Some(dir)) => (vec![oca_file.clone()], dir.clone()),
    })
}

pub fn visit_dirs_recursive(dir: &Path) -> Result<Vec<PathBuf>, CliError> {
    let mut paths = Vec::new();
    for entry in WalkDir::new(dir).into_iter() {
        if let Ok(entry_path) = entry {
            let path = entry_path.path();
            if path.is_dir() {
                continue;
            }
            if let Some(ext) = path.extension() {
                if ext == "ocafile" {
                    paths.push(path.to_path_buf());
                }
            }
        } else {
            return Err(CliError::NonexistentPath(dir.to_owned()));
        }
    }
    Ok(paths)
}

pub fn visit_current_dir(dir: &Path) -> Result<Vec<PathBuf>, CliError> {
    let mut paths = Vec::new();
    if !dir.exists() {
        return Err(CliError::NonexistentPath(dir.to_owned()));
    };
    if !dir.is_dir() {
        return Err(CliError::NotDirectory(dir.to_owned()));
    };
    let files = fs::read_dir(dir).map_err(CliError::DirectoryReadFailed)?;
    for entry in files {
        let entry = entry.map_err(CliError::DirectoryReadFailed)?;
        let path = entry.path();
        if path.is_dir() {
        } else if let Some(ext) = path.extension() {
            if ext == "ocafile" {
                paths.push(path.to_path_buf());
            }
        }
    }
    Ok(paths)
}

pub fn parse_url(url: String) -> Result<Url, CliError> {
    let url = if !url.ends_with("/") {
        let mut tmp = url.clone();
        tmp.push('/');
        tmp
    } else {
        url
    };
    Ok(url::Url::parse(&url)?)
}
