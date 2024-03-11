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
