use std::{
    collections::HashMap,
    fs::{self, File},
    path::{Path, PathBuf},
};

use base64::{prelude::BASE64_STANDARD, Engine};
use oca_rs::{facade::bundle::BundleElement, Facade, HashFunctionCode, SerializationFormats};
use sha2::{Digest, Sha256};

use crate::{
    cache::{PathCache, SaidCache}, dependency_graph::{parse_node, GraphError, MutableGraph, Node, NodeParsingError}, error::CliError
};
use oca_rs::EncodeBundle;

#[derive(thiserror::Error, Debug)]
pub enum CacheError {
    #[error(transparent)]
    PathError(#[from] std::io::Error),
    #[error("Cache is empty")]
    EmptyCache,
    #[error(transparent)]
    CacheFormat(#[from] serde_json::Error),
    #[error("No changes detected")]
    NoChanges,
    #[error("Graph error: {0}")]
    GraphError(#[from] GraphError),
    #[error("Node parsing error: {0}")]
    NodeError(#[from] NodeParsingError),
}

pub fn load_changed_nodes(
    cache_path: &PathCache,
    all_paths: &[PathBuf],
) -> Result<Vec<Node>, CacheError> {
    // let cache = load_cache(cache_path)?;
    let mut filtered_paths = changed_files(all_paths.iter(), &cache_path)
        .into_iter()
        .peekable();

    if filtered_paths.peek().is_none() {
        Err(CacheError::NoChanges)
    } else {
        let graph = MutableGraph::new(all_paths)?;
        // Find files that filtered files depends on
        Ok(join_with_dependencies(&graph, filtered_paths, true)?)
    }
}

pub fn load_cache(cache_path: &Path) -> Result<HashMap<PathBuf, String>, CacheError> {
    let cache_contents = fs::read_to_string(cache_path)?;
    if cache_contents.is_empty() {
        Err(CacheError::EmptyCache)
    } else {
        Ok(serde_json::from_str(&cache_contents)?)
    }
}

// Filter already build elements, basing on provided cache
pub fn changed_files<'a>(
    all_paths: impl IntoIterator<Item = &'a PathBuf>,
    hashes_cache: &PathCache,
) -> Vec<&'a PathBuf> {
    all_paths
        .into_iter()
        .filter(|path| {
            let unparsed_file = fs::read_to_string(path)
                .map_err(|e| CliError::ReadFileFailed(path.to_path_buf(), e))
                .unwrap();
            let hash = compute_hash(unparsed_file.trim());

            match hashes_cache.get(*path).unwrap() {
                Some(cache) if hash.eq(&cache) => {
                    info!("Already built: {:?}. Skipping", &path);
                    false
                }
                Some(_) => {
                    info!("File changed: {:?}", &path);
                    true
                }
                None => {
                    info!("New ocafile: {:?}", &path);
                    true
                }
            }
        })
        .collect()
}

/// Build node. If caches provided, save change there. Returns list of contents
/// of successfully built ocafiles that can be published in next step.
pub fn build(facade: &mut Facade, node: &Node, said_cache: Option<&SaidCache>,
path_cache: Option<&PathCache>) -> Result<Vec<String>, CliError> {
    info!("Building: {:?}", node);
    let mut oca_files_to_publish = vec![];
    let path = &node.path;
    let unparsed_file = fs::read_to_string(&path)
        .map_err(|e| CliError::ReadFileFailed(path.clone(), e))?;
    let hash = compute_hash(unparsed_file.trim());

    let oca_bundle_element = facade
        .build_from_ocafile(unparsed_file.clone())
        .map_err(|e| CliError::BuildingError(path.clone(), e.into()))?;
    if let Some(path_cache) = path_cache {
            path_cache.insert(path.clone(), hash.clone())?;
            info!("Inserting to cache: {:?}", &path);
    };

    match oca_bundle_element {
        BundleElement::Mechanics(oca_bundle) => {
            if let Some(said_cache) = said_cache {
                said_cache.insert(hash, oca_bundle.said.as_ref().unwrap().clone())?;
            };
            let refs = facade.fetch_all_refs().unwrap();
            let schema_name = refs
                .iter()
                .find(|&(_, v)| *v == oca_bundle.said.clone().unwrap().to_string());
            if let Some((refs, _)) = schema_name {
                println!(
                    "OCA bundle created in local repository with SAID: {} and name: {}",
                    oca_bundle.said.unwrap(),
                    refs
                );
            } else {
                println!(
                    "OCA bundle created in local repository with SAID: {:?}",
                    oca_bundle.said.unwrap()
                );
            };
        }
        BundleElement::Transformation(transformation_file) => {
            let code = HashFunctionCode::Blake3_256;
            let format = SerializationFormats::JSON;
            let transformation_file_json =
                transformation_file.encode(&code, &format).unwrap();
            println!("{}", String::from_utf8(transformation_file_json).unwrap());
        }
    };
    oca_files_to_publish.push(unparsed_file);
    Ok(oca_files_to_publish)
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
) -> Result<Vec<Node>, CacheError> {
    // For each updated file find files that depends on it. They need to be updated due to said change.
    let start_nodes = paths
        .into_iter()
        .map(|path| parse_node(path).map(|(node, _)| node).map_err(|e| e.into()))
        .collect::<Result<Vec<_>, CacheError>>()?;
    let refns = start_nodes.iter().map(|node| node.refn.as_str());
    Ok(graph.get_ancestors(refns, include_starting_node)?)
}

/// Returns nodes that need to be updated
pub fn handle_cache(directory_path: &Path, all_nodes: &[Node]) -> Result<(PathCache, SaidCache, Vec<Node>), CacheError> {
    // Load cache if exists
    let mut said_cache_path = directory_path.to_path_buf();
    said_cache_path.push(".oca-saids");
    let _file = File::create(&said_cache_path).unwrap();
    let cache_said = SaidCache::new(said_cache_path.clone());

    let mut cache_path = directory_path.to_path_buf();
    cache_path.push(".oca-bin");
    info!("Cache path: {:?}", &cache_path);
    let cache_paths = PathCache::new(cache_path);

    let all_paths = all_nodes
        .iter()
        .map(|node| node.path.clone())
        .collect::<Vec<_>>();

    
    match load_changed_nodes(&cache_paths, &all_paths) {
        Ok(nodes) => {
            Ok((cache_paths, cache_said, nodes))
        }
        Err(CacheError::EmptyCache) | Err(CacheError::PathError(_)) => {
            Ok((cache_paths, cache_said, all_nodes.to_vec()))
        }
        Err(e) => return Err(e.into()),
    }
}

#[test]
pub fn test_cache() -> anyhow::Result<()> {
    use std::{fs::File, io::Write};
    use tempdir::TempDir;

    let tmp_dir = TempDir::new("example")?;

    let first_ocafile_str = "-- name=first\nADD ATTRIBUTE d=Text i=Text passed=Boolean";
    let second_ocafile_str = "-- name=second\nADD ATTRIBUTE list=Array[Text] el=Text";
    let third_ocafile_str = "-- name=third\nADD ATTRIBUTE first=refn:first second=refn:second";
    let fourth_ocafile_str = "-- name=fourth\nADD ATTRIBUTE whatever=Text";
    let fifth_ocafile_str = "-- name=fifth\nADD ATTRIBUTE third=refn:third four=refn:fourth";

    let mut list = [
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

    let mut hashes = list
        .clone()
        .iter()
        .map(|(_path, content)| compute_hash(content))
        .collect::<Vec<_>>();

    let cache_path = tmp_dir.path().join(".oca-bin");
    let cache = PathCache::new(cache_path);
    cache.insert(paths[0].clone(), hashes[0].clone()).unwrap();

    let nodes = load_changed_nodes(&cache, &paths)?;
    assert_eq!(
        nodes
            .iter()
            .map(|node| node.refn.clone())
            .collect::<Vec<_>>(),
        vec!["fourth", "second", "third", "fifth"]
    );

    // Edit first file
    let edited_first_ocafile_str = "-- name=first\nADD ATTRIBUTE d=Text";
    list[0].1 = edited_first_ocafile_str;
    let mut tmp_file = File::create(&paths[0])?;
    println!("Updating file {:?}", &paths[0]);
    writeln!(tmp_file, "{}", edited_first_ocafile_str)?;
    tmp_file.flush().unwrap();

    let nodes = load_changed_nodes(&cache, &paths)?;
    assert_eq!(
        nodes
            .iter()
            .map(|node| node.refn.clone())
            .collect::<Vec<_>>(),
        vec!["fourth", "second", "first", "third", "fifth"]
    );

    // Add all files to cache
    // update first element hash
    hashes[0] = compute_hash(&edited_first_ocafile_str);
    for (path, hash) in paths.clone().into_iter().zip(hashes) {
        cache.insert(path.clone(), hash.clone()).unwrap();
    }

    let nodes = load_changed_nodes(&cache, &paths).unwrap_err();
    assert!(matches!(CacheError::NoChanges, nodes));

    // Edit fifth file
    let edited_fifth_ocafile_str = "-- name=fifth\nADD ATTRIBUTE third=refn:third";
    list[4].1 = edited_fifth_ocafile_str;
    let mut tmp_file = File::create(&paths[4])?;
    println!("Updating file {:?}", &paths[4]);
    writeln!(tmp_file, "{}", edited_fifth_ocafile_str)?;
    tmp_file.flush().unwrap();

    let nodes = load_changed_nodes(&cache, &paths)?;
    assert_eq!(
        nodes
            .iter()
            .map(|node| node.refn.clone())
            .collect::<Vec<_>>(),
        vec!["fifth"]
    );

    Ok(())
}

#[test]
pub fn test_build_utils() -> anyhow::Result<()> {
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

    let cache = crate::Cache::new(tmp_dir.path().to_path_buf());

    let fifth_hash = compute_hash(fifth_ocafile_str);
    let path = tmp_dir.path().join("fifth.ocafile");
    cache.insert(path.clone(), fifth_hash).unwrap();

    let nodes = changed_files(paths.iter(), &cache);
    assert!(!nodes.contains(&&path));
    assert_eq!(nodes.len(), 4);

    let second_hash = compute_hash(second_ocafile_str);
    let second_path = tmp_dir.path().join("second.ocafile");
    cache.insert(second_path.clone(), second_hash).unwrap();

    let nodes = changed_files(paths.iter(), &cache);
    assert!(!nodes.contains(&&path));
    assert!(!nodes.contains(&&second_path));
    assert_eq!(nodes.len(), 3);

    let graph = MutableGraph::new(&paths)?;
    let fourth_path = tmp_dir.path().join("fourth.ocafile");
    let deps = join_with_dependencies(&graph, vec![&fourth_path, &second_path], true)?;
    assert_eq!(
        deps.iter().map(|dep| dep.refn.clone()).collect::<Vec<_>>(),
        vec!["second", "third", "fourth", "fifth"]
    );

    let deps = join_with_dependencies(&graph, vec![&fourth_path, &second_path], false)?;
    assert_eq!(
        deps.iter().map(|dep| dep.refn.clone()).collect::<Vec<_>>(),
        vec!["third", "fifth"]
    );

    Ok(())
}
