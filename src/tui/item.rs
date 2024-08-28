use std::{
    collections::HashMap,
    fmt::Display,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

use itertools::Itertools;
use oca_ast_semantics::ast::{NestedAttrType, RefValue};
use oca_rs::Facade;
use ratatui::{
    style::{Color, Modifier, Style},
    text::{Line, Span},
};
use tui_tree_widget::TreeItem;

use crate::{
    dependency_graph::{
        parse_node, DependencyGraph, GraphError, MutableGraph, Node, NodeParsingError,
    },
    error::CliError,
    utils::visit_current_dir,
};

use super::{
    bundle_info::{BundleInfo, Status},
    bundle_list::{BundleListError, Indexer},
    get_oca_bundle, get_oca_bundle_by_said,
};

#[derive(Debug)]
pub struct ListElement {
    bundle: Element,
    status: Status,
}

#[derive(Clone, Debug)]
pub enum Element {
    Ok(ElementOk),
    Error(ElementError),
}

#[derive(Clone, Debug)]
pub struct GenericElement<T> {
    index: Option<String>,
    pub err: T,
    path: PathBuf,
}

pub type ElementError = GenericElement<BundleListError>;
pub type ElementOk = GenericElement<BundleInfo>;

impl<T: Display> GenericElement<T> {
    pub fn new(t: T, path: PathBuf) -> Self {
        Self {
            index: None,
            err: t,
            path,
        }
    }

    pub fn get(&self) -> &T {
        &self.err
    }

    pub fn to_str(&self) -> String {
        self.err.to_string()
    }

    pub fn index(&self) -> Option<String> {
        self.index.clone()
    }

    pub fn update_idx(&mut self, index: String) {
        self.index = Some(index)
    }

    pub fn path(&self) -> &Path {
        self.path.as_path()
    }
}

impl Element {
    pub fn path(&self) -> &Path {
        match self {
            Element::Ok(p) => p.path(),
            Element::Error(e) => e.path(),
        }
    }
}

impl ListElement {
    pub fn new_bundle_info(bi: BundleInfo, path: PathBuf) -> Self {
        Self {
            bundle: Element::Ok(ElementOk::new(bi, path)),
            status: Status::Unselected,
        }
    }

    pub fn new_error(bi: BundleListError, path: PathBuf) -> Self {
        Self {
            bundle: Element::Error(ElementError {
                index: None,
                err: bi,
                path,
            }),
            status: Status::Unselected,
        }
    }

    pub fn _update_index(&mut self, index: String) {
        match &mut self.bundle {
            Element::Ok(ok) => ok.update_idx(index),
            Element::Error(err) => err.update_idx(index),
        }
    }

    pub fn index(&self) -> Option<String> {
        match &self.bundle {
            Element::Ok(ok) => ok.index.clone(),
            Element::Error(err) => err.index.clone(),
        }
    }

    pub fn change_state(&mut self) {
        self.status = self.status.toggle()
    }

    fn list_item_from_refn(
        refn: &str,
        path: PathBuf,
        graph: &DependencyGraph,
        facade: Arc<Mutex<Facade>>,
    ) -> Result<Self, GraphError> {
        let oca_bundle = get_oca_bundle(refn, facade);
        match oca_bundle {
            Ok(oca_bundle) => {
                let deps = graph.neighbors(refn)?;
                Ok(Self::new_bundle_info(
                    BundleInfo {
                        refn: refn.to_string(),
                        dependencies: deps,
                        oca_bundle,
                    },
                    path,
                ))
            }
            Err(_) => Ok(Self::new_error(
                GraphError::UnknownRefn(refn.to_string()).into(),
                path,
            )),
        }
    }
}

pub struct Items {
    tree_elements: HashMap<String, TreeItem<'static, String>>,
    nodes: Vec<ListElement>,
    indexer: Indexer,
    currently_selected: Vec<String>,
}

impl Items {
    pub fn new() -> Self {
        Items {
            nodes: Vec::new(),
            indexer: Indexer::new(),
            tree_elements: HashMap::new(),
            currently_selected: Vec::new(),
        }
    }

    pub fn all_indexes(&self) -> Option<Vec<String>> {
        self.nodes.iter().map(|n| n.index()).collect()
    }

    pub fn select_all(&mut self) -> Vec<String> {
        self.nodes.iter_mut().for_each(|bi| {
            bi.status = Status::Selected;
        });
        let all_indexes: Vec<_> = self.all_indexes().unwrap();
        for i in &all_indexes {
            let tree_item = self.tree_elements.get(i).unwrap().clone();
            let tree_item = tree_item.style(Style::default().bg(Color::Green).fg(Color::White));
            self.tree_elements.insert(i.to_string(), tree_item);
        }
        self.currently_selected = all_indexes.clone();
        all_indexes
    }

    pub fn unselect_all(&mut self) {
        self.nodes.iter_mut().for_each(|bi| {
            bi.status = Status::Unselected;
        });
        for i in self.all_indexes().unwrap() {
            let tree_item = self.tree_elements.get(&i).unwrap().clone();
            let tree_item = tree_item.style(Style::default());
            self.tree_elements.insert(i.to_string(), tree_item);
        }
        self.currently_selected = vec![];
    }

    pub fn selected_bundles(&self) -> Vec<Element> {
        self.currently_selected
            .clone()
            .iter()
            .map(|i| self.element(i))
            .collect::<Option<_>>()
            .unwrap_or_default()
    }

    pub fn items(&self) -> Vec<TreeItem<'static, String>> {
        self.tree_elements
            .values()
            .map(|el| el.to_owned())
            .collect_vec()
    }

    pub fn new_items<I: IntoIterator<Item = Result<Node, NodeParsingError>>>(
        to_show: I,
        facade: Arc<Mutex<Facade>>,
        graph: &DependencyGraph,
    ) -> Self {
        let mut items = Items::new();
        items.build(to_show, facade.clone(), graph);
        items.to_tree_items(facade, graph);
        items
    }

    fn rebuild<I: IntoIterator<Item = Result<Node, NodeParsingError>>>(
        &mut self,
        to_show: I,
        facade: Arc<Mutex<Facade>>,
        graph: &DependencyGraph,
    ) {
        self.nodes.clear();
        self.indexer = Indexer::new();
        self.build(to_show, facade.clone(), graph);
        self.tree_elements.clear();
        self.currently_selected = vec![];
        self.to_tree_items(facade, graph);
    }

    fn build<I: IntoIterator<Item = Result<Node, NodeParsingError>>>(
        &mut self,
        to_show: I,
        facade: Arc<Mutex<Facade>>,
        graph: &DependencyGraph,
    ) {
        to_show.into_iter().for_each(|node| match node {
            Ok(node) => self.nodes.push(
                ListElement::list_item_from_refn(&node.refn, node.path, graph, facade.clone())
                    .unwrap(),
            ),
            Err(NodeParsingError::MissingRefn(path)) => self.nodes.push(ListElement::new_error(
                BundleListError::RefnMissing(path.clone()),
                path,
            )),
            Err(NodeParsingError::FileParsing(path))
            | Err(NodeParsingError::WrongCharacterRefn(_, path)) => {
                self.nodes.push(ListElement::new_error(
                    BundleListError::GraphError(GraphError::NodeParsingError(
                        NodeParsingError::FileParsing(path.clone()),
                    )),
                    path,
                ))
            }
        });
    }

    fn to_tree_items(&mut self, facade: Arc<Mutex<Facade>>, graph: &DependencyGraph) {
        self.nodes
            .iter_mut()
            .for_each(|item| match &mut item.bundle {
                Element::Ok(ref mut bundle_el) => {
                    let bundle = bundle_el.get();
                    let attributes = &bundle.oca_bundle.capture_base.attributes;
                    let tree_items = attributes
                        .into_iter()
                        .map(|(key, attr)| {
                            to_tree_item(key.to_owned(), attr, &self.indexer, facade.clone(), graph)
                        })
                        .collect::<Vec<_>>();
                    let line = Span::styled(bundle.refn.clone(), Style::default());
                    let index = self.indexer.current();
                    let tree_item = TreeItem::new(index.clone(), line, tree_items).unwrap();
                    self.tree_elements.insert(index.clone(), tree_item);
                    bundle_el.update_idx(index.clone());
                }
                Element::Error(ref mut err) => {
                    let error_comment = err.get().to_string();
                    let line = Span::styled(
                        format!("! {:?}", error_comment),
                        Style::default()
                            .fg(Color::Red)
                            .add_modifier(Modifier::ITALIC),
                    );
                    let index = self.indexer.current();
                    err.update_idx(index.clone());
                    let tree_item = TreeItem::new_leaf(index.clone(), line);
                    self.tree_elements.insert(index.clone(), tree_item);
                }
            });
    }

    pub fn update_state(&mut self, i: &str) {
        info!("Updating index: {}", i);
        let _ = self
            .nodes
            .iter_mut()
            .filter_map(|item| {
                item.index().and_then(|ind| {
                    if ind.eq(i) {
                        item.change_state();
                        match item.status {
                            Status::Selected => self.currently_selected.push(i.to_string()),
                            Status::Unselected => {
                                self.currently_selected.retain(|el| el.ne(&i));
                            }
                        };

                        let style = match item.status {
                            Status::Selected => Style::default().bg(Color::Green).fg(Color::White),
                            Status::Unselected => Style::default(),
                        };
                        let tree_item = self.tree_elements.get(i).unwrap().clone();
                        let tree_item = tree_item.style(style);
                        self.tree_elements.insert(i.to_string(), tree_item);
                        Some(())
                    } else {
                        None
                    }
                })
            })
            .collect::<Vec<_>>();
    }

    pub fn _bundle_info(&self, k: &str) -> Option<BundleInfo> {
        self.nodes.iter().find_map(|node| match &node.bundle {
            Element::Ok(bi) => {
                let bundle = bi.get();
                bi.index
                    .clone()
                    .and_then(|i| if i.eq(k) { Some(bundle.clone()) } else { None })
            }
            Element::Error(_) => None,
        })
    }

    /// Returns element of given index
    pub fn element(&self, k: &str) -> Option<Element> {
        self.nodes.iter().find_map(|node| {
            node.index().and_then(|i| {
                if i.eq(k) {
                    Some(node.bundle.clone())
                } else {
                    None
                }
            })
        })
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
        .map(|of| parse_node(to_show_dir, &of).map(|(node, _)| node));
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
    let path_and_bundle = match reference {
        RefValue::Said(said) => {
            get_oca_bundle_by_said(said, facade.clone()).and_then(|(refn, bundle)| {
                { graph.oca_file_path(&refn).map(|path| (path, bundle)) }
                    .map_err(CliError::GraphError)
            })
        }
        RefValue::Name(refn) => get_oca_bundle(refn, facade.clone()).and_then(|bundle| {
            { graph.oca_file_path(refn).map(|path| (path, bundle)) }.map_err(CliError::GraphError)
        }),
    };
    match path_and_bundle {
        Ok((ocafile_path, oca_bundle)) => {
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
