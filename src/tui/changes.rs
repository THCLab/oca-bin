use std::{path::{Path, PathBuf}, sync::{Arc, Mutex}};

use git2::{IndexAddOption, Repository, RepositoryInitOptions};
use itertools::Itertools;
use ratatui::{buffer::Buffer, layout::Rect, widgets::{Block, Paragraph, Widget}};

pub struct ChangesWindow {
	changes: Arc<Mutex<Changes>>
}

pub struct Changes {
	repo: Repository
}

impl ChangesWindow {
	pub fn new<P : AsRef<Path>>(path: P) -> Self {
		Self {changes: Arc::new(Mutex::new(Changes::init(path)))}
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
	pub fn init<P : AsRef<Path>>(path: P) -> Self {
		let mut config = RepositoryInitOptions::new();
		config.no_reinit(true);
		let repo = match git2::Repository::init_opts(&path, &config) {
			Ok(repo) => repo,
			Err(_) => {
				// Repo already exists. Open it.
				git2::Repository::open(path).unwrap()
			},
		};
		create_initial_commit(&repo);
		
		Self {repo}
	}

	pub fn update(&self) {
		add_all(&self.repo);
		commit(&self.repo);
	}

	pub fn get_changes(&self) -> String {
		let stats = self.repo.statuses(None).unwrap();
    	let out = stats.into_iter().map(|s| {format!("{:?}", s.status())}).collect::<Vec<_>>().join("\n");
		out

	}

	
}

#[test]
fn test_git2() {
    let repo_path: PathBuf = "repo".parse().unwrap();
    let file_name = "some-file";

	let changes = Changes::init(&repo_path);
	create_file(&repo_path, file_name);
	changes.get_changes();
	changes.update();
	changes.get_changes();
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
