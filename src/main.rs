use std::fs;
use std::path::PathBuf;

use clap::Parser as ClapParser;
use clap::Subcommand;
use oca_rs::Facade;
use oca_rs::data_storage::SledDataStorageConfig;
use oca_rs::repositories::SQLiteConfig;

use oca_rs::data_storage::SledDataStorage;
use oca_rs::data_storage::DataStorage;
use reqwest;

extern crate dirs;

#[macro_use]
extern crate log;


const OCA_CACHE_DB_DIR: &str = "oca_cache";
const OCA_REPOSITORY_DIR: &str = "oca_repository";
const OCA_INDEX_DIR: &str = "read_db";


#[derive(clap::Parser)]
#[command(author, version, about, long_about = None)]
struct Args {
    #[command(subcommand)]
    command: Option<Commands>,
    #[arg(short, long)]
    local_repository_path: Option<String>,
}

#[derive(Subcommand)]
enum Commands {
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

fn main() {
    env_logger::init();

    let args = Args::parse();

    let local_repository_path = match &args.local_repository_path {
        Some(v) => PathBuf::from(v),
        None => {
            let mut p = dirs::home_dir().unwrap();
            p.push(".ocatool");
            p
        }
    };


    match &args.command {
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
            info!("List OCA object from local repository");
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
