use oca_bundle::state::oca::OCABundle;
use ratatui::{prelude::*, widgets::*};

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
}

#[derive(Copy, Clone, Debug)]
pub enum Status {
    Todo,
    Completed,
}

#[derive(Debug)]
pub struct BundleInfo {
    pub oca_bundle: OCABundle,
    pub refn: String,
    pub dependencies: Vec<String>,
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

    fn to_budnle(&self) {}
}
