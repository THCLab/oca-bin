use std::{collections::HashMap, fs, path::PathBuf};

use base64::{prelude::BASE64_STANDARD, Engine};
use sha2::{Digest, Sha256};

use crate::{
    dependency_graph::{parse_node, MutableGraph, Node},
    error::CliError,
};

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
    assert_eq!(
        deps.iter().map(|dep| dep.refn.clone()).collect::<Vec<_>>(),
        vec!["second", "third", "fourth", "fifth"]
    );

    let deps =
        join_with_dependencies(&graph, vec![&fourth_path, &second_path], false).collect::<Vec<_>>();
    assert_eq!(
        deps.iter().map(|dep| dep.refn.clone()).collect::<Vec<_>>(),
        vec!["third", "fifth"]
    );

    Ok(())
}
