use std::{fs, sync::{Arc, Mutex}};

use oca_rs::{data_storage::SledDataStorage, Facade};

use crate::{
    dependency_graph::{parse_name, MutableGraph, Node},
    error::CliError,
    tui::{bundle_info::BundleInfo, output_window::message_list::{Message, MessageList}},
};

pub fn validate_directory(
    facade: &SledDataStorage,
    graph: &mut MutableGraph,
    selected_bundle: Option<&BundleInfo>,
) -> Result<(Vec<Node>, Vec<CliError>), CliError> {
    let dependent_nodes = match selected_bundle {
        Some(dir) => graph.get_dependent_nodes(&dir.refn)?,
        None => graph.sort()?,
    };
    let (oks, errs): (Vec<_>, Vec<_>) = dependent_nodes
        .into_iter()
        .map(|node| {
            let path = graph.oca_file_path(&node.refn)?;
            let unparsed_file = fs::read_to_string(&path).map_err(CliError::ReadFileFailed)?;
            let (name, _) = parse_name(&path).unwrap();
            if let Some(name) = name {
                if name.ne(&node.refn) {
                    // Name changed. Update refn in graph
                    graph.update_refn(&node.refn, name)?
                }
            }
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

pub fn build(facade: Arc<Mutex<Facade>>, graph: &MutableGraph, infos: Arc<Mutex<MessageList>>) -> Result<(), CliError> {
    let sorted_graph = graph.sort().unwrap();
    info!("Sorted: {:?}", sorted_graph);
    for node in sorted_graph {
        info!("\nProcessing: {}", node.refn);
        match graph.oca_file_path(&node.refn) {
            Ok(path) => {
                let mut f = facade.lock().unwrap();
                let unparsed_file = fs::read_to_string(path).map_err(CliError::ReadFileFailed)?;
                let oca_bundle = f
                    .build_from_ocafile(unparsed_file)
                    .map_err(CliError::OcaBundleError)?;
                let refs = f.fetch_all_refs().unwrap();
                let schema_name = refs
                    .iter()
                    .find(|&(_, v)| *v == oca_bundle.said.clone().unwrap().to_string());
                let msg = if let Some((refs, _)) = schema_name {
                    format!(
                        "OCA bundle created in local repository with SAID: {} and name: {}",
                        oca_bundle.said.unwrap(),
                        refs
                    )
                } else {
                    format!(
                        "OCA bundle created in local repository with SAID: {:?}",
                        oca_bundle.said.unwrap()
                    )
                };
                let mut i = infos.lock().unwrap();
                i.append(Message::Info(msg))
            }
            _ => {
                println!("RefN not found in graph: {}", node.refn);
            }
        }
    }
    Ok(())
}
