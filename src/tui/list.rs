use oca_bundle::state::oca::OCABundle;
use ratatui::{prelude::*, widgets::*};

use crate::dependency_graph::Node;

pub struct StatefulList {
    pub state: ListState,
    pub items: Vec<BundleInfo>,
    pub last_selected: Option<usize>,
}

impl StatefulList {
    pub fn with_items<'a>(items: Vec<BundleInfo>) -> StatefulList {
        StatefulList {
            state: ListState::default(),
            items: items,
            last_selected: None,
        }
    }

    pub fn next(&mut self) {
        let i = match self.state.selected() {
            Some(i) => {
                if i >= self.items.len() - 1 {
                    0
                } else {
                    i + 1
                }
            }
            None => self.last_selected.unwrap_or(0),
        };
        self.state.select(Some(i));
    }

    pub fn previous(&mut self) {
        let i = match self.state.selected() {
            Some(i) => {
                if i == 0 {
                    self.items.len() - 1
                } else {
                    i - 1
                }
            }
            None => self.last_selected.unwrap_or(0),
        };
        self.state.select(Some(i));
    }

    pub fn unselect(&mut self) {
        let offset = self.state.offset();
        self.last_selected = self.state.selected();
        self.state.select(None);
        *self.state.offset_mut() = offset;
    }

    /// Changes the status of the selected list item
    fn change_status(&mut self) {
        if let Some(i) = self.state.selected() {
            self.items[i].status = match self.items[i].status {
                Status::Completed => Status::Todo,
                Status::Todo => Status::Completed,
            }
        }
    }

    fn go_top(&mut self) {
        self.state.select(Some(0))
    }

    fn go_bottom(&mut self) {
        self.state.select(Some(self.items.len() - 1))
    }

    fn render_list(&mut self, area: Rect, buf: &mut Buffer) {
        // We create two blocks, one is for the header (outer) and the other is for list (inner).
        let outer_block = Block::default()
            .borders(Borders::NONE)
            .title("OCA Bundles")
            .title_alignment(Alignment::Center);
        let inner_block = Block::default().borders(Borders::NONE);

        // We get the inner area from outer_block. We'll use this area later to render the table.
        let outer_area = area;
        let inner_area = outer_block.inner(outer_area);

        // We can render the header in outer_area.
        outer_block.render(outer_area, buf);

        // Iterate through all elements in the `items` and stylize them.
        let items: Vec<ListItem> = self
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
        StatefulWidget::render(items, inner_area, buf, &mut self.state);
    }


    fn render_about(&self, area: Rect, buf: &mut Buffer) {
        // We get the info depending on the item's state.
        let about_bundle = if let Some(i) = self.state.selected() {
            serde_json::to_string_pretty(&self.items[i].oca_bundle).unwrap()
        } else {
            "Nothing to see here...".to_string()
        };

        // We show the list item's info under the list in this paragraph
        let outer_info_block = Block::default()
            .borders(Borders::NONE)
            // .fg(TEXT_COLOR)
            // .bg(TODO_HEADER_BG)
            .title("OCA Bundle")
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
}
