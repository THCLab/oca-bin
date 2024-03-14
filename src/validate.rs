use std::fs;

use oca_rs::Facade;

use crate::{
    dependency_graph::{DependencyGraph, Node},
    error::CliError,
};

pub fn validate_directory(
    facade: &Facade,
    graph: &mut DependencyGraph,
) -> Result<(Vec<Node>, Vec<CliError>), CliError> {
    let sorted_graph = graph.sort().unwrap();
    info!("Sorted: {:?}", sorted_graph);
    let (oks, errs): (Vec<_>, Vec<_>) = sorted_graph
        .into_iter()
        .map(|node| {
            info!("Processing: {}", node.refn);
            let path = graph.oca_file_path(&node.refn)?;
            let unparsed_file = fs::read_to_string(path).map_err(CliError::ReadFileFailed)?;
            match Facade::validate_ocafile(facade.storage(), unparsed_file, graph) {
                Ok(_) => Ok(node),
                Err(e) => Err(CliError::ValidationError(node.path.clone(), e)),
            }
        })
        .partition(Result::is_ok);
    let oks = oks.into_iter().map(|n| n.unwrap()).collect();
    let errs = errs.into_iter().map(|e| e.unwrap_err()).collect();
    Ok((oks, errs))
}
