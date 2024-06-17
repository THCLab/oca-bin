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
    event::{self, poll, Event, KeyCode, KeyModifiers, MouseEventKind},
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
    dependency_graph::{parse_name, DependencyGraph, MutableGraph, Node},
    publish_oca_file_for, saids_to_publish,
    tui::output_window::message_list::Message,
    validate::build,
};

use super::{
    bundle_list::BundleList,
    changes::ChangesWindow,
    item::{rebuild_items, Element},
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
    remote_repository: Option<String>,
    changes: ChangesWindow,
    publish_timeout: Option<u64>,
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
        remote_repo_url: Option<String>,
        publish_timeout: Option<u64>,
    ) -> Result<App, AppError> {
        let graph = match DependencyGraph::from_paths(&base, &paths) {
            Ok(graph) => Ok(Arc::new(graph)),
            Err(e) => Err(AppError::BundleList(BundleListError::GraphError(e))),
        }?;
        let mut_graph = MutableGraph::new(&base, &paths);
        let list = BundleList::from_nodes(to_show, facade.clone(), graph)?;

        App::setup_panic_hooks()?;
        let changes = ChangesWindow::new(&base, mut_graph.clone());

        Ok(App {
            bundles: list,
            output: OutputWindow::new(size),
            active_window: Window::Bundles,
            graph: mut_graph,
            facade: facade,
            base,
            remote_repository: remote_repo_url,
            changes,
            publish_timeout,
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

    fn update_changes(&self) {
        self.changes.update();
    }

    fn handle_input(&mut self) -> Result<bool, AppError> {
        Ok(match event::read()? {
            event::Event::Key(key) => {
                let items = self.bundles.items();
                let state = match self.active_window {
                    Window::Bundles => &mut self.bundles.state,
                    // Window::Errors => (&mut self.errors.state, todo!()),
                    Window::Errors => &mut self.bundles.state,
                };
                match key.code {
                    KeyCode::Char('q') => return Ok(false),
                    KeyCode::Esc => self.bundles.unselect_all(),
                    KeyCode::Enter => state.toggle_selected(),
                    KeyCode::Char(' ') => {
                        self.bundles.select();
                        true
                    }
                    KeyCode::Char('a') if key.modifiers.eq(&KeyModifiers::CONTROL) => {
                        self.bundles.select_all()
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
                        let paths = selected.iter().map(|el| el.path().to_path_buf()).collect();
                        self.output.set_currently_validated(paths);

                        self.output.handle_validate(
                            self.facade.clone(),
                            self.graph.clone(),
                            selected,
                            self.base.clone(),
                        )?
                    }
                    KeyCode::Char('b') => {
                        let selected = self.bundles.selected_oca_bundle();
                        let paths = selected.iter().map(|el| el.path().to_path_buf()).collect();
                        self.output.set_currently_validated(paths);
                        self.handle_build(selected, self.facade.clone(), self.graph.clone())?
                    }
                    KeyCode::Char('p') => {
                        let selected = self.bundles.selected_oca_bundle();
                        let paths = selected.iter().map(|el| el.path().to_path_buf()).collect();
                        self.output.set_currently_validated(paths);
                        self.handle_publish(selected, self.facade.clone())?
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
        })
    }

    pub fn handle_build(
        &mut self,
        selected_bundle: Vec<Element>,
        facade: Arc<Mutex<Facade>>,
        mut graph: MutableGraph,
    ) -> Result<bool, AppError> {
        self.output.mark_build();
        let current_path = self.output.current_path();
        let errs = self.output.error_list_mut();
        let list = self.bundles.items.clone();
        let to_show_dir = Arc::new(self.base.clone());
        let base_path = self.base.clone();
        let changes = self.changes.changes();

        thread::spawn(move || {
            let mut updated_nodes: Vec<PathBuf> = vec![];
            let res: Vec<_> = selected_bundle
                .iter()
                .flat_map(|el| {
                    let (name, path) = match el {
                        Element::Ok(oks) => {
                            (Some(oks.get().refn.clone()), oks.path().to_path_buf())
                        }
                        Element::Error(errors) => {
                            let mut path = base_path.clone();
                            path.push(errors.path());
                            (parse_name(path.as_path()).unwrap().0, path)
                        }
                    };
                    if let Some(_) = name {
                        updated_nodes.push(path);
                    };
                    match build(name, facade.clone(), &mut graph, errs.clone()) {
                        Ok(_) => vec![],
                        Err(errs) => errs,
                    }
                })
                .collect();
            if res.is_empty() {
                update_errors(errs.clone(), vec![], &current_path);
                rebuild_items(list, &to_show_dir, facade, graph);
            } else {
                update_errors(errs, res, &current_path);
            };
            {
                let mut tmp_changes = changes.lock().unwrap();
                tmp_changes.load();
            }
        });

        Ok(true)
    }

    pub fn handle_publish(
        &mut self,
        selected_bundle: Vec<Element>,
        facade: Arc<Mutex<Facade>>,
    ) -> Result<bool, AppError> {
        info!("Handling publish");
        self.output.mark_publish();
        let current_path = self.output.current_path();
        let errs = self.output.error_list_mut();
        let remote_repository = self.remote_repository.clone();
        let timeout = self.publish_timeout;

        thread::spawn(move || {
            let saids: Result<Vec<_>, AppError> = selected_bundle
                .into_iter()
                .map(|el| match el {
                    Element::Ok(oks) => Ok(oks.get().oca_bundle.said.clone().unwrap()),
                    Element::Error(errors) => {
                        info!("Error selected for publish {:?}", &errors.path());
                        Err(AppError::BundleList(BundleListError::ErrorSelected(
                            errors.path().into(),
                        )))
                    }
                })
                .collect();
            match saids {
                Ok(saids) => {
                    // Find dependant saids for said. Returns set of unique saids that need to be published.
                    let saids_to_publish = saids_to_publish(facade.clone(), &saids);
                    // Make post request for all saids
                    let res: Vec<_> = saids_to_publish
                        .iter()
                        .flat_map(|said| {
                            match publish_oca_file_for(
                                facade.clone(),
                                said.clone(),
                                &timeout,
                                &remote_repository,
                                &None,
                            ) {
                                Ok(_) => {
                                    let mut i = errs.lock().unwrap();
                                    i.append(Message::Info(format!(
                                        "Published {} to {}",
                                        said,
                                        remote_repository.as_ref().unwrap()
                                    )));
                                    vec![]
                                }
                                Err(err) => vec![err],
                            }
                        })
                        .collect();
                    update_errors(errs.clone(), res, &current_path);
                }
                Err(AppError::BundleList(e)) => {
                    update_errors(errs.clone(), vec![e.into()], &current_path);
                }
                e => {
                    info!("Unhandled error: {:?}", e);
                    todo!()
                }
            }
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
        let horizontal = Layout::horizontal([Constraint::Percentage(70), Constraint::Min(0)]);
        let [list_area, changes_area] = horizontal.areas(list_area);

        self.render_title(header_area, buf);
        self.bundles.render(list_area, buf);
        self.output.render(output_area, buf);
        self.changes.render(changes_area, buf);
        self.render_footer(footer_area, buf);
    }
}

impl App {
    fn setup_panic_hooks() -> io::Result<()> {
        let original_hook = std::panic::take_hook();

        let reset_terminal = || -> io::Result<()> {
            disable_raw_mode()?;
            crossterm::execute!(io::stdout(), LeaveAlternateScreen)?;
            Ok(())
        };

        std::panic::set_hook(Box::new(move |panic| {
            let _ = reset_terminal();
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
        Paragraph::new("\nUse ↓↑ to move, ← → to expand/collapse list element, space to select element, `v` to validate selected elements, 'b' to build selected OCA files, 'p' to publish selected OCA files.")
            .centered()
            .render(area, buf);
    }
}
