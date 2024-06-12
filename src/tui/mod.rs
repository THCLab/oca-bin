use anyhow::Result;
use app::App;
use crossterm::{
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
    ExecutableCommand,
};
use oca_bundle::state::oca::OCABundle;
use oca_rs::Facade;
use ratatui::prelude::*;
use said::SelfAddressingIdentifier;
use std::{
    io::stdout,
    path::PathBuf,
    sync::{Arc, Mutex},
};

use crate::dependency_graph::Node;

use self::app::AppError;

pub mod app;
pub mod bundle_info;
pub mod bundle_list;
pub mod changes;
mod item;
pub(crate) mod logging;
pub mod output_window;

pub fn draw<I>(
    base_dir: PathBuf,
    nodes_to_show: I,
    paths: Vec<PathBuf>,
    facade: Arc<Mutex<Facade>>,
    repository_url: Option<String>,
) -> Result<(), AppError>
where
    I: IntoIterator<Item = Node> + Clone,
{
    stdout().execute(EnterAlternateScreen)?;
    enable_raw_mode()?;
    let mut terminal = Terminal::new(CrosstermBackend::new(stdout()))?;
    terminal.clear()?;
    let size = terminal.size().unwrap().width;

    let res = App::new(
        base_dir,
        nodes_to_show,
        facade,
        paths,
        size as usize,
        repository_url,
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

pub fn get_oca_bundle(refn: &str, facade: Arc<Mutex<Facade>>) -> Option<OCABundle> {
    let f = facade.lock().unwrap();
    let refs = f.fetch_all_refs().unwrap();
    refs.into_iter()
        .find(|(name, _s)| *name == refn)
        .and_then(|(_, said)| {
            f.get_oca_bundle(said.parse().unwrap(), false)
                .map(|b| b.bundle)
                .ok()
        })
}

fn get_oca_bundle_by_said(
    said: &SelfAddressingIdentifier,
    facade: Arc<Mutex<Facade>>,
) -> Option<(String, OCABundle)> {
    let f = facade.lock().unwrap();
    let refs = f.fetch_all_refs().unwrap();
    let (refn, _said) = refs
        .into_iter()
        .find(|(_name, s)| *s == said.to_string())
        .unwrap_or_else(|| panic!("Unknown oca bundle of said: {}", said));
    let oca_bun = f.get_oca_bundle(said.clone(), false).unwrap();
    Some((refn, oca_bun.bundle))
}
