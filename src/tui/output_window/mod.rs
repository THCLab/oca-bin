pub mod message_list;

use std::{
    collections::HashSet,
    panic::AssertUnwindSafe,
    path::PathBuf,
    sync::{Arc, Mutex},
    thread,
};

use itertools::Itertools;
use oca_sdk_rs::overlay_registry::OverlayLocalRegistry;
use oca_store::Facade as Store;
use ratatui::{
    buffer::Buffer,
    layout::{Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{
        Block, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState, StatefulWidget, Widget,
        Wrap,
    },
};
use tui_widget_list::ListState;

use crate::{
    dependency_graph::{parse_name, MutableGraph},
    error::CliError,
    utils::handle_panic,
    validate::validate_directory,
};

use message_list::{Busy, LastAction, Message, MessageList};

use super::item::Element;

pub struct OutputWindow {
    pub state: ListState,
    errors: Arc<Mutex<MessageList>>,
    currently_validated: Vec<PathBuf>,
    active: bool,
    scroll: u16,
}

impl OutputWindow {
    pub fn new(size: usize) -> Self {
        Self {
            errors: Arc::new(Mutex::new(MessageList::new(size))),
            state: ListState::default(),
            currently_validated: vec![],
            active: false,
            scroll: 0,
        }
    }

    pub fn set_active(&mut self, active: bool) {
        self.active = active;
    }

    pub fn scroll_down(&mut self, amount: u16) {
        self.scroll = self.scroll.saturating_add(amount);
    }

    pub fn scroll_up(&mut self, amount: u16) {
        self.scroll = self.scroll.saturating_sub(amount);
    }

    pub fn set_currently_validated(&mut self, path: Vec<PathBuf>) {
        self.currently_validated = path;
    }

    pub fn current_path(&self) -> Vec<PathBuf> {
        self.currently_validated.clone()
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
                    let currently_validated = self.current_path();
                    let comment = if currently_validated.is_empty() {
                        "No element selected".to_string()
                    } else {
                        format!(
                            "Validation successful for: {}",
                            &currently_validated
                                .iter()
                                .map(|p| p.to_str().unwrap())
                                .join(", ")
                        )
                    };
                    self.render_action_result(&comment, area, buf);
                }
                LastAction::NoAction => {
                    self.render_building_process(area, buf);
                }
                LastAction::Pushing => self.render_building_process(area, buf),
            },
            Busy::Publish => {
                let layout = Layout::vertical([Constraint::Length(2), Constraint::Fill(2)]);
                let [building_title, output_area] = layout.areas(area);
                let simple = throbber_widgets_tui::Throbber::default()
                    .label("Publishing in progress. It may take some time.")
                    .style(ratatui::style::Style::default().fg(Color::Yellow));
                Widget::render(simple, building_title, buf);
                self.render_building_process(output_area, buf);
            }
        }
    }

    fn render_action_result(&mut self, success_comment: &str, area: Rect, buf: &mut Buffer) {
        let block = OutputWindow::output_block(self.active);
        let (lines, has_error) = {
            let errors = self.errors.lock().unwrap();
            let lines = errors
                .items
                .iter()
                .map(|msg| match msg {
                    Message::Info(info) => Line::from(Span::styled(
                        info.clone(),
                        Style::default().fg(Color::Green),
                    )),
                    Message::Error(err) => Line::from(Span::styled(
                        err.to_string(),
                        Style::default().fg(Color::Red),
                    )),
                })
                .collect::<Vec<_>>();
            (lines, errors.any_error())
        };
        if !has_error {
            let widget = {
                let span = Span::styled(success_comment, Style::default().fg(Color::Green));
                Paragraph::new(span).block(block)
            };
            widget.render(area, buf)
        } else {
            self.render_lines(area, buf, lines)
        }
    }

    fn render_building_process(&mut self, area: Rect, buf: &mut Buffer) {
        let lines = {
            let errors = self.errors.lock().unwrap();
            errors
                .items
                .iter()
                .map(|msg| match msg {
                    Message::Info(info) => Line::from(Span::styled(
                        info.clone(),
                        Style::default().fg(Color::Green),
                    )),
                    Message::Error(err) => Line::from(Span::styled(
                        err.to_string(),
                        Style::default().fg(Color::Red),
                    )),
                })
                .collect::<Vec<_>>()
        };
        self.render_lines(area, buf, lines)
    }

    pub fn handle_validate(
        &self,
        facade: Arc<Mutex<Store>>,
        graph: MutableGraph,
        bundle_infos: Vec<Element>,
        registry: OverlayLocalRegistry,
    ) -> Result<bool, CliError> {
        {
            let mut errors = self.errors.lock().unwrap();
            errors.busy = Busy::Validation;
            errors.items = vec![];
        }
        let err_list = self.errors.clone();
        let path = self.current_path();

        thread::spawn(move || {
            let mut cache = HashSet::new();
            let errs = bundle_infos
                .iter()
                .flat_map(|bundle_info| {
                    let name = match bundle_info {
                        Element::Ok(oks_elements) => Some(oks_elements.get().refn.clone()),
                        Element::Error(errors) => {
                            let path = errors.path().to_path_buf();
                            parse_name(path.as_path()).unwrap().0
                        }
                    };
                    let res = std::panic::catch_unwind(AssertUnwindSafe(|| {
                        let (to_cache, validation_errors) = validate_directory(
                            facade.clone(),
                            &mut graph.clone(),
                            name,
                            registry.clone(),
                            &cache,
                        )
                        .unwrap();
                        cache.extend(to_cache);

                        validation_errors
                    }));
                    match res {
                        Ok(err) => err,
                        Err(panic) => {
                            vec![handle_panic(panic)]
                        }
                    }
                })
                .collect();
            update_errors(err_list.clone(), errs, &path);
        });
        Ok(true)
    }

    pub fn mark_build(&self) {
        let mut errors = self.errors.lock().unwrap();
        errors.busy = Busy::Building;
        errors.items = vec![];
    }

    pub fn mark_publish(&self) {
        let mut errors = self.errors.lock().unwrap();
        errors.busy = Busy::Publish;
        errors.items = vec![];
    }

    pub fn error_list_mut(&self) -> Arc<Mutex<MessageList>> {
        self.errors.clone()
    }

    fn render_lines(&mut self, area: Rect, buf: &mut Buffer, lines: Vec<ratatui::text::Line<'_>>) {
        let block = OutputWindow::output_block(self.active);
        let content_len = lines.len();
        let viewport = area.height.saturating_sub(2) as usize;
        let max_scroll = content_len.saturating_sub(viewport) as u16;
        if self.scroll > max_scroll {
            self.scroll = max_scroll;
        }

        let paragraph = Paragraph::new(lines)
            .block(block)
            .wrap(Wrap { trim: false })
            .scroll((self.scroll, 0));
        paragraph.render(area, buf);

        self.render_scrollbar(area, buf, content_len, max_scroll);
    }

    fn render_scrollbar(&self, area: Rect, buf: &mut Buffer, content_len: usize, max_scroll: u16) {
        if max_scroll == 0 {
            return;
        }
        let mut scrollbar_state = ScrollbarState::new(content_len).position(self.scroll as usize);
        let scrollbar = Scrollbar::new(ScrollbarOrientation::VerticalRight)
            .begin_symbol(None)
            .track_symbol(None)
            .end_symbol(None);
        scrollbar.render(area, buf, &mut scrollbar_state);
    }

    fn output_block(active: bool) -> Block<'static> {
        let title_style = if active {
            Style::default()
                .add_modifier(Modifier::BOLD)
                .add_modifier(Modifier::UNDERLINED)
        } else {
            Style::default()
        };
        Block::bordered().title(Span::styled("Output", title_style))
    }
}

pub fn update_errors(
    errs: Arc<Mutex<MessageList>>,
    new_errors: Vec<CliError>,
    source_path: &[PathBuf],
) {
    let mut errors = errs.lock().unwrap();
    let messages = new_errors.into_iter().map(Message::Error).collect();
    errors.update(messages, source_path);
}

pub fn _push_message(errs: Arc<Mutex<MessageList>>, message: Message) {
    let mut messages_list = errs.lock().unwrap();
    messages_list.append(message);
}
