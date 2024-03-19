use std::{io, path::PathBuf, sync::Arc, time::Duration};

pub use super::bundle_list::BundleListError;
use anyhow::Result;
use crossterm::event::{self, poll, Event, KeyCode, MouseEventKind};
use oca_rs::{data_storage::SledDataStorage, Facade};
use ratatui::{
    backend::Backend,
    buffer::Buffer,
    layout::{Constraint, Layout, Rect},
    style::Stylize,
    widgets::{Paragraph, Widget},
    Terminal,
};
use thiserror::Error;

use crate::dependency_graph::{DependencyGraph, MutableGraph, Node};

use super::{bundle_list::BundleList, output_window::errors_window::ErrorsWindow};

#[derive(Error, Debug)]
pub enum AppError {
    #[error(transparent)]
    BundleList(#[from] BundleListError),
    #[error(transparent)]
    Input(#[from] io::Error),
    #[error("Validation error: {0}")]
    Validation(String),
}
pub struct App<'a> {
    bundles: BundleList<'a>,
    errors: ErrorsWindow,
    storage: Arc<SledDataStorage>,
    graph: MutableGraph,
    active_window: Window,
}

enum Window {
    Errors,
    Bundles,
}

impl<'a> App<'a> {
    pub fn new<I: IntoIterator<Item = Node>>(
        base: PathBuf,
        to_show: I,
        facade: Facade,
        paths: Vec<PathBuf>,
        storage: SledDataStorage,
        size: usize,
    ) -> Result<App<'a>, AppError> {
        let graph = DependencyGraph::from_paths(&base, &paths).unwrap();
        let mut_graph = MutableGraph::new(&base, &paths);
        Ok(
            BundleList::from_nodes(to_show, &facade, &graph).map(|bundles| App {
                bundles,
                errors: ErrorsWindow::new(size),
                storage: Arc::new(storage),
                active_window: Window::Bundles,
                graph: mut_graph,
            })?,
        )
    }
}

impl<'a> App<'a> {
    pub fn run(&mut self, mut terminal: Terminal<impl Backend>) -> Result<(), AppError> {
        loop {
            if poll(Duration::from_millis(100))? && !self.handle_input()? {
                return Ok(());
            }

            self.draw(&mut terminal)?;
        }
    }

    fn change_window(&mut self) -> bool {
        match self.active_window {
            Window::Errors => self.active_window = Window::Bundles,
            Window::Bundles => self.active_window = Window::Errors,
        }

        true
    }

    fn handle_input(&mut self) -> Result<bool, AppError> {
        match event::read()? {
            event::Event::Key(key) => {
                let (state, items) = match self.active_window {
                    Window::Bundles => (&mut self.bundles.state, &self.bundles.items),
                    // Window::Errors => (&mut self.errors.state, todo!()),
                    Window::Errors => (&mut self.bundles.state, &self.bundles.items),
                };
                match key.code {
                    KeyCode::Char('q') | KeyCode::Esc => return Ok(false),
                    KeyCode::Char(' ') | KeyCode::Enter => state.toggle_selected(),
                    KeyCode::Left => state.key_left(),
                    KeyCode::Right => state.key_right(),
                    KeyCode::Down => self.handle_key_down(),
                    KeyCode::Up => self.handle_key_up(),
                    KeyCode::Home => state.select_first(items),
                    KeyCode::End => state.select_last(&items),
                    KeyCode::PageDown => state.select_visible_relative(items, |current| {
                        current.map_or(0, |current| current.saturating_add(10))
                    }),
                    KeyCode::PageUp => state.select_visible_relative(items, |current| {
                        current.map_or(0, |current| current.saturating_sub(10))
                    }),
                    KeyCode::Char('v') => {
                        let selected = self.bundles.selected_oca_bundle();

                        self.errors
                            .check(self.storage.clone(), self.graph.clone(), selected)?
                    }
                    KeyCode::Tab => self.change_window(),
                    _ => false,
                }
            }
            Event::Mouse(mouse) => match mouse.kind {
                MouseEventKind::ScrollDown => self.bundles.state.scroll_down(1),
                MouseEventKind::ScrollUp => self.bundles.state.scroll_up(1),
                _ => false,
            },
            _ => false,
        };
        Ok(true)
    }

    fn handle_key_down(&mut self) -> bool {
        match self.active_window {
            Window::Bundles => {
                let (state, items) = (&mut self.bundles.state, &self.bundles.items);
                state.key_down(items);
            }
            Window::Errors => {
                let state = &mut self.errors.state;
                state.next()
            }
        };
        true
    }

    fn handle_key_up(&mut self) -> bool {
        match self.active_window {
            Window::Bundles => {
                let (state, items) = (&mut self.bundles.state, &self.bundles.items);
                state.key_up(items);
            }
            Window::Errors => {
                let state = &mut self.errors.state;
                state.previous()
            }
        };
        true
    }

    fn draw(&mut self, terminal: &mut Terminal<impl Backend>) -> Result<(), AppError> {
        terminal.draw(|f| f.render_widget(self, f.size()))?;
        Ok(())
    }
}

impl<'a> Widget for &mut App<'a> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        // Create a space for header, todo list and the footer.
        let vertical = Layout::vertical([
            Constraint::Length(2),
            Constraint::Min(0),
            Constraint::Length(2),
        ]);
        let [header_area, rest_area, footer_area] = vertical.areas(area);

        // Create two chunks with equal horizontal screen space. One for the list and dependencies and the other for
        // the changes block.
        let vertical = Layout::vertical([Constraint::Percentage(70), Constraint::Min(0)]);
        let [list_area, changes_area] = vertical.areas(rest_area);

        self.render_title(header_area, buf);
        self.bundles.render(list_area, buf);
        self.errors.render(changes_area, buf);
        self.render_footer(footer_area, buf);
    }
}

impl<'a> App<'a> {
    fn render_title(&self, area: Rect, buf: &mut Buffer) {
        Paragraph::new("OCA Tool")
            .bold()
            .centered()
            .render(area, buf);
    }

    fn render_footer(&self, area: Rect, buf: &mut Buffer) {
        Paragraph::new("\nUse ↓↑ to move, space or enter to expand/collapse list element, Tab to change active window and `v` to validate.")
            .centered()
            .render(area, buf);
    }
}
