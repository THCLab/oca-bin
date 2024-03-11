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
use std::{io::stdout, path::PathBuf};

use crate::dependency_graph::{DependencyGraph, Node};

pub mod app;
// mod list;
mod bundle_info;
mod bundle_list;

pub fn draw<I>(nodes_to_show: I, graph: &DependencyGraph, facade: &Facade) -> Result<()>
where
    I: IntoIterator<Item = Node>,
{
    stdout().execute(EnterAlternateScreen)?;
    enable_raw_mode()?;
    let mut terminal = Terminal::new(CrosstermBackend::new(stdout()))?;
    terminal.clear()?;

    let res = App::new(nodes_to_show, facade, graph).run(terminal);

    if let Err(err) = res {
        println!("{err:?}");
    }
    // restore terminal
    disable_raw_mode()?;
    stdout().execute(LeaveAlternateScreen)?;

    Ok(())
}

fn get_oca_bundle(refn: &str, facade: &Facade) -> Option<OCABundle> {
    let refs = facade.fetch_all_refs().unwrap();
    let (_refn, said) = refs
        .into_iter()
        .find(|(name, s)| *name == refn)
        .expect(&format!("Unknown oca bundle of refn: {}", refn));
    let oca_bun = facade.get_oca_bundle(said.parse().unwrap(), false).unwrap();
    Some(oca_bun.bundle)
}

fn get_oca_bundle_by_said(
    said: &SelfAddressingIdentifier,
    facade: &Facade,
) -> Option<(String, OCABundle)> {
    let refs = facade.fetch_all_refs().unwrap();
    let (refn, _said) = refs
        .into_iter()
        .find(|(_name, s)| *s == said.to_string())
        .expect(&format!("Unknown oca bundle of said: {}", said));
    let oca_bun = facade.get_oca_bundle(said.clone(), false).unwrap();
    Some((refn, oca_bun.bundle))
}
