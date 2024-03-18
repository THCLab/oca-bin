use itertools::Itertools;
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Paragraph, Widget, Wrap},
};
use tui_widget_list::ListableWidget;

use crate::error::CliError;

pub struct SimpleErrorsList {
    items: Vec<CliError>,
    pub busy: bool,
    size: usize,
}

impl SimpleErrorsList {
    pub fn new(size: usize) -> Self {
        Self {
            items: vec![],
            busy: false,
            size,
        }
    }
    pub fn update(&mut self, new_list: Vec<CliError>) {
        self.items = new_list;
        self.busy = false;
    }

    pub fn items<'a>(&self) -> Vec<ErrorLine<'a>> {
        self.items
            .iter()
            .map(|c| ErrorLine::new(c, self.size))
            .collect_vec()
    }
}

pub struct ErrorLine<'a>(Line<'a>, usize, Style);

impl<'a> ErrorLine<'a> {
    pub fn new(er: &CliError, size: usize) -> Self {
        let line = match er {
            CliError::GrammarError(file, errors) => errors
                .into_iter()
                .flat_map(|err| {
                    vec![
                        Span::styled(
                            format!("! Error in file "),
                            Style::default()
                                .fg(Color::Red)
                                .add_modifier(Modifier::ITALIC),
                        ),
                        Span::styled(
                            format!("{}:", file.to_str().unwrap()),
                            Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
                        ),
                        Span::styled(
                            format!(" {}", err.to_string()),
                            Style::default()
                                .fg(Color::Red)
                                .add_modifier(Modifier::ITALIC),
                        ),
                    ]
                })
                .collect::<Vec<_>>(),
            e => vec![Span::styled(e.to_string(), Style::default())],
        };
        let height = line.iter().map(|l| l.content.len()).sum::<usize>() as f32 / size as f32;
        Self(
            Line::from(line).style(Style::default()),
            height.ceil() as usize,
            Style::default(),
        )
    }
}

impl<'a> Widget for ErrorLine<'a> {
    fn render(self, area: Rect, buf: &mut Buffer)
    where
        Self: Sized,
    {
        let l = Text::from(self.0.clone());
        let par = Paragraph::new(l).wrap(Wrap { trim: true }).style(self.2);
        par.render(area, buf)
    }
}

impl<'a> ListableWidget for ErrorLine<'a> {
    fn size(&self, scroll_direction: &tui_widget_list::ScrollAxis) -> usize {
        self.1
    }

    fn highlight(mut self) -> Self {
        let style = Style::default().bg(Color::White);
        self.2 = style;
        self
    }
}
