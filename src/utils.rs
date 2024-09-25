use std::{
    any::Any,
    fs,
    path::{Path, PathBuf},
};

use url::Url;
use walkdir::WalkDir;

use crate::{
    dependency_graph::{parse_node, GraphError, MutableGraph, Node},
    error::CliError,
};

pub fn load_ocafiles_all(
    file_path: Option<&PathBuf>,
    dir_path: Option<&PathBuf>,
) -> Result<Vec<PathBuf>, CliError> {
    Ok(match (file_path, dir_path) {
        (None, None) => panic!(
            "Specify the base working directory where you keep your ocafiles or path to ocafile"
        ),
        (None, Some(dir)) => visit_dirs_recursive(dir)?,
        (Some(oca_file), None) => vec![oca_file.clone()],
        (Some(oca_file), Some(_dir)) => {
            vec![oca_file.clone()]
        }
    })
}

pub fn load_nodes(
    file_path: Option<Vec<PathBuf>>,
    dir_path: Option<&PathBuf>,
) -> Result<Vec<Node>, CliError> {
    Ok(match (file_path, dir_path) {
        (None, None) => unreachable!("At least one argument needed"),
        (None, Some(base_dir)) => {
            let paths = visit_dirs_recursive(&base_dir)?;
            let graph = MutableGraph::new(paths)?;
            graph.sort()?
        }
        (Some(oca_file), None) => {
            let graph = MutableGraph::new(oca_file)?;
            graph.sort()?
        }
        (Some(oca_file), Some(base_dir)) => {
            let paths = visit_dirs_recursive(&base_dir)?;
            let graph = MutableGraph::new(paths).unwrap();

            let mut desc = vec![];
            for ocafile in oca_file {
                let (node, dependencies) = parse_node(&ocafile)
                    .map_err(|e| CliError::GraphError(GraphError::NodeParsingError(e)))?;
                match graph.insert_node(node.clone(), dependencies) {
                    Ok(_) => (),
                    Err(GraphError::DuplicateKey {
                        refn,
                        first_path,
                        second_path,
                    }) => {
                        info!("Saving node skipped because it's already in graph: name: {}, paths: {:?}, {:?}", refn, first_path, second_path,);
                        ()
                    }
                    Err(e) => return Err(e.into()),
                };
                let mut graph_desc = graph.get_descendants(&node.refn)?;
                desc.append(&mut graph_desc);
                desc.push(node);
            }
            desc
        }
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

pub fn handle_panic(panic: Box<dyn Any + Send>) -> CliError {
    let err = if let Some(panic_message) = panic.downcast_ref::<&str>() {
        CliError::Panic(panic_message.to_string())
    } else if let Some(panic_message) = panic.downcast_ref::<String>() {
        CliError::Panic(panic_message.clone())
    } else {
        CliError::Panic("Caught an unknown panic".to_string())
    };
    err
}
