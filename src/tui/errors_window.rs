use std::{
    sync::{Arc, Mutex},
    thread,
};

use oca_rs::data_storage::SledDataStorage;
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Span, Text},
    widgets::{Block, Scrollbar, ScrollbarOrientation, StatefulWidget, Widget},
};
use tui_tree_widget::{Tree, TreeItem, TreeState};

use crate::{
    dependency_graph::MutableGraph,
    error::CliError,
    validate::validate_directory,
};

use super::{app::AppError, bundle_list::Indexer};

struct ErrorList {
    list: Vec<CliError>,
    pub busy: bool,
    pub items: Vec<TreeItem<'static, String>>,
}

impl ErrorList {
    fn new() -> Self {
        Self {
            list: Vec::new(),
            busy: false,
            items: Vec::new(),
        }
    }
    fn update(&mut self, new_list: Vec<CliError>) {
        self.list = new_list;
        self.busy = false;
    }

    fn items(&mut self) -> Vec<TreeItem<'static, String>> {
        let i = Indexer::new();
        let items: Vec<_> = self
            .list
            .iter()
            .map(|dep| match dep {
                CliError::GrammarError(file, errors) => {
                    let children = errors
                        .into_iter()
                        .map(|err| {
                            let line = Span::styled(
                                format!("! {}", err.to_string()),
                                Style::default()
                                    .fg(Color::Red)
                                    .add_modifier(Modifier::ITALIC),
                            );
                            TreeItem::new_leaf(i.current(), Text::from(line))
                        })
                        .collect();
                    TreeItem::new(i.current(), file.to_str().unwrap().to_owned(), children).unwrap()
                }
                CliError::GraphError(e) => TreeItem::new_leaf(i.current(), e.to_string()),
                e => TreeItem::new_leaf(i.current(), Text::from(e.to_string())),
            })
            .collect();
        self.items = items.clone();
        items
    }
}

// struct MutableErrorList(ErrorList<);

pub struct ErrorsWindow {
    pub state: TreeState<String>,
    errors: Arc<Mutex<ErrorList>>,
}

impl ErrorsWindow {
    pub fn new() -> Self {
        Self {
            errors: Arc::new(Mutex::new(ErrorList::new())),
            // busy: false,
            state: TreeState::default(),
            // items: vec![],
        }
    }

    fn busy(&self) -> bool {
        let e = self.errors.lock().unwrap();
        e.busy.clone()
    }

    pub fn render(&mut self, area: Rect, buf: &mut Buffer) {
        if self.busy() {
            let simple = throbber_widgets_tui::Throbber::default()
                .label("Validation in progress. It may take some time.")
                .style(ratatui::style::Style::default().fg(ratatui::style::Color::Yellow));
            Widget::render(simple, area, buf);
        } else {
            let widget = Tree::new(self.items())
                .expect("all item identifiers are unique")
                .block(Block::bordered().title("Output"))
                .experimental_scrollbar(Some(
                    Scrollbar::new(ScrollbarOrientation::VerticalRight)
                        .begin_symbol(None)
                        .track_symbol(None)
                        .end_symbol(None),
                ))
                .highlight_style(
                    Style::new()
                        .fg(Color::Black)
                        .bg(Color::Red)
                        .add_modifier(Modifier::BOLD),
                )
                .highlight_symbol("> ");

            StatefulWidget::render(widget, area, buf, &mut self.state);
        }
    }

    pub fn items(&self) -> Vec<TreeItem<'static, String>> {
        let mut errs = self.errors.lock().unwrap();
        errs.items()
    }

    pub fn check(
        &mut self,
        storage: Arc<SledDataStorage>,
        graph: MutableGraph,
    ) -> Result<bool, AppError> {
        {
            let mut errors = self.errors.lock().unwrap();
            errors.busy = true;
        }
        let err_list = self.errors.clone();
        thread::spawn(move || {
            let (_oks, errs) = validate_directory(&storage.clone(), &mut graph.clone()).unwrap();
            update_errors(err_list.clone(), errs);
        });

        Ok(true)
    }
}

fn update_errors(errs: Arc<Mutex<ErrorList>>, new_errors: Vec<CliError>) {
    let mut errors = errs.lock().unwrap();
    errors.update(new_errors);
}
