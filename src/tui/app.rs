use std::{
    collections::HashMap,
    io,
    panic::AssertUnwindSafe,
    path::PathBuf,
    sync::{Arc, Mutex},
    thread,
    time::{Duration, Instant},
};

pub use super::bundle_list::BundleListError;
use anyhow::Result;
use crossterm::event::{self, poll, Event, KeyCode, KeyModifiers, MouseEventKind};
use oca_rs::Facade;
use ratatui::{
    backend::Backend,
    buffer::Buffer,
    layout::{Constraint, Layout, Rect},
    style::{Modifier, Style, Stylize},
    text::{Line, Span},
    widgets::{Paragraph, Widget},
    Terminal,
};
use thiserror::Error;
use url::Url;

use crate::{
    dependency_graph::{parse_name, DependencyGraph, MutableGraph, Node, NodeParsingError},
    error::CliError,
    publish_oca_file_for, saids_to_publish,
    tui::{details::Details, get_oca_bundle_by_said, output_window::message_list::Message},
    utils::{handle_panic, parse_url},
    validate::build,
};

use super::{
    bundle_list::BundleList,
    changes::ChangesWindow,
    details::DetailsWindow,
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
    #[error("No repository path set. You can set it by adding `repository_url` to config file.")]
    UnknownRemoteRepoUrl,
    #[error("Remote repository url parse error: {0}")]
    WrongUrl(#[from] url::ParseError),
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
    details: DetailsWindow,
    publish_timeout: Option<u64>,
}

enum Window {
    Errors,
    Bundles,
    Help,
    Changes,
}

impl App {
    pub fn new<I: IntoIterator<Item = Result<Node, NodeParsingError>> + Clone>(
        base: PathBuf,
        to_show: I,
        facade: Arc<Mutex<Facade>>,
        paths: Vec<PathBuf>,
        size: usize,
        remote_repo_url: Option<String>,
        publish_timeout: Option<u64>,
    ) -> Result<App, AppError> {
        let graph = match DependencyGraph::from_paths(&paths) {
            Ok(graph) => Ok(Arc::new(graph)),
            Err(e) => Err(AppError::BundleList(BundleListError::GraphError(e))),
        }?;
        let mut_graph = MutableGraph::new(&paths)
            .map_err(|e| AppError::BundleList(BundleListError::GraphError(e)))?;
        let list = BundleList::from_nodes(to_show, facade.clone(), graph, base.clone())?;

        App::setup_panic_hooks()?;
        let changes = ChangesWindow::new(&base, mut_graph.clone());
        let details = DetailsWindow::new();

        Ok(App {
            bundles: list,
            output: OutputWindow::new(size),
            active_window: Window::Bundles,
            graph: mut_graph,
            facade,
            base,
            remote_repository: remote_repo_url,
            changes,
            publish_timeout,
            details,
        })
    }
}

impl App {
    pub fn run(&mut self, mut terminal: Terminal<impl Backend>) -> Result<(), AppError> {
        loop {
            if poll(Duration::from_millis(100))? && !self.handle_input() {
                return Ok(());
            }

            self.draw(&mut terminal)?;
        }
    }

    fn change_window(&mut self) -> bool {
        match self.active_window {
            Window::Errors => self.active_window = Window::Bundles,
            Window::Bundles => self.active_window = Window::Changes,
            Window::Help => self.active_window = Window::Bundles,
            Window::Changes => self.active_window = Window::Errors,
        }

        true
    }

    fn handle_input(&mut self) -> bool {
        let output = if let Window::Help = self.active_window {
            match event::read() {
                Ok(_) => {
                    self.active_window = Window::Bundles;
                    Ok(true)
                }
                Err(e) => Err(CliError::Input(e)),
            }
        } else {
            let output = match event::read() {
                Ok(event::Event::Key(key)) => {
                    let items = self.bundles.items();
                    let state = match self.active_window {
                        Window::Errors => &mut self.bundles.state,
                        Window::Bundles => &mut self.bundles.state,
                        Window::Changes => &mut self.changes.state,
                        Window::Help => todo!(),
                    };
                    match key.code {
                        KeyCode::Char('q') => return false,
                        KeyCode::Esc => Ok(self.bundles.unselect_all()),
                        KeyCode::Enter => Ok(state.toggle_selected()),
                        KeyCode::Char(' ') => {
                            self.bundles.select();
                            Ok(true)
                        }
                        KeyCode::Char('a') if key.modifiers.eq(&KeyModifiers::CONTROL) => {
                            self.bundles.select_all();
                            Ok(true)
                        }
                        KeyCode::Left => {
                            state.key_left();
                            Ok(true)
                        }
                        KeyCode::Right => {
                            state.key_right();
                            Ok(true)
                        }
                        KeyCode::Down => Ok(self.handle_key_down()),
                        KeyCode::Up => Ok(self.handle_key_up()),
                        KeyCode::Home => {
                            state.select_first(&items);
                            Ok(true)
                        }
                        KeyCode::End => {
                            state.select_last(&items);
                            Ok(true)
                        }
                        KeyCode::PageDown => Ok(state.select_visible_relative(&items, |current| {
                            current.map_or(0, |current| current.saturating_add(10))
                        })),
                        KeyCode::PageUp => Ok(state.select_visible_relative(&items, |current| {
                            current.map_or(0, |current| current.saturating_sub(10))
                        })),
                        KeyCode::Char('v') => {
                            let selected = self.bundles.selected_oca_bundle();
                            let paths = selected.iter().map(|el| el.path().to_path_buf()).collect();
                            self.output.set_currently_validated(paths);

                            self.output.handle_validate(
                                self.facade.clone(),
                                self.graph.clone(),
                                selected,
                            )
                        }
                        KeyCode::Char('b') => {
                            let selected = self.bundles.selected_oca_bundle();
                            let paths = selected.iter().map(|el| el.path().to_path_buf()).collect();
                            self.output.set_currently_validated(paths);
                            self.handle_build(selected, self.facade.clone(), self.graph.clone())
                        }
                        KeyCode::Char('p') => {
                            let selected = self.bundles.selected_oca_bundle();
                            let paths = selected.iter().map(|el| el.path().to_path_buf()).collect();
                            self.output.set_currently_validated(paths);
                            self.handle_publish(selected, self.facade.clone())
                        }
                        KeyCode::Tab => Ok(self.change_window()),
                        KeyCode::F(1) => {
                            self.active_window = Window::Help;
                            Ok(true)
                        }
                        _ => Ok(true),
                    }
                }
                Ok(Event::Mouse(mouse)) => Ok(match mouse.kind {
                    MouseEventKind::ScrollDown => self.bundles.state.scroll_down(1),
                    MouseEventKind::ScrollUp => self.bundles.state.scroll_up(1),
                    _ => true,
                }),
                Ok(_) => Ok(true),
                Err(e) => Err(CliError::Input(e)),
            };
            match self.bundles.currently_pointed() {
                Some(pointed) => {
                    let dependent = self.graph.get_ancestors([pointed.refn.as_str()], false);
                    match dependent {
                        Ok(dependent) => {
                            self.details.set(Details {
                                id: pointed.oca_bundle.said.unwrap(),
                                name: pointed.refn,
                                dependent,
                            });
                            output
                        }
                        Err(e) => Err(CliError::GraphError(e)),
                    }
                }
                None => {
                    self.details.clear();
                    output
                }
            }
        };
        match output {
            Ok(out) => out,
            Err(er) => {
                let output_window = self.output.error_list_mut();
                let mut out = output_window.lock().unwrap();
                out.append(Message::Error(er));
                true
            }
        }
    }

    pub fn handle_build(
        &mut self,
        selected_bundle: Vec<Element>,
        facade: Arc<Mutex<Facade>>,
        mut graph: MutableGraph,
    ) -> Result<bool, CliError> {
        if let Err(e) = self.graph.reload(&self.base) {
            let err_msg = Message::Error(e.into());
            let errs = self.output.error_list_mut();
            let mut mut_errs = errs.lock().unwrap();
            mut_errs.append(err_msg);
            return Ok(true);
        };

        self.output.mark_build();
        let current_path = self.output.current_path();
        let errs = self.output.error_list_mut();
        let list = self.bundles.items.clone();
        let to_show_dir = Arc::new(self.base.clone());
        let changes = self.changes.changes();

        thread::spawn(move || {
            let start = Instant::now();
            let mut updated_nodes: Vec<PathBuf> = vec![];
            let mut cache = vec![];
            let unwind_res = std::panic::catch_unwind(AssertUnwindSafe(|| {
                selected_bundle
                    .iter()
                    .flat_map(|el| {
                        let (name, path, index) = match el {
                            Element::Ok(oks) => (
                                Some(oks.get().refn.clone()),
                                oks.path().to_path_buf(),
                                oks.index(),
                            ),
                            Element::Error(errors) => {
                                let path = errors.path().to_path_buf();
                                (parse_name(path.as_path()).unwrap().0, path, errors.index())
                            }
                        };
                        if name.is_some() {
                            updated_nodes.push(path);
                        };
                        info!("{:?}", &cache);
                        match build(
                            name.clone(),
                            facade.clone(),
                            &mut graph,
                            errs.clone(),
                            &cache,
                        ) {
                            Ok(mut cached) => {
                                cache.append(&mut cached);
                                let mut items = list.lock().unwrap();
                                items.update_state(&index.unwrap());
                                vec![]
                            }
                            Err(errs) => errs,
                        }
                    })
                    .collect::<Vec<_>>()
            }));
            let elapsed = start.elapsed();

            info!("Building time: {} seconds", elapsed.as_secs());

            let res = match unwind_res {
                Ok(err) => err,
                Err(panic) => {
                    vec![handle_panic(panic)]
                }
            };

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
        &self,
        selected_bundle: Vec<Element>,
        facade: Arc<Mutex<Facade>>,
    ) -> Result<bool, CliError> {
        info!("Handling publish");
        let current_path = self.output.current_path();
        let errs = self.output.error_list_mut();
        let remote_repository: Url = parse_url(
            self.remote_repository
                .as_ref()
                .ok_or(CliError::UnknownRemoteRepoUrl)?
                .clone(),
        )?;
        self.output.mark_publish();
        let timeout = self.publish_timeout;
        let list = self.bundles.items.clone();

        thread::spawn(move || {
            let mut said_index_map = HashMap::new();
            let saids: Result<Vec<_>, AppError> = selected_bundle
                .into_iter()
                .map(|el| match el {
                    Element::Ok(oks) => {
                        let said = oks.get().oca_bundle.said.clone().unwrap();
                        if let Some(index) = oks.index() {
                            said_index_map.insert(said.clone(), index);
                        }
                        Ok(said)
                    }
                    Element::Error(errors) => Err(AppError::BundleList(
                        BundleListError::ErrorSelected(errors.path().into()),
                    )),
                })
                .collect();
            match saids {
                Ok(saids) => {
                    // Find dependant saids for said. Returns set of unique saids that need to be published.
                    let saids_to_publish = saids_to_publish(facade.clone(), &saids);
                    // Make post request for all saids
                    let unwind_res = std::panic::catch_unwind(AssertUnwindSafe(|| {
                        saids_to_publish
                            .iter()
                            .flat_map(|said| {
                                match publish_oca_file_for(
                                    facade.clone(),
                                    said.clone(),
                                    &timeout,
                                    remote_repository.clone(),
                                ) {
                                    Ok(_) => {
                                        match get_oca_bundle_by_said(said, facade.clone()) {
                                            Ok((name, _bundle)) => {
                                                {
                                                    let mut i = errs.lock().unwrap();
                                                    i.append(Message::Info(format!(
                                                        "Published {} to {}",
                                                        name,
                                                        remote_repository.as_ref()
                                                    )));
                                                }
                                                {
                                                    let mut items = list.lock().unwrap();
                                                    if let Some(index) = said_index_map.get(said) {
                                                        items.update_state(index);
                                                    };
                                                }
                                            }
                                            Err(e) => {
                                                let mut i = errs.lock().unwrap();
                                                i.append(Message::Error(e));
                                            }
                                        };

                                        vec![]
                                    }
                                    Err(err) => vec![err],
                                }
                            })
                            .collect::<Vec<_>>()
                    }));
                    info!("{:?}", unwind_res);
                    let res = match unwind_res {
                        Ok(res) => res,
                        Err(panic) => vec![handle_panic(panic)],
                    };
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
            Window::Help => {
                self.active_window = Window::Bundles;
            }
            Window::Changes => {
                let items = self.changes.items();
                let state: &mut tui_tree_widget::TreeState<String> = &mut self.changes.state;
                state.key_down(&items);
            }
        };
        true
    }

    fn handle_key_up(&mut self) -> bool {
        match self.active_window {
            Window::Bundles => {
                let items = self.bundles.items();
                let state = &mut self.bundles.state;
                state.key_up(&items);
            }
            Window::Errors => {
                let state = &mut self.output.state;
                state.previous()
            }
            Window::Help => {
                self.active_window = Window::Bundles;
            }
            Window::Changes => {
                let items = self.changes.items();
                let state: &mut tui_tree_widget::TreeState<String> = &mut self.changes.state;
                state.key_up(&items);
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
        // Create a space for header, list and the footer.
        let vertical = Layout::vertical([
            Constraint::Length(2),
            Constraint::Min(0),
            Constraint::Length(2),
        ]);

        if let Window::Help = self.active_window {
            let [header_area, rest_area, _footer] = vertical.areas(area);
            self.render_title(header_area, buf, "Help");
            self.render_help(rest_area, buf);
        } else {
            let [header_area, rest_area, footer_area] = vertical.areas(area);

            // Create two chunks with equal horizontal screen space. One for the list and dependencies and the other for
            // the changes block.
            let vertical = Layout::vertical([Constraint::Percentage(70), Constraint::Min(0)]);
            let [list_area, output_area] = vertical.areas(rest_area);
            let horizontal = Layout::horizontal([Constraint::Percentage(70), Constraint::Min(0)]);
            let [list_area, details_area] = horizontal.areas(list_area);

            self.render_title(header_area, buf, "OCA tool");
            self.bundles.render(list_area, buf);
            self.output.render(output_area, buf);
            // self.changes.render(changes_area, buf);
            self.details.render(details_area, buf);
            self.render_footer(footer_area, buf);
        }
    }
}

impl App {
    pub fn setup_panic_hooks() -> io::Result<()> {
        std::panic::set_hook(Box::new(move |panic| error!("{:?}", panic)));
        Ok(())
    }

    fn render_title(&self, area: Rect, buf: &mut Buffer, title: &str) {
        Paragraph::new(title).bold().centered().render(area, buf);
    }

    fn render_footer(&self, area: Rect, buf: &mut Buffer) {
        Paragraph::new("Press F1 to open help window")
            .centered()
            .render(area, buf);
    }

    fn render_help(&self, area: Rect, buf: &mut Buffer) {
        let commands = vec![
            ("↓↑", "scroll list elements"),
            ("← →", "expand/collapse list element"),
            ("PageUp", "move 10 position up the list"),
            ("PageDown", "move 10 positions down the list"),
            ("Home", "move to first element"),
            ("End", "move to last element"),
            ("space", "select element"),
            ("Ctrl + A", "select all"),
            ("v", "validate selected OCA files"),
            ("b", "build selected OCA files"),
            ("p", "publish selected OCA files"),
            ("F1", "Open help"),
        ];

        let lines: Vec<_> = commands
            .into_iter()
            .map(|(command, role)| {
                Line::from(vec![
                    Span::styled(command, Style::default().add_modifier(Modifier::BOLD)),
                    Span::styled(format!("    {}", role), Style::default()),
                ])
            })
            .collect();

        Paragraph::new(lines).render(area, buf);
    }
}
