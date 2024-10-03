use std::{collections::HashMap, fs::{self, File}, io::Write, path::{Path, PathBuf}, sync::Mutex};

use said::SelfAddressingIdentifier;
use serde::{de::DeserializeOwned, Deserialize, Serialize};

use crate::build::{CacheError};
use std::hash::Hash;

pub struct Cache<K, V> where
    K: Eq + Hash {
	path: PathBuf,
	cache: Mutex<HashMap<K, V>>
}

impl<K: Eq + Hash + Serialize + DeserializeOwned, V: Serialize + DeserializeOwned + Clone> Cache<K, V> {
	pub fn new(path : PathBuf) -> Self {
		Cache::load(path.clone()).unwrap_or(Cache { path: path, cache: Mutex::new(HashMap::new()) })
	}
	pub fn save(&self) -> Result<(), CacheError> {
		let mut file = File::create(&self.path)?;
		file.write_all(
			&serde_json::to_vec(&self.cache).map_err(CacheError::CacheFormat)?,
		)?;
		Ok(())
	}

	pub fn load(cache_path: PathBuf) -> Result<Self, CacheError> {
		let cache_contents = fs::read_to_string(&cache_path)?;
		if cache_contents.is_empty() {
			Err(CacheError::EmptyCache)
		} else {
			let map = serde_json::from_str(&cache_contents)?;
			Ok(Cache {path: cache_path, cache: map})
		}
	}

	pub fn insert(&self, hash: K, said: V) -> Result<(), CacheError> {
		let mut locked = self.cache.lock().unwrap();
		locked.insert(hash, said);
		Ok(())
	}

	pub fn get(&self, hash: &K) -> Result<Option<V>, CacheError> {
		let locked = self.cache.lock().unwrap();
		let said = locked.get(hash);
		Ok(said.cloned())
	}


	// pub fn show(&self) -> Result<(), CacheError> {
	// 	let locked = self.cache.lock().unwrap();
	// 	println!("Keys {:?}", locked.keys());
	// 	// let said = locked.get(hash);
	// 	Ok(())
	// }
}

pub type SaidCache = Cache<String, SelfAddressingIdentifier>;
pub type PathCache = Cache<PathBuf, String>;