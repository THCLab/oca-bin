pub mod message_list;

use std::{
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    thread,
};

use itertools::Itertools;
use oca_rs::Facade;
use ratatui::{
    buffer::Buffer,
    layout::{Constraint, Layout, Rect},
    style::{Color, Style},
    text::Span,
    widgets::{Block, Paragraph, StatefulWidget, Widget},
};
use tui_widget_list::{List, ListState};

use crate::{
    dependency_graph::{parse_name, MutableGraph}, error::CliError, tui::app::AppError,
    validate::validate_directory,
};

use message_list::{Busy, LastAction, Message, MessageList};

use super::item::Element;

pub struct OutputWindow {
    pub state: ListState,
    errors: Arc<Mutex<MessageList>>,
    currently_validated: Vec<PathBuf>,
    base_dir: PathBuf,
}

impl OutputWindow {
    pub fn new(size: usize, base_dir: PathBuf) -> Self {
        Self {
            errors: Arc::new(Mutex::new(MessageList::new(size))),
            state: ListState::default(),
            currently_validated: vec![],
            base_dir,
        }
    }

    pub fn set_currently_validated(&mut self, path: Vec<PathBuf>) {
        self.currently_validated = path;
    }

    pub fn current_path(&self) -> Vec<PathBuf> {
        if !self.currently_validated.is_empty() {
            self.currently_validated.clone()
        } else {
            vec![self.base_dir.clone()]
        }
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
            }
            Busy::Building => {
                let layout = Layout::vertical([Constraint::Length(2), Constraint::Fill(2)]);
                let [building_title, output_area] = layout.areas(area);
                let simple = throbber_widgets_tui::Throbber::default()
                    .label("Building in progress. It may take some time.")
                    .style(ratatui::style::Style::default().fg(Color::Yellow));
                Widget::render(simple, building_title, buf);
                self.render_building_process(output_area, buf);
            }
            Busy::NoTask => match &self.last_action() {
                LastAction::Building => self.render_building_process(area, buf),
                LastAction::Validating => {
                    self.render_action_result(
                        &format!(
                            "Validation successful for: {}",
                            &self
                                .current_path()
                                .iter()
                                .map(|p| p.to_str().unwrap())
                                .join(", ")
                        ),
                        area,
                        buf,
                    );
                }
                LastAction::NoAction => Paragraph::new("")
                    .block(Block::bordered().title("Output"))
                    .render(area, buf),
            },
        }
    }

    fn render_action_result(&mut self, success_comment: &str, area: Rect, buf: &mut Buffer) {
        let block = Block::bordered().title("Output");
        let errors = self.errors.lock().unwrap();
        let items = errors.items();
        if !errors.any_error() {
            let widget = {
                let span = Span::styled(success_comment, Style::default().fg(Color::Green));
                Paragraph::new(span).block(block)
            };
            widget.render(area, buf)
        } else {
            let index = items.len() - 1;
            let widget = List::new(items).block(Block::bordered().title("Output"));
            self.state.select(Some(index));
            widget.render(area, buf, &mut self.state)
        }
    }

    fn render_building_process(&mut self, area: Rect, buf: &mut Buffer) {
        let errors = self.errors.lock().unwrap();
        let errors = errors.items();

        let index = errors.len().saturating_sub(1);
        let widget = List::new(errors).block(Block::bordered().title("Output"));
        self.state.select(Some(index));
        widget.render(area, buf, &mut self.state)
    }

    pub fn check(
        &mut self,
        facade: Arc<Mutex<Facade>>,
        graph: MutableGraph,
        bundle_infos: Vec<Element>,
        base_path: PathBuf,
    ) -> Result<bool, AppError> {
        {
            let mut errors = self.errors.lock().unwrap();
            errors.busy = Busy::Validation;
            errors.items = vec![];
        }
        let err_list = self.errors.clone();
        let path = self.current_path();
        
        thread::spawn(move || {
            let errs = bundle_infos
                .iter()
                .flat_map(|bundle_info| {
                    let name = match bundle_info {
                    Element::Ok(oks_elements) => {
                        Some(oks_elements.get().refn.clone())
                        
                    }
                    Element::Error(errors) => {
                        let mut path = base_path.clone();
                        path.push(errors.path());
                        info!("Building path: {:?}", &path);
                        parse_name(path.as_path()).unwrap().0
                    }
                };
                validate_directory(facade.clone(), &mut graph.clone(), name)
                            .unwrap()
                })
                .collect();
            update_errors(err_list.clone(), errs, &path);
        });
        Ok(true)
    }

    pub fn mark_build(&self) {
        let mut errors = self.errors.lock().unwrap();
        info!("Building");
        errors.busy = Busy::Building;
        errors.items = vec![];
    }

    pub fn error_list_mut(&self) -> Arc<Mutex<MessageList>> {
        self.errors.clone()
    }
}

pub fn update_errors(errs: Arc<Mutex<MessageList>>, new_errors: Vec<CliError>, source_path: &[PathBuf]) {
    let mut errors = errs.lock().unwrap();
    let messages = new_errors.into_iter().map(Message::Error).collect();
    errors.update(messages, source_path);
}

pub fn _push_message(errs: Arc<Mutex<MessageList>>, message: Message) {
    let mut messages_list = errs.lock().unwrap();
    messages_list.append(message);
}
