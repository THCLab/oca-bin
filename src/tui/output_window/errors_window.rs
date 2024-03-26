use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
    thread,
};

use oca_rs::{data_storage::SledDataStorage, facade::build::ValidationError, Facade};
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Style},
    text::Span,
    widgets::{Block, Paragraph, StatefulWidget, Widget},
};
use tui_widget_list::{List, ListState};

use crate::{
    dependency_graph::MutableGraph,
    error::CliError,
    tui::{app::AppError, bundle_info::BundleInfo},
    validate::{build, validate_directory},
};

use super::error_list::{Busy, ErrorLine, LastAction, SimpleErrorsList};


pub struct ErrorsWindow {
    pub state: ListState,
    errors: Arc<Mutex<SimpleErrorsList>>,
    currently_validated: Option<PathBuf>,
}

impl ErrorsWindow {
    pub fn new(size: usize) -> Self {
        Self {
            errors: Arc::new(Mutex::new(SimpleErrorsList::new(size))),
            state: ListState::default(),
            currently_validated: None,
        }
    }

    pub fn set_currently_validated(&mut self, path: PathBuf) {
        self.currently_validated = Some(path)
    }

    fn busy(&self) -> Busy {
        let e = self.errors.lock().unwrap();
        e.busy.clone()
    }

    fn last_action(&self) -> LastAction {
        let e = self.errors.lock().unwrap();
        e.last_action.clone()
    }

    // pub fn update(&self, errors: Vec<CliError>) -> Result<(), CliError> {
    //     let mut errs = self.errors.lock().unwrap();
    //     errs.update(errors);
    //     Ok(())
    // }

    pub fn render(&mut self, area: Rect, buf: &mut Buffer) {
        match self.busy() {
            Busy::Validation => {
                let simple = throbber_widgets_tui::Throbber::default()
                    .label("Validation in progress. It may take some time.")
                    .style(ratatui::style::Style::default().fg(Color::Yellow));
                Widget::render(simple, area, buf);
            },
            Busy::Building => {
                let simple = throbber_widgets_tui::Throbber::default()
                    .label("Building in progress. It may take some time.")
                    .style(ratatui::style::Style::default().fg(Color::Yellow));
                Widget::render(simple, area, buf);
            },
            Busy::NoTask => {
               match &self.last_action() {
                    LastAction::Building => self.render_action_result("Build successful", area, buf),
                    LastAction::Validating => {
                        let validated = self.currently_validated.as_ref().unwrap().to_str().unwrap();
                        self.render_action_result(&format!("Validation successful for file: {}", &validated), area, buf);
                    },
                    LastAction::NoAction => Paragraph::new("").block(Block::bordered().title("Output")).render(area, buf),
                }
            }
        }
    }

    fn render_action_result(&mut self, success_comment: &str, area: Rect, buf: &mut Buffer) {
        let block = Block::bordered().title("Output");
        let errors = self.items();
        if errors.is_empty() {
            let widget = {let span = Span::styled(
                    success_comment,
                    Style::default().fg(Color::Green),
                );
                Paragraph::new(span).block(block)
            } ;
            widget.render(area, buf)
        } else {
            let widget = List::new(errors).block(Block::bordered().title("Output"));
            widget.render(area, buf, &mut self.state)
        }
    }

    // pub fn items(&self) -> Vec<ListItem<'static>> {
    pub fn items<'a>(&self) -> Vec<ErrorLine<'a>> {
        let errs = self.errors.lock().unwrap();
        errs.items()
    }

    pub fn check(
        &mut self,
        storage: Arc<SledDataStorage>,
        graph: MutableGraph,
        bundle_info: Option<BundleInfo>,
    ) -> Result<bool, AppError> {
        {
            let mut errors = self.errors.lock().unwrap();
            errors.busy = Busy::Validation;
        }
        let err_list = self.errors.clone();
        thread::spawn(move || {
            let (_oks, errs) =
                validate_directory(&storage.clone(), &mut graph.clone(), bundle_info.as_ref())
                    .unwrap();
            update_errors(err_list.clone(), errs);
        });

        Ok(true)
    }

    pub fn mark_build(&self) {
        let mut errors = self.errors.lock().unwrap();
        errors.busy = Busy::Building;
    }

    pub fn error_list_mut(&self) -> Arc<Mutex<SimpleErrorsList>> {
        self.errors.clone()
    }
}

pub fn update_errors(errs: Arc<Mutex<SimpleErrorsList>>, new_errors: Vec<CliError>) {
    let mut errors = errs.lock().unwrap();
    errors.update(new_errors);
}
