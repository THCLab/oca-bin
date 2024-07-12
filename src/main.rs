use crate::mapping::mapping;
use config::create_or_open_local_storage;
use config::OCA_CACHE_DB_DIR;
use config::OCA_INDEX_DIR;
use config::OCA_REPOSITORY_DIR;
use error::CliError;
use oca_presentation::presentation::Presentation;
use presentation_command::PresentationCommand;
use std::collections::HashSet;
use std::sync::Arc;
use std::sync::Mutex;
use std::{env, fs, fs::File, io::Write, path::PathBuf, process, str::FromStr};

use clap::Parser as ClapParser;
use clap::Subcommand;
use oca_rs::{repositories::SQLiteConfig, Facade};
use url::Url;

use crate::config::{init_or_read_config, write_config, Config, OCA_DIR_NAME};
use crate::dependency_graph::parse_node;
use crate::dependency_graph::DependencyGraph;
use crate::dependency_graph::MutableGraph;
use crate::presentation_command::{handle_generate, handle_validate, Format};
use crate::tui::logging::initialize_logging;
use crate::utils::{load_ocafiles_all, visit_current_dir};
use said::SelfAddressingIdentifier;
use serde::{Deserialize, Serialize};

extern crate dirs;

#[macro_use]
extern crate log;

mod config;
mod dependency_graph;
pub mod error;
mod mapping;
pub mod presentation_command;
mod tui;
mod utils;
mod validate;

#[derive(clap::Parser)]
#[command(author, version, about, long_about = None)]
struct Args {
    #[command(subcommand)]
    command: Option<Commands>,
    #[arg(short, long)]
    config_path: Option<String>,
}

#[derive(Subcommand)]
#[command(arg_required_else_help = true)]
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
        ocafile: Option<PathBuf>,
        /// Build oca objects from directory (recursive)
        #[arg(short, long, group = "build")]
        directory: Option<PathBuf>,
    },
    /// Validate oca objects out of ocafile
    #[clap(group = clap::ArgGroup::new("build").required(true).args(&["ocafile", "directory"]))]
    Validate {
        /// Specify ocafile to validate from
        #[arg(short = 'f', long, group = "build")]
        ocafile: Option<PathBuf>,
        /// Validate oca objects from directory (recursive)
        #[arg(short, long, group = "build")]
        directory: Option<PathBuf>,
    },
    /// Publish oca objects into online repository
    Publish {
        #[arg(short, long)]
        repository_url: Option<String>,
        #[arg(short, long)]
        said: String,
        #[arg(short, long)]
        timeout: Option<u64>,
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
    /// Launches a terminal user interface application to browse OCA objects
    Tui {
        /// Browse oca objects from directory (recursive)
        #[arg(short, long)]
        dir: Option<PathBuf>,
        /// Publishing timeout in seconds. Default is 30.
        #[arg(short, long)]
        timeout: Option<u64>,
    },
    /// Generate json file with all fields of oca object for specified said
    Mapping {
        #[arg(short, long)]
        said: String,
    },
}

fn get_oca_facade(local_repository_path: PathBuf) -> Facade {
    let db = create_or_open_local_storage(local_repository_path.join(OCA_REPOSITORY_DIR));
    let cache = create_or_open_local_storage(local_repository_path.join(OCA_CACHE_DB_DIR));
    let cache_storage_config = SQLiteConfig::build()
        .path(local_repository_path.join(OCA_INDEX_DIR))
        .unwrap();
    Facade::new(Box::new(db.clone()), Box::new(cache), cache_storage_config)
}

fn saids_to_publish(
    facade: Arc<Mutex<Facade>>,
    saids: &[SelfAddressingIdentifier],
) -> HashSet<SelfAddressingIdentifier> {
    let mut to_publish = HashSet::new();
    for said in saids {
        to_publish.insert(said.clone());
        let dep_said = dependant_saids(facade.clone(), said);

        match dep_said {
            Some(deps) => {
                let more_deps = saids_to_publish(facade.clone(), &deps);
                to_publish.extend(more_deps);
            }
            None => (),
        };
    }
    to_publish
}

fn dependant_saids(
    facade: Arc<Mutex<Facade>>,
    said: &SelfAddressingIdentifier,
) -> Option<Vec<SelfAddressingIdentifier>> {
    let facade_locked = facade.lock().unwrap();
    let bundles = facade_locked.get_oca_bundle(said.clone(), true).unwrap();
    let saids = bundles.dependencies;

    if saids.is_empty() {
        None
    } else {
        Some(
            saids
                .iter()
                .map(|bdle| bdle.said.as_ref().unwrap().clone())
                .collect::<Vec<_>>(),
        )
    }
}

/// Publish oca bundle pointed by SAID to configured repository
///
/// # Arguments
/// * `said` - SAID of oca bundle to publish
///
///
fn publish_oca_file_for(
    facade: Arc<Mutex<Facade>>,
    said: SelfAddressingIdentifier,
    timeout: &Option<u64>,
    repository_url: Url,
) -> Result<(), CliError> {
    let timeout = timeout.unwrap_or(666);
    let facade = facade.lock().unwrap();

    match facade.get_oca_bundle_ocafile(said.clone(), false) {
        Ok(ocafile) => {
            let client = reqwest::blocking::Client::builder()
                .timeout(std::time::Duration::from_secs(timeout))
                .build()
                .expect("Failed to create reqwest client");
            let url = repository_url.join("oca-bundles")?;
            info!("Publish OCA bundle to: {} with payload: {}", url, ocafile);
            match client.post(url).body(ocafile).send() {
                Ok(v) => match v.error_for_status() {
                    Ok(v) => {
                        info!("{},{}", v.status(), v.text().unwrap());
                        Ok(())
                    }
                    Err(er) => {
                        info!("error: {:?}", er);
                        Err(CliError::PublishError(said, vec![er.to_string()]))
                    }
                },
                Err(e) => {
                    info!("Error while uploading OCAFILE: {}", e);
                    Err(CliError::PublishError(
                        said,
                        vec![format!("Sending error: {}", e)],
                    ))
                }
            }
        }
        Err(errors) => Err(CliError::PublishError(said, errors)),
    }
}

fn main() -> Result<(), CliError> {
    initialize_logging().unwrap();

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
    let remote_repo_url_from_config = config.repository_url;

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
            let (paths, base_dir) = load_ocafiles_all(ocafile.as_ref(), directory.as_ref())?;

            let mut facade = get_oca_facade(local_repository_path);
            let graph = DependencyGraph::from_paths(&base_dir, paths).unwrap();
            let sorted_graph = graph.sort().unwrap();

            info!("Sorted: {:?}", sorted_graph);
            for node in sorted_graph {
                info!("Processing: {}", node.refn);
                match graph.oca_file_path(&node.refn) {
                    Ok(path) => {
                        let unparsed_file =
                            fs::read_to_string(&path).map_err(CliError::ReadFileFailed)?;
                        let oca_bundle = facade
                            .build_from_ocafile(unparsed_file)
                            .map_err(|e| CliError::BuildingError(path, e))?;
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
                    _ => {
                        println!("RefN not found in graph: {}", node.refn);
                    }
                }
            }
            Ok(())
        }

        Some(Commands::Publish {
            repository_url,
            said,
            timeout,
        }) => {
            info!("Publish OCA bundle to repository");
            let facade = get_oca_facade(local_repository_path);
            let facade = Arc::new(Mutex::new(facade));
            match SelfAddressingIdentifier::from_str(said) {
                Ok(said) => {
                    // Find dependant saids for said.
                    let saids_to_publish = saids_to_publish(facade.clone(), &[said.clone()]);
                    let remote_repo_url = match (repository_url, remote_repo_url_from_config) {
                        (None, None) => {
                            println!("Error: {}", CliError::UnknownRemoteRepoUrl.to_string());
                            return Ok(());
                        }
                        (None, Some(config_url)) => url::Url::parse(&config_url)?,
                        (Some(repo_url), _) => url::Url::parse(&repo_url)?,
                    };
                    // Make post request for all saids
                    let res: Vec<_> = saids_to_publish
                        .iter()
                        .flat_map(|said| {
                            match publish_oca_file_for(
                                facade.clone(),
                                said.clone(),
                                &timeout,
                                remote_repo_url.clone(),
                            ) {
                                Ok(_) => {
                                    vec![]
                                }
                                Err(err) => vec![err.to_string()],
                            }
                        })
                        .collect();
                    if res.is_empty() {
                        Ok(())
                    } else {
                        Err(CliError::PublishError(said, res))
                    }
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
        Some(Commands::Validate { ocafile, directory }) => {
            let (paths, base_dir) = load_ocafiles_all(ocafile.as_ref(), directory.as_ref())?;

            let facade = get_oca_facade(local_repository_path);
            let facade = Arc::new(Mutex::new(facade));
            let mut graph = MutableGraph::new(&base_dir, paths);
            let errs = validate::validate_directory(facade, &mut graph, None)?;
            for err in errs {
                println!("{}", err)
            }
            Ok(())
        }
        Some(Commands::Tui { dir, timeout }) => {
            if let Some(directory) = dir.as_ref() {
                let (all_oca_files, _base_dir) = load_ocafiles_all(None, Some(directory))
                    .unwrap_or_else(|err| {
                        eprintln!("{err}");
                        process::exit(1);
                    });
                let facade = Arc::new(Mutex::new(get_oca_facade(local_repository_path)));

                let to_show = visit_current_dir(directory)?
                    .into_iter()
                    .map(|of| parse_node(directory, &of).map(|(node, _)| node));
                tui::draw(
                    directory.clone(),
                    to_show,
                    all_oca_files,
                    facade,
                    remote_repo_url_from_config,
                    timeout.clone(),
                )
                .unwrap_or_else(|err| {
                    eprintln!("{err}");
                    process::exit(1);
                });
                Ok(())
            } else {
                eprintln!("No file or directory provided");
                process::exit(1);
            }
        }
        Some(Commands::Mapping { said }) => {
            let said = SelfAddressingIdentifier::from_str(said)?;
            let (paths, base_dir) = load_ocafiles_all(None, Some(&local_repository_path))?;
            let facade = get_oca_facade(local_repository_path);

            let graph = DependencyGraph::from_paths(&base_dir, paths).unwrap();

            let o = mapping(said, &facade, &graph).unwrap();

            let actual_json = serde_json::to_string_pretty(&o).unwrap();
            println!("{}", actual_json);
            Ok(())
        }
        None => Ok(()),
    }
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
