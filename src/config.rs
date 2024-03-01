use std::io::{self, Error, Write};
use std::{env, path::PathBuf};
use std::{fs, process};

use oca_rs::data_storage::{DataStorage, SledDataStorage, SledDataStorageConfig};
use serde::{Deserialize, Serialize};

pub const OCA_CACHE_DB_DIR: &str = "oca_cache";
pub const OCA_REPOSITORY_DIR: &str = "oca_repository";
pub const OCA_INDEX_DIR: &str = "read_db";
pub const OCA_DIR_NAME: &str = ".oca";

#[derive(Default, Debug, Serialize, Deserialize)]
pub struct Config {
    pub local_repository_path: PathBuf,
    pub remote_repo_url: Option<String>,
}

impl Config {
    pub fn new(local_repository_path: PathBuf) -> Self {
        Config {
            local_repository_path,
            ..Default::default()
        }
    }
}

pub fn read_config(path: &PathBuf) -> Result<Config, Error> {
    let content = fs::read_to_string(path)?;
    let config: Config = toml::from_str(&content)?;
    Ok(config)
}

pub fn write_config(config: &Config, path: &PathBuf) -> Result<(), Error> {
    let content = toml::to_string_pretty(config).unwrap();
    if let Some(parent) = path.parent() {
        info!("Create local repository: {:?}", parent);
        fs::create_dir_all(parent)?;
    }
    fs::write(path, content)?;
    Ok(())
}

pub fn write_default_config(path: &PathBuf) -> Result<Config, Error> {
    let local_repository_path = path.parent().unwrap().to_path_buf();
    let config = Config::new(local_repository_path);
    write_config(&config, path)?;
    Ok(config)
}

pub fn create_or_open_local_storage(path: PathBuf) -> SledDataStorage {
    let config = SledDataStorageConfig::build().path(path).unwrap();
    SledDataStorage::new().config(config)
}

pub fn ask_for_confirmation(prompt: &str) -> bool {
    print!("{} ", prompt);
    io::stdout().flush().unwrap();

    let mut input = String::new();
    io::stdin()
        .read_line(&mut input)
        .expect("Failed to read line");

    let input = input.trim().to_lowercase();
    input == "y" || input == "yes"
}

pub fn init_or_read_config() -> Config {
    let local_config_path = env::current_dir()
        .unwrap()
        .join(OCA_DIR_NAME)
        .join("config.toml");
    if local_config_path.is_file() {
        read_config(&local_config_path).unwrap()
    } else {
        // Try to read home directory configuration
        let p = dirs::home_dir()
            .unwrap()
            .join(OCA_DIR_NAME)
            .join("config.toml");
        match read_config(&p) {
            Ok(config) => config,
            Err(_) => {
                if ask_for_confirmation("OCA config not found do you want to initialize it in your home directory? (y/N)") {
                write_default_config(&p).unwrap()
             } else {
                println!("Consider runing oca init in this directory to initialize local repository");
                process::exit(1)
             }
            }
        }
    }
    // Check currnet path
    // Check home
    // ask to initialize home or run oca init to create it in local directory
}
