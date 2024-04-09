use std::{
    io,
    path::PathBuf,
    sync::{Arc, Mutex},
    thread,
    time::Duration,
};

pub use super::bundle_list::BundleListError;
use anyhow::Result;
use crossterm::{
    event::{self, poll, Event, KeyCode, MouseEventKind},
    terminal::{disable_raw_mode, LeaveAlternateScreen},
};
use oca_rs::Facade;
use ratatui::{
    backend::Backend,
    buffer::Buffer,
    layout::{Constraint, Layout, Rect},
    style::Stylize,
    widgets::{Paragraph, Widget},
    Terminal,
};
use thiserror::Error;

use crate::{
    dependency_graph::{DependencyGraph, MutableGraph, Node},
    validate::build,
};

use super::{
    bundle_info::BundleInfo,
    bundle_list::BundleList,
    item::rebuild_items,
    output_window::{update_errors, OutputWindow},
};

#[derive(Error, Debug)]
pub enum AppError {
    #[error(transparent)]
    BundleList(#[from] BundleListError),
    #[error(transparent)]
    Input(#[from] io::Error),
    #[error("Validation error: {0}")]
    Validation(String),
}
pub struct App {
    bundles: BundleList,
    output: OutputWindow,
    facade: Arc<Mutex<Facade>>,
    graph: MutableGraph,
    active_window: Window,
    base: PathBuf,
}

enum Window {
    Errors,
    Bundles,
}

impl App {
    pub fn new<I: IntoIterator<Item = Node> + Clone>(
        base: PathBuf,
        to_show: I,
        facade: Arc<Mutex<Facade>>,
        paths: Vec<PathBuf>,
        size: usize,
    ) -> Result<App, AppError> {
        let graph = Arc::new(DependencyGraph::from_paths(&base, &paths).unwrap());
        let mut_graph = MutableGraph::new(&base, &paths);
        let list = BundleList::from_nodes(to_show, facade.clone(), graph)?;

        App::setup_panic_hooks().unwrap();

        Ok(App {
            bundles: list,
            output: OutputWindow::new(size, base.clone()),
            active_window: Window::Bundles,
            graph: mut_graph,
            facade: facade,
            base,
        })
    }
}

impl App {
    pub fn run(&mut self, mut terminal: Terminal<impl Backend>) -> Result<(), AppError> {
        loop {
            if poll(Duration::from_millis(100))? && !self.handle_input()? {
                return Ok(());
            }

            self.draw(&mut terminal)?;
        }
    }

    fn change_window(&mut self) -> bool {
        match self.active_window {
            Window::Errors => self.active_window = Window::Bundles,
            Window::Bundles => self.active_window = Window::Errors,
        }

        true
    }

    fn handle_input(&mut self) -> Result<bool, AppError> {
        match event::read()? {
            event::Event::Key(key) => {
                let items = self.bundles.items();
                let state = match self.active_window {
                    Window::Bundles => &mut self.bundles.state,
                    // Window::Errors => (&mut self.errors.state, todo!()),
                    Window::Errors => &mut self.bundles.state,
                };
                match key.code {
                    KeyCode::Char('q') => return Ok(false),
                    KeyCode::Esc => self.bundles.state.select(vec![]),
                    KeyCode::Char(' ') => state.toggle_selected(),
                    KeyCode::Enter => {
                        self.bundles.select();
                        true
                    }
                    KeyCode::Left => state.key_left(),
                    KeyCode::Right => state.key_right(),
                    KeyCode::Down => self.handle_key_down(),
                    KeyCode::Up => self.handle_key_up(),
                    KeyCode::Home => state.select_first(&items),
                    KeyCode::End => state.select_last(&items),
                    KeyCode::PageDown => state.select_visible_relative(&items, |current| {
                        current.map_or(0, |current| current.saturating_add(10))
                    }),
                    KeyCode::PageUp => state.select_visible_relative(&items, |current| {
                        current.map_or(0, |current| current.saturating_sub(10))
                    }),
                    KeyCode::Char('v') => {
                        let selected = self.bundles.selected_oca_bundle();
                        info!("Selected: {:?}", &selected.as_ref().unwrap().len());
                        if let Some(selection) = selected {
                            let paths = selection
                                .clone()
                                .iter()
                                .map(|el| self.graph.oca_file_path(&el.refn).unwrap())
                                .collect();
                            self.output.set_currently_validated(paths);

                            self.output.check(
                                self.facade.clone(),
                                self.graph.clone(),
                                selection,
                            )?;
                        };
                        true
                    }
                    KeyCode::Char('b') => {
                        let selected = self.bundles.selected_oca_bundle();
                        if let Some(selection) = selected {
                            let paths = selection
                                .clone()
                                .iter()
                                .map(|el| self.graph.oca_file_path(&el.refn).unwrap())
                                .collect();
                            self.output.set_currently_validated(paths);
                            for bundle_info in selection {
                                self.handle_build(
                                    Some(bundle_info),
                                    self.facade.clone(),
                                    self.graph.clone(),
                                )?;
                            }
                        }
                        true
                    }
                    KeyCode::Tab => self.change_window(),
                    _ => false,
                }
            }
            Event::Mouse(mouse) => match mouse.kind {
                MouseEventKind::ScrollDown => self.bundles.state.scroll_down(1),
                MouseEventKind::ScrollUp => self.bundles.state.scroll_up(1),
                _ => false,
            },
            _ => false,
        };
        Ok(true)
    }

    pub fn handle_build(
        &mut self,
        selected_bundle: Option<BundleInfo>,
        facade: Arc<Mutex<Facade>>,
        mut graph: MutableGraph,
    ) -> Result<bool, AppError> {
        self.output.mark_build();
        let current_path = self.output.current_path();
        let errs = self.output.error_list_mut();
        let list = self.bundles.items.clone();
        let to_show_dir = Arc::new(self.base.clone());

        thread::spawn(move || {
            let res = build(
                selected_bundle.as_ref(),
                facade.clone(),
                &mut graph,
                errs.clone(),
            );
            match res {
                Ok(_) => {
                    update_errors(errs.clone(), vec![], &current_path.last().unwrap());
                    rebuild_items(list, &to_show_dir, facade, graph);
                }
                Err(res) => {
                    update_errors(errs, res, &current_path.last().unwrap());
                }
            };
        });

        Ok(true)
    }

    fn handle_key_down(&mut self) -> bool {
        let items = self.bundles.items();
        match self.active_window {
            Window::Bundles => {
                let state = &mut self.bundles.state;
                state.key_down(&items);
            }
            Window::Errors => {
                let state = &mut self.output.state;
                state.next()
            }
        };
        true
    }

    fn handle_key_up(&mut self) -> bool {
        let items = self.bundles.items();
        match self.active_window {
            Window::Bundles => {
                let state = &mut self.bundles.state;
                state.key_up(&items);
            }
            Window::Errors => {
                let state = &mut self.output.state;
                state.previous()
            }
        };
        true
    }

    fn draw(&mut self, terminal: &mut Terminal<impl Backend>) -> Result<(), AppError> {
        terminal.draw(|f| f.render_widget(self, f.size()))?;
        Ok(())
    }
}

impl Widget for &mut App {
    fn render(self, area: Rect, buf: &mut Buffer) {
        // Create a space for header, todo list and the footer.
        let vertical = Layout::vertical([
            Constraint::Length(2),
            Constraint::Min(0),
            Constraint::Length(2),
        ]);
        let [header_area, rest_area, footer_area] = vertical.areas(area);

        // Create two chunks with equal horizontal screen space. One for the list and dependencies and the other for
        // the changes block.
        let vertical = Layout::vertical([Constraint::Percentage(70), Constraint::Min(0)]);
        let [list_area, output_area] = vertical.areas(rest_area);

        self.render_title(header_area, buf);
        self.bundles.render(list_area, buf);
        self.output.render(output_area, buf);
        self.render_footer(footer_area, buf);
    }
}

impl App {
    fn setup_panic_hooks() -> Result<()> {
        let original_hook = std::panic::take_hook();

        let reset_terminal = || -> Result<()> {
            disable_raw_mode()?;
            crossterm::execute!(io::stdout(), LeaveAlternateScreen)?;
            Ok(())
        };

        std::panic::set_hook(Box::new(move |panic| {
            reset_terminal().unwrap();
            original_hook(panic);
        }));
        Ok(())
    }

    fn render_title(&self, area: Rect, buf: &mut Buffer) {
        Paragraph::new("OCA Tool")
            .bold()
            .centered()
            .render(area, buf);
    }

    fn render_footer(&self, area: Rect, buf: &mut Buffer) {
        Paragraph::new("\nUse ↓↑ to move, space or enter to expand/collapse list element, `v` to validate, 'b' to build selected OCA file.")
            .centered()
            .render(area, buf);
    }
}
