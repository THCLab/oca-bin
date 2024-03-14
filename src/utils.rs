use std::{
    fs,
    path::{Path, PathBuf},
};

use walkdir::WalkDir;

use crate::error::CliError;

pub fn load_ocafiles_all(
    file_path: Option<&PathBuf>,
    dir_path: Option<&PathBuf>,
) -> Result<Vec<PathBuf>, CliError> {
    if let Some(directory) = dir_path {
        info!(
            "Building OCA bundle from directory {}",
            directory.to_str().unwrap()
        );
        visit_dirs_recursive(directory)
    } else if let Some(file) = file_path {
        info!(
            "Building OCA bundle from oca file {}",
            file.to_str().unwrap()
        );
        Ok(vec![PathBuf::from(file)])
    } else {
        panic!("No file or directory provided");
    }
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
    let files = fs::read_dir(dir).map_err(|err| CliError::DirectoryReadFailed(err))?;
    for entry in files {
        let entry = entry.map_err(|err| CliError::DirectoryReadFailed(err))?;
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
