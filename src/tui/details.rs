use oca_sdk_rs::oca::utils::said::SelfAddressingIdentifier;
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{
        Block, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState, StatefulWidget, Widget,
    },
};

use crate::dependency_graph::Node;

pub struct Details {
    pub id: SelfAddressingIdentifier,
    // path: PathBuf,
    pub name: String,
    pub dependent: Vec<Node>,
    pub attributes: Vec<(String, String)>,
    pub overlays: Vec<String>,
}

pub struct DetailsWindow {
    details: Option<Details>,
    scroll: u16,
    active: bool,
}

impl DetailsWindow {
    pub fn new() -> Self {
        Self {
            details: None,
            scroll: 0,
            active: false,
        }
    }

    pub fn set_active(&mut self, active: bool) {
        self.active = active;
    }

    pub fn render(&mut self, area: Rect, buf: &mut Buffer) {
        let title_style = if self.active {
            Style::default()
                .add_modifier(Modifier::BOLD)
                .add_modifier(Modifier::UNDERLINED)
        } else {
            Style::default()
        };
        let title = Span::styled("OCA bundle details", title_style);
        let mut max_scroll = 0u16;
        let mut content_len = 0usize;
        let widget = match &self.details {
            Some(details) => {
                let mut dependencies = details
                    .dependent
                    .iter()
                    .map(|node| {
                        Line::from(format!(
                            "      name: {}, path: {}",
                            node.refn,
                            node.path.to_str().unwrap()
                        ))
                    })
                    .collect::<Vec<_>>();
                let mut lines = vec![
                    Line::from(format!("name: {}", &details.name)),
                    Line::from(format!("id: {}", &details.id)),
                ];
                if !details.attributes.is_empty() {
                    lines.push(Line::from("Attributes: "));
                    for (name, attr_type) in &details.attributes {
                        lines.push(Line::from(format!("      {}: {}", name, attr_type)));
                    }
                }
                if !details.overlays.is_empty() {
                    lines.push(Line::from("Overlays: "));
                    for overlay in &details.overlays {
                        lines.push(Line::from(format!("      {}", overlay)));
                    }
                }
                if !dependencies.is_empty() {
                    lines.push(Line::from("Dependent files: "));
                    lines.append(&mut dependencies);
                }
                content_len = lines.len();
                let viewport = area.height.saturating_sub(2) as usize;
                max_scroll = content_len.saturating_sub(viewport) as u16;
                if self.scroll > max_scroll {
                    self.scroll = max_scroll;
                }
                Paragraph::new(lines)
                    .block(Block::bordered().title(title.clone()))
                    .scroll((self.scroll, 0))
            }
            None => Paragraph::new(vec![])
                .block(Block::bordered().title(title))
                .scroll((0, 0)),
        };
        Widget::render(widget, area, buf);
        self.render_scrollbar(area, buf, content_len, max_scroll);
    }

    pub fn set(&mut self, details: Details) {
        self.details = Some(details);
        self.scroll = 0;
    }

    pub fn clear(&mut self) {
        self.details = None;
        self.scroll = 0;
    }

    pub fn scroll_down(&mut self, amount: u16) {
        self.scroll = self.scroll.saturating_add(amount);
    }

    pub fn scroll_up(&mut self, amount: u16) {
        self.scroll = self.scroll.saturating_sub(amount);
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
}
