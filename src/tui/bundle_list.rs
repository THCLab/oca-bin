use std::sync::Mutex;

use oca_ast::ast::{NestedAttrType, RefValue};
use oca_rs::Facade;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Scrollbar, ScrollbarOrientation, StatefulWidget};

use thiserror::Error;
use tui_tree_widget::{Tree, TreeItem, TreeState};

use crate::dependency_graph::{DependencyGraph, GraphError, Node};

use super::bundle_info::{BundleInfo, Status};
use super::{get_oca_bundle, get_oca_bundle_by_said};

#[derive(Error, Debug)]
pub enum BundleListError {
    #[error("All references are unknown")]
    AllRefnUnknown,
    #[error(transparent)]
    GraphError(#[from] GraphError),
}

pub struct BundleList<'a> {
    pub state: TreeState<String>,
    pub items: Vec<TreeItem<'a, String>>,
}

struct Indexer(Mutex<u32>);
impl Indexer {
    fn new() -> Self {
        Self(Mutex::new(0))
    }

    fn current(&self) -> String {
        let mut s = self.0.lock().unwrap();
        *s += 1;
        s.to_string()
    }
}

impl<'a> BundleList<'a> {
    pub fn from_nodes<I: IntoIterator<Item = Node>>(
        to_show: I,
        facade: &Facade,
        graph: &DependencyGraph,
    ) -> Result<Self, BundleListError> {
        let dependencies = to_show
            .into_iter()
            .map(|node| bundle_info_from_refn(&node.refn, graph, facade));
        // if dependencies.all(|d| d.is_err()) {return Err(BundleListError::AllRefnUnknown);};

        let i = Indexer::new();
        let deps = dependencies
            .map(|dep| result_to_tree_item(dep, &i, facade, graph))
            .collect();

        Ok(Self {
            state: TreeState::default(),
            items: deps,
        })
    }

    pub fn render(&mut self, area: Rect, buf: &mut Buffer) {
        let widget = Tree::new(self.items.clone())
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

    pub fn ten_down(&mut self) -> bool {
        self.state.select_visible_relative(&self.items, |current| {
            current.map_or(0, |current| current.saturating_add(10))
        })
    }

    pub fn ten_up(&mut self) -> bool {
        self.state.select_visible_relative(&self.items, |current| {
            current.map_or(0, |current| current.saturating_sub(10))
        })
    }
}

pub fn bundle_info_from_refn(
    refn: &str,
    graph: &DependencyGraph,
    facade: &Facade,
) -> Result<BundleInfo, BundleListError> {
    let deps = graph.neighbors(&refn).unwrap();
    let oca_bundle = get_oca_bundle(&refn, facade);
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

fn result_to_tree_item<'a>(
    ob: Result<BundleInfo, BundleListError>,
    i: &Indexer,
    facade: &Facade,
    graph: &DependencyGraph,
) -> TreeItem<'a, String> {
    match ob {
        Ok(bundle) => {
            let attributes = bundle.oca_bundle.capture_base.attributes;
            let attrs = attributes
                .into_iter()
                .map(|(key, attr)| to_tree_item(key, &attr, &i, facade, graph))
                .collect::<Vec<_>>();
            TreeItem::new(i.current(), bundle.refn, attrs).unwrap()
        }
        Err(err) => {
            let line = Span::styled(
                format!("! {}", err.to_string()),
                Style::default()
                    .fg(Color::Red)
                    .add_modifier(Modifier::ITALIC),
            );
            TreeItem::new_leaf(i.current(), line)
        }
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
    let line = match ocafile_path {
        Ok(ocafile_path) => {
            vec![
                Span::styled(line, Style::default()),
                Span::styled(
                    format!("      • {}", ocafile_path.to_str().unwrap()),
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::ITALIC),
                ),
            ]
        }
        Err(e) => {
            vec![
                Span::styled(line, Style::default().fg(Color::Red)),
                Span::styled(
                    format!("      ! {}", e.to_string()),
                    Style::default()
                        .fg(Color::Red)
                        .add_modifier(Modifier::ITALIC),
                ),
            ]
        }
    };
    // let mixed_line = vec![
    //     Span::styled(line, Style::default()),
    //     Span::styled(
    //         format!("      • {}", ocafile_path.unwrap().to_str().unwrap()),
    //         Style::default()
    //             .fg(Color::Yellow)
    //             .add_modifier(Modifier::ITALIC),
    //     ),
    // ];
    let children: Vec<TreeItem<'a, String>> = oca_bundle
        .capture_base
        .attributes
        .into_iter()
        .map(|(key, attr)| to_tree_item(key, &attr, i, facade, graph))
        .collect();
    TreeItem::new(i.current(), Line::from(line), children).unwrap()
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
