use std::{
    collections::HashMap,
    path::Path,
    sync::{Arc, Mutex},
};

use indexmap::IndexMap;
use itertools::Itertools;
use oca_ast::ast::{NestedAttrType, RefValue};
use oca_rs::Facade;
use ratatui::{
    style::{Color, Modifier, Style},
    text::{Line, Span},
};
use said::SelfAddressingIdentifier;
use tui_tree_widget::TreeItem;

use crate::{
    dependency_graph::{parse_node, DependencyGraph, GraphError, MutableGraph, Node},
    utils::visit_current_dir,
};

use super::{
    bundle_info::{BundleInfo, Status},
    bundle_list::{BundleListError, Indexer},
    get_oca_bundle, get_oca_bundle_by_said,
};

pub struct Items {
    tree_elements: HashMap<String, TreeItem<'static, String>>,
    indexes: IndexMap<String, SelfAddressingIdentifier>,
    nodes: Vec<Result<BundleInfo, BundleListError>>,
    indexer: Indexer,
}

impl Items {
    pub fn new() -> Self {
        Items {
            indexes: IndexMap::new(),
            nodes: Vec::new(),
            indexer: Indexer::new(),
            tree_elements: HashMap::new(),
        }
    }

    pub fn items(&self) -> Vec<TreeItem<'static, String>> {
        self.tree_elements
            .values()
            .map(|el| el.to_owned())
            .collect_vec()
    }

    pub fn new_items<I: IntoIterator<Item = Node>>(
        to_show: I,
        facade: Arc<Mutex<Facade>>,
        graph: &DependencyGraph,
    ) -> Self {
        let mut items = Items::new();
        items.build(to_show, facade.clone(), graph);
        items.to_tree_items(facade, graph);
        items
    }

    fn rebuild<I: IntoIterator<Item = Node>>(
        &mut self,
        to_show: I,
        facade: Arc<Mutex<Facade>>,
        graph: &DependencyGraph,
    ) {
        // let mut nodes = self.nodes.lock().unwrap();
        self.nodes.clear();
        self.indexes.clear();
        self.indexer = Indexer::new();
        self.build(to_show, facade, graph)
    }

    fn build<I: IntoIterator<Item = Node>>(
        &mut self,
        to_show: I,
        facade: Arc<Mutex<Facade>>,
        graph: &DependencyGraph,
    ) {
        to_show.into_iter().for_each(|node| {
            self.nodes
                .push(bundle_info_from_refn(&node.refn, graph, facade.clone()))
        });
    }

    fn to_tree_items(&mut self, facade: Arc<Mutex<Facade>>, graph: &DependencyGraph) {
        // let nodes = self.nodes.lock().unwrap();
        self.nodes.iter().for_each(|item| {
            match item {
                Ok(bundle) => {
                    let color = match bundle.status {
                        Status::Selected => Color::Green,
                        Status::Unselected => Color::Blue,
                    };
                    let attributes = &bundle.oca_bundle.capture_base.attributes;
                    let tree_items = attributes
                        .into_iter()
                        .map(|(key, attr)| {
                            to_tree_item(key.to_owned(), attr, &self.indexer, facade.clone(), graph)
                        })
                        .collect::<Vec<_>>();
                    let line = Span::styled(
                        bundle.refn.clone(),
                        Style::default().fg(color).add_modifier(Modifier::ITALIC),
                    );
                    let index = self.indexer.current();
                    // let mut indexes = self.indexes.lock().unwrap();
                    let tree_item = TreeItem::new(index.clone(), line, tree_items).unwrap();
                    self.tree_elements.insert(index.clone(), tree_item);
                    self.indexes.insert(
                        index.clone(),
                        bundle.oca_bundle.said.as_ref().unwrap().clone(),
                    );
                }
                Err(err) => {
                    let line = Span::styled(
                        format!("! {}", err),
                        Style::default()
                            .fg(Color::Red)
                            .add_modifier(Modifier::ITALIC),
                    );
                    let index = self.indexer.current();
                    let tree_item = TreeItem::new_leaf(index.clone(), line);
                    self.tree_elements.insert(index.clone(), tree_item);
                }
            }
        });
    }

    pub fn update_state(&mut self, i: &str, facade: Arc<Mutex<Facade>>, graph: &DependencyGraph) {
        info!("Updating index: {}", i);
        let said = self.indexes.get(i).map(|s| s.to_owned());
        info!("Updating said: {:?}", &said);
        let _ = self
            .nodes
            .iter_mut()
            .filter_map(|item| match item {
                Ok(item) => {
                    if item.oca_bundle.said.eq(&said) {
                        item.change_state();

                        let style = match item.status {
                            Status::Selected => Style::default()
                                .bg(Color::Green)
                                .fg(Color::White)
                                .add_modifier(Modifier::ITALIC),
                            Status::Unselected => Style::default(),
                        };
                        let tree_item = self.tree_elements.get(i).unwrap().clone();
                        let tree_item = tree_item.style(style);
                        self.tree_elements.insert(i.to_string(), tree_item);
                        Some(())
                    } else {
                        None
                    }
                }
                Err(_er) => None,
            })
            .collect::<Vec<_>>();
    }

    pub fn bundle_info(&self, k: &str) -> Option<BundleInfo> {
        // let indexes = self.indexes.lock().unwrap();
        let said = self.indexes.get(k).map(|c| c.clone());
        self.nodes.iter().find_map(|node| match node {
            Ok(bi) => {
                if bi.oca_bundle.said.eq(&said) {
                    Some(bi.clone())
                } else {
                    None
                }
            }
            Err(_) => None,
        })
    }
}

fn bundle_info_from_refn(
    refn: &str,
    graph: &DependencyGraph,
    facade: Arc<Mutex<Facade>>,
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

pub fn rebuild_items(
    items: Arc<Mutex<Items>>,
    to_show_dir: &Path,
    facade: Arc<Mutex<Facade>>,
    graph: MutableGraph,
) {
    let graph = graph.graph.lock().unwrap();
    let to_show_list = visit_current_dir(to_show_dir)
        .unwrap()
        .into_iter()
        // Files without refn are ignored
        .filter_map(|of| parse_node(to_show_dir, &of).ok().map(|v| v.0));
    let mut items = items.lock().unwrap();
    items.rebuild(to_show_list, facade, &graph);
}

fn to_tree_item<'a>(
    key: String,
    attr: &NestedAttrType,
    i: &Indexer,
    facade: Arc<Mutex<Facade>>,
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
    facade: Arc<Mutex<Facade>>,
    graph: &DependencyGraph,
    i: &Indexer,
) -> TreeItem<'a, String> {
    let (ocafile_path, oca_bundle) = match reference {
        RefValue::Said(said) => {
            let (refn, bundle) = get_oca_bundle_by_said(said, facade.clone())
                .unwrap_or_else(|| panic!("Unknown said: {}", &said));
            (graph.oca_file_path(&refn), bundle)
        }
        RefValue::Name(refn) => {
            let bundle = get_oca_bundle(refn, facade.clone())
                .unwrap_or_else(|| panic!("Unknown refn: {}", &refn));
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
                .map(|(key, attr)| to_tree_item(key, &attr, i, facade.clone(), graph))
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
    facade: Arc<Mutex<Facade>>,
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
