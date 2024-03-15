use std::{io, path::PathBuf, time::Duration};

pub use super::bundle_list::BundleListError;
use anyhow::Result;
use crossterm::event::{self, poll, Event, KeyCode, MouseEventKind};
use oca_rs::{facade::build::ValidationError, Facade};
use ratatui::{prelude::*, widgets::*};
use thiserror::Error;

use crate::{dependency_graph::{DependencyGraph, Node}, error::CliError, validate};

use super::{bundle_list::BundleList, errors_window::ErrorsWindow};

#[derive(Error, Debug)]
pub enum AppError {
    #[error(transparent)]
    BundleListError(#[from] BundleListError),
    #[error(transparent)]
    InputError(#[from] io::Error),
    #[error("Validation error: {0}")]
    ValidationError(String),
}
pub struct App<'a> {
    bundles: BundleList<'a>,
    errors: ErrorsWindow<'a>,
    facade: Facade,
    // graph: &'a mut DependencyGraph,
    active_window: Window,
    paths: Vec<PathBuf>,
    base_dir: PathBuf,
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
        // graph: &'a mut DependencyGraph,
    ) -> Result<App<'a>, AppError> {
        
        let graph = DependencyGraph::from_paths(&base, &paths).unwrap();
        Ok(BundleList::from_nodes(to_show, &facade, &graph).map(|bundles| App { base_dir: base, bundles, errors: ErrorsWindow::new(), facade, paths, active_window: Window::Bundles })?)
    }
}

impl<'a> App<'a> {
    pub fn run(&mut self, mut terminal: Terminal<impl Backend>) -> Result<(), AppError> {
        loop {
            if poll(Duration::from_millis(100))? {
                if !self.handle_input()? {
                return Ok(());
                }
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
                    Window::Errors => (&mut self.errors.state, &self.errors.items)
                };
                match key.code {
                    KeyCode::Char('q') | KeyCode::Esc => return Ok(false),
                    KeyCode::Char(' ') | KeyCode::Enter => state.toggle_selected(),
                    KeyCode::Left => state.key_left(),
                    KeyCode::Right => state.key_right(),
                    KeyCode::Down => state.key_down(&items),
                    KeyCode::Up => state.key_up(&items),
                    KeyCode::Home => state.select_first(&items),
                    KeyCode::End => state.select_last(&items),
                    KeyCode::PageDown => state.select_visible_relative(&items, |current| {
                        current.map_or(0, |current| current.saturating_add(10))
                    }),
                    KeyCode::PageUp => state.select_visible_relative(&items, |current| {
                        current.map_or(0, |current| current.saturating_sub(10))
                    }),
                    KeyCode::Char('v') => {
                        let mut graph = DependencyGraph::from_paths(&self.base_dir, &self.paths).unwrap();
                        self.errors.check(&self.facade, &mut graph)?
                    },
                    KeyCode::Tab => self.change_window(),
                    _ => false,
                }},
            Event::Mouse(mouse) => match mouse.kind {
                MouseEventKind::ScrollDown => self.bundles.state.scroll_down(1),
                MouseEventKind::ScrollUp => self.bundles.state.scroll_up(1),
                _ => false,
            },
            _ => false,
        };
        Ok(true)
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
