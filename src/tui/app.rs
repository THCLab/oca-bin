use std::{io, time::Duration};

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
    errors: ErrorsWindow,
    facade: Facade,
    graph: &'a mut DependencyGraph,
}
impl<'a> App<'a> {
    pub fn new<I: IntoIterator<Item = Node>>(
        to_show: I,
        facade: Facade,
        graph: &'a mut DependencyGraph,
    ) -> Result<App<'a>, AppError> {
        
        Ok(BundleList::from_nodes(to_show, &facade, graph).map(|bundles| App { bundles, errors: ErrorsWindow::new(), facade, graph })?)
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

    fn handle_input(&mut self) -> Result<bool, AppError> {
        match event::read()? {
            event::Event::Key(key) => match key.code {
                KeyCode::Char('q') | KeyCode::Esc => return Ok(false),
                KeyCode::Char(' ') | KeyCode::Enter => self.bundles.state.toggle_selected(),
                KeyCode::Left => self.bundles.state.key_left(),
                KeyCode::Right => self.bundles.state.key_right(),
                KeyCode::Down => self.bundles.state.key_down(&self.bundles.items),
                KeyCode::Up => self.bundles.state.key_up(&self.bundles.items),
                KeyCode::Home => self.bundles.state.select_first(&self.bundles.items),
                KeyCode::End => self.bundles.state.select_last(&self.bundles.items),
                KeyCode::PageDown => self.bundles.ten_down(),
                KeyCode::PageUp => self.bundles.ten_up(),
                KeyCode::Char('v') => self.errors.check(&self.facade, self.graph)?,
                _ => false,
            },
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
        let vertical = Layout::vertical([Constraint::Percentage(80), Constraint::Min(0)]);
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
        Paragraph::new("\nUse ↓↑ to move, space to expand/collapse bundle attributes.")
            .centered()
            .render(area, buf);
    }
}
