use std::path::PathBuf;

use itertools::Itertools;

use crate::error::CliError;

#[derive(Debug)]
pub enum Message {
    Error(CliError),
    Info(String),
}

#[derive(Default, Clone)]
pub(crate) enum Busy {
    Validation,
    Building,
    Publish,
    #[default]
    NoTask,
}

#[derive(Clone)]
pub enum LastAction {
    Building,
    Validating,
    Pushing,
    NoAction,
}

pub struct MessageList {
    pub items: Vec<Message>,
    pub busy: Busy,
    pub last_action: LastAction,
}

impl MessageList {
    pub fn new() -> Self {
        Self {
            items: vec![],
            busy: Busy::NoTask,
            last_action: LastAction::NoAction,
        }
    }
    pub fn update(&mut self, new_list: Vec<Message>, source_path: &[PathBuf]) {
        for msg in new_list {
            self.items.push(msg);
        }
        match self.busy {
            Busy::Validation => self.validation_completed(),
            Busy::Building => self.build_completed(source_path),
            Busy::NoTask => self.last_action = LastAction::NoAction,
            Busy::Publish => self.pushing_completed(source_path),
        }
        self.busy = Busy::NoTask;
    }

    pub fn append(&mut self, new_list: Message) {
        self.items.push(new_list);
    }

    pub fn validation_completed(&mut self) {
        self.last_action = LastAction::Validating
    }

    pub fn pushing_completed(&mut self, path: &[PathBuf]) {
        if !self.any_error() {
            let comment = if path.is_empty() {
                "No element selected".to_string()
            } else {
                format!(
                    "Publishing successful for: {}",
                    &path.iter().map(|p| p.to_str().unwrap()).join(", ")
                )
            };
            self.items.push(Message::Info(comment));
        }
        self.last_action = LastAction::Pushing
    }

    pub fn any_error(&self) -> bool {
        self.items.iter().any(|item| match item {
            Message::Error(_) => true,
            Message::Info(_) => false,
        })
    }

    pub fn build_completed(&mut self, path: &[PathBuf]) {
        if !self.any_error() {
            let comment = if path.is_empty() {
                "No element selected".to_string()
            } else {
                format!(
                    "Building successful for: {}",
                    &path.iter().map(|p| p.to_str().unwrap()).join(", ")
                )
            };
            self.items.push(Message::Info(comment));
        }
        self.last_action = LastAction::Building
    }
}
