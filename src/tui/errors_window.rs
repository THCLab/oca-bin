use oca_rs::Facade;
use ratatui::{buffer::Buffer, layout::Rect, style::{Color, Modifier, Style}, text::Span, widgets::{Block, Paragraph, Scrollbar, ScrollbarOrientation, StatefulWidget, Widget}};
use tui_tree_widget::{Tree, TreeItem, TreeState};

use crate::{dependency_graph::DependencyGraph, error::CliError, validate};

use super::{app::AppError, bundle_list::Indexer};

pub struct ErrorsWindow<'a> {
	pub state: TreeState<String>,
	pub items: Vec<TreeItem<'a, String>>,
	errors: Vec<CliError>,
	busy: bool,
}

impl<'a> ErrorsWindow<'a> {
	pub fn new() -> Self {
		Self {errors: Vec::new(), busy: false, state: TreeState::default(), items: vec![] }
	}
	pub fn render(&mut self, area: Rect, buf: &mut Buffer) {
		// if self.busy {
		// 	// let simple = throbber_widgets_tui::Throbber::default();
		// 	// simple.render(area, buf);
		// 	let errs_view = Paragraph::new("Validation in progress. It may take some time.".to_string());
		// 	errs_view.render(area, buf);
		// } else {
        	let widget = Tree::new(self.items.clone())
            .expect("all item identifiers are unique")
            .block(Block::bordered().title("Errors"))
            .experimental_scrollbar(Some(
                Scrollbar::new(ScrollbarOrientation::VerticalRight)
                    .begin_symbol(None)
                    .track_symbol(None)
                    .end_symbol(None),
            ))
            .highlight_style(
                Style::new()
                    .fg(Color::Black)
                    .bg(Color::Red)
                    .add_modifier(Modifier::BOLD),
            )
            .highlight_symbol("> ");

        StatefulWidget::render(widget, area, buf, &mut self.state);
		// }
	}

	fn update_errors(&mut self) {
		let i = Indexer::new();
		let items = self.errors.iter()
            .map(|dep| {
				match dep {
						CliError::ValidationError(file, errors) => {
							let children = errors.into_iter().map(|err| {
								let line = Span::styled(
								format!("! {}", err.to_string()),
								Style::default()
									.fg(Color::Red)
									.add_modifier(Modifier::ITALIC),
								);
            					TreeItem::new_leaf(i.current(), line)
							}).collect();
							TreeItem::new(i.current(), file.to_str().unwrap().to_owned(), children).unwrap()

						},
						_ => todo!(),
					}
			})
            .collect();
		self.items = items;

	}

	pub fn check(&mut self, facade: &Facade, graph: &mut DependencyGraph) -> Result<bool, AppError> {
		self.busy = true;
		
        let (_oks, errs) = validate::validate_directory(&facade, graph).unwrap();
		self.errors = errs;
		self.update_errors();
		self.busy = false;

		Ok(true)
	}
}