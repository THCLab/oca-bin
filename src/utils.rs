use std::{
    any::Any,
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
};

use base64::{prelude::BASE64_STANDARD, Engine};
use sha2::{Digest, Sha256};
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

// Filter already build elements, basing on provided cache
pub fn changed_files<'a>(
    all_paths: impl IntoIterator<Item = &'a PathBuf>,
    hashes_cache: &HashMap<PathBuf, String>,
) -> Vec<&'a PathBuf> {
    all_paths
        .into_iter()
        .filter_map(|path| {
            let unparsed_file = fs::read_to_string(&path)
                .map_err(|e| CliError::ReadFileFailed(path.to_path_buf(), e))
                .unwrap();
            let hash = compute_hash(&unparsed_file.trim());

            match hashes_cache.get(path) {
                Some(cache) if hash.eq(cache) => {
                    info!("Already built: {:?}. Skipping", &path);
                    None
                }
                Some(_) => {
                    info!("File changed: {:?}", &path);
                    Some(path)
                }
                None => {
                    info!("New ocafile: {:?}", &path);
                    Some(path)
                }
            }
        })
        .collect()
}

pub fn compute_hash(content: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(content);
    let result = hasher.finalize();
    BASE64_STANDARD.encode(result)
}

pub fn join_with_dependencies<'a>(
    graph: &MutableGraph,
    paths: impl IntoIterator<Item = &'a PathBuf>,
    include_starting_node: bool,
) -> Box<dyn Iterator<Item = Node>> {
    // For each updated file find files that depends on it. They need to be updated due to said change.
    let start_nodes = paths
        .into_iter()
        .cloned()
        .map(|path| {
            let (node, _) = parse_node(&path).unwrap();
            node
        })
        .collect::<Vec<_>>();
    let refns = start_nodes.iter().map(|node| node.refn.as_str());
    let anc = graph.get_ancestors(refns, include_starting_node).unwrap();

    Box::new(anc.into_iter())
}

#[test]
pub fn test_changed_files() -> anyhow::Result<()> {
    use std::{fs::File, io::Write};
    use tempdir::TempDir;

    let tmp_dir = TempDir::new("example")?;

    let first_ocafile_str = "-- name=first\nADD ATTRIBUTE d=Text i=Text passed=Boolean";
    let second_ocafile_str = "-- name=second\nADD ATTRIBUTE list=Array[Text] el=Text";
    let third_ocafile_str = "-- name=third\nADD ATTRIBUTE first=refn:first second=refn:second";
    let fourth_ocafile_str = "-- name=fourth\nADD ATTRIBUTE whatever=Text";
    let fifth_ocafile_str = "-- name=fifth\nADD ATTRIBUTE third=refn:third four=refn:fourth";

    let list = [
        ("first.ocafile", first_ocafile_str),
        ("second.ocafile", second_ocafile_str),
        ("third.ocafile", third_ocafile_str),
        ("fourth.ocafile", fourth_ocafile_str),
        ("fifth.ocafile", fifth_ocafile_str),
    ];

    let mut paths = vec![];
    for (name, contents) in list {
        let path = tmp_dir.path().join(name);
        let mut tmp_file = File::create(&path)?;
        writeln!(tmp_file, "{}", contents)?;
        paths.push(path)
    }

    let mut cache = HashMap::new();

    let fifth_hash = compute_hash(fifth_ocafile_str);
    let path = tmp_dir.path().join("fifth.ocafile");
    cache.insert(path.clone(), fifth_hash);

    let nodes = changed_files(paths.iter(), &cache);
    assert!(!nodes.contains(&&path));
    assert_eq!(nodes.len(), 4);

    let second_hash = compute_hash(second_ocafile_str);
    let second_path = tmp_dir.path().join("second.ocafile");
    cache.insert(second_path.clone(), second_hash);

    let nodes = changed_files(paths.iter(), &cache);
    assert!(!nodes.contains(&&path));
    assert!(!nodes.contains(&&second_path));
    assert_eq!(nodes.len(), 3);

    let graph = MutableGraph::new(&paths)?;
    let fourth_path = tmp_dir.path().join("fourth.ocafile");
    let deps =
        join_with_dependencies(&graph, vec![&fourth_path, &second_path], true).collect::<Vec<_>>();
    assert_eq!(deps.iter().map(|dep| dep.refn.clone()).collect::<Vec<_>>(), vec!["second", "third", "fourth", "fifth"]);
    	
    let deps = join_with_dependencies(&graph, vec![&fourth_path, &second_path], false).collect::<Vec<_>>();
    assert_eq!(deps.iter().map(|dep| dep.refn.clone()).collect::<Vec<_>>(), vec!["third", "fifth"]);
    
    Ok(())
}

