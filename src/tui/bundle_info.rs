use oca_bundle::state::oca::OCABundle;

use crate::dependency_graph::Node;

#[derive(Copy, Clone, Debug)]
pub enum Status {
    _Selected,
    Unselected,
}

#[derive(Debug, Clone)]
pub struct BundleInfo {
    pub oca_bundle: OCABundle,
    pub refn: String,
    pub dependencies: Vec<Node>,
    pub status: Status,
}
