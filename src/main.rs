use std::env;
use std::fs;
use std::io;
use std::io::Write;
use std::path::PathBuf;
use std::process;

use clap::Parser as ClapParser;
use clap::Subcommand;
use oca_rs::Facade;
use oca_rs::data_storage::SledDataStorageConfig;
use oca_rs::repositories::SQLiteConfig;

use oca_rs::data_storage::SledDataStorage;
use oca_rs::data_storage::DataStorage;
use reqwest;
use serde::Deserialize;
use serde::Serialize;

extern crate dirs;

#[macro_use]
extern crate log;


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
    Init {
    },
    /// Show configuration where data are stored
    Config {
    },
    /// Build oca objects out of ocafile
    Build {
        #[arg(short, long)]
        file: Option<String>,
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
    List {
    }

}

use std::io::Error;

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
   SledDataStorage::new()
                .config(config)
}


fn get_oca_facade(local_repository_path: PathBuf) -> Facade {
    let db = create_or_open_local_storage(local_repository_path.join(OCA_REPOSITORY_DIR));
    let cache = create_or_open_local_storage(local_repository_path.join(OCA_CACHE_DB_DIR));
    let cache_storage_config = SQLiteConfig::build().path(local_repository_path.join(OCA_INDEX_DIR)).unwrap();
    Facade::new(Box::new(db), Box::new(cache), cache_storage_config)
}

fn ask_for_confirmation(prompt: &str) -> bool {
    print!("{} ", prompt);
    io::stdout().flush().unwrap();

    let mut input = String::new();
    io::stdin().read_line(&mut input).expect("Failed to read line");

    let input = input.trim().to_lowercase();
    input == "y" || input == "yes"
}

/// Publish oca bundle pointed by SAID to configured repository
///
/// # Arguments
/// * `said` - SAID of oca bundle to publish
///
///
fn publish_oca_file_for(facade: &Facade, said: String, repository_url: &Option<String>, remote_repo_url: &Option<String>) {
    match facade.get_oca_bundle_ocafile(said, true) {
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
            debug!("Publish OCA bundle to: {} with payload: {}", api_url, ocafile);
            match client.post(api_url).body(ocafile).send() {
                Ok(v) => println!("{},{}", v.status(), v.text().unwrap() ),
                Err(e) => println!("Error while uploading OCAFILE: {}",e)
            };
        }
        Err(errors) => {
            println!("{:?}", errors);
        }
    }

}

fn init_or_read_config() -> Config {

    let local_config_path = env::current_dir().unwrap().join(OCA_DIR_NAME).join("config.toml");
    if local_config_path.is_file() {
        read_config(&local_config_path).unwrap()
    } else {
        // Try to read home directory configuration
        let p = dirs::home_dir().unwrap().join(OCA_DIR_NAME).join("config.toml");
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

fn main() {
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
        Some(Commands::Init { } ) => {
            info!("Initialize local repository");
            match env::current_dir() {
                Ok(path) => {
                    info!("Initialize repository at: {:?}", path);
                    let local_repository_path = path.join(OCA_DIR_NAME);
                    let config = Config::new(local_repository_path);
                    let config_file = path.join(OCA_DIR_NAME).join("config.toml");
                    match write_config(&config, &config_file) {
                        Ok(it) => it,
                        Err(err) => println!("{}", err),
                    };                },
                Err(err) => println!("Error getting current directory: {}", err),
            }
        }
        Some(Commands::Config { } ) => {
            info!("Configuration of oca");
            println!("Local repository: {:?} ", local_repository_path.join(OCA_REPOSITORY_DIR));
            println!("OCA Cache: {:?} ", local_repository_path.join(OCA_CACHE_DB_DIR));
            println!("Index DB: {:?}", local_repository_path.join(OCA_INDEX_DIR));
        }
        Some(Commands::Build { file }) => {
            info!("Building OCA bundle from oca file");

            let unparsed_file = match file {
                Some(file) => fs::read_to_string(file).expect("Can't read file"),
                None => fs::read_to_string("OCAfile").expect("Can't read file"),
            };

            let mut facade = get_oca_facade(local_repository_path);
            // TODO build from ocafile does everything including storing that in db
            // maybe we could get better naming for it
            let result = facade.build_from_ocafile(unparsed_file);

            if let Ok(oca_bundle) = result {
                let serialized_bundle = serde_json::to_string_pretty(&oca_bundle).unwrap();
                fs::write("output".to_string() + ".ocabundle", serialized_bundle).expect("Unable to write file");
                let refs = facade.fetch_all_refs().unwrap();
                let schema_name = refs.iter().find(|&(_, v)| *v == oca_bundle.said.clone().unwrap().to_string());
                if let Some((refs, _)) = schema_name {
                    println!("OCA bundle created in local repository with SAID: {} and name: {}", oca_bundle.said.unwrap(), refs);
                } else {
                    println!("OCA bundle created in local repository with SAID: {:?}", oca_bundle.said.unwrap());
                }
            } else {
                println!("{:?}", result);
            }
        }
        Some(Commands::Publish { repository_url, said }) => {
            info!("Publish OCA bundle to repository");
            let facade = get_oca_facade(local_repository_path);
            // TODO since we can fetch all dependencies we should publish them all
            let bundle = facade.get_oca_bundle(said.to_string(), false).unwrap().last().unwrap().clone();
            publish_oca_file_for(&facade, bundle.said.clone().unwrap().to_string(), repository_url, &remote_repo_url);
            let references = facade.get_all_references(bundle.said.clone().unwrap().to_string());
            debug!("Found references: {:?}", references);
            for said in references {
                publish_oca_file_for(&facade, said, repository_url, &remote_repo_url);
            }
        }
        Some(Commands::Sign { scid: _ }) => {
            info!("Sign OCA bundle byc SCID");
            unimplemented!("Coming soon!")
        }
        Some(Commands::List { }) => {
            info!("List OCA object from local repository: {:?}", local_repository_path);
            let facade = get_oca_facade(local_repository_path);
            let mut result = facade.fetch_all_oca_bundle(10, 1).unwrap();
            let meta = result.metadata;
            let mut count = 0;
            info!("Found {} objects", meta.total);
            let refs = facade.fetch_all_refs().unwrap();
            loop {
                if count == meta.total {
                    break;
                }
                let records = result.records;
                count = count + records.len();
                for bundle in records {
                    let said = bundle.said.unwrap();
                    let matching_ref = refs.iter().find(|&(_, v)| *v == said.to_string());
                    match matching_ref {
                        Some((refs, _)) => {
                            println!("SAID: {}, name: {}", said, refs);
                        },
                        None => {
                            println!("SAID: {}", said);
                        }
                    }
                }
                result = facade.fetch_all_oca_bundle(10, meta.page + 1).unwrap();
            }
        }
        Some(Commands::Show { said, ast, dereference } )=> {
            info!("Search for OCA object in local repository");
            let facade = get_oca_facade(local_repository_path);
            if *ast  {
                match facade.get_oca_bundle_ast(said.to_string()) {
                    Ok(oca_ast) => {
                        serde_json::to_writer_pretty(std::io::stdout(), &oca_ast).expect("Faild to format oca ast");
                    },
                    Err(errors) => {
                        println!("{:?}", errors);
                    }
                }
            } else {
                match facade.get_oca_bundle_ocafile(said.to_string(), *dereference) {
                    Ok(ocafile) => {
                        println!("{}", ocafile);
                    },
                    Err(errors) => {
                        println!("{:?}", errors);
                    }
                }
            }
        }
        Some(Commands::Get { said, with_dependencies }) => {
            let facade = get_oca_facade(local_repository_path);
            match facade.get_oca_bundle(said.to_string(), *with_dependencies) {
             Ok(oca_bundles) => {
                 let content = serde_json::to_value(oca_bundles).expect("Field to read oca bundle");
                 println!("{}", serde_json::to_string_pretty(&content).expect("Faild to format oca bundle"));
             },
             Err(errors) => {
                println!("{:?}", errors);
             }
            }
        }
        None => {}
    }
}

// ocafile build -i OCAfile
// ocafile build -s scid
// ocafile publish
// ocafile fetch SAI
// ocafile inspect
