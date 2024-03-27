use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
    thread,
};

use oca_rs::data_storage::SledDataStorage;
use ratatui::{
    buffer::Buffer,
    layout::{Constraint, Layout, Rect},
    style::{Color, Style},
    text::Span,
    widgets::{Block, Paragraph, StatefulWidget, Widget},
};
use tui_widget_list::{List, ListState};

use crate::{
    dependency_graph::MutableGraph,
    error::CliError,
    tui::{app::AppError, bundle_info::BundleInfo},
    validate::validate_directory,
};

use super::message_list::{Busy, LastAction, Message, MessageList};


pub struct OutputWindow {
    pub state: ListState,
    errors: Arc<Mutex<MessageList>>,
    currently_validated: Option<PathBuf>,
}

impl OutputWindow {
    pub fn new(size: usize) -> Self {
        Self {
            errors: Arc::new(Mutex::new(MessageList::new(size))),
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

    pub fn render(&mut self, area: Rect, buf: &mut Buffer) {
        match self.busy() {
            Busy::Validation => {
                let simple = throbber_widgets_tui::Throbber::default()
                    .label("Validation in progress. It may take some time.")
                    .style(ratatui::style::Style::default().fg(Color::Yellow));
                Widget::render(simple, area, buf);
            },
            Busy::Building => {
                let layout = Layout::vertical([
                    Constraint::Length(2),
                    Constraint::Fill(2),
                ]);
                let [building_title, output_area] = layout.areas(area);
                let simple = throbber_widgets_tui::Throbber::default()
                    .label("Building in progress. It may take some time.")
                    .style(ratatui::style::Style::default().fg(Color::Yellow));
                Widget::render(simple, building_title, buf);
                self.render_building_process(output_area, buf);
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
        let errors = self.errors.lock().unwrap();
        // errs.items()
        let errors = errors.items();
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

    fn render_building_process(&mut self, area: Rect, buf: &mut Buffer) {
        let errors = self.errors.lock().unwrap();
        let errors = errors.items();

        let widget = List::new(errors).block(Block::bordered().title("Output"));
        widget.render(area, buf, &mut self.state)
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

    pub fn error_list_mut(&self) -> Arc<Mutex<MessageList>> {
        self.errors.clone()
    }
}

pub fn update_errors(errs: Arc<Mutex<MessageList>>, new_errors: Vec<CliError>) {
    let mut errors = errs.lock().unwrap();
    let messages = new_errors.into_iter().map(Message::Error).collect();
    errors.update(messages);
}

pub fn push_message(errs: Arc<Mutex<MessageList>>, message: Message) {
    let mut messages_list = errs.lock().unwrap();
    messages_list.append(message);
}
