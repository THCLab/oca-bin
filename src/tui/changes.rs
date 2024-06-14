use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

use ratatui::{
    buffer::Buffer,
    layout::Rect,
    widgets::{Block, Paragraph, Widget},
};
use said::{
    derivation::{HashFunction, HashFunctionCode},
    SelfAddressingIdentifier,
};

use crate::{
    dependency_graph::{parse_name, MutableGraph},
    utils::visit_dirs_recursive,
};

pub enum Change {
    Delete(PathBuf),
    Modified(PathBuf),
    New(PathBuf),
}

pub struct SavedData {
    /// map between path and ocafile hash
    /// updated when build
    saved: HashMap<PathBuf, SelfAddressingIdentifier>,
    graph: MutableGraph,
    dir: PathBuf,
}

impl SavedData {
    pub fn new(graph: MutableGraph, dir: &Path) -> Self {
        Self {
            saved: HashMap::new(),
            graph,
            dir: dir.to_path_buf(),
        }
    }
    pub fn load(&mut self) {
        let paths = visit_dirs_recursive(&self.dir).unwrap();
        for path in paths {
            let contents =
                fs::read_to_string(&path).expect("Should have been able to read the file");
            let current_said =
                HashFunction::from(HashFunctionCode::SHA2_256).derive(contents.as_bytes());
            info!(
                "Insering change in file: {} and said: {}",
                &path.to_str().unwrap(),
                &current_said
            );
            self.saved.insert(path, current_said);
        }
    }

    pub fn changes(&self) -> Vec<Change> {
        self.saved
            .iter()
            .filter_map(|(path, said)| match fs::read_to_string(&path) {
                Ok(contents) => {
                    let current_said =
                        HashFunction::from(HashFunctionCode::SHA2_256).derive(contents.as_bytes());
                    if current_said.eq(said) {
                        None
                    } else {
                        Some(Change::Modified(path.clone()))
                    }
                }
                Err(_) => Some(Change::Delete(path.clone())),
            })
            .collect()
    }

    pub fn show_changes(&self) -> String {
        let stats = self.changes();
        let out = stats
            .into_iter()
            .map(|change| match change {
                Change::Delete(path) => ["DELETED", path.to_str().unwrap()].join(": "),
                Change::Modified(path) => {
                    let (name, _) = parse_name(&path).unwrap();
                    let deps = self.graph.format_ancestor(name.as_ref().unwrap()).unwrap();
                    let change_line = ["MODIFIED", path.to_str().unwrap()].join(": ");
                    [change_line, deps].join("\n")
                }
                Change::New(path) => ["NEW", path.to_str().unwrap()].join(": "),
            })
            .collect::<Vec<_>>()
            .join("\n");
        out
    }
}

pub struct ChangesWindow {
    // changes: Arc<Mutex<Changes>>
    changes: Arc<Mutex<SavedData>>,
}

// pub struct Changes {
// 	repo: Repository,
// 	graph: MutableGraph,
// 	base: PathBuf,
// }

impl ChangesWindow {
    pub fn new<P: AsRef<Path>>(path: P, graph: MutableGraph) -> Self {
        let mut sd = SavedData::new(graph, path.as_ref());
        sd.load();
        Self {
            changes: Arc::new(Mutex::new(sd)),
        }
    }

    pub fn changes(&self) -> Arc<Mutex<SavedData>> {
        self.changes.clone()
    }

    fn changes_locked(&self) -> String {
        let window = self.changes.lock().unwrap();
        window.show_changes()
    }

    pub fn update(&self) {
        let mut window = self.changes.lock().unwrap();
        window.load();
    }

    pub fn render(&mut self, area: Rect, buf: &mut Buffer) {
        Paragraph::new(self.changes_locked())
            .block(Block::bordered().title("Changes"))
            .render(area, buf)
    }
}
