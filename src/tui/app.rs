use std::path::PathBuf;

use anyhow::Result;
use crossterm::event::{self, Event, KeyCode, MouseEventKind};
use oca_rs::Facade;
use ratatui::{prelude::*, widgets::*};

use crate::get_oca_facade;

use super::bundle_list::BundleList;

pub struct App<'a> {
    bundles: BundleList<'a>,
    facade: Facade,
}
impl<'a> App<'a> {
    pub fn new(paths: Vec<PathBuf>, local_bundle_path: PathBuf) -> App<'a> {
        let facade = get_oca_facade(local_bundle_path);
        let bundles = BundleList::new(paths, &facade);

        App { bundles, facade }
    }
}

impl<'a> App<'a> {
    pub fn run(&mut self, mut terminal: Terminal<impl Backend>) -> Result<()> {
        loop {
            self.draw(&mut terminal)?;
            if !self.handle_input()? {
                return Ok(());
            }
        }
    }

    fn handle_input(&mut self) -> Result<bool> {
        match event::read()? {
            event::Event::Key(key) => match key.code {
                KeyCode::Char('q') | KeyCode::Esc => return Ok(false),
                KeyCode::Char('\n' | ' ') => self.bundles.state.toggle_selected(),
                KeyCode::Left => self.bundles.state.key_left(),
                KeyCode::Right => self.bundles.state.key_right(),
                KeyCode::Down => self.bundles.state.key_down(&self.bundles.items),
                KeyCode::Up => self.bundles.state.key_up(&self.bundles.items),
                KeyCode::Home => self.bundles.state.select_first(&self.bundles.items),
                KeyCode::End => self.bundles.state.select_last(&self.bundles.items),
                KeyCode::PageDown => self.bundles.state.scroll_down(3),
                KeyCode::PageUp => self.bundles.state.scroll_up(3),
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

    fn draw(&mut self, terminal: &mut Terminal<impl Backend>) -> Result<()> {
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
        let vertical = Layout::horizontal([Constraint::Percentage(50), Constraint::Percentage(50)]);
        let [list_area, changes_area] = vertical.areas(rest_area);

        self.render_title(header_area, buf);
        self.bundles.render(rest_area, buf);
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
