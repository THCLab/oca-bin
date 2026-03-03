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
use oca_sdk_rs::oca::{
    bundle::OCABundle,
    overlay_file::{NestedAttrType, OverlayLocalRegistry},
};
use oca_store::Facade as Store;
use ratatui::{
    backend::Backend,
    buffer::Buffer,
    layout::{Constraint, Layout, Rect},
    style::{Modifier, Style, Stylize},
    text::{Line, Span},
    widgets::{Paragraph, Widget},
    Terminal,
};
use serde_json::Value;
use thiserror::Error;
use url::Url;

use crate::{
    config::Config,
    dependency_graph::{parse_name, DependencyGraph, MutableGraph, Node, NodeParsingError},
    error::CliError,
    publish_oca_file_for, saids_to_publish,
    tui::{
        details::Details,
        get_oca_bundle_by_said,
        output_window::message_list::{Message, MessageList},
    },
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
    facade: Arc<Mutex<Store>>,
    graph: MutableGraph,
    active_window: Window,
    base: PathBuf,
    changes: ChangesWindow,
    details: DetailsWindow,
    // TODO move to config
    publish_timeout: Option<u64>,
    config: Config,
}

#[allow(dead_code)]
enum Window {
    Errors,
    Bundles,
    Help,
    Changes,
    Details,
}

fn format_attr_type(attr_type: &NestedAttrType) -> String {
    match attr_type {
        NestedAttrType::Reference(reference) => format!("reference({})", reference),
        NestedAttrType::Value(value) => value.to_string(),
        NestedAttrType::Array(inner) => format!("array<{}>", format_attr_type(inner)),
        NestedAttrType::Null => "null".to_string(),
    }
}

fn format_overlay_name(name: &str) -> String {
    name.strip_prefix("overlay/").unwrap_or(name).to_string()
}

fn format_json_value(value: &Value) -> String {
    match value {
        Value::Null => "null".to_string(),
        Value::Bool(v) => v.to_string(),
        Value::Number(v) => v.to_string(),
        Value::String(v) => v.clone(),
        Value::Array(items) => {
            let rendered = items
                .iter()
                .map(format_json_value)
                .collect::<Vec<_>>()
                .join(", ");
            format!("[{}]", rendered)
        }
        Value::Object(map) => {
            let rendered = map
                .iter()
                .map(|(k, v)| format!("{}={}", k, format_json_value(v)))
                .collect::<Vec<_>>()
                .join(", ");
            format!("{{{}}}", rendered)
        }
    }
}

fn append_info_lines(errs: Arc<Mutex<MessageList>>, header: &str, content: &str) {
    let mut output = errs.lock().unwrap();
    output.append(Message::Info(header.to_string()));
    for line in content.lines() {
        output.append(Message::Info(line.to_string()));
    }
}

impl App {
    pub fn new<I: IntoIterator<Item = Result<Node, NodeParsingError>> + Clone>(
        base: PathBuf,
        to_show: I,
        facade: Arc<Mutex<Store>>,
        paths: Vec<PathBuf>,
        publish_timeout: Option<u64>,
        config: Config,
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
            output: OutputWindow::new(),
            active_window: Window::Bundles,
            graph: mut_graph,
            facade,
            base,
            changes,
            publish_timeout,
            details,
            config,
        })
    }
}

impl App {
    pub fn run(
        &mut self,
        mut terminal: Terminal<impl Backend<Error = io::Error>>,
    ) -> Result<(), AppError> {
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
            Window::Bundles => self.active_window = Window::Details,
            Window::Help => self.active_window = Window::Bundles,
            Window::Changes => self.active_window = Window::Bundles,
            Window::Details => self.active_window = Window::Errors,
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
                    match key.code {
                        KeyCode::Char('q') => return false,
                        KeyCode::Esc => Ok(self.bundles.unselect_all()),
                        KeyCode::Enter => match self.active_window {
                            Window::Bundles => Ok(self.bundles.state.toggle_selected()),
                            Window::Changes => Ok(self.changes.state.toggle_selected()),
                            _ => Ok(true),
                        },
                        KeyCode::Char(' ') => {
                            self.bundles.select();
                            Ok(true)
                        }
                        KeyCode::Char('a') if key.modifiers.eq(&KeyModifiers::CONTROL) => {
                            self.bundles.select_all();
                            Ok(true)
                        }
                        KeyCode::Left => match self.active_window {
                            Window::Bundles => {
                                self.bundles.state.key_left();
                                Ok(true)
                            }
                            Window::Changes => {
                                self.changes.state.key_left();
                                Ok(true)
                            }
                            _ => Ok(true),
                        },
                        KeyCode::Right => match self.active_window {
                            Window::Bundles => {
                                self.bundles.state.key_right();
                                Ok(true)
                            }
                            Window::Changes => {
                                self.changes.state.key_right();
                                Ok(true)
                            }
                            _ => Ok(true),
                        },
                        KeyCode::Down => Ok(self.handle_key_down()),
                        KeyCode::Up => Ok(self.handle_key_up()),
                        KeyCode::Home => match self.active_window {
                            Window::Bundles => {
                                self.bundles.state.select_first();
                                Ok(true)
                            }
                            Window::Changes => {
                                self.changes.state.select_first();
                                Ok(true)
                            }
                            _ => Ok(true),
                        },
                        KeyCode::End => match self.active_window {
                            Window::Bundles => {
                                self.bundles.state.select_last();
                                Ok(true)
                            }
                            Window::Changes => {
                                self.changes.state.select_last();
                                Ok(true)
                            }
                            _ => Ok(true),
                        },
                        KeyCode::PageDown => match self.active_window {
                            Window::Bundles => Ok(self.bundles.state.select_relative(|current| {
                                current.map_or(0, |current| current.saturating_add(10))
                            })),
                            Window::Changes => Ok(self.changes.state.select_relative(|current| {
                                current.map_or(0, |current| current.saturating_add(10))
                            })),
                            Window::Errors => {
                                self.output.scroll_down(10);
                                Ok(true)
                            }
                            Window::Details => {
                                self.details.scroll_down(10);
                                Ok(true)
                            }
                            Window::Help => Ok(true),
                        },
                        KeyCode::PageUp => match self.active_window {
                            Window::Bundles => Ok(self.bundles.state.select_relative(|current| {
                                current.map_or(0, |current| current.saturating_sub(10))
                            })),
                            Window::Changes => Ok(self.changes.state.select_relative(|current| {
                                current.map_or(0, |current| current.saturating_sub(10))
                            })),
                            Window::Errors => {
                                self.output.scroll_up(10);
                                Ok(true)
                            }
                            Window::Details => {
                                self.details.scroll_up(10);
                                Ok(true)
                            }
                            Window::Help => Ok(true),
                        },
                        KeyCode::Char('v') => {
                            let selected = self.bundles.selected_oca_bundle();
                            let paths = selected.iter().map(|el| el.path().to_path_buf()).collect();
                            // TODO take from config
                            let registry = OverlayLocalRegistry::from_dir(
                                "../oca-rs/overlay-file/core_overlays/",
                            )
                            .unwrap();
                            self.output.set_currently_validated(paths);

                            self.output.handle_validate(
                                self.facade.clone(),
                                self.graph.clone(),
                                selected,
                                registry.clone(),
                            )
                        }
                        KeyCode::Char('b') => {
                            let selected = self.bundles.selected_oca_bundle();
                            let paths = selected.iter().map(|el| el.path().to_path_buf()).collect();
                            self.output.set_currently_validated(paths);
                            self.handle_build(selected, self.facade.clone(), self.graph.clone())
                        }
                        KeyCode::Char('o') => {
                            let errs = self.output.error_list_mut();
                            match self.bundles.currently_pointed() {
                                Some(pointed) => {
                                    let said = pointed.oca_bundle.digest.clone().unwrap();
                                    match self
                                        .facade
                                        .lock()
                                        .unwrap()
                                        .get_oca_bundle_ocafile(said, false)
                                    {
                                        Ok(ocafile) => {
                                            append_info_lines(errs, "OCAFILE:", &ocafile);
                                            Ok(true)
                                        }
                                        Err(errs) => Err(CliError::OcaBundleAstError(errs)),
                                    }
                                }
                                None => {
                                    let mut output = errs.lock().unwrap();
                                    output.append(Message::Info("No bundle selected".to_string()));
                                    Ok(true)
                                }
                            }
                        }
                        KeyCode::Char('s') => {
                            let errs = self.output.error_list_mut();
                            match self.bundles.currently_pointed() {
                                Some(pointed) => {
                                    let oca_bundle = OCABundle::from(pointed.oca_bundle.clone());
                                    match serde_json::to_string_pretty(&oca_bundle) {
                                        Ok(json) => {
                                            append_info_lines(errs, "OCA BUNDLE JSON:", &json);
                                            Ok(true)
                                        }
                                        Err(err) => Err(CliError::WriteOcaError(err)),
                                    }
                                }
                                None => {
                                    let mut output = errs.lock().unwrap();
                                    output.append(Message::Info("No bundle selected".to_string()));
                                    Ok(true)
                                }
                            }
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
                    MouseEventKind::ScrollDown => self.handle_key_down(),
                    MouseEventKind::ScrollUp => self.handle_key_up(),
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
                            let attributes = pointed
                                .oca_bundle
                                .capture_base
                                .attributes
                                .iter()
                                .map(|(name, attr_type)| {
                                    (name.clone(), format_attr_type(attr_type))
                                })
                                .collect();
                            let overlays = pointed
                                .oca_bundle
                                .overlays
                                .iter()
                                .map(|overlay| {
                                    let base_name = overlay
                                        .overlay_def
                                        .as_ref()
                                        .map(|def| def.get_full_name())
                                        .unwrap_or_else(|| format_overlay_name(&overlay.name));

                                    let (Some(def), Some(props)) =
                                        (overlay.overlay_def.as_ref(), overlay.properties.as_ref())
                                    else {
                                        return base_name;
                                    };

                                    if def.unique_keys.is_empty() {
                                        return base_name;
                                    }

                                    let mut key_values = Vec::new();
                                    for key in &def.unique_keys {
                                        if let Some(value) = props.get(key) {
                                            let rendered = serde_json::to_value(value)
                                                .map(|val| format_json_value(&val))
                                                .unwrap_or_else(|_| "<unserializable>".to_string());
                                            key_values.push(format!("{}={}", key, rendered));
                                        }
                                    }

                                    if key_values.is_empty() {
                                        base_name
                                    } else {
                                        format!("{} ({})", base_name, key_values.join(", "))
                                    }
                                })
                                .collect();
                            self.details.set(Details {
                                id: pointed.oca_bundle.digest.unwrap(),
                                name: pointed.refn,
                                dependent,
                                attributes,
                                overlays,
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
        facade: Arc<Mutex<Store>>,
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
        let registry = OverlayLocalRegistry::from_dir(
            self.config.overlay_definitions_path.clone(),
        )
        .map_err(|e| {
            CliError::OverlayRegistryError(self.config.overlay_definitions_path.clone(), e)
        })?;

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
                            registry.clone(),
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
        facade: Arc<Mutex<Store>>,
    ) -> Result<bool, CliError> {
        info!("Handling publish");
        let current_path = self.output.current_path();
        let errs = self.output.error_list_mut();
        let remote_repository: Url = parse_url(
            self.config
                .repository_url
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
                        let said = oks.get().oca_bundle.digest.clone().unwrap();
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
        match self.active_window {
            Window::Bundles => {
                let state = &mut self.bundles.state;
                state.key_down();
            }
            Window::Errors => {
                self.output.scroll_down(1);
            }
            Window::Help => {
                self.active_window = Window::Bundles;
            }
            Window::Changes => {
                let state: &mut tui_tree_widget::TreeState<String> = &mut self.changes.state;
                state.key_down();
            }
            Window::Details => {
                self.details.scroll_down(1);
            }
        };
        true
    }

    fn handle_key_up(&mut self) -> bool {
        match self.active_window {
            Window::Bundles => {
                let state = &mut self.bundles.state;
                state.key_up();
            }
            Window::Errors => {
                self.output.scroll_up(1);
            }
            Window::Help => {
                self.active_window = Window::Bundles;
            }
            Window::Changes => {
                let state: &mut tui_tree_widget::TreeState<String> = &mut self.changes.state;
                state.key_up();
            }
            Window::Details => {
                self.details.scroll_up(1);
            }
        };
        true
    }

    fn draw<B: Backend<Error = io::Error>>(
        &mut self,
        terminal: &mut Terminal<B>,
    ) -> Result<(), AppError> {
        terminal.draw(|f| f.render_widget(self, f.area()))?;
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
            self.bundles
                .set_active(matches!(self.active_window, Window::Bundles));
            self.details
                .set_active(matches!(self.active_window, Window::Details));
            self.output
                .set_active(matches!(self.active_window, Window::Errors));
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
            ("o", "show ocafile for the pointed bundle"),
            ("s", "show JSON for the pointed bundle"),
            ("p", "publish selected OCA files"),
            ("Tab", "switch focus (bundles/details/output)"),
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
