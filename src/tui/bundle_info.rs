use oca_bundle::state::oca::OCABundle;

use crate::dependency_graph::Node;

#[derive(Copy, Clone, Debug)]
pub enum Status {
    Selected,
    Unselected,
}

impl Status {
    pub fn toggle(&self) -> Self {
        match self {
            Status::Selected => Self::Unselected,
            Status::Unselected => Self::Selected,
        }
    }
}

#[derive(Debug, Clone)]
pub struct BundleInfo {
    pub oca_bundle: OCABundle,
    pub refn: String,
    pub dependencies: Vec<Node>,
    pub status: Status,
}

impl BundleInfo {
    pub fn change_state(&mut self) {
        info!("changing state: {}", &self.refn);
        self.status = self.status.toggle()
    }
}
