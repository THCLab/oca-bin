use ratatui::{
    buffer::Buffer,
    layout::Rect,
    text::Line,
    widgets::{Block, Paragraph, Widget},
};
use said::SelfAddressingIdentifier;

pub struct Details {
    pub id: SelfAddressingIdentifier,
    // path: PathBuf,
    pub name: String,
}

pub struct DetailsWindow {
    details: Option<Details>,
}

impl DetailsWindow {
    pub fn new() -> Self {
        Self { details: None }
    }

    pub fn render(&mut self, area: Rect, buf: &mut Buffer) {
        let widget = match &self.details {
            Some(details) => {
                let lines = vec![
                    Line::from(format!("name: {}", &details.name)),
                    Line::from(format!("id: {}", &details.id)),
                ];
                Paragraph::new(lines).block(Block::bordered().title("OCA bundle details"))
            }
            None => Paragraph::new(vec![]).block(Block::bordered().title("OCA bundle details")),
        };
        Widget::render(widget, area, buf);
    }

    pub fn set(&mut self, details: Details) {
        self.details = Some(details);
    }

    pub fn clear(&mut self) {
        self.details = None;
    }
}
