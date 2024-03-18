use std::{
    sync::{Arc, Mutex},
    thread,
};

use oca_rs::data_storage::SledDataStorage;
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    widgets::{Block, StatefulWidget, Widget, Wrap},
};
use tui_widget_list::{List, ListState};

use crate::{
    dependency_graph::MutableGraph,
    error::CliError,
    tui::{app::AppError, bundle_list::Indexer},
    validate::validate_directory,
};

use super::error_list::{ErrorLine, SimpleErrorsList};

pub struct ErrorsWindow {
    pub state: ListState,
    errors: Arc<Mutex<SimpleErrorsList>>,
}

impl ErrorsWindow {
    pub fn new(size: usize) -> Self {
        Self {
            errors: Arc::new(Mutex::new(SimpleErrorsList::new(size))),
            // busy: false,
            state: ListState::default(),
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
            let errors = self.items();
            let mut widget = List::new(errors).block(Block::bordered().title("Output"));
            widget.render(area, buf, &mut self.state)
        }
    }

    // pub fn items(&self) -> Vec<ListItem<'static>> {
    pub fn items<'a>(&self) -> Vec<ErrorLine<'a>> {
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

fn update_errors(errs: Arc<Mutex<SimpleErrorsList>>, new_errors: Vec<CliError>) {
    let mut errors = errs.lock().unwrap();
    errors.update(new_errors);
}
