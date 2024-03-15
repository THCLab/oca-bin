use std::os::unix::thread;

use oca_rs::Facade;
use ratatui::{buffer::Buffer, layout::Rect, widgets::{Paragraph, Widget}};

use crate::{dependency_graph::DependencyGraph, error::CliError, validate};

use super::app::AppError;

pub struct ErrorsWindow {
	errors: Vec<CliError>,
	busy: bool,
}

impl ErrorsWindow {
	pub fn new() -> Self {
		Self {errors: Vec::new(), busy: false}
	}
	pub fn render(&mut self, area: Rect, buf: &mut Buffer) {
		if self.busy {
			let simple = throbber_widgets_tui::Throbber::default();
			simple.render(area, buf);
		} else {
        	let errs = self.errors.iter().map(|err| err.to_string()).collect::<Vec<_>>().join("\n");
			let errs_view = Paragraph::new(errs);
			errs_view.render(area, buf);
		}
	}

	pub fn check(&mut self, facade: &Facade, graph: &mut DependencyGraph) -> Result<bool, AppError> {
		self.busy = true;
		
        let (_oks, errs) = validate::validate_directory(&facade, graph).unwrap();
		self.errors = errs;
		self.busy = false;

		Ok(true)
	}
}