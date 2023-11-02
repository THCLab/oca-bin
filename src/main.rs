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
        repository_url: String,
        #[arg(short, long)]
        said: String,
    },
    /// Sign specific object to claim ownership
    Sign {
        #[arg(short, long)]
        scid: String,
    },
    Show {
        #[arg(short, long)]
        said: String,
    },
    Get {
        #[arg(short, long)]
        said: String,
    },
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
    env_logger::Builder::new()
        .filter(None, log::LevelFilter::Info)
        .init();

    let args = Args::parse();


    // Any command triggered
    // Check if config exist if not ask user to initialize the repo
    // or read the config to properly parse all stuff
    // first check if there is local .oca
    // then check home dir for global .oca
    // default is one repo for all in home dir
    //

    let config = init_or_read_config();
    let local_repository_path = config.local_repository_path;



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
            // build from ocafile does everything including storing that in db
            // maybe we could get better naming for it
            let result = facade.build_from_ocafile(unparsed_file);

            if let Ok(oca_bundle) = result {
                let serialized_bundle = serde_json::to_string_pretty(&oca_bundle).unwrap();
                fs::write("output".to_string() + ".ocabundle", serialized_bundle).expect("Unable to write file");
                println!("OCA bundle created in local repository with SCID: {:?}", oca_bundle.said.unwrap());
            } else {
                println!("{:?}", result);
            }
        }
        Some(Commands::Publish { repository_url, said }) => {
            info!("Publish OCA bundle to repository");
            let facade = get_oca_facade(local_repository_path);
            match facade.get_oca_bundle_ocafile(said.to_string()) {
             Ok(ocafile) => {
                 let client = reqwest::blocking::Client::new();
                 let api_url = format!("{}{}", repository_url, "/api/oca-bundles");
                 info!("Repository: {}", api_url);
                 match client.post(api_url).body(ocafile).send() {
                     Ok(v) => println!("{:?}", v.text() ),
                     Err(e) => println!("Error while uploading OCAFILE: {}",e)
                 };
             }
             Err(errors) => {
                println!("{:?}", errors);
             }
            }
        }
        Some(Commands::Sign { scid: _ }) => {
            info!("Sign OCA bundle byc SCID");
            unimplemented!("Coming soon!")
        }
        Some(Commands::List { }) => {
            info!("List OCA object from local repository: {:?}", local_repository_path);
            let facade = get_oca_facade(local_repository_path);
            let result = facade.fetch_all_oca_bundle(10, 1).unwrap().records;
            info!("Found {}, results", result.len());
            for bundle in result {
                println!("SAID: {}", bundle.said.unwrap());
            }
        }
        Some(Commands::Show { said } )=> {
            info!("Search for OCA object in local repository");
            let facade = get_oca_facade(local_repository_path);
            match facade.get_oca_bundle_ocafile(said.to_string()) {
             Ok(ocafile) => {
                println!("{}", ocafile);
             },
             Err(errors) => {
                println!("{:?}", errors);
             }
            }
        }
        Some(Commands::Get { said }) => {
            let facade = get_oca_facade(local_repository_path);
            match facade.get_oca_bundle(said.to_string()) {
             Ok(oca_bundle) => {
                 let content = serde_json::to_value(oca_bundle).expect("Field to read oca bundle");
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
