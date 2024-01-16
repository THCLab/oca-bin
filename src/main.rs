use error::CliError;
use std::env;
use std::fs;
use std::fs::File;
use std::io;
use std::io::Write;
use std::path::PathBuf;
use std::process;
use std::str::FromStr;
use walkdir::WalkDir;

use clap::Parser as ClapParser;
use clap::Subcommand;
use oca_rs::data_storage::SledDataStorageConfig;
use oca_rs::repositories::SQLiteConfig;
use oca_rs::Facade;

use oca_rs::data_storage::DataStorage;
use oca_rs::data_storage::SledDataStorage;
use said::SelfAddressingIdentifier;
use serde::Deserialize;
use serde::Serialize;

extern crate dirs;

#[macro_use]
extern crate log;

pub mod error;
pub mod presentation_command;

const OCA_CACHE_DB_DIR: &str = "oca_cache";
const OCA_REPOSITORY_DIR: &str = "oca_repository";
const OCA_INDEX_DIR: &str = "read_db";
const OCA_DIR_NAME: &str = ".oca";

#[derive(clap::Parser)]
#[command(author, version, about, long_about = None)]
struct Args {
    #[command(subcommand)]
    command: Option<Commands>,
    #[arg(short, long)]
    config_path: Option<String>,
}

#[derive(Default, Debug, Serialize, Deserialize)]
struct Config {
    local_repository_path: PathBuf,
    remote_repo_url: Option<String>,
}

impl Config {
    fn new(local_repository_path: PathBuf) -> Self {
        Config {
            local_repository_path,
            ..Default::default()
        }
    }
}

#[derive(Subcommand)]
enum Commands {
    /// Initialize new local repository
    Init {},
    /// Show configuration where data are stored
    Config {},
    /// Build oca objects out of ocafile
    #[clap(group = clap::ArgGroup::new("build").required(true).args(&["ocafile", "directory"]))]
    Build {
        /// Specify ocafile to build from
        #[arg(short = 'f', long, group = "build")]
        ocafile: Option<String>,
        /// Build oca objects from directory (recursive)
        #[arg(short, long, group = "build")]
        directory: Option<String>,
    },
    /// Publish oca objects into online repository
    Publish {
        #[arg(short, long)]
        repository_url: Option<String>,
        #[arg(short, long)]
        said: String,
    },
    /// Sign specific object to claim ownership
    Sign {
        #[arg(short, long)]
        scid: String,
    },
    /// Show ocafile for specify said
    Show {
        #[arg(short, long)]
        said: String,
        #[arg(short, long)]
        ast: bool,
        #[arg(short, long)]
        dereference: bool,
    },
    /// Get oca bundle for specify said
    Get {
        #[arg(short, long)]
        said: String,
        #[arg(short, long)]
        with_dependencies: bool,
    },
    /// List of all oca objects stored in local repository
    List {},
    /// Generate or parse presentation for oca object
    Presentation {
        #[command(subcommand)]
        command: PresentationCommand,
    },
}

#[derive(Subcommand)]
enum PresentationCommand {
    Get {
        #[arg(short, long)]
        said: String,
    },
    Generate,
    Parse {
        #[arg(short, long)]
        from_file: PathBuf,
        #[arg(short, long)]
        output: Option<PathBuf>,
    },
}

use std::io::Error;

use crate::presentation_command::handle_get;
use crate::presentation_command::handle_parse;

fn read_config(path: &PathBuf) -> Result<Config, Error> {
    let content = fs::read_to_string(path)?;
    let config: Config = toml::from_str(&content)?;
    Ok(config)
}

fn write_config(config: &Config, path: &PathBuf) -> Result<(), Error> {
    let content = toml::to_string_pretty(config).unwrap();
    if let Some(parent) = path.parent() {
        info!("Create local repository: {:?}", parent);
        fs::create_dir_all(parent)?;
    }
    fs::write(path, content)?;
    Ok(())
}

fn write_default_config(path: &PathBuf) -> Result<Config, Error> {
    let local_repository_path = path.parent().unwrap().to_path_buf();
    let config = Config::new(local_repository_path);
    write_config(&config, &path)?;
    Ok(config)
}

fn create_or_open_local_storage(path: PathBuf) -> SledDataStorage {
    let config = SledDataStorageConfig::build().path(path).unwrap();
    SledDataStorage::new().config(config)
}

fn get_oca_facade(local_repository_path: PathBuf) -> Facade {
    let db = create_or_open_local_storage(local_repository_path.join(OCA_REPOSITORY_DIR));
    let cache = create_or_open_local_storage(local_repository_path.join(OCA_CACHE_DB_DIR));
    let cache_storage_config = SQLiteConfig::build()
        .path(local_repository_path.join(OCA_INDEX_DIR))
        .unwrap();
    Facade::new(Box::new(db), Box::new(cache), cache_storage_config)
}

fn ask_for_confirmation(prompt: &str) -> bool {
    print!("{} ", prompt);
    io::stdout().flush().unwrap();

    let mut input = String::new();
    io::stdin()
        .read_line(&mut input)
        .expect("Failed to read line");

    let input = input.trim().to_lowercase();
    input == "y" || input == "yes"
}

/// Publish oca bundle pointed by SAID to configured repository
///
/// # Arguments
/// * `said` - SAID of oca bundle to publish
///
///
fn publish_oca_file_for(
    facade: &Facade,
    said: SelfAddressingIdentifier,
    repository_url: &Option<String>,
    remote_repo_url: &Option<String>,
) {
    match facade.get_oca_bundle_ocafile(said, false) {
        Ok(ocafile) => {
            let client = reqwest::blocking::Client::new();
            let api_url = if let Some(repository_url) = repository_url {
                info!("Override default repository with: {}", repository_url);
                format!("{}{}", repository_url, "/oca-bundles")
            } else if let Some(remote_repo_url) = remote_repo_url {
                info!("Use default repository: {}", remote_repo_url);
                format!("{}{}", remote_repo_url, "/oca-bundles")
            } else {
                panic!("No repository url provided")
            };
            debug!(
                "Publish OCA bundle to: {} with payload: {}",
                api_url, ocafile
            );
            match client.post(api_url).body(ocafile).send() {
                Ok(v) => println!("{},{}", v.status(), v.text().unwrap()),
                Err(e) => println!("Error while uploading OCAFILE: {}", e),
            };
        }
        Err(errors) => {
            println!("{:?}", errors);
        }
    }
}

fn init_or_read_config() -> Config {
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
            Ok(config) => return config,
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

fn main() -> Result<(), CliError> {
    env_logger::init();

    let args = Args::parse();

    // Any command triggered
    // Check if config exist if not ask user to initialize the repo
    // or read the config to properly parse all stuff
    // first check if there is local .oca
    // then check home dir for global .oca
    // default is one repo for all in home dir
    //

    let config = init_or_read_config();
    info!("Config: {:?}", config);
    let local_repository_path = config.local_repository_path;
    let remote_repo_url = config.remote_repo_url;

    match &args.command {
        Some(Commands::Init {}) => {
            info!("Initialize local repository");
            match env::current_dir() {
                Ok(path) => {
                    info!("Initialize repository at: {:?}", path);
                    let local_repository_path = path.join(OCA_DIR_NAME);
                    let config = Config::new(local_repository_path);
                    let config_file = path.join(OCA_DIR_NAME).join("config.toml");
                    match write_config(&config, &config_file) {
                        Ok(it) => Ok(it),
                        Err(err) => {
                            println!("{}", err);
                            Err(CliError::WriteFileFailed(err))
                        }
                    }
                }
                Err(err) => Err(CliError::CurrentDirFailed(err)),
            }
        }
        Some(Commands::Config {}) => {
            info!("Configuration of oca");
            println!(
                "Local repository: {:?} ",
                local_repository_path.join(OCA_REPOSITORY_DIR)
            );
            println!(
                "OCA Cache: {:?} ",
                local_repository_path.join(OCA_CACHE_DB_DIR)
            );
            println!("Index DB: {:?}", local_repository_path.join(OCA_INDEX_DIR));
            Ok(())
        }
        Some(Commands::Build { ocafile, directory }) => {
            let mut paths = Vec::new();
            if let Some(directory) = directory {
                info!("Building OCA bundle from directory");
                for entry in WalkDir::new(directory).into_iter().filter_map(|e| e.ok()) {
                    let path = entry.path();
                    if path.is_dir() {
                        continue;
                    }
                    if let Some(ext) = path.extension() {
                        if ext == "ocafile" {
                            paths.push(path.to_path_buf());
                        }
                    }
                }
            } else if let Some(file) = ocafile {
                info!("Building OCA bundle from oca file");
                paths.push(PathBuf::from(file));
            } else {
                println!("No file or directory provided");
                process::exit(1);
            }

            let mut facade = get_oca_facade(local_repository_path);
            for path in paths {
                let unparsed_file = fs::read_to_string(path).map_err(CliError::ReadFileFailed)?;
                let oca_bundle = facade
                    .build_from_ocafile(unparsed_file)
                    .map_err(CliError::OcaBundleError)?;
                let refs = facade.fetch_all_refs().unwrap();
                let schema_name = refs
                    .iter()
                    .find(|&(_, v)| *v == oca_bundle.said.clone().unwrap().to_string());
                if let Some((refs, _)) = schema_name {
                    println!(
                        "OCA bundle created in local repository with SAID: {} and name: {}",
                        oca_bundle.said.unwrap(),
                        refs
                    );
                } else {
                    println!(
                        "OCA bundle created in local repository with SAID: {:?}",
                        oca_bundle.said.unwrap()
                    );
                };
            }
            Ok(())
        }
        Some(Commands::Publish {
            repository_url,
            said,
        }) => {
            info!("Publish OCA bundle to repository");
            let facade = get_oca_facade(local_repository_path);
            match SelfAddressingIdentifier::from_str(said) {
                Ok(said) => {
                    let with_dependencies = true;
                    let bundles = facade.get_oca_bundle(said, with_dependencies).unwrap();
                    // Publish main object
                    publish_oca_file_for(
                        &facade,
                        bundles.bundle.said.clone().unwrap(),
                        repository_url,
                        &remote_repo_url,
                    );
                    // Publish dependencies if available
                    for bundle in bundles.dependencies {
                        publish_oca_file_for(
                            &facade,
                            bundle.said.clone().unwrap(),
                            repository_url,
                            &remote_repo_url,
                        );
                    }
                    Ok(())
                }
                Err(err) => {
                    println!("Invalid SAID: {}", err);
                    Err(err.into())
                }
            }
        }
        Some(Commands::Sign { scid: _ }) => {
            info!("Sign OCA bundle byc SCID");
            unimplemented!("Coming soon!")
        }
        Some(Commands::List {}) => {
            info!(
                "List OCA object from local repository: {:?}",
                local_repository_path
            );
            let facade = get_oca_facade(local_repository_path);
            let mut result = facade.fetch_all_oca_bundle(10, 1).unwrap();
            let meta = result.metadata;
            let mut count = 0;
            info!("Found {} objects", meta.total);
            let refs = facade.fetch_all_refs().unwrap();
            loop {
                if count >= meta.total {
                    break;
                }
                let records = result.records;
                count += records.len();
                info!("Found {} objects", count);
                for bundle in records {
                    let said = bundle.said.unwrap();
                    let matching_ref = refs.iter().find(|&(_, v)| *v == said.to_string());
                    match matching_ref {
                        Some((refs, _)) => {
                            println!("SAID: {}, name: {}", said, refs);
                        }
                        None => {
                            println!("SAID: {}", said);
                        }
                    }
                }
                result = facade.fetch_all_oca_bundle(10, meta.page + 1).unwrap();
            }
            Ok(())
        }
        Some(Commands::Show {
            said,
            ast,
            dereference,
        }) => {
            info!("Search for OCA object in local repository");
            let facade = get_oca_facade(local_repository_path);
            match SelfAddressingIdentifier::from_str(said) {
                Ok(said) => {
                    if *ast {
                        let oca_ast = facade
                            .get_oca_bundle_ast(said)
                            .map_err(CliError::OcaBundleAstError)?;
                        serde_json::to_writer_pretty(std::io::stdout(), &oca_ast)
                            .expect("Faild to format oca ast");
                        Ok(())
                    } else {
                        let ocafile = facade
                            .get_oca_bundle_ocafile(said, *dereference)
                            .map_err(CliError::OcaBundleAstError)?;
                        println!("{}", ocafile);
                        Ok(())
                    }
                }
                Err(err) => {
                    println!("Invalid SAID: {}", err);
                    Err(CliError::InvalidSaid(err.into()))
                }
            }
        }
        Some(Commands::Get {
            said,
            with_dependencies,
        }) => {
            let facade = get_oca_facade(local_repository_path);
            let said = SelfAddressingIdentifier::from_str(said)?;
            let oca_bundles = facade
                .get_oca_bundle(said, *with_dependencies)
                .map_err(CliError::OcaBundleAstError)?;
            let content = serde_json::to_value(oca_bundles).map_err(CliError::ReadOcaError)?;
            println!(
                "{}",
                serde_json::to_string_pretty(&content).map_err(CliError::WriteOcaError)?
            );
            Ok(())
        }
        Some(Commands::Presentation { command }) => {
            match command {
                PresentationCommand::Get { said } => {
                    let said = SelfAddressingIdentifier::from_str(said)?;
                    handle_get(said, local_repository_path)?;
                    Ok(())
                },
                PresentationCommand::Parse { from_file, output } => {
                    let file_contents =
                        fs::read_to_string(from_file).map_err(|e| CliError::ReadFileFailed(e))?;
                    let pres = handle_parse(&file_contents)?;
                    // save to file
                    let out_path = if let Some(out) = output {
                        out
                    } else {
                        from_file
                    };
                    let mut file =
                        File::create(out_path).map_err(|e| CliError::WriteFileFailed(e))?;
                    file.write_all(serde_json::to_string_pretty(&pres).unwrap().as_bytes())
                        .map_err(|e| CliError::WriteFileFailed(e))?;
                    Ok(())
                },
                PresentationCommand::Generate => todo!(),
            }
        }
        None => {
            todo!()
        }
    }
}

// ocafile build -i OCAfile
// ocafile build -s scid
// ocafile publish
// ocafile fetch SAI
// ocafile inspect
