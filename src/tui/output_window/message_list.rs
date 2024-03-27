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

pub enum Message {
    Error(CliError),
    Info(String),
}

#[derive(Default, Clone)]
pub(crate) enum Busy {
    Validation,
    Building,
    #[default]
    NoTask
}

#[derive(Clone)]
pub enum LastAction {
    Building,
    Validating,
    NoAction,
}

pub struct MessageList {
    items: Vec<Message>,
    pub busy: Busy,
    size: usize,
    pub last_action: LastAction,
}

impl MessageList {
    pub fn new(size: usize) -> Self {
        Self {
            items: vec![],
            busy: Busy::NoTask,
            size,
            last_action: LastAction::NoAction,
        }
    }
    pub fn update(&mut self, new_list: Vec<CliError>) {
        let errs = new_list.into_iter().map(Message::Error).collect();
        self.items = errs;
        match self.busy {
            Busy::Validation => self.validation_completed(),
            Busy::Building => self.build_completed(),
            Busy::NoTask => self.last_action = LastAction::NoAction,
        }
        self.busy = Busy::NoTask;
    }

    pub fn items<'a>(&'a self) -> Vec<MessageLine<'a>> {
        self.items
            .iter()
            .map(|c| MessageLine::new(c, self.size))
            .collect_vec()
    }


    pub fn validation_completed(&mut self) {
        self.last_action = LastAction::Validating
    }

    pub fn build_completed(&mut self) {
        self.last_action = LastAction::Building
    }

}

pub struct MessageLine<'a>(Line<'a>, usize, Style);

impl<'a> MessageLine<'a> {
    pub fn new(er: &'a Message, size: usize) -> Self {
        let line = match er {
            Message::Error(CliError::GrammarError(file, errors)) => errors
                .iter()
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
                            format!(" {}", err),
                            Style::default()
                                .fg(Color::Red)
                                .add_modifier(Modifier::ITALIC),
                        ),
                    ]
                })
                .collect::<Vec<_>>(),
            Message::Error(e) => vec![Span::styled(e.to_string(), Style::default())],
            Message::Info(info) => vec![Span::styled(info,  Style::default().fg(Color::Green))],
        };
        let height = line.iter().map(|l| l.content.len()).sum::<usize>() as f32 / size as f32;
        Self(
            Line::from(line).style(Style::default()),
            height.ceil() as usize,
            Style::default(),
        )
    }
}

impl<'a> Widget for MessageLine<'a> {
    fn render(self, area: Rect, buf: &mut Buffer)
    where
        Self: Sized,
    {
        let l = Text::from(self.0.clone());
        let par = Paragraph::new(l).wrap(Wrap { trim: true }).style(self.2);
        par.render(area, buf)
    }
}

impl<'a> ListableWidget for MessageLine<'a> {
    fn size(&self, _scroll_direction: &tui_widget_list::ScrollAxis) -> usize {
        self.1
    }

    fn highlight(mut self) -> Self {
        let style = Style::default().bg(Color::White);
        self.2 = style;
        self
    }
}
