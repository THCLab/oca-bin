use std::path::PathBuf;
use std::sync::Mutex;

use oca_ast::ast::NestedAttrType;
use oca_rs::Facade;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Scrollbar, ScrollbarOrientation, StatefulWidget};

use tui_tree_widget::{Tree, TreeItem, TreeState};

use crate::dependency_graph::DependencyGraph;

use super::bundle_info::{BundleInfo, Status};
use super::{get_oca_bundle, get_oca_bundle_by_said};
// use super::list::{BundleInfo, Status};

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
    pub fn new(paths: Vec<PathBuf>, facade: &Facade) -> Self {
        let graph = DependencyGraph::new(paths);
        let sorted_refn = graph.sort();

        let dependencies: Vec<BundleInfo> = sorted_refn
            .into_iter()
            .map(|node| {
                let deps = graph.neighbors(&node.refn);
                let oca_bundle = get_oca_bundle(&node.refn, &facade)
                    .expect(&format!("Unknown refn: {}", &node.refn));
                BundleInfo {
                    refn: node.refn,
                    dependencies: deps,
                    status: Status::Completed,
                    oca_bundle,
                }
            })
            .collect();

        let i = Indexer::new();
        let deps = dependencies
            .into_iter()
            .map(|dep| {
                let attributes = dep.oca_bundle.capture_base.attributes;
                let attrs = attributes
                    .into_iter()
                    .map(|(key, attr)| Self::to_tree_item(key, attr, &i, &facade, &graph))
                    .collect::<Vec<_>>();
                TreeItem::new(i.current(), dep.refn, attrs)
            })
            .collect::<Result<Vec<_>, _>>()
            .unwrap();

        Self {
            state: TreeState::default(),
            items: deps,
        }
    }

    fn to_tree_item(
        key: String,
        attr: NestedAttrType,
        i: &Indexer,
        facade: &Facade,
        graph: &DependencyGraph,
    ) -> TreeItem<'a, String> {
        match attr {
            oca_ast::ast::NestedAttrType::Reference(reference) => {
                let (ocafile_path, oca_bundle) = match reference {
                    oca_ast::ast::RefValue::Said(said) => {
                        let (refn, bundle) = get_oca_bundle_by_said(&said, facade)
                            .expect(&format!("Unknown said: {}", &said));
                        (graph.oca_file_path(&refn), bundle)
                    }
                    oca_ast::ast::RefValue::Name(refn) => {
                        let bundle = get_oca_bundle(&refn, facade)
                            .expect(&format!("Unknown refn: {}", &refn));
                        (graph.oca_file_path(&refn), bundle)
                    }
                };
                let mixed_line = vec![
                    Span::styled(format!("{}: Reference", key), Style::default()),
                    Span::styled(
                        format!("      • {}", ocafile_path.unwrap().to_str().unwrap()),
                        Style::default()
                            .fg(Color::Yellow)
                            .add_modifier(Modifier::ITALIC),
                    ),
                ];
                let children: Vec<TreeItem<'a, String>> = oca_bundle
                    .capture_base
                    .attributes
                    .into_iter()
                    .map(|(key, attr)| Self::to_tree_item(key, attr, i, facade, graph))
                    .collect();
                TreeItem::new(i.current(), Line::from(mixed_line), children).unwrap()
            }
            oca_ast::ast::NestedAttrType::Value(attr) => {
                TreeItem::new_leaf(i.current(), format!("{}: {}", key, attr.to_string()))
            }
            oca_ast::ast::NestedAttrType::Array(_arr) => {
                TreeItem::new_leaf(i.current(), format!("{}: {}", key, "Array"))
            }
            oca_ast::ast::NestedAttrType::Null => todo!(),
        }
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
}
