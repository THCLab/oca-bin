use config::create_or_open_local_storage;
use config::OCA_CACHE_DB_DIR;
use config::OCA_INDEX_DIR;
use config::OCA_REPOSITORY_DIR;
use error::CliError;
use oca_presentation::presentation::Presentation;
use presentation_command::PresentationCommand;
use std::path::Path;
use std::{env, fs, fs::File, io::Write, path::PathBuf, process, str::FromStr};
use walkdir::WalkDir;

use clap::Parser as ClapParser;
use clap::Subcommand;
use oca_rs::{repositories::SQLiteConfig, Facade};

use crate::config::{init_or_read_config, write_config, Config, OCA_DIR_NAME};
use crate::dependency_graph::parse_oca_file;
use crate::dependency_graph::DependencyGraph;
use crate::presentation_command::{handle_generate, handle_validate, Format};
use said::SelfAddressingIdentifier;
use serde::{Deserialize, Serialize};

extern crate dirs;

#[macro_use]
extern crate log;

mod config;
mod dependency_graph;
pub mod error;
pub mod presentation_command;
mod tui;

#[derive(clap::Parser)]
#[command(author, version, about, long_about = None)]
struct Args {
    #[command(subcommand)]
    command: Option<Commands>,
    #[arg(short, long)]
    config_path: Option<String>,
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
    Tui {
        /// Browse oca objects from directory (recursive)
        #[arg(short, long)]
        dir: Option<PathBuf>,
    },
}

fn get_oca_facade(local_repository_path: PathBuf) -> Facade {
    let db = create_or_open_local_storage(local_repository_path.join(OCA_REPOSITORY_DIR));
    let cache = create_or_open_local_storage(local_repository_path.join(OCA_CACHE_DB_DIR));
    let cache_storage_config = SQLiteConfig::build()
        .path(local_repository_path.join(OCA_INDEX_DIR))
        .unwrap();
    Facade::new(Box::new(db), Box::new(cache), cache_storage_config)
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
            let paths = if let Some(directory) = directory {
                visit_dirs(Path::new(directory)).unwrap()
            } else {
                println!("No file or directory provided");
                process::exit(1);
            };

            let mut facade = get_oca_facade(local_repository_path);
            let graph = DependencyGraph::new(paths);
            let sorted_graph = graph.sort();
            info!("Sorted: {:?}", sorted_graph);
            for node in sorted_graph {
                debug!("Processing: {}", node.refn);
                match graph.oca_file_path(&node.refn) {
                    Some(path) => {
                        let unparsed_file =
                            fs::read_to_string(path).map_err(CliError::ReadFileFailed)?;
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
                    None => {
                        println!("RefN not found in graph: {}", node.refn);
                    }
                }
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
            let mut page = 1;
            let page_size = 20;
            let mut result = facade.fetch_all_oca_bundle(page_size, page).unwrap();
            let meta = result.metadata;
            let mut count = 0;
            info!("Found {} objects in repo", meta.total);
            let refs = facade.fetch_all_refs().unwrap();
            info!("Found {:#?} refs in repo", refs);
            loop {
                info!("Processing page: {}, count: {}", page, count);
                if count >= meta.total {
                    break;
                }
                let records = result.records;
                count += records.len();
                info!("Processing {} objects", records.len());
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
                page += 1;
                result = facade.fetch_all_oca_bundle(page_size, page).unwrap();
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
                    Err(CliError::InvalidSaid(err))
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
                PresentationCommand::Generate { said, format } => {
                    let said = SelfAddressingIdentifier::from_str(said)?;
                    let facade = get_oca_facade(local_repository_path);
                    let presentation = handle_generate(said, &facade)?;
                    let wrapped_presentation = WrappedPresentation { presentation };
                    let output = match format {
                        Some(f) => f.format(&wrapped_presentation),
                        None => Format::JSON.format(&wrapped_presentation),
                    };
                    println!("{}", output);
                    Ok(())
                }
                PresentationCommand::Validate {
                    from_file,
                    output,
                    format,
                    recalculate,
                } => {
                    let ext = from_file.extension();
                    let extension = match ext {
                        Some(ext) => match ext.to_str() {
                            Some(ext) => Format::from_str(ext)
                                .map_err(|e| CliError::FileExtensionError(e.to_string())),
                            None => Err(CliError::FileExtensionError(
                                "Unsupported file extension".to_string(),
                            )),
                        },
                        None => {
                            // CliError::ExtensionError("Missing file extension".to_string())
                            warn!("Missing input file extension. Using JSON");
                            Ok(Format::JSON)
                        }
                    }?;

                    let file_contents =
                        fs::read_to_string(from_file).map_err(CliError::ReadFileFailed)?;
                    let pres: WrappedPresentation = match extension {
                        Format::JSON => serde_json::from_str(&file_contents).unwrap(),
                        Format::YAML => serde_yaml::from_str(&file_contents).unwrap(),
                    };
                    let pres = handle_validate(pres.presentation, *recalculate);
                    match pres {
                        Ok(pres) => {
                            let presentation_wrapped = WrappedPresentation { presentation: pres };
                            // save to file
                            let (path, content) = match (output, format) {
                                (None, None) => {
                                    (from_file.into(), extension.format(&presentation_wrapped))
                                }
                                (None, Some(format)) => match format {
                                    Format::JSON => {
                                        let mut output_path = from_file.clone();
                                        output_path.set_extension("json");
                                        (
                                            output_path,
                                            serde_json::to_string_pretty(&presentation_wrapped)
                                                .unwrap(),
                                        )
                                    }
                                    Format::YAML => {
                                        let mut output_path = from_file.clone();
                                        output_path.set_extension("yaml");
                                        (
                                            output_path,
                                            serde_yaml::to_string(&presentation_wrapped).unwrap(),
                                        )
                                    }
                                },
                                (Some(out), None) => {
                                    (out.into(), extension.format(&presentation_wrapped))
                                }
                                (Some(out), Some(format)) => {
                                    (out.into(), format.format(&presentation_wrapped))
                                }
                            };

                            let mut file = File::create(path).map_err(CliError::WriteFileFailed)?;

                            file.write_all(content.as_bytes())
                                .map_err(CliError::WriteFileFailed)?;
                            println!("Presentation SAID is valid");
                        }
                        Err(e) => {
                            println!("Error: {}", &e.to_string());
                        }
                    };
                    Ok(())
                }
            }
        }
        Some(Commands::Tui { dir }) => {
            let all_oca_files = if let Some(directory) = dir.as_ref() {
                info!("Building OCA bundle from directory");
                visit_dirs(directory)
            } else {
                println!("No file or directory provided");
                process::exit(1);
            }
            .unwrap();
            let facade = get_oca_facade(local_repository_path);
            let graph = DependencyGraph::new(all_oca_files);

            let to_show = visit_current_dir(dir.as_ref().unwrap())
                .unwrap()
                .into_iter()
                .map(|of| parse_oca_file(&of).0);
            tui::draw(to_show, &graph, &facade).unwrap();
            Ok(())
        }
        None => {
            todo!()
        }
    }
}

fn visit_dirs(dir: &Path) -> anyhow::Result<Vec<PathBuf>> {
    let mut paths = Vec::new();
    for entry in WalkDir::new(dir).into_iter().filter_map(|e| e.ok()) {
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
    Ok(paths)
}

fn visit_current_dir(dir: &Path) -> anyhow::Result<Vec<PathBuf>> {
    let mut paths = Vec::new();
    if dir.is_dir() {
        for entry in fs::read_dir(dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.is_dir() {
            } else if let Some(ext) = path.extension() {
                if ext == "ocafile" {
                    paths.push(path.to_path_buf());
                }
            }
        }
    };
    Ok(paths)
}

#[derive(Serialize, Deserialize, Debug)]
pub struct WrappedPresentation {
    presentation: Presentation,
}

// ocafile build -i OCAfile
// ocafile build -s scid
// ocafile publish
// ocafile fetch SAI
// ocafile inspect
