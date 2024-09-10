use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
};

use oca_rs::Facade;
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    text::{Span, Text},
    widgets::{Block, Paragraph, Scrollbar, ScrollbarOrientation, StatefulWidget, Widget},
};

use thiserror::Error;
use tui_tree_widget::{Tree, TreeItem, TreeState};

use crate::dependency_graph::{DependencyGraph, GraphError, Node, NodeParsingError};

use super::item::Items;
use super::{bundle_info::BundleInfo, item::Element};

#[derive(Error, Debug, Clone)]
pub enum BundleListError {
    #[error("All references are unknown")]
    AllRefnUnknown,
    #[error(transparent)]
    GraphError(#[from] GraphError),
    #[error("Selected element isn't built properly: {0}")]
    ErrorSelected(PathBuf),
    #[error("Missing refn in file: {0}")]
    RefnMissing(PathBuf),
}

pub struct BundleList {
    path: PathBuf,
    pub state: TreeState<String>,
    pub items: Arc<Mutex<Items>>,
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
    pub fn from_nodes<I: IntoIterator<Item = Result<Node, NodeParsingError>>>(
        to_show: I,
        facade: Arc<Mutex<Facade>>,
        graph: Arc<DependencyGraph>,
        directory: PathBuf,
    ) -> Result<Self, BundleListError> {
        let items = Arc::new(Mutex::new(Items::new_items(
            to_show,
            facade.clone(),
            &graph,
        )));
        // let tree_items = items.to_tree_items(facade.clone(), &graph);
        let state = TreeState::default();
        let out = Self {
            state,
            items,
            path: directory,
        };
        Ok(out)
    }

    pub fn items(&self) -> Vec<TreeItem<'static, String>> {
        let items = self.items.lock().unwrap();
        items.items()
    }

    pub fn select(&mut self) {
        let selected = self.state.selected();
        selected.into_iter().for_each(|i| {
            let mut items = self.items.lock().unwrap();
            items.update_state(&i);
        });
    }

    pub fn select_all(&mut self) -> bool {
        let mut items = self.items.lock().unwrap();
        let all = items.select_all();
        self.state.select(all)
    }

    pub fn unselect_all(&mut self) -> bool {
        let mut items = self.items.lock().unwrap();
        items.unselect_all();
        self.state.select(vec![])
    }

    pub fn selected_oca_bundle(&self) -> Vec<Element> {
        let items = self.items.lock().unwrap();
        items.selected_bundles()
    }

    pub fn render(&mut self, area: Rect, buf: &mut Buffer) {
        let items = self.items();
        if items.is_empty() {
            Paragraph::new(Text::from(format!(
                "There are no ocafile files in the specified directory: ({})",
                std::fs::canonicalize(&self.path).unwrap().to_str().unwrap()
            )))
            .centered()
            .block(Block::bordered().title("OCA Bundles"))
            .render(area, buf);
        } else {
            let widget = Tree::new(self.items())
                .expect("all item identifiers are unique")
                .block(Block::bordered().title("OCA Bundles"))
                .experimental_scrollbar(Some(
                    Scrollbar::new(ScrollbarOrientation::VerticalRight)
                        .begin_symbol(None)
                        .track_symbol(None)
                        .end_symbol(None),
                ))
                .highlight_symbol("> ");

            StatefulWidget::render(widget, area, buf, &mut self.state);
        }
    }

    pub fn currently_pointed(&self) -> Option<BundleInfo> {
        let current = self.state.selected();
        let i = self.items.lock().unwrap();
        match current.first() {
            Some(current) => i.bundle_info(current),
            None => None,
        }
    }
}
