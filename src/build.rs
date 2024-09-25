use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
};

use base64::{prelude::BASE64_STANDARD, Engine};
use sha2::{Digest, Sha256};

use crate::{
    dependency_graph::{parse_node, GraphError, MutableGraph, Node, NodeParsingError},
    error::CliError,
};

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

pub fn load_nodes_to_build(
    cache_path: &Path,
    all_paths: &[PathBuf],
) -> Result<(HashMap<PathBuf, String>, Vec<Node>), CacheError> {
    let cache = load_cache(cache_path)?;
    let mut filtered_paths = changed_files(all_paths.iter(), &cache)
        .into_iter()
        .peekable();

    if filtered_paths.peek().is_none() {
        Err(CacheError::NoChanges)
    } else {
        let graph = MutableGraph::new(all_paths)?;
        // Find files that filtered files depends on
        Ok((cache, join_with_dependencies(&graph, filtered_paths, true)?))
    }
}

fn load_cache(cache_path: &Path) -> Result<HashMap<PathBuf, String>, CacheError> {
    let cache_contents = fs::read_to_string(cache_path)?;
    if cache_contents.is_empty() {
        Err(CacheError::EmptyCache)
    } else {
        Ok(serde_json::from_str(&cache_contents)?)
    }
}

// Filter already build elements, basing on provided cache
fn changed_files<'a>(
    all_paths: impl IntoIterator<Item = &'a PathBuf>,
    hashes_cache: &HashMap<PathBuf, String>,
) -> Vec<&'a PathBuf> {
    all_paths
        .into_iter()
        .filter(|path| {
            let unparsed_file = fs::read_to_string(path)
                .map_err(|e| CliError::ReadFileFailed(path.to_path_buf(), e))
                .unwrap();
            let hash = compute_hash(unparsed_file.trim());

            match hashes_cache.get(*path) {
                Some(cache) if hash.eq(cache) => {
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

    // create temporary cache file
    let mut cache_map = HashMap::new();
    cache_map.insert(paths[0].clone(), hashes[0].clone());

    let cache_path = tmp_dir.path().join(".oca-bin");
    let mut cache_tmp_file = File::create(&cache_path)?;
    writeln!(
        cache_tmp_file,
        "{}",
        serde_json::to_string(&cache_map).unwrap()
    )?;

    let (_cache_before_change, nodes) = load_nodes_to_build(cache_path.as_path(), &paths)?;
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

    let (_cache_after_change, nodes) = load_nodes_to_build(cache_path.as_path(), &paths)?;
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
    let mut cache_map = HashMap::new();
    for (path, hash) in paths.clone().into_iter().zip(hashes) {
        cache_map.insert(path.clone(), hash.clone());
    }
    let cache_path = tmp_dir.path().join(".oca-bin");
    let mut cache_tmp_file = File::create(&cache_path)?;
    writeln!(
        cache_tmp_file,
        "{}",
        serde_json::to_string(&cache_map).unwrap()
    )?;
    cache_tmp_file.flush().unwrap();

    let nodes = load_nodes_to_build(cache_path.as_path(), &paths).unwrap_err();
    assert!(matches!(CacheError::NoChanges, nodes));

    // Edit fifth file
    let edited_fifth_ocafile_str = "-- name=fifth\nADD ATTRIBUTE third=refn:third";
    list[4].1 = edited_fifth_ocafile_str;
    let mut tmp_file = File::create(&paths[4])?;
    println!("Updating file {:?}", &paths[4]);
    writeln!(tmp_file, "{}", edited_fifth_ocafile_str)?;
    tmp_file.flush().unwrap();

    let (_cache_after_change, nodes) = load_nodes_to_build(cache_path.as_path(), &paths)?;
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
