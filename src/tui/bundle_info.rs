use oca_bundle::state::oca::OCABundle;
use ratatui::{style::Color, text::Line, widgets::ListItem};

use crate::dependency_graph::Node;

#[derive(Copy, Clone, Debug)]
pub enum Status {
    Todo,
    Completed,
}

#[derive(Debug)]
pub struct BundleInfo {
    pub oca_bundle: OCABundle,
    pub refn: String,
    pub dependencies: Vec<Node>,
    pub status: Status,
}

impl BundleInfo {
    pub fn to_list_item(&self, index: usize) -> ListItem {
        let bg_color = match index % 2 {
            0 => Color::Green,
            _ => Color::Red,
        };
        let line = match self.status {
            Status::Todo => Line::styled(format!(" ☐ {}", self.refn), bg_color),
            Status::Completed => Line::styled(format!(" ✓ {}", self.refn), bg_color),
        };

        ListItem::new(line)
    }
}
