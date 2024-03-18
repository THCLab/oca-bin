
use std::fs;

use oca_rs::{
    data_storage::SledDataStorage,
    Facade,
};

use crate::{
    dependency_graph::{MutableGraph, Node},
    error::CliError,
};

pub fn validate_directory(
    facade: &SledDataStorage,
    graph: &mut MutableGraph,
) -> Result<(Vec<Node>, Vec<CliError>), CliError> {
    let sorted_graph = graph.sort().unwrap();
    info!("Sorted: {:?}", sorted_graph);
    let (oks, errs): (Vec<_>, Vec<_>) = sorted_graph
        .into_iter()
        .map(|node| {
            // println!("Processing: {}", node.refn);
            let path = graph.oca_file_path(&node.refn)?;
            let unparsed_file = fs::read_to_string(path).map_err(CliError::ReadFileFailed)?;
            match Facade::validate_ocafile(facade, unparsed_file, graph) {
                Ok(_) => Ok(node),
                Err(e) => Err(CliError::GrammarError(node.path.clone(), e)),
            }
        })
        .partition(Result::is_ok);
    let oks = oks.into_iter().map(|n| n.unwrap()).collect();
    let errs = errs.into_iter().map(|e| e.unwrap_err()).collect();
    Ok((oks, errs))
}