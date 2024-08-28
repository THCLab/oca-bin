use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

use petgraph::{graph::NodeIndex, graphmap::GraphMap, visit::EdgeRef, Directed};
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    widgets::{Block, Scrollbar, ScrollbarOrientation, StatefulWidget},
};
use said::{
    derivation::{HashFunction, HashFunctionCode},
    SelfAddressingIdentifier,
};
use tui_tree_widget::{Tree, TreeItem, TreeState};

use crate::{
    dependency_graph::{parse_name, DependencyGraph, GraphError, MutableGraph},
    utils::visit_dirs_recursive,
};

use super::bundle_list::Indexer;

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
            .filter_map(|(path, said)| match fs::read_to_string(path) {
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

    pub fn show_changes(&self) -> Vec<TreeItem<'static, String>> {
        let stats = self.changes();
        let index = Indexer::new();
        let out = stats
            .into_iter()
            .map(|change| match change {
                Change::Delete(path) => TreeItem::new_leaf(
                    index.current(),
                    ["DELETED", path.to_str().unwrap()].join(": "),
                ),
                Change::Modified(path) => {
                    let (name, _) = parse_name(&path).unwrap();
                    let change_line = ["MODIFIED", path.to_str().unwrap()].join(": ");
                    if name.is_none() {
                        return TreeItem::new_leaf(index.current(), change_line);
                    };
                    match format_ancestor_tree(name.as_ref().unwrap(), &self.graph) {
                        Ok(deps) => TreeItem::new(index.current(), change_line, deps).unwrap(),
                        Err(_) => TreeItem::new_leaf(index.current(), change_line),
                    }
                }
                Change::New(path) => {
                    TreeItem::new_leaf(index.current(), ["NEW", path.to_str().unwrap()].join(": "))
                }
            })
            .collect::<Vec<_>>();
        out
    }
}

pub struct ChangesWindow {
    pub state: TreeState<String>,
    changes: Arc<Mutex<SavedData>>,
}

impl ChangesWindow {
    pub fn new<P: AsRef<Path>>(path: P, graph: MutableGraph) -> Self {
        let mut sd = SavedData::new(graph, path.as_ref());
        sd.load();
        Self {
            state: TreeState::default(),
            changes: Arc::new(Mutex::new(sd)),
        }
    }

    pub fn changes(&self) -> Arc<Mutex<SavedData>> {
        self.changes.clone()
    }

    // fn changes_locked(&self) -> String {
    //     let window = self.changes.lock().unwrap();
    //     window.show_changes()
    // }

    // pub fn render(&mut self, area: Rect, buf: &mut Buffer) {
    //     Paragraph::new(self.changes_locked())
    //         .block(Block::bordered().title("Changes"))
    //         .render(area, buf)
    // }

    pub fn items(&self) -> Vec<TreeItem<'static, String>> {
        let changes = self.changes.lock().unwrap();
        changes.show_changes()
    }

    pub fn render(&mut self, area: Rect, buf: &mut Buffer) {
        let widget = Tree::new(self.items())
            .expect("all item identifiers are unique")
            .block(Block::bordered().title("Changes"))
            .experimental_scrollbar(Some(
                Scrollbar::new(ScrollbarOrientation::VerticalRight)
                    .begin_symbol(None)
                    .track_symbol(None)
                    .end_symbol(None),
            ))
            // .highlight_style(
            //     Style::new()
            //         .fg(Color::Black)
            //         .bg(Color::LightGreen)
            //         .add_modifier(Modifier::BOLD),
            // )
            .highlight_symbol("> ");

        StatefulWidget::render(widget, area, buf, &mut self.state);
    }
}

fn changes_tree(
    start_node: NodeIndex,
    ancestor_graph: &GraphMap<NodeIndex, (), Directed>,
    full_graph: &DependencyGraph,
    i: &Indexer,
) -> Vec<TreeItem<'static, String>> {
    let anc = ancestor_graph
        .edges_directed(start_node, petgraph::Direction::Outgoing)
        .map(|e| e.target())
        .collect::<Vec<_>>();
    anc.into_iter()
        .map(|index: NodeIndex| {
            if !ancestor_graph
                .edges_directed(index, petgraph::Direction::Outgoing)
                .map(|e| e.target())
                .collect::<Vec<_>>()
                .is_empty()
            {
                let path = full_graph.node(index);
                let children = changes_tree(index, ancestor_graph, full_graph, i);
                TreeItem::new(
                    i.current(),
                    path.path.to_str().unwrap().to_string(),
                    children,
                )
                .unwrap()
            } else {
                let p = full_graph.node(index);
                TreeItem::new_leaf(i.current(), p.path.to_str().unwrap().to_string())
            }
        })
        .collect()
}

pub fn format_ancestor_tree(
    refn: &str,
    graph: &MutableGraph,
) -> Result<Vec<TreeItem<'static, String>>, GraphError> {
    let full_graph = graph.graph.lock().unwrap();
    let start_node = full_graph.get_index(refn)?;
    let ancestor_graph = MutableGraph::ancestor_graph(start_node, &full_graph)?;
    let i = Indexer::new();
    Ok(changes_tree(start_node, &ancestor_graph, &full_graph, &i))
}
