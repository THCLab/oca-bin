use std::{
    borrow::Borrow, fs, path::Path, sync::{Arc, Mutex}, thread, time::Duration
};

use oca_rs::{data_storage::{DataStorage, SledDataStorage}, Facade};

use crate::{
    dependency_graph::{DependencyGraph, MutableGraph, Node},
    error::CliError,
    get_oca_facade,
    utils::load_ocafiles_all,
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

#[test]
fn test() {
    let dir = Path::new("../example");
    let dir_oca = Path::new("../exampleoca");

    let all_oca_files = load_ocafiles_all(None, Some(&dir.to_path_buf())).unwrap();
    // println!("{:?}", all_oca_files);
    let (facade, storage) = get_oca_facade(dir_oca.to_path_buf());
    let mut graph = MutableGraph::new(dir, all_oca_files);

    thread::spawn(move || {

        let res = validate_directory(&storage, &mut graph.clone()).unwrap();
    });
    loop {
        {
            thread::sleep(Duration::from_secs(5));
            println!("waiting");
        }
    }

    // let mut main_i = Arc::new(Mutex::new("".to_string()));
    // let i = main_i.clone();
    // thread::spawn(move || {
    //     loop {
    //     thread::sleep(Duration::from_secs(3));
    //     {
    //         let mut tmp_i = i.lock().unwrap();
    //         tmp_i.push_str("test");
    //         println!("{}", tmp_i);
    //     }}
    // });
    // loop {
    //     {
    //         let i = main_i.clone();
    //         let mut tmp_i = i.lock().unwrap();
    //         println!("{}", tmp_i);
    //     }
    // }
}
