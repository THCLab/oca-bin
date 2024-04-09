use std::sync::{Arc, Mutex};

use oca_rs::Facade;
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    widgets::{Block, Scrollbar, ScrollbarOrientation, StatefulWidget},
};

use thiserror::Error;
use tui_tree_widget::{Tree, TreeItem, TreeState};

use crate::dependency_graph::{DependencyGraph, GraphError, Node};

use super::bundle_info::BundleInfo;
use super::item::Items;

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
        facade: Arc<Mutex<Facade>>,
        graph: Arc<DependencyGraph>,
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
            items: items,
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

    pub fn selected_oca_bundle(&self) -> Option<Vec<BundleInfo>> {
        let items = self.items.lock().unwrap();
        items.selected_bundles()
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
