use std::{
    any::Any,
    fs,
    path::{Path, PathBuf},
};

use said::SelfAddressingIdentifier;
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

/// Loads elements (nodes) from the provided file(s) or directory, and returns
/// them sorted by their references. Each element comes after the ones it
/// depends on.
///
/// This function handles the following cases:
/// - If `file_path` is provided, it loads nodes from the specified file(s).
/// - If `dir_path` is provided, it recursively loads all nodes from the specified directory.
/// - If both `file_path` and `dir_path` are provided, it combines nodes from both the file(s) and directory.
pub fn load_nodes(
    file_path: Option<Vec<PathBuf>>,
    dir_path: Option<&PathBuf>,
) -> Result<Vec<Node>, CliError> {
    Ok(match (file_path, dir_path) {
        (None, None) => unreachable!("At least one argument needed"),
        (None, Some(base_dir)) => {
            let paths = visit_dirs_recursive(base_dir)?;
            let graph = MutableGraph::new(paths)?;
            graph.sort()?
        }
        (Some(oca_file), None) => {
            let graph = MutableGraph::new(oca_file)?;
            graph.sort()?
        }
        (Some(oca_file), Some(base_dir)) => {
            let paths = visit_dirs_recursive(base_dir)?;
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

pub fn load_remote_repo_url(
    repository_url: &Option<String>,
    remote_repo_url_from_config: Option<String>,
) -> Result<Url, CliError> {
    match (repository_url, remote_repo_url_from_config) {
        (None, None) => Err(CliError::UnknownRemoteRepoUrl),
        (None, Some(config_url)) => parse_url(config_url),
        (Some(repo_url), _) => parse_url(repo_url.clone()),
    }
}

pub fn send_to_repo(repository_url: &Url, ocafile: String, timeout: u64) -> Result<(), CliError> {
    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(timeout))
        .build()
        .expect("Failed to create reqwest client");
    let url = repository_url.join("oca-bundles")?;
    info!("Publish OCA bundle to: {} with payload: {}", url, ocafile);
    match client.post(url).body(ocafile).send() {
        Ok(v) => match v.error_for_status() {
            Ok(v) => {
                info!("{},{}", v.status(), v.text().unwrap());
                Ok(())
            }
            Err(er) => {
                info!("error: {:?}", er);
                Err(CliError::PublishError(
                    SelfAddressingIdentifier::default(),
                    vec![er.to_string()],
                ))
            }
        },
        Err(e) => {
            info!("Error while uploading OCAFILE: {}", e);
            Err(CliError::PublishError(
                SelfAddressingIdentifier::default(),
                vec![format!("Sending error: {}", e)],
            ))
        }
    }
}
