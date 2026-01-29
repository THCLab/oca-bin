use anyhow::Result;
use app::App;
use crossterm::{
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
    ExecutableCommand,
};
use oca_sdk_rs::{OCABundleModel, SelfAddressingIdentifier};
use oca_store::Facade as Store;
use ratatui::prelude::*;
use std::{
    io::stdout,
    path::PathBuf,
    sync::{Arc, Mutex},
};

use crate::{
    config::Config,
    dependency_graph::{Node, NodeParsingError},
    error::CliError,
};

use self::app::AppError;

pub mod app;
pub mod bundle_info;
pub mod bundle_list;
pub mod changes;
mod details;
mod item;
pub(crate) mod logging;
pub mod output_window;

pub fn draw<I>(
    base_dir: PathBuf,
    nodes_to_show: I,
    paths: Vec<PathBuf>,
    facade: Arc<Mutex<Store>>,
    publish_timeout: Option<u64>,
    config: Config,
) -> Result<(), AppError>
where
    I: IntoIterator<Item = Result<Node, NodeParsingError>> + Clone,
{
    stdout().execute(EnterAlternateScreen)?;
    enable_raw_mode()?;
    let mut terminal = Terminal::new(CrosstermBackend::new(stdout()))?;
    terminal.clear()?;
    let res = App::new(
        base_dir,
        nodes_to_show,
        facade,
        paths,
        publish_timeout,
        config,
    )?
    .run(terminal);

    if let Err(err) = res {
        println!("{err:?}");
    }
    // restore terminal
    disable_raw_mode()?;
    stdout().execute(LeaveAlternateScreen)?;

    Ok(())
}

pub fn get_oca_bundle(refn: &str, facade: Arc<Mutex<Store>>) -> Result<OCABundleModel, CliError> {
    let f = facade.lock().unwrap();
    let refs = f.fetch_all_refs().unwrap();
    refs.into_iter()
        .find(|(name, _s)| *name == refn)
        .and_then(|(_, said)| f.get_oca_bundle(said.parse().unwrap()).ok())
        .ok_or(CliError::OCABundleRefnNotFound(refn.to_string()))
}

pub fn get_oca_bundle_by_said(
    said: &SelfAddressingIdentifier,
    facade: Arc<Mutex<Store>>,
) -> Result<(String, OCABundleModel), CliError> {
    let f = facade.lock().unwrap();
    let refs = f.fetch_all_refs().unwrap();
    refs.into_iter()
        .find(|(_name, s)| *s == said.to_string())
        .map(|(refn, _s)| -> Result<_, CliError> {
            let oca_bun = f
                .get_oca_bundle(said.clone())
                .map_err(CliError::OcaBundleAstError)?;
            Ok((refn, oca_bun))
        })
        .ok_or(CliError::OCABundleSAIDNotFound(said.clone()))?
}
