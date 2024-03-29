use std::{
    fs,
    sync::{Arc, Mutex},
};

use oca_rs::Facade;

use crate::{
    dependency_graph::{parse_name, MutableGraph},
    error::CliError,
    tui::{
        bundle_info::BundleInfo,
        output_window::message_list::{Message, MessageList},
    },
};

pub fn validate_directory(
    facade: Arc<Mutex<Facade>>,
    graph: &mut MutableGraph,
    selected_bundle: Option<&BundleInfo>,
) -> Result<Vec<CliError>, CliError> {
    let dependent_nodes = match selected_bundle {
        Some(dir) => graph.get_dependent_nodes(&dir.refn)?,
        None => graph.sort()?,
    };
    let errs = dependent_nodes
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
            let facade = facade.lock().unwrap();
            match facade.validate_ocafile_with_external_references(unparsed_file, graph) {
                Ok(_) => Ok(node),
                Err(e) => Err(CliError::GrammarError(node.path.clone(), e)),
            }
        })
        .filter_map(|e| if let Err(e) = e { Some(e) } else { None })
        .collect::<Vec<_>>();

    Ok(errs)
}

pub fn build(
    selected_bundle: Option<&BundleInfo>,
    facade: Arc<Mutex<Facade>>,
    graph: &mut MutableGraph,
    infos: Arc<Mutex<MessageList>>,
) -> Result<(), Vec<CliError>> {
    let dependent_nodes = match selected_bundle {
        Some(dir) => graph.get_dependent_nodes(&dir.refn).unwrap(),
        None => graph.sort().unwrap(),
    };
    // Validate nodes before updating local oca database.
    // Warning. This updates names in `refn` -> `said` mapping.
    let (oks, errs): (Vec<_>, _) = dependent_nodes
        .iter()
        .map(|node| {
            let path = graph.oca_file_path(&node.refn).unwrap();
            let unparsed_file = fs::read_to_string(&path)
                .map_err(CliError::ReadFileFailed)
                .unwrap();
            let (name, _) = parse_name(&path).unwrap();
            if let Some(name) = name {
                if name.ne(&node.refn) {
                    // Name changed. Update refn in graph
                    graph.update_refn(&node.refn, name).unwrap();
                }
            }
            let mut f = facade.lock().unwrap();
            f.validate_ocafile(unparsed_file)
                .map(|ok| (path.clone(), ok))
                .map_err(|b| (path.clone(), b))
        })
        .partition(Result::is_ok);

    if !errs.is_empty() {
        let output = errs
            .into_iter()
            .map(|e| {
                let (path, e) = e.unwrap_err();
                CliError::GrammarError(path.clone(), e)
            })
            .collect();
        return Err(output);
    }

    // If no validation errors, update local oca database.
    let (_building_oks, building_errs): (Vec<_>, Vec<_>) = oks
        .into_iter()
        .map(|oca_build| {
            let (path, oca_build) = oca_build.as_ref().unwrap();
            let mut f = facade.lock().unwrap();
            match f.build(oca_build) {
                Ok(oca_bundle) => {
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
                            "OCA bundle created in local repository with SAID: {}",
                            oca_bundle.said.unwrap()
                        )
                    };
                    let mut i = infos.lock().unwrap();
                    i.append(Message::Info(msg));
                    Ok(())
                }
                Err(e) => Err(CliError::BuildingError(path.clone(), e)),
            }
        })
        .partition(Result::is_ok);
    if building_errs.is_empty() {
        Ok(())
    } else {
        Err(building_errs.into_iter().map(Result::unwrap_err).collect())
    }
}
