use std::{path::{Path, PathBuf}, sync::{Arc, Mutex}};

use git2::{IndexAddOption, Repository, RepositoryInitOptions};
use itertools::Itertools;
use ratatui::{buffer::Buffer, layout::Rect, widgets::{Block, Paragraph, Widget}};

use crate::dependency_graph::{parse_name, MutableGraph};

pub struct ChangesWindow {
	changes: Arc<Mutex<Changes>>
}

pub struct Changes {
	repo: Repository,
	graph: MutableGraph,
	base: PathBuf,
}

impl ChangesWindow {
	pub fn new<P : AsRef<Path>>(path: P, graph: MutableGraph) -> Self {
		Self {changes: Arc::new(Mutex::new(Changes::init(path, graph)))}
	}

	pub fn changes(&self) -> Arc<Mutex<Changes>> {
		self.changes.clone()
	}

	fn changes_locked(&self) -> String {
		let window = self.changes.lock().unwrap();
		window.get_changes()
	}

	pub fn render(&mut self, area: Rect, buf: &mut Buffer) {
		
        Paragraph::new(self.changes_locked())
                    .block(Block::bordered().title("Changes"))
                    .render(area, buf)
        }
}

impl Changes {
	pub fn init<P : AsRef<Path>>(path: P, graph: MutableGraph) -> Self {
		let mut config = RepositoryInitOptions::new();
		config.no_reinit(true);
		let repo = match git2::Repository::init_opts(&path, &config) {
			Ok(repo) => {
				create_initial_commit(&repo);
				repo
			},
			Err(_) => {
				// Repo already exists. Open it.
				git2::Repository::open(&path).unwrap()
			},
		};
		let path = path.as_ref().to_path_buf();
		Self {repo, graph, base: path.clone()}
	}

	pub fn update(&self) {
		add_all(&self.repo);
		commit(&self.repo);
	}

	pub fn get_changes(&self) -> String {
		let stats = self.repo.statuses(None).unwrap();
    	let out = stats.into_iter().map(|s| {
			let path = s.path().unwrap();
			let mut file_path = self.base.clone();
			file_path.push(path);
			let (name, _) = parse_name(&file_path).unwrap();
			let deps = self.graph.get_ancestors(name.as_ref().unwrap()).unwrap();
			let deps_str = deps.into_iter().map(|node| format!("    └─ {}", node.path.to_str().unwrap())).collect::<Vec<String>>().join("\n");

			format!("{}\n{}", s.path().unwrap(), deps_str)
		}).collect::<Vec<_>>().join("\n");
		out
	}

	
}

fn create_file(repo_path: &Path, file_name: &str) {
    let filepath = repo_path.join(file_name);
    std::fs::File::create(filepath).unwrap();
}

fn add_all(repo: &git2::Repository) {
    let mut index = repo.index().unwrap();
    
    index
        .add_all(&["."], git2::IndexAddOption::DEFAULT, None)
        .unwrap();
    index.write().unwrap();
}

fn commit(repo: &git2::Repository) {
    let mut index = repo.index().unwrap();
    let oid = index.write_tree().unwrap();
    let signature = repo.signature().unwrap();
    let parent_commit = repo.head().unwrap().peel_to_commit().unwrap();
    let tree = repo.find_tree(oid).unwrap();
    repo.commit(
        Some("HEAD"),
        &signature,
        &signature,
        "added some file",
        &tree,
        &[&parent_commit],
    )
    .unwrap();
}

fn create_initial_commit(repo: &git2::Repository) {
    let signature = repo.signature().unwrap();
    let oid = repo.index().unwrap().write_tree().unwrap();
    let tree = repo.find_tree(oid).unwrap();
    repo.commit(
        Some("HEAD"),
        &signature,
        &signature,
        "Initial commit",
        &tree,
        &[],
    )
    .unwrap();
}
