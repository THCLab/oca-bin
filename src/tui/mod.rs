use crate::get_oca_facade;
use anyhow::Result;
use app::App;
use crossterm::{
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
    ExecutableCommand,
};
use oca_bundle::state::oca::OCABundle;
use ratatui::prelude::*;
use std::{io::stdout, path::PathBuf};

pub mod app;
mod list;

pub fn draw(path: Vec<PathBuf>, local_bundle_path: PathBuf) -> Result<()> {
    stdout().execute(EnterAlternateScreen)?;
    enable_raw_mode()?;
    let mut terminal = Terminal::new(CrosstermBackend::new(stdout()))?;
    terminal.clear()?;

    App::new(path, local_bundle_path).run(terminal)?;

    disable_raw_mode()?;
    stdout().execute(LeaveAlternateScreen)?;
    Ok(())
}

fn get_oca_bundle(oca_repo: PathBuf, refn: String) -> Option<OCABundle> {
    let facade = get_oca_facade(oca_repo);
    let page = 1;
    let page_size = 20;
    let result = facade.fetch_all_oca_bundle(page_size, page).unwrap();
    // Lista (said, refn)
    let refs = facade.fetch_all_refs().unwrap();
    let (refn, digest) = refs.into_iter().find(|(name, s)| *name == refn).unwrap();
    let oca_bundle = result
        .records
        .into_iter()
        .find(|oca_bundle| oca_bundle.said.as_ref().unwrap() == &digest.parse().unwrap());
    oca_bundle
}
