use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use oca_ast::ast::{NestedAttrType, RefValue};
use oca_rs::Facade;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Scrollbar, ScrollbarOrientation, StatefulWidget};

use thiserror::Error;
use tui_tree_widget::{Tree, TreeItem, TreeState};

use crate::dependency_graph::{DependencyGraph, GraphError, MutableGraph, Node};

use super::bundle_info::{BundleInfo, Status};
use super::{get_oca_bundle, get_oca_bundle_by_said};

#[derive(Error, Debug)]
pub enum BundleListError {
    #[error("All references are unknown")]
    AllRefnUnknown,
    #[error(transparent)]
    GraphError(#[from] GraphError),
}

pub struct BundleList {
    pub state: TreeState<String>,
    pub items: Arc<Mutex<Items>>,
}

pub struct Items {
    pub items: Vec<TreeItem<'static, String>>,
    nodes: HashMap<String, BundleInfo>,
}

impl Items {
    pub fn new() -> Self {
        Items {
            items: vec![],
            nodes: HashMap::new(),
        }
    }

    pub fn result_to_tree_item(
        &mut self,
        ob: Result<BundleInfo, BundleListError>,
        i: &Indexer,
        facade: &Facade,
        graph: &DependencyGraph,
    ) {
        match ob {
            Ok(bundle) => {
                let attributes = &bundle.oca_bundle.capture_base.attributes;
                let tree_items = attributes
                    .into_iter()
                    .map(|(key, attr)| to_tree_item(key.to_owned(), &attr, i, facade, graph))
                    .collect::<Vec<_>>();
                let current_i = i.current();
                self.items.push(
                    TreeItem::new(current_i.clone(), bundle.refn.clone(), tree_items).unwrap(),
                );
                self.nodes.insert(current_i.clone(), bundle);
            }
            Err(err) => {
                let line = Span::styled(
                    format!("! {}", err),
                    Style::default()
                        .fg(Color::Red)
                        .add_modifier(Modifier::ITALIC),
                );
                self.items.push(TreeItem::new_leaf(i.current(), line));
            }
        }
    }

    pub fn update_nodes<I: IntoIterator<Item = Node>>(
        &mut self,
        to_show: I,
        facade: &Facade,
        graph: &DependencyGraph,
    ) {
        let i = Indexer::new();
        to_show
            .into_iter()
            .map(|node| bundle_info_from_refn(&node.refn, graph, facade))
            .for_each(|dep| self.result_to_tree_item(dep, &i, facade, graph));
    }

    pub fn bundle_info(&self, k: &str) -> Option<BundleInfo> {
        self.nodes.get(k).map(|b| b.clone())
    }
}

pub fn rebuild_items<I: IntoIterator<Item = Node> + Clone>(
    items: Arc<Mutex<Items>>,
    to_show: I,
    facade: Arc<Mutex<Facade>>,
    graph: MutableGraph,
) {
    let mut items = items.lock().unwrap();
    let facade = facade.lock().unwrap();
    let graph = graph.graph.lock().unwrap();
    items.nodes = HashMap::new();
    items.items = vec![];
    items.update_nodes(to_show, &facade, &graph);
}

pub struct Indexer(Mutex<u32>);
impl Indexer {
    pub fn new() -> Self {
        Self(Mutex::new(0))
    }

    pub fn current(&self) -> String {
        let mut s = self.0.lock().unwrap();
        *s += 1;
        s.to_string()
    }
}

impl BundleList {
    pub fn from_nodes<I: IntoIterator<Item = Node>>(
        to_show: I,
        facade: &Facade,
        graph: &DependencyGraph,
    ) -> Result<Self, BundleListError> {
        let mut items = Items::new();
        items.update_nodes(to_show, facade, graph);
        Ok(Self {
            state: TreeState::default(),
            items: Arc::new(Mutex::new(items)),
        })
    }

    pub fn items(&self) -> Vec<TreeItem<'static, String>> {
        let items = self.items.lock().unwrap();
        items.items.clone()
    }

    pub fn selected_oca_bundle(&self) -> Option<BundleInfo> {
        let items = self.items.lock().unwrap();
        self.state
            .selected()
            .get(0)
            .and_then(|i| items.bundle_info(i))
    }

    pub fn render(&mut self, area: Rect, buf: &mut Buffer) {
        let widget = Tree::new(self.items())
            .expect("all item identifiers are unique")
            .block(Block::bordered().title("OCA Bundles"))
            .experimental_scrollbar(Some(
                Scrollbar::new(ScrollbarOrientation::VerticalRight)
                    .begin_symbol(None)
                    .track_symbol(None)
                    .end_symbol(None),
            ))
            .highlight_style(
                Style::new()
                    .fg(Color::Black)
                    .bg(Color::LightGreen)
                    .add_modifier(Modifier::BOLD),
            )
            .highlight_symbol("> ");

        StatefulWidget::render(widget, area, buf, &mut self.state);
    }
}

fn bundle_info_from_refn(
    refn: &str,
    graph: &DependencyGraph,
    facade: &Facade,
) -> Result<BundleInfo, BundleListError> {
    let deps = graph.neighbors(refn)?;
    let oca_bundle = get_oca_bundle(refn, facade);
    match oca_bundle {
        Some(oca_bundle) => Ok(BundleInfo {
            refn: refn.to_string(),
            dependencies: deps,
            status: Status::Unselected,
            oca_bundle,
        }),
        None => Err(GraphError::UnknownRefn(refn.to_string()).into()),
    }
}

fn to_tree_item<'a>(
    key: String,
    attr: &NestedAttrType,
    i: &Indexer,
    facade: &Facade,
    graph: &DependencyGraph,
) -> TreeItem<'a, String> {
    match attr {
        NestedAttrType::Reference(reference) => {
            handle_reference_type(format!("{}: Reference", key), reference, facade, graph, i)
        }
        NestedAttrType::Value(attr) => {
            TreeItem::new_leaf(i.current(), format!("{}: {}", key, attr))
        }
        NestedAttrType::Array(arr_type) => handle_arr_type(key, arr_type, facade, graph, i),
        NestedAttrType::Null => todo!(),
    }
}

fn handle_reference_type<'a>(
    line: String,
    reference: &RefValue,
    facade: &Facade,
    graph: &DependencyGraph,
    i: &Indexer,
) -> TreeItem<'a, String> {
    let (ocafile_path, oca_bundle) = match reference {
        RefValue::Said(said) => {
            let (refn, bundle) = get_oca_bundle_by_said(said, facade)
                .unwrap_or_else(|| panic!("Unknown said: {}", &said));
            (graph.oca_file_path(&refn), bundle)
        }
        RefValue::Name(refn) => {
            let bundle =
                get_oca_bundle(refn, facade).unwrap_or_else(|| panic!("Unknown refn: {}", &refn));
            (graph.oca_file_path(refn), bundle)
        }
    };
    match ocafile_path {
        Ok(ocafile_path) => {
            let line = vec![
                Span::styled(line, Style::default()),
                Span::styled(
                    format!("      • {}", ocafile_path.to_str().unwrap()),
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::ITALIC),
                ),
            ];
            let children: Vec<TreeItem<'a, String>> = oca_bundle
                .capture_base
                .attributes
                .into_iter()
                .map(|(key, attr)| to_tree_item(key, &attr, i, facade, graph))
                .collect();
            TreeItem::new(i.current(), Line::from(line), children).unwrap()
        }
        Err(e) => {
            let line = vec![
                Span::styled(line, Style::default().fg(Color::Red)),
                Span::styled(
                    format!("      ! {}", e),
                    Style::default()
                        .fg(Color::Red)
                        .add_modifier(Modifier::ITALIC),
                ),
            ];
            TreeItem::new_leaf(i.current(), Line::from(line))
        }
    }
}

fn handle_arr_type<'a>(
    key: String,
    arr_type: &NestedAttrType,
    facade: &Facade,
    graph: &DependencyGraph,
    i: &Indexer,
) -> TreeItem<'a, String> {
    match arr_type {
        NestedAttrType::Reference(reference) => handle_reference_type(
            format!("{}: Array[Reference]", key),
            reference,
            facade,
            graph,
            i,
        ),
        NestedAttrType::Value(value) => {
            TreeItem::new_leaf(i.current(), format!("{}: Array[{}]", key, value))
        }
        NestedAttrType::Array(arr_t) => handle_arr_type(key, arr_t, facade, graph, i),
        NestedAttrType::Null => todo!(),
    }
}
