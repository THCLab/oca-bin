use std::{
    fs::{self},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

use base64::{prelude::BASE64_STANDARD, Engine};
use itertools::Itertools;
use oca_rs::{facade::bundle::BundleElement, Facade, HashFunctionCode, SerializationFormats};
use said::SelfAddressingIdentifier;
use sha2::{Digest, Sha256};
use url::Url;

use crate::{
    cache::{PathCache, SaidCache},
    dependency_graph::{parse_node, GraphError, MutableGraph, Node, NodeParsingError},
    error::CliError,
    publish_oca_file_for,
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
    let mut filtered_paths = changed_files(all_paths.iter(), cache_path)
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

/// Build node. If caches provided, save change there. Returns SAID of built ocafile, and its contents.
pub fn build(
    facade: Arc<Mutex<Facade>>,
    node: &Node,
    said_cache: Option<&SaidCache>,
    path_cache: Option<&PathCache>,
) -> Result<Option<(SelfAddressingIdentifier, String)>, CliError> {
    info!("Building: {:?}", node);
    let path = &node.path;
    let unparsed_file =
        fs::read_to_string(path).map_err(|e| CliError::ReadFileFailed(path.clone(), e))?;
    let hash = compute_hash(unparsed_file.trim());
    let oca_bundle_element = {
        let mut facade_locked = facade.lock().unwrap();
        facade_locked
            .build_from_ocafile(unparsed_file.clone())
            .map_err(|e| CliError::BuildingError(path.clone(), e.into()))?
    };

    if let Some(path_cache) = path_cache {
        path_cache.insert(path.clone(), hash.clone())?;
    };

    match oca_bundle_element {
        BundleElement::Mechanics(oca_bundle) => {
            let said = oca_bundle.said.as_ref().unwrap();
            if let Some(said_cache) = said_cache {
                said_cache.insert(hash, said.clone())?;
            };
            let refs = {
                let facade_locked = facade.lock().unwrap();
                facade_locked.fetch_all_refs().unwrap()
            };
            let schema_name = refs.iter().find(|&(_, v)| *v == said.to_string());
            if let Some((refs, _)) = schema_name {
                println!(
                    "OCA bundle created in local repository with SAID: {} and name: {}",
                    &said, refs
                );
            } else {
                println!(
                    "OCA bundle created in local repository with SAID: {:?}",
                    &said
                );
            };
            Ok(Some((said.clone(), unparsed_file)))
        }
        BundleElement::Transformation(transformation_file) => {
            let code = HashFunctionCode::Blake3_256;
            let format = SerializationFormats::JSON;
            let transformation_file_json = transformation_file.encode(&code, &format).unwrap();
            println!("{}", String::from_utf8(transformation_file_json).unwrap());
            Ok(None)
        }
    }
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
pub fn detect_changes(all_nodes: &[Node], cache: &PathCache) -> Result<Vec<Node>, CacheError> {
    let all_paths = all_nodes
        .iter()
        .map(|node| node.path.clone())
        .collect::<Vec<_>>();

    match load_changed_nodes(cache, &all_paths) {
        Ok(nodes) => Ok(nodes),
        Err(CacheError::EmptyCache) | Err(CacheError::PathError(_)) => Ok(all_nodes.to_vec()),
        Err(e) => Err(e),
    }
}

// Returns list of nodes that was rebuilt and caches.
pub fn rebuild(
    directory: &Path,
    facade: Arc<Mutex<Facade>>,
    nodes: Vec<Node>,
) -> Result<(Vec<Node>, SaidCache, PathCache), CliError> {
    let (cached_digests, cache_saids, nodes_to_build) = {
        // Load cache if exists
        let mut said_cache_path = directory.to_path_buf();
        said_cache_path.push(".oca-saids");
        let cache_saids = SaidCache::new(said_cache_path.clone());

        let mut cache_path = directory.to_path_buf();
        cache_path.push(".oca-bin");
        let cache_paths = PathCache::new(cache_path);

        match detect_changes(&nodes, &cache_paths) {
            Ok(nodes_to_update) => {
                let paths_to_rebuild = nodes_to_update
                    .iter()
                    .map(|node| node.path.to_str().unwrap())
                    .join("\n\t•");
                if !paths_to_rebuild.is_empty() {
                    println!(
                        "The following files will be rebuilt: \n\t• {}",
                        paths_to_rebuild
                    );
                };

                (cache_paths, cache_saids, nodes_to_update)
            }
            Err(CacheError::NoChanges) => {
                println!("Up to date");
                return Ok((vec![], cache_saids, cache_paths));
            }
            Err(e) => return Err(e.into()),
        }
    };

    // Handle build
    for node in nodes_to_build.iter() {
        build(
            facade.clone(),
            node,
            Some(&cache_saids),
            Some(&cached_digests),
        )?;
    }
    cache_saids.save()?;
    cached_digests.save()?;
    Ok((nodes_to_build, cache_saids, cached_digests))
}

pub fn handle_publish(
    facade: Arc<Mutex<Facade>>,
    remote_repo_url: Url,
    nodes: &[Node],
    said_cache: &SaidCache,
    path_cache: &PathCache,
) -> Result<(), CliError> {
    for node in nodes {
        let file_hash = if let Some(file_hash) = path_cache.get(&node.path)? {
            file_hash
        } else {
            let unparsed_file = fs::read_to_string(&node.path)
                .map_err(|e| CliError::ReadFileFailed(node.path.to_path_buf(), e))?;
            compute_hash(unparsed_file.trim())
        };
        match said_cache.get(&file_hash)? {
            Some(said) => {
                println!(
                    "Publishing SAID {} (name: {}) to {}",
                    &said, &node.refn, &remote_repo_url
                );
                publish_oca_file_for(facade.clone(), said, &None, remote_repo_url.clone())?;
            }
            // Should never happen. All saids should be in cache, because it was build before.
            None => return Err(CliError::FileUpdated(node.path.to_path_buf())),
        }
    }
    Ok(())
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

    let cache = crate::cache::Cache::new(tmp_dir.path().to_path_buf());

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
