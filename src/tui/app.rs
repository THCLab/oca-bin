use std::path::PathBuf;

use anyhow::Result;
use crossterm::event::{self, KeyCode, KeyEventKind};
use ratatui::{prelude::*, widgets::*};

use crate::{build_dependency_graph, topological_sort};

use super::{
    get_oca_bundle,
    list::{BundleInfo, StatefulList, Status},
};

pub struct App {
    items: StatefulList,
}
impl App {
    pub fn new<'a>(path: Vec<PathBuf>, local_bundle_path: PathBuf) -> App {
        let graph = build_dependency_graph(path);
        let sorted_refn = topological_sort(&graph);

        let dependencies: Vec<BundleInfo> = sorted_refn
            .into_iter()
            .map(|refn| {
                let deps = graph.get(&refn);
                let oca_bundle = get_oca_bundle(local_bundle_path.clone(), refn.clone()).unwrap();
                BundleInfo {
                    refn: refn,
                    dependencies: deps.unwrap().dependencies.clone(),
                    status: Status::Completed,
                    oca_bundle,
                }
            })
            .collect();

        let items = StatefulList::with_items(dependencies);

        App { items }
    }

    /// Changes the status of the selected list item
    fn change_status(&mut self) {
        if let Some(i) = self.items.state.selected() {
            self.items.items[i].status = match self.items.items[i].status {
                Status::Completed => Status::Todo,
                Status::Todo => Status::Completed,
            }
        }
    }

    fn go_top(&mut self) {
        self.items.state.select(Some(0))
    }

    fn go_bottom(&mut self) {
        self.items.state.select(Some(self.items.items.len() - 1))
    }
}

impl App {
    pub fn run(&mut self, mut terminal: Terminal<impl Backend>) -> Result<()> {
        loop {
            self.draw(&mut terminal)?;

            if let event::Event::Key(key) = event::read()? {
                if key.kind == KeyEventKind::Press {
                    use KeyCode::*;
                    match key.code {
                        Char('q') | Esc => return Ok(()),
                        Char('h') | Left => self.items.unselect(),
                        Char('j') | Down => self.items.next(),
                        Char('k') | Up => self.items.previous(),
                        Char('l') | Right | Enter | Char(' ') => self.change_status(),
                        Char('g') => self.go_top(),
                        Char('G') => self.go_bottom(),
                        _ => {}
                    }
                }
            }
        }
    }

    fn draw(&mut self, terminal: &mut Terminal<impl Backend>) -> Result<()> {
        terminal.draw(|f| f.render_widget(self, f.size()))?;
        Ok(())
    }
}

impl Widget for &mut App {
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

        let vertical = Layout::vertical([Constraint::Percentage(50), Constraint::Percentage(50)]);
        let [list_area, deps_area] = vertical.areas(list_area);

        self.render_title(header_area, buf);
        self.render_list(list_area, buf);
        self.render_info(deps_area, buf);
        self.render_about(changes_area, buf);
        self.render_footer(footer_area, buf);
    }
}

impl App {
    fn render_title(&self, area: Rect, buf: &mut Buffer) {
        Paragraph::new("OCA Tool")
            .bold()
            .centered()
            .render(area, buf);
    }

    fn render_list(&mut self, area: Rect, buf: &mut Buffer) {
        // We create two blocks, one is for the header (outer) and the other is for list (inner).
        let outer_block = Block::default()
            .borders(Borders::NONE)
            // .fg(TEXT_COLOR)
            // .bg(TODO_HEADER_BG)
            .title("OCA Bundles")
            .title_alignment(Alignment::Center);
        let inner_block = Block::default().borders(Borders::NONE);
        // .fg(TEXT_COLOR)
        // .bg(NORMAL_ROW_COLOR);

        // We get the inner area from outer_block. We'll use this area later to render the table.
        let outer_area = area;
        let inner_area = outer_block.inner(outer_area);

        // We can render the header in outer_area.
        outer_block.render(outer_area, buf);

        // Iterate through all elements in the `items` and stylize them.
        let items: Vec<ListItem> = self
            .items
            .items
            .iter()
            .enumerate()
            .map(|(i, todo_item)| todo_item.to_list_item(i))
            .collect();

        // Create a List from all list items and highlight the currently selected one
        let items = List::new(items)
            .block(inner_block)
            .highlight_style(
                Style::default()
                    .add_modifier(Modifier::BOLD)
                    .add_modifier(Modifier::REVERSED), // .fg(SELECTED_STYLE_FG),
            )
            .highlight_symbol(">")
            .highlight_spacing(HighlightSpacing::Always);

        // We can now render the item list
        // (look careful we are using StatefulWidget's render.)
        // ratatui::widgets::StatefulWidget::render as stateful_render
        StatefulWidget::render(items, inner_area, buf, &mut self.items.state);
    }

    fn render_info(&self, area: Rect, buf: &mut Buffer) {
        // We get the info depending on the item's state.
        let info = if let Some(i) = self.items.state.selected() {
            match self.items.items[i].status {
                Status::Completed => {
                    "✓ ".to_string() + &self.items.items[i].dependencies.join("\n")
                }
                Status::Todo => "".to_string() + &self.items.items[i].dependencies.join("\n"),
            }
        } else {
            "Nothing to see here...".to_string()
        };

        // We show the list item's info under the list in this paragraph
        let outer_info_block = Block::default()
            .borders(Borders::NONE)
            // .fg(TEXT_COLOR)
            // .bg(TODO_HEADER_BG)
            .title("Dependencies")
            .title_alignment(Alignment::Center);
        let inner_info_block = Block::default()
            .borders(Borders::NONE)
            // .bg(NORMAL_ROW_COLOR)
            .padding(Padding::horizontal(1));

        // This is a similar process to what we did for list. outer_info_area will be used for
        // header inner_info_area will be used for the list info.
        let outer_info_area = area;
        let inner_info_area = outer_info_block.inner(outer_info_area);

        // We can render the header. Inner info will be rendered later
        outer_info_block.render(outer_info_area, buf);

        let info_paragraph = Paragraph::new(info)
            .block(inner_info_block)
            // .fg(TEXT_COLOR)
            .wrap(Wrap { trim: false });

        // We can now render the item info
        info_paragraph.render(inner_info_area, buf);
    }

    fn render_about(&self, area: Rect, buf: &mut Buffer) {
        // We get the info depending on the item's state.
        let about_bundle = if let Some(i) = self.items.state.selected() {
            serde_json::to_string_pretty(&self.items.items[i].oca_bundle).unwrap()
        } else {
            "Nothing to see here...".to_string()
        };

        // We show the list item's info under the list in this paragraph
        let outer_info_block = Block::default()
            .borders(Borders::NONE)
            // .fg(TEXT_COLOR)
            // .bg(TODO_HEADER_BG)
            .title("Dependencies")
            .title_alignment(Alignment::Center);
        let inner_info_block = Block::default()
            .borders(Borders::NONE)
            // .bg(NORMAL_ROW_COLOR)
            .padding(Padding::horizontal(1));

        // This is a similar process to what we did for list. outer_info_area will be used for
        // header inner_info_area will be used for the list info.
        let outer_info_area = area;
        let inner_info_area = outer_info_block.inner(outer_info_area);

        // We can render the header. Inner info will be rendered later
        outer_info_block.render(outer_info_area, buf);

        let info_paragraph = Paragraph::new(about_bundle)
            .block(inner_info_block)
            // .fg(TEXT_COLOR)
            .wrap(Wrap { trim: false });

        // We can now render the item info
        info_paragraph.render(inner_info_area, buf);
    }

    fn render_footer(&self, area: Rect, buf: &mut Buffer) {
        Paragraph::new(
            "\nUse ↓↑ to move, ← to unselect, → to change status, g/G to go top/bottom.",
        )
        .centered()
        .render(area, buf);
    }
}
